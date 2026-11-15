//! Memory accounting: where every byte of `MemTotal` goes, per app (cgroup v2) plus system rows.
//! The method and every source are described in `docs/accounting.md`.
//!
//! [`read`] returns a [`MemorySnapshot`] whose parts add up to [`MemorySnapshot::total`]:
//!
//! ```text
//! total = Σ apps (ram + file_cache) + free + zswap_pool + kernel_and_drivers
//! ```
//!
//! Swap is not RAM, so [`AppMemory::swap`] sits outside that sum. All sizes are bytes.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cgroup::groups_with_processes;
use crate::naming::clean_name;

/// The live cgroup v2 hierarchy.
pub const CGROUP_ROOT: &str = "/sys/fs/cgroup";
/// The live procfs.
pub const PROC_ROOT: &str = "/proc";

/// One refresh of the memory accounting.
#[derive(Debug, Clone, PartialEq)]
pub struct MemorySnapshot {
    /// `MemTotal`: physical RAM the kernel manages.
    pub total: u64,
    /// Apps, biggest RAM in use first.
    pub apps: Vec<AppMemory>,
    /// `MemFree`.
    pub free: u64,
    /// The part of the zswap pool no app accounts for (each app's share is in its `ram`).
    pub zswap_pool: u64,
    /// RAM no cgroup is charged for: the remainder that makes the parts sum to `total`.
    pub kernel_and_drivers: u64,
    /// `MemAvailable`, for the summary sentence.
    pub available: u64,
    /// Share of the last 10 s some task waited for memory, 0–100 (`some avg10`).
    pub pressure_avg10: f64,
}

/// One app: every populated memory leaf cgroup that shares a cleaned name.
#[derive(Debug, Clone, PartialEq)]
pub struct AppMemory {
    /// Cleaned name, see [`clean_name`] (the key apps are grouped by); AppImage groups use their main process name.
    pub name: String,
    /// RAM in use: `anon + shmem + kernel + zswap`.
    pub ram: u64,
    /// File cache: `file - shmem`, freed on demand.
    pub file_cache: u64,
    /// Swapped out: `memory.swap.current`.
    pub swap: u64,
    /// The app's groups with processes, relative to the cgroup root: the keys of
    /// [`crate::cpu_io::Sample`], for CPU, disk and drill-down.
    pub cgroups: Vec<PathBuf>,
}

/// Reads the accounting from a cgroup v2 root and a procfs root ([`CGROUP_ROOT`] and
/// [`PROC_ROOT`] on a live system; a fixture tree in tests).
pub fn read(cgroup_root: &Path, proc_root: &Path) -> io::Result<MemorySnapshot> {
    let meminfo = fs::read_to_string(proc_root.join("meminfo"))?;
    let kb = |key: &str| {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
            .map(|v| v * 1024)
            .ok_or_else(|| io::Error::other(format!("meminfo has no {key}")))
    };
    let (total, free, available, zswap) = (
        kb("MemTotal")?,
        kb("MemFree")?,
        kb("MemAvailable")?,
        kb("Zswap").unwrap_or(0),
    );

    // Memory is charged to the nearest group with the memory controller, which may be an
    // ancestor of the groups that hold the processes (`systemd-udevd.service/udev`).
    let mut charged_to: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for group in groups_with_processes(cgroup_root)? {
        let mut memory_group = group.as_path();
        while !cgroup_root.join(memory_group).join("memory.stat").exists() {
            match memory_group.parent() {
                Some(parent) if parent != Path::new("") => memory_group = parent,
                _ => break,
            }
        }
        let memory_group = memory_group.to_path_buf();
        charged_to.entry(memory_group).or_default().push(group);
    }

    let mut apps: Vec<AppMemory> = Vec::new();
    let mut app_zswap = 0;
    for (memory_group, groups) in charged_to {
        // A group gone since the walk is simply not counted.
        let Some(leaf) = read_leaf(&cgroup_root.join(&memory_group)) else {
            continue;
        };
        let unit = memory_group
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let mut name = clean_name(&unit);
        if name.starts_with("appimagekit_") {
            name = main_process(&cgroup_root.join(&groups[0]), proc_root).unwrap_or(name);
        }
        app_zswap += leaf.zswap;
        match apps.iter_mut().find(|a| a.name == name) {
            Some(app) => {
                app.ram += leaf.ram;
                app.file_cache += leaf.file_cache;
                app.swap += leaf.swap;
                app.cgroups.extend(groups);
            }
            None => apps.push(AppMemory {
                name,
                ram: leaf.ram,
                file_cache: leaf.file_cache,
                swap: leaf.swap,
                cgroups: groups,
            }),
        }
    }
    apps.sort_by(|a, b| b.ram.cmp(&a.ram).then_with(|| a.name.cmp(&b.name)));

    let charged: u64 = apps.iter().map(|a| a.ram + a.file_cache).sum();
    // Saturating: the remainders can't be negative in the kernel's books, but the pool size
    // and the per-cgroup charges are counted separately and can disagree by a few pages.
    let zswap_pool = zswap.saturating_sub(app_zswap);
    let kernel_and_drivers = total.saturating_sub(free + charged + zswap_pool);

    let pressure = fs::read_to_string(proc_root.join("pressure/memory")).unwrap_or_default();
    let pressure_avg10 = pressure
        .lines()
        .find(|l| l.starts_with("some "))
        .and_then(|l| l.split(' ').find_map(|f| f.strip_prefix("avg10=")))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);

    Ok(MemorySnapshot {
        total,
        apps,
        free,
        zswap_pool,
        kernel_and_drivers,
        available,
        pressure_avg10,
    })
}

struct Leaf {
    ram: u64,
    file_cache: u64,
    swap: u64,
    zswap: u64,
}

fn read_leaf(dir: &Path) -> Option<Leaf> {
    let stat = fs::read_to_string(dir.join("memory.stat")).ok()?;
    let get = |key: &str| {
        stat.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(' '))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let (anon, file, kernel, shmem, zswap) = (
        get("anon"),
        get("file"),
        get("kernel"),
        get("shmem"),
        get("zswap"),
    );
    // Absent when the kernel has no swap accounting: nothing of the group is in swap then.
    let swap = fs::read_to_string(dir.join("memory.swap.current"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    Some(Leaf {
        ram: anon + shmem + kernel + zswap,
        file_cache: file.saturating_sub(shmem),
        swap,
        zswap,
    })
}

/// The most common process name (`comm`) in a group, for generated unit names.
fn main_process(group: &Path, proc_root: &Path) -> Option<String> {
    let procs = fs::read_to_string(group.join("cgroup.procs")).ok()?;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for pid in procs.lines() {
        if let Ok(comm) = fs::read_to_string(proc_root.join(pid).join("comm")) {
            *counts.entry(comm.trim().to_string()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(name, _)| name)
}
