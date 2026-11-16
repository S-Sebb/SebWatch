//! Intel and AMD GPU use per app, from the DRM clients the kernel lists in `/proc/<pid>/fdinfo`.
//!
//! Each open DRM file is a client with an id (`drm-client-id`), cumulative busy time per engine
//! (`drm-engine-<name>`, ns) and its memory (`drm-resident-<region>`, or the older
//! `drm-memory-<region>`). Two reads one refresh apart give each engine's busy share. A client
//! shared by several fds or processes counts once, for the first process that holds it.
//! Only the user's own processes are read: another user's fds are closed to us.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The GPU read here: an Intel or AMD display controller the kernel lists under `/sys/class/drm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// PCI address, as fdinfo's `drm-pdev` gives it.
    pub pdev: String,
    /// "Intel" or "AMD", from the driver.
    pub vendor: &'static str,
    /// Where the driver tells how busy the whole GPU is, compositor included; None where it
    /// doesn't (the busy % then sums the clients read).
    pub whole: Option<WholeSource>,
}

/// A sysfs file with the whole GPU's use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WholeSource {
    /// Milliseconds the GPU has spent idle in its lowest power state since boot: i915's
    /// `gt/gt0/rc6_residency_ms`, xe's `gtidle/idle_residency_ms`.
    IdleMs(PathBuf),
    /// Busy % now: amdgpu's `gpu_busy_percent`.
    BusyPercent(PathBuf),
}

/// A reading of a [`WholeSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Whole {
    IdleMs(u64),
    BusyPercent(u64),
}

impl WholeSource {
    /// The first of the driver's files that exists.
    fn find(card: &Path, driver: &str) -> Option<Self> {
        let dev = card.join("device");
        let (files, idle) = match driver {
            "i915" => (
                vec![
                    card.join("gt/gt0/rc6_residency_ms"),
                    card.join("power/rc6_residency_ms"),
                ],
                true,
            ),
            "xe" => (vec![dev.join("tile0/gt0/gtidle/idle_residency_ms")], true),
            _ => (vec![dev.join("gpu_busy_percent")], false),
        };
        let file = files.into_iter().find(|f| fs::read_to_string(f).is_ok())?;
        Some(if idle {
            WholeSource::IdleMs(file)
        } else {
            WholeSource::BusyPercent(file)
        })
    }

    fn read(&self) -> Option<Whole> {
        let number = |f: &Path| fs::read_to_string(f).ok()?.trim().parse().ok();
        match self {
            WholeSource::IdleMs(f) => number(f).map(Whole::IdleMs),
            WholeSource::BusyPercent(f) => number(f).map(Whole::BusyPercent),
        }
    }
}

/// The first Intel or AMD GPU under `drm_root` (`/sys/class/drm`), the one the firmware set up
/// for the display (`boot_vga`) first. None without one.
pub fn find_device(drm_root: &Path) -> Option<Device> {
    let mut cards: Vec<_> = fs::read_dir(drm_root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            name.strip_prefix("card")
                .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    cards.sort();
    let found = cards.into_iter().filter_map(|card| {
        let dev = card.join("device");
        let driver = fs::read_link(dev.join("driver")).ok()?;
        let driver = driver.file_name()?.to_str()?;
        let vendor = match driver {
            "i915" | "xe" => "Intel",
            "amdgpu" => "AMD",
            _ => return None,
        };
        let pdev = fs::canonicalize(&dev)
            .ok()?
            .file_name()?
            .to_str()?
            .to_owned();
        let boot = fs::read_to_string(dev.join("boot_vga")).is_ok_and(|s| s.trim() == "1");
        let whole = WholeSource::find(&card, driver);
        Some((
            boot,
            Device {
                pdev,
                vendor,
                whole,
            },
        ))
    });
    found.max_by_key(|(boot, _)| *boot).map(|(_, d)| d)
}

/// One DRM client at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Client {
    /// The holding process's cgroup, relative to the cgroup root.
    group: PathBuf,
    /// Busy ns and the number of engines of that class (`drm-engine-capacity-<name>`, default 1).
    engines: BTreeMap<String, (u64, u64)>,
    memory: u64,
}

/// The device's clients, keyed by `drm-client-id`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sample {
    clients: BTreeMap<u64, Client>,
    /// Groups holding one of the user's processes whose fds can't be read: a process with extra
    /// privileges is closed even to its own user (KWin, which has `CAP_SYS_NICE`).
    pub unreadable: BTreeSet<PathBuf>,
    whole: Option<Whole>,
}

/// What one client said in its fdinfo, if it is a client of `pdev`.
#[derive(Debug, PartialEq, Eq)]
struct Fdinfo {
    id: u64,
    engines: BTreeMap<String, (u64, u64)>,
    memory: u64,
}

fn parse_fdinfo(text: &str, pdev: &str) -> Option<Fdinfo> {
    let (mut id, mut on_device) = (None, false);
    let mut engines = BTreeMap::<String, (u64, u64)>::new();
    let (mut resident, mut legacy) = (None::<u64>, 0);
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if key == "drm-client-id" {
            id = value.parse().ok();
        } else if key == "drm-pdev" {
            on_device = value == pdev;
        } else if let Some(name) = key.strip_prefix("drm-engine-capacity-") {
            engines.entry(name.into()).or_insert((0, 1)).1 = value.parse().ok()?;
        } else if let Some(name) = key.strip_prefix("drm-engine-") {
            let ns = value.strip_suffix(" ns")?.parse().ok()?;
            engines.entry(name.into()).or_insert((0, 1)).0 = ns;
        } else if key.starts_with("drm-resident-") {
            *resident.get_or_insert(0) += bytes(value)?;
        } else if key.starts_with("drm-memory-") {
            legacy += bytes(value)?;
        }
    }
    on_device.then_some(())?;
    Some(Fdinfo {
        id: id?,
        engines,
        memory: resident.unwrap_or(legacy),
    })
}

/// `137616 KiB`, `12 MiB` or a plain byte count.
fn bytes(value: &str) -> Option<u64> {
    let (n, unit) = value.split_once(' ').unwrap_or((value, ""));
    let scale = match unit {
        "" => 1,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        _ => return None,
    };
    Some(n.parse::<u64>().ok()? * scale)
}

/// Reads the DRM clients of one device, remembering which fds of each process are DRM files:
/// finding them means a `readlink` per open fd of every process (about 13 000 on a desktop,
/// 27 ms), while reading the known ones takes a stat per process. A new process is searched at
/// once; every [`RESCAN`] reads all are, which catches a process opening the GPU late.
#[derive(Debug, Default)]
pub struct Scanner {
    drm_fds: HashMap<u32, Vec<OsString>>,
    reads: u32,
}

/// Reads between full searches: 30 s at the window's 2 s refresh.
pub const RESCAN: u32 = 15;

impl Scanner {
    /// The clients of `device` held by processes in `proc_root` that belong to `uid`.
    pub fn read(&mut self, proc_root: &Path, uid: u32, device: &Device) -> Sample {
        if self.reads.is_multiple_of(RESCAN) {
            self.drm_fds.clear();
        }
        self.reads += 1;
        let mut pids: Vec<u32> = fs::read_dir(proc_root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.metadata().is_ok_and(|m| m.uid() == uid))
            .filter_map(|e| e.file_name().to_str()?.parse().ok())
            .collect();
        // Lowest PID first: a client shared with a child counts for the parent.
        pids.sort_unstable();
        let mut sample = Sample {
            whole: device.whole.as_ref().and_then(WholeSource::read),
            ..Sample::default()
        };
        let mut seen = HashMap::new();
        for pid in pids {
            let dir = proc_root.join(pid.to_string());
            let group = || {
                let text = fs::read_to_string(dir.join("cgroup")).ok()?;
                Some(PathBuf::from(
                    text.lines().find_map(|l| l.strip_prefix("0::/"))?,
                ))
            };
            let fds = match self.drm_fds.remove(&pid) {
                Some(fds) => fds,
                None => match drm_fds(&dir) {
                    Ok(fds) => fds,
                    Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                        sample.unreadable.extend(group());
                        continue;
                    }
                    Err(_) => continue, // exited
                },
            };
            let mut held = None;
            for fd in &fds {
                let Some(info) = fs::read_to_string(dir.join("fdinfo").join(fd))
                    .ok()
                    .and_then(|text| parse_fdinfo(&text, &device.pdev))
                else {
                    continue;
                };
                if sample.clients.contains_key(&info.id) {
                    continue;
                }
                let Some(group) = held.get_or_insert_with(group).clone() else {
                    break; // exited
                };
                let client = Client {
                    group,
                    engines: info.engines,
                    memory: info.memory,
                };
                sample.clients.insert(info.id, client);
            }
            seen.insert(pid, fds);
        }
        self.drm_fds = seen;
        sample
    }
}

/// The process's fds open on a DRM device (`/dev/dri/*`).
fn drm_fds(dir: &Path) -> io::Result<Vec<OsString>> {
    Ok(fs::read_dir(dir.join("fd"))?
        .flatten()
        .filter(|fd| fs::read_link(fd.path()).is_ok_and(|t| t.starts_with("/dev/dri/")))
        .map(|fd| fd.file_name())
        .collect())
}

/// The device's use over `elapsed` between two samples.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    /// The whole GPU's busy %: 100 % less its idle share where the driver reports one (i915's
    /// RC6 residency), else the driver's busy % (amdgpu), else the busiest engine summed over
    /// every client read, which leaves out the processes that can't be read.
    pub busy_percent: f64,
    /// `busy_percent` comes from the driver and covers every process, KWin's included.
    pub whole_device: bool,
    /// Keyed by cgroup path relative to the cgroup root, like `cpu_io`.
    pub apps: BTreeMap<PathBuf, AppUsage>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AppUsage {
    /// The busiest engine's busy %, summed over the group's clients.
    pub busy_percent: f64,
    /// The clients' memory in bytes now. Integrated GPUs keep it in RAM.
    pub memory_bytes: u64,
}

/// Busy shares between `prev` and `cur`. A client new since `prev` shows no busy time this
/// round; one that went away is dropped.
pub fn usage(prev: &Sample, cur: &Sample, elapsed: Duration) -> Usage {
    let ns = elapsed.as_nanos() as f64;
    // Busy share per engine, per group and for the whole device.
    let mut groups = BTreeMap::<&Path, (BTreeMap<&str, f64>, u64)>::new();
    let mut device = BTreeMap::<&str, f64>::new();
    for (id, client) in &cur.clients {
        let (busy, memory) = groups.entry(&client.group).or_default();
        *memory += client.memory;
        let Some(before) = prev.clients.get(id) else {
            busy.entry("").or_default();
            continue;
        };
        for (engine, &(now, capacity)) in &client.engines {
            let then = before.engines.get(engine).map_or(now, |e| e.0);
            let share = if ns > 0.0 {
                now.saturating_sub(then) as f64 / ns / capacity.max(1) as f64
            } else {
                0.0
            };
            *busy.entry(engine).or_default() += share;
            *device.entry(engine).or_default() += share;
        }
    }
    let busiest = |engines: &BTreeMap<&str, f64>| {
        engines.values().fold(0.0, |a: f64, &b| a.max(b)).min(1.0) * 100.0
    };
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
    let busy_percent = match (prev.whole, cur.whole) {
        (Some(Whole::IdleMs(then)), Some(Whole::IdleMs(now))) if elapsed_ms > 0.0 => {
            let idle = now.saturating_sub(then) as f64 / elapsed_ms;
            (1.0 - idle).clamp(0.0, 1.0) * 100.0
        }
        (_, Some(Whole::IdleMs(_))) => 0.0, // the first read: no interval yet
        (_, Some(Whole::BusyPercent(p))) => p.min(100) as f64,
        (_, None) => busiest(&device),
    };
    Usage {
        busy_percent,
        whole_device: cur.whole.is_some(),
        apps: groups
            .into_iter()
            .map(|(group, (busy, memory_bytes))| {
                let usage = AppUsage {
                    busy_percent: busiest(&busy),
                    memory_bytes,
                };
                (group.to_path_buf(), usage)
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PDEV: &str = "0000:00:02.0";

    fn fdinfo(id: u64, render_ns: u64, video_ns: u64) -> String {
        format!(
            "pos:\t0\nflags:\t02100002\nmnt_id:\t43\nino:\t541\ndrm-driver:\ti915\n\
             drm-client-id:\t{id}\ndrm-pdev:\t{PDEV}\ndrm-total-system0:\t137616 KiB\n\
             drm-resident-system0:\t1024 KiB\ndrm-resident-stolen-system0:\t0\n\
             drm-engine-render:\t{render_ns} ns\ndrm-engine-copy:\t0 ns\n\
             drm-engine-video:\t{video_ns} ns\ndrm-engine-capacity-video:\t2\n"
        )
    }

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("sebwatch-drm-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Fixture(dir)
        }

        /// A process in `group` with these fds: (fd number, device path, fdinfo text).
        fn process(&self, pid: u32, group: &str, fds: &[(u32, &str, &str)]) {
            let dir = self.0.join(pid.to_string());
            fs::create_dir_all(dir.join("fd")).unwrap();
            fs::create_dir_all(dir.join("fdinfo")).unwrap();
            fs::write(dir.join("cgroup"), format!("0::/{group}\n")).unwrap();
            for (fd, target, info) in fds {
                let link = dir.join("fd").join(fd.to_string());
                let _ = fs::remove_file(&link);
                std::os::unix::fs::symlink(target, link).unwrap();
                fs::write(dir.join("fdinfo").join(fd.to_string()), info).unwrap();
            }
        }

        fn sample(&self) -> Sample {
            let uid = fs::metadata(&self.0).unwrap().uid();
            let device = Device {
                pdev: PDEV.into(),
                vendor: "Intel",
                whole: None,
            };
            Scanner::default().read(&self.0, uid, &device)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn parses_engines_capacity_and_resident_memory() {
        let info = parse_fdinfo(&fdinfo(117, 3_039_526_672, 8), PDEV).unwrap();
        assert_eq!(info.id, 117);
        assert_eq!(info.engines["render"], (3_039_526_672, 1));
        assert_eq!(info.engines["video"], (8, 2));
        // Resident, not total: the total includes pages not in memory.
        assert_eq!(info.memory, 1024 * 1024);
        // Another GPU's client, or a file that isn't a DRM client.
        assert_eq!(parse_fdinfo(&fdinfo(1, 0, 0), "0000:01:00.0"), None);
        assert_eq!(parse_fdinfo("pos:\t0\nflags:\t02\n", PDEV), None);
        // amdgpu before 6.x: only drm-memory-*.
        let amd = "drm-driver:\tamdgpu\ndrm-pdev:\t0000:00:02.0\ndrm-client-id:\t5\n\
                   drm-memory-vram:\t2 MiB\ndrm-memory-gtt:\t512 KiB\ndrm-engine-gfx:\t10 ns\n";
        assert_eq!(parse_fdinfo(amd, PDEV).unwrap().memory, 2560 * 1024);
    }

    #[test]
    fn a_client_on_two_fds_counts_once_and_busy_is_the_busiest_engine() {
        let fx = Fixture::new("busy");
        let (shell, game) = (
            "user.slice/plasmashell.service",
            "user.slice/app-game-1.scope",
        );
        let shell_fds = |ns| {
            let a = fdinfo(7, ns, 0);
            fx.process(
                10,
                shell,
                &[
                    (5, "/dev/dri/renderD128", &a),
                    (9, "/dev/dri/card1", &a),
                    (3, "/dev/null", ""),
                ],
            );
        };
        // The game's two processes share client 20 (fd passed on); the child also has its own.
        let game_fds = |shared: u64, own_video: u64| {
            let s = fdinfo(20, shared, 0);
            let own = fdinfo(21, 0, own_video);
            fx.process(30, game, &[(4, "/dev/dri/renderD128", &s)]);
            fx.process(
                31,
                game,
                &[
                    (4, "/dev/dri/renderD128", &s),
                    (6, "/dev/dri/renderD128", &own),
                ],
            );
        };
        shell_fds(1_000_000_000);
        game_fds(0, 0);
        let first = fx.sample();
        assert_eq!(first.clients.len(), 3);
        assert_eq!(first.clients[&20].group, PathBuf::from(game));

        // Over 2 s: the shell renders 0.2 s, the game 0.6 s, and its child decodes 1.6 s
        // on one of two video engines' worth (2 × 2 s).
        shell_fds(1_200_000_000);
        game_fds(600_000_000, 1_600_000_000);
        let second = fx.sample();
        let u = usage(&first, &second, Duration::from_secs(2));
        let percent = |g: &str| u.apps[Path::new(g)].busy_percent;
        assert!((percent(shell) - 10.0).abs() < 1e-9);
        // Render 30 %, video 40 %: the busiest engine.
        assert!((percent(game) - 40.0).abs() < 1e-9);
        assert_eq!(u.apps[Path::new(game)].memory_bytes, 2 * 1024 * 1024);
        // The device: render 10 + 30 = 40 %, video 40 %.
        assert!((u.busy_percent - 40.0).abs() < 1e-9);

        // On the first sample a client has no busy time yet, but its app is listed.
        let fresh = usage(&Sample::default(), &second, Duration::from_secs(2));
        assert_eq!(fresh.busy_percent, 0.0);
        assert_eq!(fresh.apps[Path::new(game)].busy_percent, 0.0);
    }

    #[test]
    fn the_whole_gpu_is_busy_while_it_is_out_of_rc6() {
        let at = |idle_ms| Sample {
            whole: Some(Whole::IdleMs(idle_ms)),
            ..Sample::default()
        };
        // 1.5 s of 2 s idle in RC6: 25 % busy, whatever the clients read say.
        let u = usage(&at(9_198_586), &at(9_200_086), Duration::from_secs(2));
        assert!((u.busy_percent - 25.0).abs() < 1e-9);
        assert!(u.whole_device);
        // Idle the whole time (a counter running a little ahead of our clock): 0, not below.
        let u = usage(&at(1_000), &at(3_010), Duration::from_secs(2));
        assert_eq!(u.busy_percent, 0.0);
        // The first read has no interval yet.
        assert_eq!(
            usage(&Sample::default(), &at(5), Duration::ZERO).busy_percent,
            0.0
        );
        // amdgpu says its busy % outright.
        let amd = Sample {
            whole: Some(Whole::BusyPercent(37)),
            ..Sample::default()
        };
        assert_eq!(usage(&amd, &amd, Duration::from_secs(2)).busy_percent, 37.0);
    }

    #[test]
    fn finds_the_rc6_counter_of_an_i915_card() {
        let fx = Fixture::new("sysfs");
        let card = fx.0.join("card1");
        fs::create_dir_all(card.join("gt/gt0")).unwrap();
        fs::write(card.join("gt/gt0/rc6_residency_ms"), "9198586\n").unwrap();
        let source = WholeSource::find(&card, "i915").unwrap();
        assert_eq!(
            source,
            WholeSource::IdleMs(card.join("gt/gt0/rc6_residency_ms"))
        );
        assert_eq!(source.read(), Some(Whole::IdleMs(9_198_586)));
        // No such file: the busy % falls back to the clients.
        assert_eq!(WholeSource::find(&card, "xe"), None);
    }
}
