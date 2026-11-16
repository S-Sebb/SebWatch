//! Drill-down: the processes inside one app, read only while its row is expanded.
//!
//! [`read_sample`] reads every process of the app's groups (`AppMemory::cgroups`); [`drill_down`]
//! turns two samples one refresh apart into rows whose RAM plus
//! [`DrillDown::kernel_and_zswap`] sums to the app's RAM in use. Sizes are bytes.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `USER_HZ`, the unit of `utime`/`stime` in `/proc/<pid>/stat`: 100 on every architecture
/// Linux desktops run on, fixed by the ABI.
const TICKS_PER_SEC: f64 = 100.0;

/// One process at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    /// See [`full_name`].
    pub name: String,
    /// `Pss_Anon + Pss_Shmem`: the process's share of the app's RAM in use. `None` when
    /// `smaps_rollup` is unreadable (another user's process).
    pub ram: Option<u64>,
    /// `SwapPss`. `None` as for `ram`.
    pub swap: Option<u64>,
    /// `utime + stime`, all threads, in ticks.
    cpu_ticks: u64,
    /// Start time in ticks since boot: tells a reused PID from the process seen before.
    start_time: u64,
}

/// The processes of each group, keyed by the group's path relative to the cgroup root.
pub type Sample = BTreeMap<PathBuf, Vec<Process>>;

/// Reads every process in `groups` (relative to `cgroup_root`) from `proc_root`. A group or
/// process that is gone mid-read is left out.
pub fn read_sample(cgroup_root: &Path, proc_root: &Path, groups: &[PathBuf]) -> Sample {
    let mut sample = Sample::new();
    for group in groups {
        let Ok(procs) = fs::read_to_string(cgroup_root.join(group).join("cgroup.procs")) else {
            continue;
        };
        let processes = procs
            .lines()
            .filter_map(|pid| read_process(&proc_root.join(pid), pid.parse().ok()?))
            .collect();
        sample.insert(group.clone(), processes);
    }
    sample
}

fn read_process(dir: &Path, pid: u32) -> Option<Process> {
    let stat = fs::read_to_string(dir.join("stat")).ok()?;
    // `pid (comm) state ...`: comm may hold spaces and parentheses, so split at the last `)`.
    let (head, rest) = stat.rsplit_once(')')?;
    let name = full_name(dir, head.split_once('(')?.1);
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // Fields after comm start at field 3 (state): utime is 14, stime 15, starttime 22.
    let field = |n: usize| fields.get(n - 3)?.parse::<u64>().ok();
    let (cpu_ticks, start_time) = (field(14)? + field(15)?, field(22)?);

    let (ram, swap) = match fs::read_to_string(dir.join("smaps_rollup")) {
        Ok(rollup) => {
            let kb = |key: &str| {
                rollup
                    .lines()
                    .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
                    .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
                    .unwrap_or(0)
                    * 1024
            };
            (Some(kb("Pss_Anon") + kb("Pss_Shmem")), Some(kb("SwapPss")))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None, // exited mid-read
        Err(_) => (None, None),
    };
    Some(Process {
        pid,
        name,
        ram,
        swap,
        cpu_ticks,
        start_time,
    })
}

/// `comm` is the kernel's process name cut to 15 bytes. A name that long may be cut, so it is
/// completed from the base name of `argv[0]` (`cmdline`), else of `exe`, when that starts with
/// it (a process can rewrite its `argv[0]`). `dir` is the process's procfs directory.
pub fn full_name(dir: &Path, comm: &str) -> String {
    const COMM_LEN: usize = 15;
    if comm.len() < COMM_LEN {
        return comm.to_owned();
    }
    let argv0 = fs::read(dir.join("cmdline")).ok().and_then(|cmdline| {
        let argv0 = cmdline.split(|&b| b == 0).next()?;
        Some(String::from_utf8_lossy(argv0).into_owned())
    });
    let exe = || {
        Some(
            fs::read_link(dir.join("exe"))
                .ok()?
                .to_string_lossy()
                .into_owned(),
        )
    };
    argv0
        .into_iter()
        .chain(std::iter::once_with(exe).flatten())
        .map(|path| path.rsplit('/').next().unwrap_or_default().to_owned())
        .find(|name| name.len() > COMM_LEN && name.starts_with(comm))
        .unwrap_or_else(|| comm.to_owned())
}

/// The rows under an expanded app.
#[derive(Debug, Clone, PartialEq)]
pub struct DrillDown {
    /// The app's groups, biggest RAM first; an app of one group has one.
    pub groups: Vec<GroupRows>,
    /// "Kernel memory and zswap (not per process)": the app's RAM in use minus its processes'
    /// RAM, so the rows sum to the app's total.
    pub kernel_and_zswap: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupRows {
    /// Relative to the cgroup root.
    pub path: PathBuf,
    /// Biggest RAM first.
    pub processes: Vec<ProcessRow>,
    /// Sums over `processes` (unreadable ones count as 0).
    pub ram: u64,
    pub swap: u64,
    pub cpu_cores: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessRow {
    pub pid: u32,
    pub name: String,
    pub ram: Option<u64>,
    pub swap: Option<u64>,
    /// 1.0 = one core busy for the whole interval.
    pub cpu_cores: f64,
}

/// Rows for the processes in `cur`, CPU over `elapsed` since `prev` (empty on the first read).
/// A process new since `prev`, or a PID reused by a new process, shows zero CPU this round;
/// a process that exited is dropped. `app_ram` is the app's [`crate::memory::AppMemory::ram`].
pub fn drill_down(app_ram: u64, prev: &Sample, cur: &Sample, elapsed: Duration) -> DrillDown {
    let secs = elapsed.as_secs_f64();
    let mut groups: Vec<GroupRows> = cur
        .iter()
        .map(|(path, procs)| {
            let before = prev.get(path).map_or(&[][..], Vec::as_slice);
            let mut processes: Vec<ProcessRow> = procs
                .iter()
                .map(|p| {
                    let cpu_cores = match before
                        .iter()
                        .find(|b| b.pid == p.pid && b.start_time == p.start_time)
                    {
                        Some(b) if secs > 0.0 => {
                            p.cpu_ticks.saturating_sub(b.cpu_ticks) as f64 / TICKS_PER_SEC / secs
                        }
                        _ => 0.0,
                    };
                    ProcessRow {
                        pid: p.pid,
                        name: p.name.clone(),
                        ram: p.ram,
                        swap: p.swap,
                        cpu_cores,
                    }
                })
                .collect();
            processes.sort_by(|a, b| b.ram.cmp(&a.ram).then_with(|| a.pid.cmp(&b.pid)));
            GroupRows {
                path: path.clone(),
                ram: processes.iter().filter_map(|p| p.ram).sum(),
                swap: processes.iter().filter_map(|p| p.swap).sum(),
                cpu_cores: processes.iter().map(|p| p.cpu_cores).sum(),
                processes,
            }
        })
        .collect();
    groups.sort_by(|a, b| b.ram.cmp(&a.ram).then_with(|| a.path.cmp(&b.path)));
    // Saturating: PSS splits pages shared with other apps' processes, while the cgroup charges
    // each page to one group, so the processes can exceed the app by a little.
    let processes_ram: u64 = groups.iter().map(|g| g.ram).sum();
    DrillDown {
        groups,
        kernel_and_zswap: app_ram.saturating_sub(processes_ram),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("sebwatch-processes-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Fixture(dir)
        }

        fn group(&self, rel: &str, pids: &[u32]) {
            let dir = self.0.join("cgroup").join(rel);
            fs::create_dir_all(&dir).unwrap();
            let procs: String = pids.iter().map(|p| format!("{p}\n")).collect();
            fs::write(dir.join("cgroup.procs"), procs).unwrap();
        }

        /// `rollup` = Some((Pss_Anon, Pss_Shmem, SwapPss)) in kB; None = unreadable.
        fn process(
            &self,
            pid: u32,
            comm: &str,
            ticks: u64,
            start: u64,
            rollup: Option<(u64, u64, u64)>,
        ) {
            let dir = self.0.join("proc").join(pid.to_string());
            fs::create_dir_all(&dir).unwrap();
            // Fields 3..=22; utime (14) carries all the ticks, stime (15) is 0.
            let mut fields = vec!["0"; 20];
            let (ticks, start) = (ticks.to_string(), start.to_string());
            fields[14 - 3] = &ticks;
            fields[22 - 3] = &start;
            fs::write(
                dir.join("stat"),
                format!("{pid} ({comm}) {}\n", fields.join(" ")),
            )
            .unwrap();
            let rollup_path = dir.join("smaps_rollup");
            match rollup {
                Some((anon, shmem, swap)) => fs::write(
                    rollup_path,
                    format!(
                        "Rss: 9999 kB\nPss: 9999 kB\nPss_Anon: {anon} kB\nPss_File: 777 kB\n\
                         Pss_Shmem: {shmem} kB\nSwap: 9999 kB\nSwapPss: {swap} kB\n"
                    ),
                )
                .unwrap(),
                // A directory where the file should be: reading fails with an error other than
                // NotFound, as for another user's process.
                None => fs::create_dir_all(rollup_path).unwrap(),
            }
        }

        fn exit(&self, pid: u32) {
            fs::remove_dir_all(self.0.join("proc").join(pid.to_string())).unwrap();
        }

        fn sample(&self, groups: &[PathBuf]) -> Sample {
            read_sample(&self.0.join("cgroup"), &self.0.join("proc"), groups)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rows_sum_to_app_ram_across_groups_and_exits() {
        let fx = Fixture::new("sum");
        let (agent_a, agent_b) = (
            PathBuf::from("user.slice/app-claude-1.scope"),
            PathBuf::from("user.slice/app-claude-2.scope"),
        );
        let groups = [
            agent_a.clone(),
            agent_b.clone(),
            PathBuf::from("gone.scope"),
        ];
        fx.group("user.slice/app-claude-1.scope", &[100, 101, 102]);
        fx.group("user.slice/app-claude-2.scope", &[200]);
        fx.process(100, "claude (main)", 1_000, 5, Some((40, 10, 3)));
        fx.process(101, "cargo", 500, 6, Some((20, 0, 0)));
        fx.process(102, "rustc", 0, 7, Some((5, 0, 0)));
        fx.process(200, "root-helper", 300, 8, None);
        let first = fx.sample(&groups);

        // Between reads: 101 exits but still sits in cgroup.procs (exits mid-read), 102's PID
        // is reused by a new process with a later start, 103 starts, the others run on.
        fx.exit(101);
        fx.process(102, "node", 900, 50, Some((30, 0, 1)));
        fx.group("user.slice/app-claude-1.scope", &[100, 101, 102, 103]);
        fx.process(103, "bash", 40, 60, Some((2, 0, 0)));
        fx.process(100, "claude (main)", 1_150, 5, Some((50, 10, 3)));
        fx.process(200, "root-helper", 400, 8, None);
        let second = fx.sample(&groups);

        let app_ram = 200 * 1024;
        let d = drill_down(app_ram, &first, &second, Duration::from_secs(2));

        let rows: Vec<_> = d.groups[0]
            .processes
            .iter()
            .map(|p| (p.pid, p.name.as_str(), p.ram, p.swap, p.cpu_cores))
            .collect();
        assert_eq!(d.groups[0].path, agent_a);
        assert_eq!(
            rows,
            [
                (100, "claude (main)", Some(60 * 1024), Some(3 * 1024), 0.75),
                (102, "node", Some(30 * 1024), Some(1024), 0.0),
                (103, "bash", Some(2 * 1024), Some(0), 0.0),
            ]
        );
        assert_eq!((d.groups[0].ram, d.groups[0].swap), (92 * 1024, 4 * 1024));
        // The other user's process is listed with its CPU but no memory.
        assert_eq!(d.groups[1].path, agent_b);
        assert_eq!(d.groups[1].processes[0].ram, None);
        assert_eq!(d.groups[1].cpu_cores, 0.5);
        assert_eq!(d.groups.len(), 2);

        let rows_ram: u64 = d.groups.iter().map(|g| g.ram).sum();
        assert_eq!(rows_ram + d.kernel_and_zswap, app_ram);
        assert_eq!(d.kernel_and_zswap, 108 * 1024);
    }

    #[test]
    fn first_read_shows_no_cpu() {
        let fx = Fixture::new("first");
        let group = PathBuf::from("app.scope");
        fx.group("app.scope", &[1]);
        fx.process(1, "app", 10_000, 1, Some((1, 0, 0)));
        let cur = fx.sample(std::slice::from_ref(&group));
        let d = drill_down(0, &Sample::new(), &cur, Duration::from_secs(1));
        assert_eq!(d.groups[0].cpu_cores, 0.0);
        // Processes over the app's charge (pages shared across apps): no remainder, no underflow.
        assert_eq!(d.kernel_and_zswap, 0);
    }

    #[test]
    fn cut_names_are_completed_from_cmdline_or_exe() {
        let fx = Fixture::new("names");
        let group = PathBuf::from("app.scope");
        fx.group("app.scope", &[1, 2, 3, 4, 5]);
        for (pid, comm) in [
            (1, "xdg-desktop-por"),
            (2, "gnome-calendar-"),
            (3, "Isolated Web Co"),
            (4, "kded6"),
            (5, "unreadable-name"),
        ] {
            fx.process(pid, comm, 0, 1, Some((1, 0, 0)));
        }
        let proc = |pid: u32, file: &str| fx.0.join("proc").join(pid.to_string()).join(file);
        fs::write(
            proc(1, "cmdline"),
            "/usr/libexec/xdg-desktop-portal\0--verbose\0",
        )
        .unwrap();
        // Empty cmdline (a zombie): the exe link.
        fs::write(proc(2, "cmdline"), "").unwrap();
        std::os::unix::fs::symlink("/usr/libexec/gnome-calendar-server", proc(2, "exe")).unwrap();
        // A rewritten argv[0] that isn't the program: the short name stays.
        fs::write(
            proc(3, "cmdline"),
            "/usr/lib/firefox/firefox -contentproc\0",
        )
        .unwrap();
        // Short names never read cmdline.
        fs::write(proc(4, "cmdline"), "/usr/bin/kded6-not-this\0").unwrap();

        let sample = fx.sample(std::slice::from_ref(&group));
        let names: Vec<_> = sample[&group].iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "xdg-desktop-portal",
                "gnome-calendar-server",
                "Isolated Web Co",
                "kded6",
                "unreadable-name"
            ]
        );
    }
}
