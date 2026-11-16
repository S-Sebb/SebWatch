//! Friendly name and icon name for a cgroup leaf (see docs/accounting.md, "Apps are cgroups").
//!
//! Order: a running Android emulator gets its own name; then the `.desktop` file of the desktop
//! or Flatpak id in the leaf name; then the unit file's `Description=`, or for a `systemd-run`
//! unit (whose Description is its command line) the program it runs; then the cleaned name.
//! Everything is read from files, once per leaf name.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Icon for anything without a desktop file (background services, terminals, transient units).
pub const SERVICE_ICON: &str = "application-x-executable";
/// Icon for coding agents (T3 Code's Claude, Codex and OpenCode units).
pub const AGENT_ICON: &str = "utilities-terminal";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppName {
    pub name: String,
    /// Icon theme name or absolute path, as the desktop file gives it.
    pub icon: String,
    /// One agent per group: the window counts the app's groups as agents.
    pub agents: bool,
}

pub struct Naming {
    desktop_dirs: Vec<PathBuf>,
    unit_dirs: Vec<PathBuf>,
    cache: HashMap<String, AppName>,
}

impl Naming {
    /// `desktop_dirs` hold `.desktop` files directly (`…/share/applications`); `unit_dirs` hold
    /// systemd unit files. Earlier directories win.
    pub fn new(desktop_dirs: Vec<PathBuf>, unit_dirs: Vec<PathBuf>) -> Self {
        Self {
            desktop_dirs,
            unit_dirs,
            cache: HashMap::new(),
        }
    }

    /// The XDG data directories plus both Flatpak export directories, and the user and system
    /// unit directories (user first: app groups live under the user manager).
    pub fn from_env() -> Self {
        let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
        let data_home =
            env::var_os("XDG_DATA_HOME").map_or(home.join(".local/share"), PathBuf::from);
        let config_home =
            env::var_os("XDG_CONFIG_HOME").map_or(home.join(".config"), PathBuf::from);
        let data_dirs =
            env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());

        let mut shares = vec![data_home.clone(), data_home.join("flatpak/exports/share")];
        shares.extend(
            data_dirs
                .split(':')
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
        );
        shares.push("/var/lib/flatpak/exports/share".into());
        let mut desktop_dirs: Vec<PathBuf> = Vec::new();
        for dir in shares.iter().map(|s| s.join("applications")) {
            if !desktop_dirs.contains(&dir) {
                desktop_dirs.push(dir);
            }
        }

        let mut unit_dirs = Vec::new();
        if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
            unit_dirs.push(runtime.join("systemd/transient"));
            unit_dirs.push(runtime.join("systemd/user"));
        }
        unit_dirs.push(config_home.join("systemd/user"));
        unit_dirs.push(data_home.join("systemd/user"));
        for dir in [
            "/etc/systemd/user",
            "/usr/local/lib/systemd/user",
            "/usr/lib/systemd/user",
            "/run/systemd/transient",
            "/etc/systemd/system",
            "/run/systemd/system",
            "/usr/local/lib/systemd/system",
            "/usr/lib/systemd/system",
        ] {
            unit_dirs.push(dir.into());
        }
        Self::new(desktop_dirs, unit_dirs)
    }

    /// Name and icon for a group such as `…/app-flatpak-app.zen_browser.zen-1234.scope`,
    /// relative to the cgroup root. `main_process` is the full name of the group's main process
    /// ([`crate::processes::full_name`]), if known.
    pub fn name(&mut self, group: impl AsRef<Path>, main_process: Option<&str>) -> &AppName {
        let group = group.as_ref();
        let leaf = group.file_name().unwrap_or_default().to_string_lossy();
        if !self.cache.contains_key(leaf.as_ref()) {
            let resolved = self.resolve(group, &leaf, main_process);
            self.cache.insert(leaf.to_string(), resolved);
        }
        &self.cache[leaf.as_ref()]
    }

    fn resolve(&self, group: &Path, leaf: &str, main_process: Option<&str>) -> AppName {
        let cleaned = clean_name(leaf);
        let generic = |name: String| AppName {
            name,
            icon: SERVICE_ICON.into(),
            agents: false,
        };

        // An emulator's unit Description is its whole shell command; the instance says more.
        if main_process.is_some_and(|p| p.starts_with("qemu-system")) {
            let instance = cleaned.strip_prefix("emulator-").unwrap_or(&cleaned);
            return generic(format!("Android emulator · {instance}"));
        }
        if let Some(app) = app_id(leaf).and_then(|id| self.desktop_entry(&id)) {
            return app;
        }
        let Some(description) = self.unit_description(leaf) else {
            return generic(cleaned);
        };
        let Some(command) = description.strip_prefix("[systemd-run] ") else {
            return generic(description);
        };
        // The main process names the program better than the command, which may be a wrapper
        // script (`claude-t3` execs `claude`).
        let program = main_process
            .map(str::to_owned)
            .or_else(|| command_program(command))
            .map_or(cleaned, |p| clean_name(&p));
        if !in_agents_slice(group) {
            return generic(program);
        }
        let mut chars = program.chars();
        let capital: String = chars
            .next()
            .into_iter()
            .flat_map(char::to_uppercase)
            .collect();
        AppName {
            name: format!("{capital}{} agents", chars.as_str()),
            icon: AGENT_ICON.into(),
            agents: true,
        }
    }

    fn desktop_entry(&self, id: &str) -> Option<AppName> {
        let text = read_first(&self.desktop_dirs, &format!("{id}.desktop"))?;
        let name = group_value(&text, "Desktop Entry", "Name")?;
        let icon = group_value(&text, "Desktop Entry", "Icon").unwrap_or(SERVICE_ICON);
        Some(AppName {
            name: name.into(),
            icon: icon.into(),
            agents: false,
        })
    }

    /// `Description=` from the unit file, or from its template for `foo@instance.service`.
    fn unit_description(&self, leaf: &str) -> Option<String> {
        let mut files = vec![leaf.to_owned()];
        if let (Some((prefix, _)), Some(dot)) = (leaf.split_once('@'), leaf.rfind('.')) {
            files.push(format!("{prefix}@{}", &leaf[dot..]));
        }
        files.iter().find_map(|file| {
            let text = read_first(&self.unit_dirs, file)?;
            group_value(&text, "Unit", "Description").map(str::to_owned)
        })
    }
}

/// The program a `systemd-run` command line runs: the first word that isn't a launcher
/// (`setsid`, `env`, `nohup`), one of their options or a `VAR=value`, as a base name.
fn command_program(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .map(|word| word.trim_matches(['"', '\'']))
        .map(|word| word.rsplit('/').next().unwrap_or(word))
        .find(|word| {
            !word.is_empty()
                && !word.starts_with('-')
                && !word.contains('=')
                && !matches!(*word, "setsid" | "env" | "nohup")
        })
        .map(str::to_owned)
}

/// T3 Code puts each agent's unit in `app-t3code-agents-<id>.slice`.
fn in_agents_slice(group: &Path) -> bool {
    group
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|slice| {
            let name = clean_name(&slice.to_string_lossy());
            name == "agents" || name.ends_with("-agents")
        })
}

/// The leaf's name without unit suffix, `app-` and launcher prefixes, random or numeric tail
/// and systemd escapes: `app-flatpak-app.zen_browser.zen-1234.scope` → `app.zen_browser.zen`.
/// Other leaves also drop a template instance and a D-Bus activation prefix:
/// `dbus-:1.2-org.kde.kdeconnect@0.service` → `org.kde.kdeconnect`.
pub fn clean_name(leaf: &str) -> String {
    app_id(leaf).unwrap_or_else(|| {
        let name = strip_suffix(leaf);
        let name = name.split_once('@').map_or(name, |(base, _)| base);
        let name = name
            .strip_prefix("dbus-:")
            .and_then(|n| n.split_once('-'))
            .map_or(name, |(_, n)| n);
        unescape(strip_tail(name))
    })
}

/// The application id of an `app[-<launcher>]-<id>[-<random>].scope` or
/// `app[-<launcher>]-<id>[@<random>].service` leaf. Dashes inside the id are escaped as `\x2d`,
/// so an unescaped dash after `app-` ends the launcher.
fn app_id(leaf: &str) -> Option<String> {
    let rest = strip_suffix(leaf).strip_prefix("app-")?;
    let rest = match rest.split_once('@') {
        Some((before, _)) => before,
        None => strip_tail(rest),
    };
    let id = rest.split_once('-').map_or(rest, |(_, id)| id);
    Some(unescape(id))
}

fn strip_suffix(leaf: &str) -> &str {
    [".scope", ".service", ".slice"]
        .iter()
        .find_map(|s| leaf.strip_suffix(s))
        .unwrap_or(leaf)
}

/// Drops a `-<digits>` or `-<8+ hex digits>` tail; `emulator-guide35` keeps its instance.
fn strip_tail(name: &str) -> &str {
    match name.rsplit_once('-') {
        Some((head, tail))
            if !head.is_empty()
                && (tail.bytes().all(|b| b.is_ascii_digit())
                    || tail.len() >= 8 && tail.bytes().all(|b| b.is_ascii_hexdigit())) =>
        {
            head
        }
        _ => name,
    }
}

/// Undoes systemd's `\xNN` escaping.
fn unescape(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 2..i + 4)
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match hex {
            Some(byte) if bytes[i] == b'\\' && bytes[i + 1] == b'x' => {
                out.push(byte);
                i += 4;
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn read_first(dirs: &[PathBuf], file: &str) -> Option<String> {
    dirs.iter()
        .find_map(|dir| fs::read_to_string(Path::new(dir).join(file)).ok())
}

/// The unlocalized `key=` value in `[group]` of an INI-style file (desktop entry or unit).
fn group_value<'a>(text: &'a str, group: &str, key: &str) -> Option<&'a str> {
    let mut in_group = false;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_group = name == group;
        } else if in_group {
            match line.split_once('=') {
                Some((k, v)) if k.trim() == key => return Some(v.trim()).filter(|v| !v.is_empty()),
                _ => {}
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A desktop dir and a unit dir under a fresh temp dir, filled with `files`.
    fn fixture(test: &str, desktop: &[(&str, &str)], units: &[(&str, &str)]) -> Naming {
        let root = env::temp_dir().join(format!("sebwatch-naming-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (apps, unit_dir) = (root.join("applications"), root.join("units"));
        for (dir, files) in [(&apps, desktop), (&unit_dir, units)] {
            fs::create_dir_all(dir).unwrap();
            for (name, text) in files {
                fs::write(dir.join(name), text).unwrap();
            }
        }
        Naming::new(vec![apps], vec![unit_dir])
    }

    fn app(name: &str, icon: &str) -> AppName {
        AppName {
            name: name.into(),
            icon: icon.into(),
            agents: false,
        }
    }

    #[test]
    fn clean_name_cases() {
        for (leaf, name) in [
            ("app-org.kde.konsole-12345.scope", "org.kde.konsole"),
            (
                "app-flatpak-com.rtosta.zapzap-2091769723.scope",
                "com.rtosta.zapzap",
            ),
            ("app-steam@autostart.service", "steam"),
            (
                "app-dolphin@6ba726cf402f46af94a837f7786b6d55.service",
                "dolphin",
            ),
            ("emulator-guide35.service", "emulator-guide35"),
            (
                r"app-mount\x2dgdrive.sh@autostart.service",
                "mount-gdrive.sh",
            ),
            ("t3code-claude-b99dfcab.scope", "t3code-claude"),
            (
                "dbus-:1.2-org.kde.kdeconnect@0.service",
                "org.kde.kdeconnect",
            ),
            ("app-t3code.slice", "t3code"),
            (
                r"app-appimagekit_ac41f740be554e532a3d39ba8e8b3233\x2dT3_Code__Beta_@45bba9a0.service",
                "appimagekit_ac41f740be554e532a3d39ba8e8b3233-T3_Code__Beta_",
            ),
        ] {
            assert_eq!(clean_name(leaf), name, "{leaf}");
        }
    }

    #[test]
    fn desktop_and_flatpak_ids_use_desktop_file() {
        let mut naming = fixture(
            "desktop",
            &[
                (
                    "app.zen_browser.zen.desktop",
                    "[Desktop Entry]\nName[de]=Zen-Browser\nName=Zen Browser\nIcon=app.zen_browser.zen\n\n[Desktop Action new]\nName=New Window\n",
                ),
                (
                    "net.local.spectacle-2.desktop",
                    "[Desktop Entry]\nName=Spectacle\nIcon=spectacle\n",
                ),
                ("discord.desktop", "[Desktop Entry]\nName=Discord\n"),
            ],
            &[],
        );
        let zen = app("Zen Browser", "app.zen_browser.zen");
        assert_eq!(
            naming.name("app-flatpak-app.zen_browser.zen-917865268.scope", None),
            &zen
        );
        assert_eq!(
            naming.name(
                "app-app.zen_browser.zen@51f9373e1d1a41259755394da85f7512.service",
                None
            ),
            &zen
        );
        let spectacle = app("Spectacle", "spectacle");
        assert_eq!(
            naming.name(r"app-net.local.spectacle\x2d2@648a8b9c.service", None),
            &spectacle
        );
        assert_eq!(
            naming.name("app-discord@autostart.service", None),
            &app("Discord", SERVICE_ICON)
        );
    }

    #[test]
    fn unit_description_then_cleaned_name() {
        let mut naming = fixture(
            "unit",
            &[],
            &[
                (
                    "pipewire.service",
                    "[Unit]\nDescription=PipeWire Multimedia Service\n",
                ),
                (
                    "app-dolphin@.service",
                    "[Unit]\nDescription=Dolphin\n[Service]\nDescription=no\n",
                ),
            ],
        );
        assert_eq!(
            naming.name("pipewire.service", None),
            &app("PipeWire Multimedia Service", SERVICE_ICON)
        );
        assert_eq!(
            naming.name("app-dolphin@6ba726cf.service", None),
            &app("Dolphin", SERVICE_ICON)
        );
        assert_eq!(
            naming.name("t3code-terminal-b67c9b1b.scope", None),
            &app("t3code-terminal", SERVICE_ICON)
        );
        assert_eq!(
            naming.name("app-sebchat-1794774551612041860.scope", None),
            &app("sebchat", SERVICE_ICON)
        );
    }

    #[test]
    fn emulator_is_named_by_instance_not_description() {
        let mut naming = fixture(
            "emulator",
            &[],
            &[(
                "emulator-guide35.service",
                "[Unit]\nDescription=[systemd-run] /usr/bin/sh -c \"exec emulator -avd guide35\"\n",
            )],
        );
        let name = naming.name("emulator-guide35.service", Some("qemu-system-x86"));
        assert_eq!(name, &app("Android emulator · guide35", SERVICE_ICON));
    }

    #[test]
    fn systemd_run_units_are_named_by_their_program() {
        let claude = "[Unit]\nDescription=[systemd-run] /home/u/.local/bin/claude-t3 --model=x \"--allowedTools=a b\"\n";
        let mut naming = fixture(
            "systemd-run",
            &[],
            &[
                ("t3code-claude-039d3df7.scope", claude),
                ("t3code-claude-265ff82f.scope", claude),
                (
                    "t3code-opencode-116db5a8.scope",
                    "[Unit]\nDescription=[systemd-run] /home/u/.opencode/bin/opencode serve --port=1\n",
                ),
                (
                    "run-u42.service",
                    "[Unit]\nDescription=[systemd-run] /usr/bin/setsid -f env A=1 /usr/bin/sebchat --night\n",
                ),
            ],
        );
        let agents = "app.slice/app-t3code.slice/app-t3code-agents.slice/app-t3code-agents-abaa00bbacbc.slice";
        let agent = |name: &str| AppName {
            name: name.into(),
            icon: AGENT_ICON.into(),
            agents: true,
        };
        // The main process wins over the wrapper script in the command line.
        assert_eq!(
            naming.name(
                format!("{agents}/t3code-claude-039d3df7.scope"),
                Some("claude")
            ),
            &agent("Claude agents")
        );
        // Without a readable main process: the command's program.
        assert_eq!(
            naming.name(format!("{agents}/t3code-claude-265ff82f.scope"), None),
            &agent("Claude-t3 agents")
        );
        assert_eq!(
            naming.name(format!("{agents}/t3code-opencode-116db5a8.scope"), None),
            &agent("Opencode agents")
        );
        // Outside an agents slice: the program's name, launchers skipped.
        assert_eq!(
            naming.name("app.slice/run-u42.service", None),
            &app("sebchat", SERVICE_ICON)
        );
    }
}
