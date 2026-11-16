//! The desktop's light/dark preference: the freedesktop portal's
//! `org.freedesktop.appearance color-scheme` (0 no preference, 1 dark, 2 light).

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "color-scheme";

fn settings(connection: &Connection) -> zbus::Result<Proxy<'_>> {
    Proxy::new(
        connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
}

fn is_dark(value: &Value) -> bool {
    match value {
        // Some portals wrap the value in one more variant.
        Value::Value(inner) => is_dark(inner),
        Value::U32(scheme) => *scheme == 1,
        _ => false,
    }
}

/// Whether the desktop prefers dark now; false without a portal.
pub fn dark() -> bool {
    let read = || -> zbus::Result<bool> {
        let connection = Connection::session()?;
        let value: OwnedValue = settings(&connection)?.call("ReadOne", &(NAMESPACE, KEY))?;
        Ok(is_dark(&value))
    };
    read().unwrap_or_else(|error| {
        eprintln!("sebwatch: no desktop colour scheme from the portal: {error}");
        false
    })
}

/// Calls `changed` on a background thread each time the desktop switches between light and dark.
pub fn watch(changed: impl Fn(bool) + Send + 'static) {
    std::thread::spawn(move || {
        let listen = || -> zbus::Result<()> {
            let connection = Connection::session()?;
            for signal in settings(&connection)?.receive_signal("SettingChanged")? {
                let (namespace, key, value): (String, String, OwnedValue) =
                    signal.body().deserialize()?;
                if namespace == NAMESPACE && key == KEY {
                    changed(is_dark(&value));
                }
            }
            Ok(())
        };
        if let Err(error) = listen() {
            eprintln!("sebwatch: stopped following the desktop colour scheme: {error}");
        }
    });
}
