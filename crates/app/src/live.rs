//! Readings of this PC from `sebwatch-core`, turned into the window's snapshot.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sebwatch_core::cpu_io::{self, Rates, Sample};
use sebwatch_core::memory::{self, CGROUP_ROOT, PROC_ROOT};
use sebwatch_core::naming::Naming;
use sebwatch_core::nvidia::{self, Nvidia};
use sebwatch_core::processes;

use crate::model::{App, Closed, Exited, Part, Snapshot, Unreadable};

pub struct Reader {
    naming: Naming,
    /// The last CPU and disk counters, to turn the next ones into rates.
    last: Option<(Instant, Sample)>,
    /// The last process sample of each expanded app, for its processes' CPU.
    drills: HashMap<String, (Instant, processes::Sample)>,
    cores: u32,
}

impl Reader {
    pub fn new() -> Self {
        Self {
            naming: Naming::from_env(),
            last: None,
            drills: HashMap::new(),
            cores: std::thread::available_parallelism().map_or(1, |n| n.get() as u32),
        }
    }

    /// The first reading shows no CPU or disk use: rates need two samples.
    /// `expanded` holds the keys of the apps whose processes are shown.
    pub fn read(&mut self, interval: Duration, expanded: &HashSet<String>) -> io::Result<Snapshot> {
        let root = Path::new(CGROUP_ROOT);
        let mem = memory::read(root, Path::new(PROC_ROOT))?;
        let now = Instant::now();
        let sample = cpu_io::read_sample(root, mem.apps.iter().flat_map(|a| &a.cgroups));
        let rates = match &self.last {
            Some((then, before)) => cpu_io::rates(before, &sample, now - *then),
            None => BTreeMap::new(),
        };
        self.last = Some((now, sample));
        // A sleeping NVIDIA GPU is idle: 0%, and no VRAM figure without waking it.
        let (gpu, asleep) = match nvidia::read(interval) {
            Some(Nvidia::Awake(gpu)) => (Some(gpu), false),
            Some(Nvidia::Asleep) => (None, true),
            None => (None, false),
        };

        let rate = |group: &PathBuf| rates.get(group).copied().unwrap_or_default();
        let disk = |r: Rates| (r.read_bytes_per_sec + r.write_bytes_per_sec) as u64;
        // Processes are read only for expanded apps; one that folds up forgets its last sample.
        self.drills.retain(|key, _| expanded.contains(key));
        let mut apps = Vec::new();
        let mut closed = Closed::default();
        for app in &mem.apps {
            if app.name == memory::CLOSED_APPS {
                closed = self.closed(app);
                continue;
            }
            // Every group's processes, for finding an app by PID.
            let pids: Vec<Vec<u32>> = app.cgroups.iter().map(|g| pids(&root.join(g))).collect();
            let main = pids[0].first().and_then(|&pid| main_process(pid));
            let named = self.naming.name(&app.cgroups[0], main.as_deref()).clone();
            let usage = |f: fn(&nvidia::AppUsage) -> f64| {
                let gpu = gpu.as_ref()?;
                let used: Vec<_> = app.cgroups.iter().filter_map(|g| gpu.apps.get(g)).collect();
                (!used.is_empty()).then(|| used.into_iter().map(f).sum::<f64>())
            };
            let (mut parts, mut unattributed_ram, mut unattributed_swap) = (Vec::new(), 0, 0);
            let (mut over_charge, mut unreadable) = (0, Vec::new());
            if expanded.contains(&app.name) {
                let now = Instant::now();
                let sample = processes::read_sample(root, Path::new(PROC_ROOT), &app.cgroups);
                let empty = processes::Sample::new();
                let (then, before) = match self.drills.get(&app.name) {
                    Some((then, before)) => (*then, before),
                    None => (now, &empty),
                };
                let drill = processes::drill_down(app.ram, before, &sample, now - then);
                let several = drill.groups.len() > 1;
                for group in &drill.groups {
                    if several {
                        parts.push(Part {
                            group: true,
                            name: leaf_name(&group.path),
                            detail: String::new(),
                            ram: Some(group.ram),
                            swap: Some(group.swap),
                            cpu: group.cpu_cores as f32,
                            disk_per_s: Some(disk(rate(&group.path))),
                        });
                    }
                    parts.extend(group.processes.iter().map(|p| Part {
                        group: false,
                        name: p.name.clone(),
                        detail: p.pid.to_string(),
                        ram: p.ram,
                        swap: p.swap,
                        cpu: p.cpu_cores as f32,
                        // Known per group only; an app of one group shows it on the app's row.
                        disk_per_s: None,
                    }));
                }
                unattributed_ram = drill.not_attributed;
                over_charge = drill.over_charge;
                unreadable = drill
                    .groups
                    .iter()
                    .flat_map(|g| &g.processes)
                    .filter_map(|p| {
                        Some(Unreadable {
                            name: p.name.clone(),
                            pid: p.pid,
                            denied: p.unreadable? == io::ErrorKind::PermissionDenied,
                        })
                    })
                    .collect();
                let process_swap: u64 = drill.groups.iter().map(|g| g.swap).sum();
                unattributed_swap = app.swap.saturating_sub(process_swap);
                self.drills.insert(app.name.clone(), (now, sample));
            }
            apps.push(App {
                key: app.name.clone(),
                name: named.name,
                detail: match (app.cgroups.len(), named.agents) {
                    (0 | 1, _) => String::new(),
                    (n, true) => format!("{n} agents"),
                    (n, false) => format!("{n} groups"),
                },
                icon: named.icon,
                groups: app.cgroups.iter().map(|g| leaf_name(g)).collect(),
                pids: pids.concat(),
                ram: app.ram,
                zswap: None,
                swap: app.swap,
                file_cache: app.file_cache,
                cpu: app.cgroups.iter().map(|g| rate(g).cpu_cores as f32).sum(),
                disk_per_s: app.cgroups.iter().map(|g| disk(rate(g))).sum(),
                gpu_percent: usage(|u| u.sm_percent).map(|p| p as f32),
                vram: usage(|u| u.vram_bytes as f64).map(|b| b as u64),
                parts,
                unattributed_ram,
                unattributed_swap,
                over_charge,
                unreadable,
            });
        }

        let total_rates = rates
            .values()
            .fold((0.0, 0), |(cpu, io), r| (cpu + r.cpu_cores, io + disk(*r)));
        Ok(Snapshot {
            taken: now,
            ram_total: mem.total,
            swap_total: swap_total(),
            zswap_pool: mem.zswap_pool,
            file_cache: mem.apps.iter().map(|a| a.file_cache).sum(),
            kernel: mem.kernel_and_drivers,
            free: mem.free,
            apps,
            closed,
            cpu_used: total_rates.0 as f32,
            cpu_cores: self.cores,
            nvidia_percent: match &gpu {
                Some(g) => Some(g.utilization_percent.unwrap_or(0) as f32),
                None => asleep.then_some(0.0),
            },
            intel_percent: None,
            vram: gpu
                .as_ref()
                .map(|g| (g.vram_used_bytes, g.vram_total_bytes)),
            gpu_asleep: asleep,
            disk_per_s: total_rates.1,
        })
    }
}

impl Reader {
    /// The closed-apps entry, its groups named like apps and merged by name.
    fn closed(&mut self, entry: &memory::AppMemory) -> Closed {
        let mut exited: Vec<Exited> = Vec::new();
        for e in &entry.exited {
            let name = self.naming.name(&e.group, None).name.clone();
            match exited.iter_mut().find(|x| x.name == name) {
                Some(x) => {
                    x.ram += e.ram;
                    x.swap += e.swap;
                    x.file_cache += e.file_cache;
                }
                None => exited.push(Exited {
                    name,
                    ram: e.ram,
                    swap: e.swap,
                    file_cache: e.file_cache,
                }),
            }
        }
        exited.sort_by_key(|e| std::cmp::Reverse(e.ram));
        Closed {
            ram: entry.ram,
            swap: entry.swap,
            file_cache: entry.file_cache,
            exited,
        }
    }
}

fn leaf_name(group: &Path) -> String {
    group
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into()
}

/// The group's processes; none once it has gone.
fn pids(group: &Path) -> Vec<u32> {
    std::fs::read_to_string(group.join("cgroup.procs"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.parse().ok())
        .collect()
}

/// Full name of a group's first process, for naming emulators and `systemd-run` units.
fn main_process(pid: u32) -> Option<String> {
    let dir = Path::new(PROC_ROOT).join(pid.to_string());
    let comm = std::fs::read_to_string(dir.join("comm")).ok()?;
    Some(processes::full_name(&dir, comm.trim_end_matches('\n')))
}

/// `SwapTotal` from /proc/meminfo; 0 without swap.
fn swap_total() -> u64 {
    let meminfo = std::fs::read_to_string(Path::new(PROC_ROOT).join("meminfo")).unwrap_or_default();
    meminfo
        .lines()
        .find_map(|l| l.strip_prefix("SwapTotal:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        .map_or(0, |kb| kb * 1024)
}
