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

use crate::model::{App, Closed, Exited, Part, Snapshot, Unreadable, Unsplit};

pub struct Reader {
    naming: Naming,
    /// The last CPU and disk counters, to turn the next ones into rates.
    last: Option<(Instant, Sample)>,
    /// The last process sample of each expanded app, for its processes' CPU.
    drills: HashMap<String, (Instant, processes::Sample)>,
    cores: u32,
    /// Memory can be split by app here (`memory::splits_by_app`).
    split: bool,
}

/// The NVIDIA GPU as read this refresh.
struct GpuReading {
    gpu: Option<nvidia::Gpu>,
    asleep: bool,
    /// Awake, but the driver didn't answer.
    unavailable: bool,
}

fn read_gpu(interval: Duration) -> GpuReading {
    let (gpu, asleep, unavailable) = match nvidia::read(interval) {
        Some(Nvidia::Awake(gpu)) => (Some(gpu), false, false),
        Some(Nvidia::Asleep) => (None, true, false),
        Some(Nvidia::NotAnswering) => (None, false, true),
        None => (None, false, false),
    };
    GpuReading {
        gpu,
        asleep,
        unavailable,
    }
}

/// A group's disk use in bytes/s, None without `io.stat`.
fn disk(r: &Rates) -> Option<u64> {
    r.io.map(|(read, write)| (read + write) as u64)
}

/// The sum of the known figures, None when none is.
fn sum_known(figures: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    figures.fold(None, |sum, f| match (sum, f) {
        (None, f) => f,
        (s, None) => s,
        (Some(s), Some(f)) => Some(s + f),
    })
}

impl Reader {
    pub fn new() -> Self {
        Self {
            naming: Naming::from_env(),
            last: None,
            drills: HashMap::new(),
            cores: std::thread::available_parallelism().map_or(1, |n| n.get() as u32),
            split: memory::splits_by_app(Path::new(CGROUP_ROOT)),
        }
    }

    /// CPU and disk rates of `groups` since the last call, and the whole CPU and disk use: None
    /// when no group's CPU, or no group's disk, could be read. The first call shows no use: rates
    /// need two samples.
    fn rates<'a>(
        &mut self,
        groups: impl IntoIterator<Item = &'a PathBuf>,
    ) -> (Instant, BTreeMap<PathBuf, Rates>, Option<f32>, Option<u64>) {
        let root = Path::new(CGROUP_ROOT);
        let now = Instant::now();
        let sample = cpu_io::read_sample(root, groups);
        let rates = match &self.last {
            Some((then, before)) => cpu_io::rates(before, &sample, now - *then),
            None => BTreeMap::new(),
        };
        let cpu = (!sample.is_empty()).then(|| rates.values().map(|r| r.cpu_cores).sum::<f64>());
        let io = sample.values().any(|c| c.io.is_some());
        let disk = io.then(|| rates.values().filter_map(disk).sum());
        self.last = Some((now, sample));
        (now, rates, cpu.map(|c| c as f32), disk)
    }

    /// `expanded` holds the keys of the apps whose processes are shown.
    pub fn read(&mut self, interval: Duration, expanded: &HashSet<String>) -> io::Result<Snapshot> {
        if !self.split {
            return self.read_unsplit(interval);
        }
        let root = Path::new(CGROUP_ROOT);
        let mem = memory::read(root, Path::new(PROC_ROOT))?;
        let (now, rates, cpu_used, disk_per_s) =
            self.rates(mem.apps.iter().flat_map(|a| &a.cgroups));
        // A sleeping NVIDIA GPU is idle: 0%, and no VRAM figure without waking it.
        let GpuReading {
            gpu,
            asleep,
            unavailable,
        } = read_gpu(interval);

        let rate = |group: &PathBuf| rates.get(group).copied().unwrap_or_default();
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
                            disk_per_s: disk(&rate(&group.path)),
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
                disk_per_s: sum_known(app.cgroups.iter().map(|g| disk(&rate(g)))),
                gpu_percent: usage(|u| u.sm_percent).map(|p| p as f32),
                vram: usage(|u| u.vram_bytes as f64).map(|b| b as u64),
                parts,
                unattributed_ram,
                unattributed_swap,
                over_charge,
                unreadable,
            });
        }

        Ok(Snapshot {
            taken: now,
            ram_total: mem.total,
            swap_total: swap().0,
            zswap_pool: mem.zswap_pool,
            file_cache: mem.apps.iter().map(|a| a.file_cache).sum(),
            kernel: mem.kernel_and_drivers,
            free: mem.free,
            apps,
            closed,
            cpu_used,
            cpu_cores: self.cores,
            nvidia_percent: match &gpu {
                Some(g) => Some(g.utilization_percent.unwrap_or(0) as f32),
                None => asleep.then_some(0.0),
            },
            vram: gpu
                .as_ref()
                .map(|g| (g.vram_used_bytes, g.vram_total_bytes)),
            gpu_asleep: asleep,
            nvidia_unavailable: unavailable,
            disk_per_s,
            unsplit: None,
        })
    }

    /// A system that can't split memory by app: totals from /proc/meminfo, and the root group's
    /// CPU and disk counters, which cover the whole system.
    fn read_unsplit(&mut self, interval: Duration) -> io::Result<Snapshot> {
        let totals = memory::totals(Path::new(PROC_ROOT))?;
        let (taken, _, cpu_used, disk_per_s) = self.rates(&[PathBuf::new()]);
        let GpuReading {
            gpu,
            asleep,
            unavailable,
        } = read_gpu(interval);
        let (swap_total, swap_free) = swap();
        Ok(Snapshot {
            taken,
            ram_total: totals.total,
            swap_total,
            zswap_pool: totals.zswap_pool,
            file_cache: totals.file_cache,
            kernel: totals
                .total
                .saturating_sub(totals.free + totals.file_cache + totals.zswap_pool),
            free: totals.free,
            apps: Vec::new(),
            closed: Closed::default(),
            cpu_used,
            cpu_cores: self.cores,
            nvidia_percent: match &gpu {
                Some(g) => Some(g.utilization_percent.unwrap_or(0) as f32),
                None => asleep.then_some(0.0),
            },
            vram: gpu
                .as_ref()
                .map(|g| (g.vram_used_bytes, g.vram_total_bytes)),
            gpu_asleep: asleep,
            nvidia_unavailable: unavailable,
            disk_per_s,
            unsplit: Some(Unsplit {
                swap_used: swap_total.saturating_sub(swap_free),
            }),
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

/// `SwapTotal` and `SwapFree` from /proc/meminfo; 0 without swap.
fn swap() -> (u64, u64) {
    let meminfo = std::fs::read_to_string(Path::new(PROC_ROOT).join("meminfo")).unwrap_or_default();
    let get = |key: &str| {
        meminfo
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
            .map_or(0, |kb| kb * 1024)
    };
    (get("SwapTotal:"), get("SwapFree:"))
}

#[cfg(test)]
mod tests {
    use super::sum_known;

    #[test]
    fn disk_sums_the_groups_that_have_it_and_is_unknown_when_none_do() {
        assert_eq!(sum_known([Some(1), None, Some(2)].into_iter()), Some(3));
        assert_eq!(sum_known([None, None].into_iter()), None);
        assert_eq!(sum_known([Some(0)].into_iter()), Some(0));
    }
}
