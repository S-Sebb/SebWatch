//! The chosen theme, table order and window size, kept in `$XDG_CONFIG_HOME/sebwatch/settings`
//! as `theme=<name>`, `sort=<column>[ ascending]` and `size=<width>x<height>` lines (logical
//! pixels).

use std::path::PathBuf;

use crate::ThemeChoice;
use crate::model::{Column, Sort};

fn path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("sebwatch/settings"))
}

const THEMES: [(ThemeChoice, &str); 4] = [
    (ThemeChoice::System, "system"),
    (ThemeChoice::Light, "light"),
    (ThemeChoice::Dark, "dark"),
    (ThemeChoice::Sebchat, "sebchat"),
];

const COLUMNS: [(Column, &str); 8] = [
    (Column::Ram, "ram"),
    (Column::Swap, "swap"),
    (Column::Cache, "cache"),
    (Column::Cpu, "cpu"),
    (Column::Disk, "disk"),
    (Column::Igpu, "igpu"),
    (Column::Gpu, "gpu"),
    (Column::Vram, "vram"),
];

fn read() -> String {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
}

fn get<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
        .unwrap_or_default()
        .trim()
}

/// The text with `key` set to `value`, other lines kept.
fn set(text: &str, key: &str, value: &str) -> String {
    let mut out: String = text
        .lines()
        .filter(|line| !line.trim().starts_with(&format!("{key}=")))
        .map(|line| format!("{line}\n"))
        .collect();
    out.push_str(&format!("{key}={value}\n"));
    out
}

fn save(key: &str, value: &str) {
    let Some(path) = path() else { return };
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, set(&read(), key, value)));
    if let Err(error) = saved {
        eprintln!(
            "sebwatch: could not save the {key} to {}: {error}",
            path.display()
        );
    }
}

fn parse_theme(text: &str) -> ThemeChoice {
    let name = get(text, "theme");
    THEMES
        .iter()
        .find(|(_, n)| *n == name)
        .map_or(ThemeChoice::System, |(choice, _)| *choice)
}

fn format_theme(choice: ThemeChoice) -> &'static str {
    THEMES
        .iter()
        .find(|(c, _)| *c == choice)
        .map_or("system", |(_, n)| n)
}

/// RAM, largest first, when nothing is saved yet.
fn parse_sort(text: &str) -> Sort {
    let mut words = get(text, "sort").split_whitespace();
    let name = words.next().unwrap_or_default();
    match COLUMNS.iter().find(|(_, n)| *n == name) {
        Some((column, _)) => Sort {
            column: *column,
            ascending: words.next() == Some("ascending"),
        },
        None => Sort::default(),
    }
}

fn format_sort(sort: Sort) -> String {
    let name = COLUMNS
        .iter()
        .find(|(c, _)| *c == sort.column)
        .map_or("ram", |(_, n)| n);
    if sort.ascending {
        format!("{name} ascending")
    } else {
        name.into()
    }
}

/// None when nothing is saved yet. The window keeps its own minimum size.
fn parse_size(text: &str) -> Option<(f32, f32)> {
    let (width, height) = get(text, "size").split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

fn format_size((width, height): (f32, f32)) -> String {
    format!("{}x{}", width.round(), height.round())
}

pub fn size() -> Option<(f32, f32)> {
    parse_size(&read())
}

pub fn save_size(size: (f32, f32)) {
    save("size", &format_size(size));
}

/// System when nothing is saved yet.
pub fn theme() -> ThemeChoice {
    parse_theme(&read())
}

pub fn save_theme(choice: ThemeChoice) {
    save("theme", format_theme(choice));
}

pub fn sort() -> Sort {
    parse_sort(&read())
}

pub fn save_sort(sort: Sort) {
    save("sort", &format_sort(sort));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_round_trips_and_defaults_to_system() {
        for (choice, _) in THEMES {
            assert_eq!(parse_theme(&set("", "theme", format_theme(choice))), choice);
        }
        assert_eq!(parse_theme(""), ThemeChoice::System);
        assert_eq!(parse_theme("theme=neon\n"), ThemeChoice::System);
    }

    #[test]
    fn sort_round_trips_beside_the_theme() {
        let text = set("theme=dark\n", "sort", "ram");
        for (column, _) in COLUMNS {
            for ascending in [false, true] {
                let sort = Sort { column, ascending };
                let saved = set(&text, "sort", &format_sort(sort));
                assert_eq!(parse_sort(&saved), sort);
                assert_eq!(parse_theme(&saved), ThemeChoice::Dark);
                assert_eq!(saved.lines().count(), 2);
            }
        }
        assert_eq!(parse_sort("theme=dark\n"), Sort::default());
        assert_eq!(parse_sort("sort=pixels\n"), Sort::default());
    }

    #[test]
    fn size_round_trips_beside_the_other_settings() {
        let text = set(
            "theme=dark\nsort=ram\n",
            "size",
            &format_size((1203.4, 911.6)),
        );
        assert_eq!(parse_size(&text), Some((1203.0, 912.0)));
        assert_eq!(parse_theme(&text), ThemeChoice::Dark);
        assert_eq!(text.lines().count(), 3);
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("size=wide\n"), None);
    }
}
