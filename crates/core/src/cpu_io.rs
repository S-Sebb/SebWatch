//! CPU and disk rates per cgroup, from two reads of `cpu.stat` and `io.stat` one refresh apart.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Cumulative counters of one cgroup at one moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub usage_usec: u64,
    /// Bytes read and written, None without `io.stat`.
    pub io: Option<(u64, u64)>,
}

/// Counters of every leaf group, keyed by its path relative to the cgroup root.
pub type Sample = BTreeMap<PathBuf, Counters>;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rates {
    /// 1.0 = one core busy for the whole interval.
    pub cpu_cores: f64,
    /// Bytes read and written per second, None where the group has no `io.stat`.
    pub io: Option<(f64, f64)>,
}

/// Reads the counters of `groups`, paths relative to `root` (the groups with processes, see
/// [`crate::cgroup::walk`]; the app passes those of the memory reading, so the
/// hierarchy is walked once per refresh). A group that vanishes mid-read is left out.
pub fn read_sample<'a>(root: &Path, groups: impl IntoIterator<Item = &'a PathBuf>) -> Sample {
    let mut sample = Sample::new();
    for rel in groups {
        let dir = root.join(rel);
        let Some(usage_usec) = fs::read_to_string(dir.join("cpu.stat"))
            .ok()
            .and_then(|s| parse_usage_usec(&s))
        else {
            continue;
        };
        // io.stat is absent when the io controller isn't enabled for this subtree: no disk numbers.
        let io = fs::read_to_string(dir.join("io.stat"))
            .ok()
            .map(|s| parse_io_stat(&s));
        sample.insert(rel.clone(), Counters { usage_usec, io });
    }
    sample
}

fn parse_usage_usec(cpu_stat: &str) -> Option<u64> {
    cpu_stat
        .lines()
        .find_map(|line| line.strip_prefix("usage_usec ")?.trim().parse().ok())
}

/// Sums `rbytes` and `wbytes` over all devices (one line per device: `8:0 rbytes=.. wbytes=..`).
fn parse_io_stat(io_stat: &str) -> (u64, u64) {
    let (mut read, mut write) = (0, 0);
    for field in io_stat.split_whitespace() {
        if let Some(v) = field.strip_prefix("rbytes=") {
            read += v.parse::<u64>().unwrap_or(0);
        } else if let Some(v) = field.strip_prefix("wbytes=") {
            write += v.parse::<u64>().unwrap_or(0);
        }
    }
    (read, write)
}

/// Rates per group over `elapsed`, for the groups in `cur`. A group that is new since `prev`
/// shows zero this round (disk unknown without `io.stat`): its counters hold its whole lifetime, not the last interval. Groups
/// that vanished are dropped. A counter that went down (path reused by a new group) counts as 0.
pub fn rates(prev: &Sample, cur: &Sample, elapsed: Duration) -> BTreeMap<PathBuf, Rates> {
    let secs = elapsed.as_secs_f64();
    cur.iter()
        .map(|(path, now)| {
            let rates = match prev.get(path) {
                Some(before) if secs > 0.0 => Rates {
                    cpu_cores: now.usage_usec.saturating_sub(before.usage_usec) as f64 / 1e6 / secs,
                    io: now.io.zip(before.io).map(|((r, w), (r0, w0))| {
                        let per_sec = |n: u64, n0: u64| n.saturating_sub(n0) as f64 / secs;
                        (per_sec(r, r0), per_sec(w, w0))
                    }),
                },
                _ => Rates {
                    cpu_cores: 0.0,
                    io: now.io.map(|_| (0.0, 0.0)),
                },
            };
            (path.clone(), rates)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cgroup::walk;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("sebwatch-cpu-io-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Fixture(dir)
        }

        fn group(&self, rel: &str, procs: &str, usage_usec: u64, io_stat: Option<&str>) {
            let dir = self.0.join(rel);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("cgroup.procs"), procs).unwrap();
            fs::write(
                dir.join("cpu.stat"),
                format!("usage_usec {usage_usec}\nuser_usec 0\nsystem_usec 0\n"),
            )
            .unwrap();
            match io_stat {
                Some(s) => fs::write(dir.join("io.stat"), s).unwrap(),
                None => drop(fs::remove_file(dir.join("io.stat"))),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rates_between_two_reads() {
        let fx = Fixture::new("rates");
        // Root counters are system-wide and must not appear.
        fs::write(fx.0.join("cgroup.procs"), "2\n").unwrap();
        fs::write(fx.0.join("cpu.stat"), "usage_usec 999999999\n").unwrap();
        // A parent slice without processes of its own is not a leaf.
        fx.group("user.slice", "", 0, None);
        let app = "user.slice/app-firefox-1.scope";
        fx.group(
            app,
            "100\n101\n",
            1_000_000,
            Some("8:0 rbytes=1000 wbytes=0 rios=1 wios=0\n259:0 rbytes=0 wbytes=500\n"),
        );
        fx.group("system.slice/old.service", "200\n", 5_000_000, None);
        let first = read_sample(&fx.0, &walk(&fx.0).unwrap().with_processes);

        fx.group(
            app,
            "100\n101\n",
            4_000_000,
            Some("8:0 rbytes=3000 wbytes=0 rios=2 wios=0\n259:0 rbytes=0 wbytes=4500\n"),
        );
        // One group vanishes between reads, one appears with a large lifetime total.
        fs::remove_dir_all(fx.0.join("system.slice/old.service")).unwrap();
        fx.group(
            "system.slice/new.service",
            "300\n",
            50_000_000,
            Some("8:0 rbytes=1000000 wbytes=1000000\n"),
        );
        let second = read_sample(&fx.0, &walk(&fx.0).unwrap().with_processes);

        let r = rates(&first, &second, Duration::from_secs(2));
        let keys: Vec<_> = r.keys().map(|k| k.to_str().unwrap()).collect();
        assert_eq!(keys, ["system.slice/new.service", app]);
        assert_eq!(
            r[Path::new(app)],
            Rates {
                cpu_cores: 1.5,
                io: Some((1000.0, 2000.0)),
            }
        );
        let new = Rates {
            cpu_cores: 0.0,
            io: Some((0.0, 0.0)),
        };
        assert_eq!(r[Path::new("system.slice/new.service")], new);
    }

    #[test]
    fn a_group_without_io_stat_has_no_disk_figures() {
        let fx = Fixture::new("no-io");
        fx.group("system.slice/a.service", "1\n", 1_000_000, None);
        let groups = walk(&fx.0).unwrap().with_processes;
        let first = read_sample(&fx.0, &groups);
        fx.group("system.slice/a.service", "1\n", 3_000_000, None);
        let second = read_sample(&fx.0, &groups);
        let r = rates(&first, &second, Duration::from_secs(2));
        assert_eq!(r[Path::new("system.slice/a.service")].io, None);
    }

    #[test]
    fn reused_path_with_lower_counters_shows_zero() {
        let path = PathBuf::from("app.scope");
        let prev = Sample::from([(
            path.clone(),
            Counters {
                usage_usec: 9_000_000,
                io: Some((500, 500)),
            },
        )]);
        let cur = Sample::from([(
            path.clone(),
            Counters {
                usage_usec: 1_000,
                io: Some((0, 0)),
            },
        )]);
        assert_eq!(
            rates(&prev, &cur, Duration::from_secs(1))[&path],
            Rates {
                cpu_cores: 0.0,
                io: Some((0.0, 0.0)),
            }
        );
    }
}
