//! The chosen theme, kept in `$XDG_CONFIG_HOME/sebwatch/settings` as `theme=<name>`.

use std::path::PathBuf;

use crate::ThemeChoice;

fn path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("sebwatch/settings"))
}

const NAMES: [(ThemeChoice, &str); 4] = [
    (ThemeChoice::System, "system"),
    (ThemeChoice::Light, "light"),
    (ThemeChoice::Dark, "dark"),
    (ThemeChoice::Sebchat, "sebchat"),
];

fn parse(text: &str) -> ThemeChoice {
    let name = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("theme="))
        .unwrap_or_default();
    NAMES
        .iter()
        .find(|(_, n)| *n == name.trim())
        .map_or(ThemeChoice::System, |(choice, _)| *choice)
}

fn format(choice: ThemeChoice) -> String {
    let name = NAMES
        .iter()
        .find(|(c, _)| *c == choice)
        .map_or("system", |(_, n)| n);
    format!("theme={name}\n")
}

/// System when nothing is saved yet.
pub fn theme() -> ThemeChoice {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map_or(ThemeChoice::System, |text| parse(&text))
}

pub fn save_theme(choice: ThemeChoice) {
    let Some(path) = path() else { return };
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, format(choice)));
    if let Err(error) = saved {
        eprintln!(
            "sebwatch: could not save the theme to {}: {error}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_round_trips_and_defaults_to_system() {
        for (choice, _) in NAMES {
            assert_eq!(parse(&format(choice)), choice);
        }
        assert_eq!(parse(""), ThemeChoice::System);
        assert_eq!(parse("theme=neon\n"), ThemeChoice::System);
    }
}
