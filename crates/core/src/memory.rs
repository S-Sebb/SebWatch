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

use crate::cgroup;
use crate::naming::clean_name;

/// The live cgroup v2 hierarchy.
pub const CGROUP_ROOT: &str = "/sys/fs/cgroup";
/// The live procfs.
pub const PROC_ROOT: &str = "/proc";
/// The entry for memory still charged to groups whose processes all exited, outside any
/// running app's unit or slice. It has no groups.
pub const CLOSED_APPS: &str = "Closed apps (cache)";

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

/// One app: every populated memory leaf cgroup that shares a cleaned name, plus the charges left
/// in groups without processes inside its unit or slice; or the [`CLOSED_APPS`] entry.
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
    /// [`crate::cpu_io::Sample`], for CPU, disk and drill-down. Empty for [`CLOSED_APPS`].
    pub cgroups: Vec<PathBuf>,
    /// For [`CLOSED_APPS`]: the groups without processes it holds, biggest RAM first; they sum
    /// to the entry. Empty for any other app.
    pub exited: Vec<Exited>,
}

/// One group without processes and its own charges (without its children's).
#[derive(Debug, Clone, PartialEq)]
pub struct Exited {
    /// Relative to the cgroup root.
    pub group: PathBuf,
    pub ram: u64,
    pub file_cache: u64,
    pub swap: u64,
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
    let groups = cgroup::walk(cgroup_root)?;
    let mut charged_to: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for group in groups.with_processes {
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
    let mut add = |apps: &mut Vec<AppMemory>, name: String, c: &Charge, groups: Vec<PathBuf>| {
        app_zswap += c.zswap;
        let (ram, file_cache) = (c.ram(), c.file_cache());
        match apps.iter_mut().find(|a| a.name == name) {
            Some(app) => {
                app.ram += ram;
                app.file_cache += file_cache;
                app.swap += c.swap;
                app.cgroups.extend(groups);
            }
            None => apps.push(AppMemory {
                name,
                ram,
                file_cache,
                swap: c.swap,
                cgroups: groups,
                exited: Vec::new(),
            }),
        }
    };

    // Every memory group's charges, to subtract children from their parents below.
    let mut charges: HashMap<PathBuf, Charge> = HashMap::new();
    // Which app each group and its ancestors hold, None where several apps share it.
    let mut owner: HashMap<PathBuf, Option<String>> = HashMap::new();
    for (memory_group, groups) in charged_to {
        // A group gone since the walk is simply not counted.
        let Some(charge) = Charge::read(&cgroup_root.join(&memory_group)) else {
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
        for dir in memory_group.ancestors().filter(|d| *d != Path::new("")) {
            owner
                .entry(dir.to_path_buf())
                .and_modify(|o| {
                    if o.as_ref() != Some(&name) {
                        *o = None;
                    }
                })
                .or_insert_with(|| Some(name.clone()));
        }
        add(&mut apps, name, &charge, groups);
        charges.insert(memory_group, charge);
    }

    // A group whose processes exited keeps its page cache (and tmpfs, kernel objects) charged;
    // removing a group moves its charges to its parent. Its own share is its memory.stat minus
    // its memory children's, which are counted on their own. A group with no memory controller
    // has no memory.stat: its charges are in its parent's.
    let empty: Vec<(PathBuf, Charge)> = groups
        .without_processes
        .into_iter()
        .filter(|g| !charges.contains_key(g))
        .filter_map(|g| Some((g.clone(), Charge::read(&cgroup_root.join(&g))?)))
        .collect();
    let mut children: HashMap<&Path, Charge> = HashMap::new();
    for (group, charge) in charges.iter().chain(empty.iter().map(|(g, c)| (g, c))) {
        if let Some(parent) = group.parent() {
            children.entry(parent).or_default().add(charge);
        }
    }
    for (group, charge) in &empty {
        let own = match children.get(group.as_path()) {
            Some(c) => charge.minus(c),
            None => charge.clone(),
        };
        if own.ram() + own.file_cache() + own.swap == 0 {
            continue;
        }
        // The running app whose unit or slice holds the group, if only one app is in there.
        let name = group
            .ancestors()
            .find_map(|d| owner.get(d))
            .cloned()
            .flatten()
            .unwrap_or_else(|| CLOSED_APPS.into());
        let closed = name == CLOSED_APPS;
        add(&mut apps, name, &own, Vec::new());
        if closed {
            let entry = apps.iter_mut().find(|a| a.name == CLOSED_APPS);
            entry.expect("just added").exited.push(Exited {
                group: group.clone(),
                ram: own.ram(),
                file_cache: own.file_cache(),
                swap: own.swap,
            });
        }
    }
    for app in &mut apps {
        app.exited
            .sort_by(|a, b| b.ram.cmp(&a.ram).then_with(|| a.group.cmp(&b.group)));
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

/// One group's `memory.stat` and `memory.swap.current`, the fields the accounting uses.
#[derive(Clone, Default)]
struct Charge {
    anon: u64,
    file: u64,
    kernel: u64,
    shmem: u64,
    zswap: u64,
    swap: u64,
}

impl Charge {
    fn read(dir: &Path) -> Option<Charge> {
        let stat = fs::read_to_string(dir.join("memory.stat")).ok()?;
        let get = |key: &str| {
            stat.lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix(' '))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
        };
        // Absent when the kernel has no swap accounting: nothing of the group is in swap then.
        let swap = fs::read_to_string(dir.join("memory.swap.current"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        Some(Charge {
            anon: get("anon"),
            file: get("file"),
            kernel: get("kernel"),
            shmem: get("shmem"),
            zswap: get("zswap"),
            swap,
        })
    }

    /// RAM in use: `anon + shmem + kernel + zswap`.
    fn ram(&self) -> u64 {
        self.anon + self.shmem + self.kernel + self.zswap
    }

    /// `file - shmem`: shmem is counted in `file` too.
    fn file_cache(&self) -> u64 {
        self.file.saturating_sub(self.shmem)
    }

    fn add(&mut self, o: &Charge) {
        self.anon += o.anon;
        self.file += o.file;
        self.kernel += o.kernel;
        self.shmem += o.shmem;
        self.zswap += o.zswap;
        self.swap += o.swap;
    }

    /// Saturating: a parent and its children are read at slightly different moments.
    fn minus(&self, o: &Charge) -> Charge {
        Charge {
            anon: self.anon.saturating_sub(o.anon),
            file: self.file.saturating_sub(o.file),
            kernel: self.kernel.saturating_sub(o.kernel),
            shmem: self.shmem.saturating_sub(o.shmem),
            zswap: self.zswap.saturating_sub(o.zswap),
            swap: self.swap.saturating_sub(o.swap),
        }
    }
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
