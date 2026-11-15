//! Friendly name and icon name for a cgroup leaf (see docs/accounting.md, "Apps are cgroups").
//!
//! Order: a running Android emulator gets its own name; then the `.desktop` file of the desktop
//! or Flatpak id in the leaf name; then the unit file's `Description=`; then the cleaned name.
//! Everything is read from files, once per leaf name.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Icon for anything without a desktop file (background services, terminals, transient units).
pub const SERVICE_ICON: &str = "application-x-executable";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppName {
    pub name: String,
    /// Icon theme name or absolute path, as the desktop file gives it.
    pub icon: String,
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

    /// Name and icon for a leaf such as `app-flatpak-app.zen_browser.zen-1234.scope`.
    /// `main_process` is the `comm` of the group's main process, if known.
    pub fn name(&mut self, leaf: &str, main_process: Option<&str>) -> &AppName {
        if !self.cache.contains_key(leaf) {
            let resolved = self.resolve(leaf, main_process);
            self.cache.insert(leaf.to_owned(), resolved);
        }
        &self.cache[leaf]
    }

    fn resolve(&self, leaf: &str, main_process: Option<&str>) -> AppName {
        let cleaned = clean_name(leaf);
        let generic = |name: String| AppName {
            name,
            icon: SERVICE_ICON.into(),
        };

        // An emulator's unit Description is its whole shell command; the instance says more.
        if main_process.is_some_and(|p| p.starts_with("qemu-system")) {
            let instance = cleaned.strip_prefix("emulator-").unwrap_or(&cleaned);
            return generic(format!("Android emulator · {instance}"));
        }
        if let Some(app) = app_id(leaf).and_then(|id| self.desktop_entry(&id)) {
            return app;
        }
        match self.unit_description(leaf) {
            Some(description) => generic(description),
            None => generic(cleaned),
        }
    }

    fn desktop_entry(&self, id: &str) -> Option<AppName> {
        let text = read_first(&self.desktop_dirs, &format!("{id}.desktop"))?;
        let name = group_value(&text, "Desktop Entry", "Name")?;
        let icon = group_value(&text, "Desktop Entry", "Icon").unwrap_or(SERVICE_ICON);
        Some(AppName {
            name: name.into(),
            icon: icon.into(),
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

/// The leaf's name without unit suffix, `app-` and launcher prefixes, random or numeric tail
/// and systemd escapes: `app-flatpak-app.zen_browser.zen-1234.scope` → `app.zen_browser.zen`.
pub fn clean_name(leaf: &str) -> String {
    app_id(leaf).unwrap_or_else(|| unescape(strip_tail(strip_suffix(leaf))))
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
}
