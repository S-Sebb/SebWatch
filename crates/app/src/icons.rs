//! App icons from the desktop's icon theme: an absolute path as is, else the name looked up in
//! the user's theme (kdeglobals), Breeze, hicolor and pixmaps, else a generic app icon shipped
//! in the app. Loaded once per name.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use slint::Image;

#[derive(Default)]
pub struct Icons {
    loaded: HashMap<String, Image>,
}

impl Icons {
    pub fn get(&mut self, icon: &str) -> Image {
        self.loaded
            .entry(icon.to_string())
            .or_insert_with(|| {
                find(icon)
                    .and_then(|path| Image::load_from_path(&path).ok())
                    .unwrap_or_else(generic)
            })
            .clone()
    }
}

/// A gear on a grey square, for apps and services without a usable icon.
fn generic() -> Image {
    Image::load_from_svg_data(include_bytes!("../icons/generic-app.svg")).expect("a valid SVG")
}

fn find(icon: &str) -> Option<PathBuf> {
    if icon.starts_with('/') {
        return Some(PathBuf::from(icon)).filter(|p| usable(p));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let data_home =
        std::env::var_os("XDG_DATA_HOME").map_or(home.join(".local/share"), PathBuf::from);
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or("/usr/local/share:/usr/share".into());
    let mut bases = vec![home.join(".icons"), data_home.join("icons")];
    bases.extend(data_dirs.split(':').map(|d| Path::new(d).join("icons")));

    let mut themes = Vec::new();
    if let Some(theme) = user_theme(&home) {
        themes.push(theme);
    }
    themes.extend(["breeze".into(), "hicolor".into()]);
    for theme in &themes {
        for base in &bases {
            if let Some(path) = in_theme(&base.join(theme), icon) {
                return Some(path);
            }
        }
    }
    ["svg", "png"]
        .iter()
        .map(|ext| Path::new("/usr/share/pixmaps").join(format!("{icon}.{ext}")))
        .find(|p| p.exists())
}

/// `[Icons] Theme=` in kdeglobals.
fn user_theme(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(home.join(".config/kdeglobals")).ok()?;
    let section = text.split("\n[").find(|s| s.starts_with("Icons]"))?;
    section
        .lines()
        .find_map(|line| line.strip_prefix("Theme="))
        .map(str::to_string)
}

/// The icon in a theme laid out as size/category or category/size: one from an `apps` folder
/// first, then scalable, then the largest bitmap.
fn in_theme(theme: &Path, icon: &str) -> Option<PathBuf> {
    let mut best: Option<((bool, u32), PathBuf)> = None;
    for outer in std::fs::read_dir(theme).ok()?.flatten() {
        for inner in std::fs::read_dir(outer.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            for ext in ["svg", "png"] {
                let path = inner.path().join(format!("{icon}.{ext}"));
                if !usable(&path) {
                    continue;
                }
                let apps = path.components().any(|c| c.as_os_str() == "apps");
                let size = if ext == "svg" {
                    u32::MAX
                } else {
                    size_of(&path)
                };
                let size = (apps, size);
                if best.as_ref().is_none_or(|(s, _)| size > *s) {
                    best = Some((size, path));
                }
            }
        }
    }
    best.map(|(_, path)| path)
}

/// Whether the file draws a full-colour icon. Not so: missing files; symbolic SVGs, drawn in the
/// desktop's text colour (e.g. Fluent-dark's white `utilities-terminal`, invisible on a light
/// background); SVGs with embedded bitmaps, which Slint's SVG renderer leaves out (Fluent's Steam).
fn usable(path: &Path) -> bool {
    if path.extension().is_none_or(|e| e != "svg") {
        return path.exists();
    }
    std::fs::read_to_string(path).is_ok_and(|svg| {
        !["ColorScheme-Text", "currentColor", "<image"]
            .iter()
            .any(|mark| svg.contains(mark))
    })
}

/// The size from a directory such as `48x48` or `48`, 0 if none.
fn size_of(path: &Path) -> u32 {
    path.components()
        .filter_map(|c| c.as_os_str().to_str()?.split('x').next()?.parse().ok())
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_a_full_colour_app_icon() {
        let theme = std::env::temp_dir().join(format!("sebwatch-icons-{}", std::process::id()));
        let write = |path: &str, svg: &str| {
            let path = theme.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, svg).unwrap();
        };
        let symbolic = r#"<svg><path class="ColorScheme-Text" style="fill:currentColor"/></svg>"#;
        let embedded = r#"<svg><image href="data:image/png;base64,AAAA"/></svg>"#;
        write("16/actions/term.svg", "<svg/>");
        write("scalable/apps/term.svg", "<svg/>");
        write("scalable/apps/dark.svg", symbolic);
        write("scalable/apps/game.svg", embedded);
        assert_eq!(
            in_theme(&theme, "term"),
            Some(theme.join("scalable/apps/term.svg"))
        );
        assert_eq!(in_theme(&theme, "dark"), None);
        assert_eq!(in_theme(&theme, "game"), None);
        std::fs::remove_dir_all(&theme).unwrap();
    }
}
