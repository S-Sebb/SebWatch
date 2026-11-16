//! Readings of this PC from `sebwatch-core`, turned into the window's snapshot.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sebwatch_core::cpu_io::{self, Rates, Sample};
use sebwatch_core::drm;
use sebwatch_core::memory::{self, CGROUP_ROOT, PROC_ROOT};
use sebwatch_core::naming::Naming;
use sebwatch_core::nvidia::{self, Nvidia};
use sebwatch_core::processes;

use crate::model::{App, Closed, Exited, Igpu, Part, Snapshot, Unreadable, Unsplit};

pub struct Reader {
    naming: Naming,
    /// The last CPU and disk counters, to turn the next ones into rates.
    last: Option<(Instant, Sample)>,
    /// The last process sample of each expanded app, for its processes' CPU.
    drills: HashMap<String, (Instant, processes::Sample)>,
    cores: u32,
    /// Memory can be split by app here (`memory::splits_by_app`).
    split: bool,
    /// The last NVIDIA reading and when it was taken, reused while the next NVML read isn't due.
    gpu: Option<(Instant, Nvidia)>,
    /// The Intel or AMD GPU, if there is one.
    igpu: Option<IgpuReader>,
}

struct IgpuReader {
    device: drm::Device,
    scanner: drm::Scanner,
    uid: u32,
    /// The last sample, to turn the next into busy shares.
    last: Option<(Instant, drm::Sample)>,
}

/// The Intel or AMD GPU as read this refresh: its use since the last, and the groups with
/// processes that couldn't be read.
struct IgpuReading {
    vendor: &'static str,
    usage: drm::Usage,
    unreadable: std::collections::BTreeSet<PathBuf>,
}

impl IgpuReader {
    fn new() -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            device: drm::find_device(Path::new("/sys/class/drm"))?,
            scanner: drm::Scanner::default(),
            uid: std::fs::metadata(Path::new(PROC_ROOT).join("self"))
                .ok()?
                .uid(),
            last: None,
        })
    }

    /// The first call shows no use: busy shares need two samples.
    fn read(&mut self) -> IgpuReading {
        let now = Instant::now();
        let sample = self
            .scanner
            .read(Path::new(PROC_ROOT), self.uid, &self.device);
        let usage = match &self.last {
            Some((then, before)) => drm::usage(before, &sample, now - *then),
            None => drm::usage(&sample, &sample, Duration::ZERO),
        };
        let unreadable = sample.unreadable.clone();
        self.last = Some((now, sample));
        IgpuReading {
            vendor: self.device.vendor,
            usage,
            unreadable,
        }
    }
}

/// The NVIDIA GPU as read this refresh.
struct GpuReading {
    gpu: Option<nvidia::Gpu>,
    asleep: bool,
    /// Awake, but the driver didn't answer.
    unavailable: bool,
}

/// How often NVML is read while the dGPU sits idle. Every read starts NVML, which may restart the
/// driver's idle timer; reading less often gives an idle dGPU its chance to runtime-suspend.
const IDLE_NVML_REFRESH: Duration = Duration::from_secs(10);

/// Whether NVML is read again, `since` the `last` reading: at once unless that reading found the
/// GPU awake and idle (0%, no process using memory or the GPU), then after `IDLE_NVML_REFRESH`.
/// An asleep GPU is checked every refresh, which reads only sysfs.
fn nvml_due(last: &Nvidia, since: Duration) -> bool {
    let idle = |gpu: &nvidia::Gpu| {
        gpu.utilization_percent.unwrap_or(0) == 0
            && gpu
                .apps
                .values()
                .all(|a| a.vram_bytes == 0 && a.sm_percent == 0.0)
    };
    !matches!(last, Nvidia::Awake(gpu) if idle(gpu)) || since >= IDLE_NVML_REFRESH
}

fn gpu_reading(nvidia: Option<Nvidia>) -> GpuReading {
    let (gpu, asleep, unavailable) = match nvidia {
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
            gpu: None,
            igpu: IgpuReader::new(),
        }
    }

    /// The NVIDIA GPU this refresh: read anew when due (`nvml_due`), else the last reading.
    fn read_gpu(&mut self, interval: Duration) -> GpuReading {
        let now = Instant::now();
        let nvidia = match &self.gpu {
            Some((at, last)) if !nvml_due(last, now - *at) => Some(last.clone()),
            _ => {
                let nvidia = nvidia::read(interval);
                self.gpu = nvidia.clone().map(|n| (now, n));
                nvidia
            }
        };
        gpu_reading(nvidia)
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
        } = self.read_gpu(interval);
        let igpu = self.igpu.as_mut().map(IgpuReader::read);

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
            // Its clients' busy % on the Intel or AMD GPU, summed over its groups. Unknown when
            // some process couldn't be read and none read was busy (KWin, whose own fds are closed).
            let (igpu_percent, igpu_unreadable) = match &igpu {
                Some(r) => {
                    let used: Vec<_> = app
                        .cgroups
                        .iter()
                        .filter_map(|g| r.usage.apps.get(g))
                        .collect();
                    let busy: f64 = used.iter().map(|u| u.busy_percent).sum();
                    let unreadable = app.cgroups.iter().any(|g| r.unreadable.contains(g));
                    match (used.is_empty(), unreadable && busy == 0.0) {
                        (_, true) => (None, true),
                        (true, false) => (None, false),
                        (false, false) => (Some(busy.min(100.0) as f32), false),
                    }
                }
                None => (None, false),
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
                cgroups: app.cgroups.clone(),
                pids: pids.concat(),
                ram: app.ram,
                zswap: None,
                swap: app.swap,
                file_cache: app.file_cache,
                cpu: app.cgroups.iter().map(|g| rate(g).cpu_cores as f32).sum(),
                disk_per_s: sum_known(app.cgroups.iter().map(|g| disk(&rate(g)))),
                gpu_percent: usage(|u| u.sm_percent).map(|p| p as f32),
                igpu_percent,
                igpu_unreadable,
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
            igpu: igpu.map(igpu_total),
            vram: gpu
                .as_ref()
                .map(|g| (g.vram_used_bytes, g.vram_total_bytes)),
            gpu_asleep: asleep,
            nvidia_unavailable: unavailable,
            disk_per_s,
            pressure: memory::pressure(Path::new(PROC_ROOT)),
            zswap: memory::zswap(Path::new(PROC_ROOT), Path::new("/sys")),
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
        } = self.read_gpu(interval);
        let igpu = self.igpu.as_mut().map(IgpuReader::read);
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
            igpu: igpu.map(igpu_total),
            vram: gpu
                .as_ref()
                .map(|g| (g.vram_used_bytes, g.vram_total_bytes)),
            gpu_asleep: asleep,
            nvidia_unavailable: unavailable,
            disk_per_s,
            pressure: memory::pressure(Path::new(PROC_ROOT)),
            zswap: memory::zswap(Path::new(PROC_ROOT), Path::new("/sys")),
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

fn igpu_total(r: IgpuReading) -> Igpu {
    Igpu {
        vendor: r.vendor.into(),
        percent: r.usage.busy_percent as f32,
        whole_device: r.usage.whole_device,
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
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use sebwatch_core::nvidia::{AppUsage, Gpu, Nvidia};

    use super::{nvml_due, sum_known};

    #[test]
    fn an_idle_gpu_is_read_every_10_s_and_a_used_or_asleep_one_every_refresh() {
        let gpu = |percent, vram_bytes| {
            Nvidia::Awake(Gpu {
                vram_used_bytes: 0,
                vram_total_bytes: 8 << 30,
                utilization_percent: Some(percent),
                apps: BTreeMap::from([(
                    PathBuf::from("app.slice/a.scope"),
                    AppUsage {
                        vram_bytes,
                        sm_percent: 0.0,
                    },
                )]),
            })
        };
        let (soon, later) = (Duration::from_secs(2), Duration::from_secs(10));
        assert!(!nvml_due(&gpu(0, 0), soon));
        assert!(nvml_due(&gpu(0, 0), later));
        assert!(nvml_due(&gpu(3, 0), soon));
        assert!(nvml_due(&gpu(0, 1 << 20), soon));
        assert!(nvml_due(&Nvidia::Asleep, soon));
        assert!(nvml_due(&Nvidia::NotAnswering, soon));
    }

    #[test]
    fn disk_sums_the_groups_that_have_it_and_is_unknown_when_none_do() {
        assert_eq!(sum_known([Some(1), None, Some(2)].into_iter()), Some(3));
        assert_eq!(sum_known([None, None].into_iter()), None);
        assert_eq!(sum_known([Some(0)].into_iter()), Some(0));
    }
}
