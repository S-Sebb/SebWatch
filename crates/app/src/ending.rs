//! End and Kill: what a selected row stands for, and stopping it. A unit of the user's own
//! systemd is stopped through it (`StopUnit`) or killed through its cgroup (`cgroup.kill`), so
//! every process in it goes; anything else is signalled process by process. Only the user's own
//! processes: SebWatch runs without root.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use sebwatch_core::memory::{CGROUP_ROOT, PROC_ROOT};

/// What a row stands for, as far as ending it goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Endable {
    /// An app or one of its groups: cgroups relative to the cgroup root.
    Groups(Vec<PathBuf>),
    Process(u32),
    /// Nothing to end, and why.
    Nothing(String),
}

#[derive(Debug, PartialEq)]
pub enum Target {
    /// A unit of the user's systemd, with its cgroup relative to the cgroup root.
    Unit {
        name: String,
        cgroup: PathBuf,
    },
    Process(u32),
}

#[derive(Debug, PartialEq)]
pub struct Plan {
    pub targets: Vec<Target>,
    /// Part of the desktop (Plasma, KWin, the login session): End asks first.
    pub desktop: bool,
}

/// This PC, or a test's stand-in for it.
pub struct System {
    pub cgroup_root: PathBuf,
    pub proc_root: PathBuf,
    pub uid: u32,
}

impl System {
    pub fn new() -> Self {
        Self {
            cgroup_root: CGROUP_ROOT.into(),
            proc_root: PROC_ROOT.into(),
            // SAFETY: getuid has no preconditions and cannot fail.
            uid: unsafe { libc::getuid() },
        }
    }

    /// What End and Kill would act on, or why there is nothing to.
    pub fn plan(&self, what: &Endable) -> Result<Plan, String> {
        match what {
            Endable::Nothing(why) => Err(why.clone()),
            Endable::Process(pid) => {
                let owner = self.owner(*pid).ok_or("Nothing to end: it has exited.")?;
                if owner != self.uid {
                    return Err(refusal(owner));
                }
                Ok(Plan {
                    targets: vec![Target::Process(*pid)],
                    desktop: self.cgroup_of(*pid).is_some_and(|g| self.desktop(&g)),
                })
            }
            Endable::Groups(groups) => {
                let mut targets = Vec::new();
                let mut others = None;
                for group in groups {
                    if group.ends_with(format!("{}/init.scope", self.manager())) {
                        return Err(
                            "Nothing to end: this is the systemd that runs your session.".into(),
                        );
                    }
                    if let Some(name) = self.unit(group) {
                        targets.push(Target::Unit {
                            name,
                            cgroup: group.clone(),
                        });
                        continue;
                    }
                    for pid in pids(&self.cgroup_root.join(group)) {
                        match self.owner(pid) {
                            Some(owner) if owner == self.uid => targets.push(Target::Process(pid)),
                            Some(owner) => others = Some(owner),
                            None => {}
                        }
                    }
                }
                if targets.is_empty() {
                    return Err(match others {
                        Some(owner) => refusal(owner),
                        None => "Nothing to end: no process runs in it.".into(),
                    });
                }
                Ok(Plan {
                    targets,
                    desktop: groups.iter().any(|g| self.desktop(g)),
                })
            }
        }
    }

    /// Asks it to stop: the unit stopped, or SIGTERM.
    pub fn end(&self, plan: &Plan) -> Result<(), String> {
        for target in &plan.targets {
            match target {
                Target::Unit { name, .. } => stop_unit(name).map_err(|e| e.to_string())?,
                Target::Process(pid) => signal(*pid, libc::SIGTERM)?,
            }
        }
        Ok(())
    }

    /// Stops it at once: every process in the unit's cgroup, or SIGKILL.
    pub fn kill(&self, plan: &Plan) -> Result<(), String> {
        for target in &plan.targets {
            match target {
                Target::Unit { cgroup, .. } => {
                    fs::write(self.cgroup_root.join(cgroup).join("cgroup.kill"), "1")
                        .map_err(|e| reason(&e))?
                }
                Target::Process(pid) => signal(*pid, libc::SIGKILL)?,
            }
        }
        Ok(())
    }

    /// Whether any of it still runs. A zombie has stopped; its parent just hasn't noticed.
    pub fn alive(&self, plan: &Plan) -> bool {
        plan.targets.iter().any(|target| match target {
            Target::Unit { cgroup, .. } => !pids(&self.cgroup_root.join(cgroup)).is_empty(),
            Target::Process(pid) => {
                fs::read_to_string(self.proc_root.join(pid.to_string()).join("stat"))
                    .ok()
                    .and_then(|stat| stat.rsplit_once(") ").map(|(_, s)| !s.starts_with('Z')))
                    .unwrap_or(false)
            }
        })
    }

    fn manager(&self) -> String {
        format!("user@{}.service", self.uid)
    }

    /// The unit a group is, when it is a scope or service of the user's own systemd.
    fn unit(&self, group: &Path) -> Option<String> {
        let leaf = group.file_name()?.to_str()?;
        let mine = group
            .parent()?
            .components()
            .any(|c| c.as_os_str() == self.manager().as_str());
        (mine && (leaf.ends_with(".scope") || leaf.ends_with(".service"))).then(|| leaf.into())
    }

    /// The desktop's own units (Plasma's session.slice) and the login session's scope.
    fn desktop(&self, group: &Path) -> bool {
        let session = format!("{}/session.slice/", self.manager());
        group.to_string_lossy().contains(&session)
            || group
                .file_name()
                .and_then(|l| l.to_str())
                .is_some_and(|l| l.starts_with("session-") && l.ends_with(".scope"))
    }

    fn owner(&self, pid: u32) -> Option<u32> {
        fs::metadata(self.proc_root.join(pid.to_string()))
            .ok()
            .map(|m| m.uid())
    }

    /// The process's cgroup, relative to the root.
    fn cgroup_of(&self, pid: u32) -> Option<PathBuf> {
        let line = fs::read_to_string(self.proc_root.join(pid.to_string()).join("cgroup")).ok()?;
        line.lines()
            .find_map(|l| l.strip_prefix("0::/"))
            .map(PathBuf::from)
    }
}

fn refusal(owner: u32) -> String {
    format!(
        "Owned by {}: SebWatch can only end your own processes.",
        user_name(owner)
    )
}

fn user_name(uid: u32) -> String {
    fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|passwd| {
            passwd.lines().find_map(|line| {
                let mut fields = line.split(':');
                let name = fields.next()?;
                (fields.nth(1)? == uid.to_string()).then(|| name.to_string())
            })
        })
        .unwrap_or_else(|| format!("user {uid}"))
}

/// The group's processes; none once it has gone.
fn pids(group: &Path) -> Vec<u32> {
    fs::read_to_string(group.join("cgroup.procs"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.parse().ok())
        .collect()
}

/// An error in plain words: "Permission denied", without "(os error 13)".
fn reason(error: &io::Error) -> String {
    let text = error.to_string();
    match text.find(" (os error") {
        Some(end) => text[..end].into(),
        None => text,
    }
}

fn signal(pid: u32, signal: i32) -> Result<(), String> {
    // SAFETY: kill only sends a signal; a bad PID is an error, not undefined behaviour.
    if unsafe { libc::kill(pid as i32, signal) } == 0 {
        Ok(())
    } else {
        Err(reason(&io::Error::last_os_error()))
    }
}

fn stop_unit(name: &str) -> zbus::Result<()> {
    zbus::blocking::Connection::session()?.call_method(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        Some("org.freedesktop.systemd1.Manager"),
        "StopUnit",
        &(name, "replace"),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    use super::{Endable, Plan, System, Target};

    /// A fake cgroup tree and /proc in a folder of its own: groups with their processes, every
    /// file owned by the user running the test, while SebWatch runs as `uid`.
    fn system(test: &str, uid: u32, groups: &[(&str, &[u32])]) -> System {
        let dir =
            std::env::temp_dir().join(format!("sebwatch-ending-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (group, pids) in groups {
            let path = dir.join("cgroup").join(group);
            std::fs::create_dir_all(&path).unwrap();
            let procs: String = pids.iter().map(|p| format!("{p}\n")).collect();
            std::fs::write(path.join("cgroup.procs"), procs).unwrap();
            for pid in *pids {
                let proc = dir.join("proc").join(pid.to_string());
                std::fs::create_dir_all(&proc).unwrap();
                std::fs::write(proc.join("cgroup"), format!("0::/{group}\n")).unwrap();
            }
        }
        System {
            cgroup_root: dir.join("cgroup"),
            proc_root: dir.join("proc"),
            uid,
        }
    }

    fn clean(system: System) {
        std::fs::remove_dir_all(system.cgroup_root.parent().unwrap()).unwrap();
    }

    /// The test's own uid, which owns the fake /proc.
    fn me() -> u32 {
        System::new().uid
    }

    const APP: &str =
        "user.slice/user-1000.slice/user@1000.service/app.slice/app-steam@autostart.service";
    const KWIN: &str =
        "user.slice/user-1000.slice/user@1000.service/session.slice/plasma-kwin_wayland.service";

    #[test]
    fn a_user_unit_is_stopped_whole_and_a_process_or_other_group_signalled() {
        let me = me();
        let app = APP.replace("1000", &me.to_string());
        let nested = format!("{app}/helpers");
        let session = format!("user.slice/user-{me}.slice/session-2.scope");
        let system = system(
            "unit",
            me,
            &[(&app, &[10, 11]), (&nested, &[12]), (&session, &[20])],
        );

        let plan = system
            .plan(&Endable::Groups(vec![app.clone().into()]))
            .unwrap();
        let unit = "app-steam@autostart.service".to_string();
        assert_eq!(
            plan,
            Plan {
                targets: vec![Target::Unit {
                    name: unit,
                    cgroup: app.clone().into()
                }],
                desktop: false
            }
        );
        // A group inside a unit is not the unit: only its own processes.
        let plan = system.plan(&Endable::Groups(vec![nested.into()])).unwrap();
        assert_eq!(plan.targets, vec![Target::Process(12)]);
        // The login session belongs to the system's systemd, not the user's, and is the desktop.
        let plan = system.plan(&Endable::Groups(vec![session.into()])).unwrap();
        assert_eq!(
            plan,
            Plan {
                targets: vec![Target::Process(20)],
                desktop: true
            }
        );
        let plan = system.plan(&Endable::Process(11)).unwrap();
        assert_eq!(
            plan,
            Plan {
                targets: vec![Target::Process(11)],
                desktop: false
            }
        );
        clean(system);
    }

    #[test]
    fn desktop_parts_ask_first() {
        let me = me();
        let kwin = KWIN.replace("1000", &me.to_string());
        let system = system("desktop", me, &[(&kwin, &[30])]);
        assert!(
            system
                .plan(&Endable::Groups(vec![kwin.into()]))
                .unwrap()
                .desktop
        );
        assert!(system.plan(&Endable::Process(30)).unwrap().desktop);
        clean(system);
    }

    #[test]
    fn refuses_others_processes_and_rows_with_nothing_to_end() {
        // The fake /proc is owned by the test's user; SebWatch here runs as someone else.
        let other = me() + 1;
        let system = system("refuse", other, &[("system.slice/sshd.service", &[40])]);
        let refused = |what| system.plan(&what).unwrap_err();
        let owned = format!(
            "Owned by {}: SebWatch can only end your own processes.",
            super::user_name(me())
        );
        assert_eq!(
            refused(Endable::Groups(vec!["system.slice/sshd.service".into()])),
            owned
        );
        assert_eq!(refused(Endable::Process(40)), owned);
        assert_eq!(
            refused(Endable::Process(41)),
            "Nothing to end: it has exited."
        );
        assert_eq!(
            refused(Endable::Groups(vec!["system.slice/gone.service".into()])),
            "Nothing to end: no process runs in it."
        );
        // Another user's systemd units are not ours to stop either.
        let theirs = APP.replace("1000", &(other + 1).to_string());
        assert!(system.unit(Path::new(&theirs)).is_none());
        // Ending systemd itself would end the whole session.
        let init = format!("user.slice/user-{other}.slice/user@{other}.service/init.scope");
        assert!(refused(Endable::Groups(vec![init.into()])).starts_with("Nothing to end"));
        assert_eq!(refused(Endable::Nothing("why".into())), "why");
        clean(system);
    }

    /// Starts `command` in a scope of its own, as an app launched from the desktop would be.
    // The child goes back to the caller, whose `gone` waits for it.
    #[allow(clippy::zombie_processes)]
    fn scope(name: &str, command: &str) -> (Child, PathBuf) {
        let mut child = Command::new("systemd-run")
            .args([
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                &format!("--unit={name}"),
            ])
            .args(["sh", "-c", command])
            .spawn()
            .expect("systemd-run");
        let file = format!("/proc/{}/cgroup", child.id());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let group = std::fs::read_to_string(&file).unwrap_or_default();
            let group = group.trim().trim_start_matches("0::/");
            if group.ends_with(&format!("{name}.scope")) {
                return (child, group.into());
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("{name} never reached its scope");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits for the child to go, killing it if the test failed to.
    fn gone(mut child: Child, system: &System, plan: &Plan) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if child.try_wait().unwrap().is_some() && !system.alive(plan) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        false
    }

    #[test]
    #[ignore = "starts throwaway scopes in the systemd user session; run through scripts/test-end-kill"]
    fn ends_and_kills_throwaway_processes() {
        let system = System::new();
        let tag = std::process::id();

        // End stops the unit: the scope's shell and the sleeps it started all go.
        let (child, group) = scope(
            &format!("sebwatch-test-end-{tag}"),
            "sleep 600 & sleep 600 & wait",
        );
        let plan = system.plan(&Endable::Groups(vec![group])).unwrap();
        assert!(
            matches!(plan.targets[..], [Target::Unit { .. }]),
            "{plan:?}"
        );
        assert!(system.alive(&plan));
        system.end(&plan).unwrap();
        assert!(gone(child, &system, &plan), "the scope survived End");

        // Kill empties a unit that ignores End's SIGTERM.
        let (child, group) = scope(
            &format!("sebwatch-test-kill-{tag}"),
            "trap '' TERM; sleep 600 & wait",
        );
        let plan = system.plan(&Endable::Groups(vec![group])).unwrap();
        system.kill(&plan).unwrap();
        assert!(gone(child, &system, &plan), "the scope survived Kill");

        // One process: SIGTERM, then SIGKILL for one that ignores it.
        let child = Command::new("sleep").arg("600").spawn().unwrap();
        let plan = system.plan(&Endable::Process(child.id())).unwrap();
        assert_eq!(plan.targets, vec![Target::Process(child.id())]);
        system.end(&plan).unwrap();
        assert!(gone(child, &system, &plan), "the process survived End");
        let child = Command::new("sh")
            .args(["-c", "trap '' TERM; while :; do sleep 1; done"])
            .spawn()
            .unwrap();
        let plan = system.plan(&Endable::Process(child.id())).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        system.end(&plan).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert!(system.alive(&plan), "the shell should ignore End");
        system.kill(&plan).unwrap();
        assert!(gone(child, &system, &plan), "the process survived Kill");
    }
}
