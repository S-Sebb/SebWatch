//! KDE Plasma: one SebWatch at a time, a second start raising the first one's window, and the
//! Win+Esc shortcut, taken over from System Monitor and given back.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use zbus::blocking::{Connection, Proxy};
use zbus::fdo::RequestNameFlags;

/// Owned while SebWatch runs, so a second start finds it.
const NAME: &str = "io.github.S_Sebb.SebWatch";
/// Meta+Esc as Qt's key code: Qt::META | Qt::Key_Escape.
const META_ESC: i32 = 0x1000_0000 | 0x0100_0000;
/// KGlobalAccel's action ids: component, action, and their names shown in System Settings.
const SYSTEM_MONITOR: &[&str] = &[
    "org.kde.plasma-systemmonitor.desktop",
    "_launch",
    "System Monitor",
    "System Monitor",
];
const SEBWATCH: &[&str] = &["sebwatch.desktop", "_launch", "SebWatch", "SebWatch"];
/// Marks the launcher entry `--bind-win-esc` writes for an AppImage, so unbinding removes only that one.
const APPIMAGE_MARK: &str = "X-SebWatch-AppImage=true";

/// The connection that owns `NAME`, kept open until the app exits.
static OWNER: OnceLock<Connection> = OnceLock::new();

/// Whether this is the only SebWatch running. If another one runs, its window is raised and this
/// returns false. Without a session bus every start runs on its own.
pub fn first_instance() -> bool {
    let claim = || -> zbus::Result<bool> {
        let connection = Connection::session()?;
        match connection.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into()) {
            Err(zbus::Error::NameTaken) => {
                if let Err(error) = raise(&connection) {
                    eprintln!("sebwatch: already running, but cannot raise its window: {error}");
                }
                Ok(false)
            }
            Err(error) => Err(error),
            Ok(_) => {
                let _ = OWNER.set(connection);
                Ok(true)
            }
        }
    };
    claim().unwrap_or_else(|error| {
        eprintln!("sebwatch: no session bus, not checking for another SebWatch: {error}");
        true
    })
}

/// Raises the running SebWatch's window with a KWin script: Wayland lets no app raise another's
/// window, and KWin's script API activates it past focus-stealing prevention.
fn raise(connection: &Connection) -> zbus::Result<()> {
    let script = std::env::temp_dir().join("sebwatch-raise.js");
    std::fs::write(
        &script,
        "for (const w of workspace.windowList()) {\n\
         \x20   if (w.resourceClass === \"sebwatch\") {\n\
         \x20       w.minimized = false;\n\
         \x20       workspace.activeWindow = w;\n\
         \x20   }\n\
         }\n",
    )
    .map_err(|error| zbus::Error::Failure(error.to_string()))?;
    let scripting = Proxy::new(
        connection,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )?;
    let plugin = "sebwatch-raise";
    // A copy left loaded by an earlier raise would make loading fail.
    let _: bool = scripting.call("unloadScript", &(plugin,))?;
    let id: i32 = scripting.call("loadScript", &(script.to_string_lossy().as_ref(), plugin))?;
    let path = format!("/Scripting/Script{id}");
    let run = Proxy::new(
        connection,
        "org.kde.KWin",
        path.as_str(),
        "org.kde.kwin.Script",
    )
    .and_then(|script| script.call::<_, _, ()>("run", &()));
    let _: bool = scripting.call("unloadScript", &(plugin,))?;
    let _ = std::fs::remove_file(script);
    run
}

fn global_accel(connection: &Connection) -> zbus::Result<Proxy<'_>> {
    Proxy::new(
        connection,
        "org.kde.kglobalaccel",
        "/kglobalaccel",
        "org.kde.KGlobalAccel",
    )
}

/// Gives Win+Esc to SebWatch and takes it from System Monitor, which stays in the app menu.
pub fn bind_win_esc() -> zbus::Result<()> {
    if let Some((appimage, appdir)) = own_appimage() {
        add_appimage_launcher(&appimage, &appdir)
            .map_err(|error| zbus::Error::Failure(error.to_string()))?;
    }
    let connection = Connection::session()?;
    let accel = global_accel(&connection)?;
    let mut keys: Vec<i32> = accel.call("shortcut", &(SYSTEM_MONITOR,))?;
    keys.retain(|&key| key != META_ESC);
    accel.call::<_, _, ()>("setForeignShortcut", &(SYSTEM_MONITOR, keys))?;
    accel.call::<_, _, ()>("doRegister", &(SEBWATCH,))?;
    accel.call::<_, _, ()>("setForeignShortcut", &(SEBWATCH, vec![META_ESC]))?;
    Ok(())
}

/// Takes Win+Esc from SebWatch and gives it back to System Monitor, unless something else has
/// taken it since.
pub fn unbind_win_esc() -> zbus::Result<()> {
    let connection = Connection::session()?;
    let accel = global_accel(&connection)?;
    accel.call::<_, _, ()>("setForeignShortcut", &(SEBWATCH, Vec::<i32>::new()))?;
    let _: bool = accel.call("unregister", &(SEBWATCH[0], SEBWATCH[1]))?;
    let holders: Vec<String> = accel.call("action", &(META_ESC,))?;
    if holders.is_empty() {
        let mut keys: Vec<i32> = accel.call("shortcut", &(SYSTEM_MONITOR,))?;
        keys.push(META_ESC);
        accel.call::<_, _, ()>("setForeignShortcut", &(SYSTEM_MONITOR, keys))?;
    }
    remove_appimage_launcher().map_err(|error| zbus::Error::Failure(error.to_string()))
}

/// The user's launcher entry and icon, where an AppImage's go.
fn user_files() -> (PathBuf, PathBuf) {
    let data = std::env::var_os("XDG_DATA_HOME").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"),
        PathBuf::from,
    );
    (
        data.join("applications/sebwatch.desktop"),
        data.join("icons/hicolor/256x256/apps/sebwatch.png"),
    )
}

/// The AppImage this binary runs from and where it is mounted. Both variables are inherited by
/// whatever an AppImage starts, so they count only when this binary is inside that mount.
fn own_appimage() -> Option<(String, PathBuf)> {
    let appimage = std::env::var("APPIMAGE").ok()?;
    let appdir = PathBuf::from(std::env::var_os("APPDIR")?);
    std::env::current_exe()
        .ok()?
        .starts_with(&appdir)
        .then_some((appimage, appdir))
}

/// A launcher entry for the AppImage at `appimage`, with its icon, so the shortcut has an entry
/// to start: an AppImage installs none.
fn add_appimage_launcher(appimage: &str, appdir: &Path) -> std::io::Result<()> {
    let (launcher, icon) = user_files();
    std::fs::create_dir_all(icon.parent().expect("a folder"))?;
    std::fs::copy(
        appdir.join("usr/share/icons/hicolor/256x256/apps/sebwatch.png"),
        icon,
    )?;
    std::fs::create_dir_all(launcher.parent().expect("a folder"))?;
    let entry = include_str!("../../../packaging/sebwatch.desktop").replace(
        "Exec=sebwatch",
        &format!("Exec=\"{appimage}\"\n{APPIMAGE_MARK}"),
    );
    std::fs::write(launcher, entry)
}

/// Removes the launcher entry and icon `add_appimage_launcher` wrote, if it wrote them.
fn remove_appimage_launcher() -> std::io::Result<()> {
    let (launcher, icon) = user_files();
    match std::fs::read_to_string(&launcher) {
        Ok(entry) if entry.contains(APPIMAGE_MARK) => {
            let _ = std::fs::remove_file(icon);
            std::fs::remove_file(launcher)
        }
        _ => Ok(()),
    }
}
