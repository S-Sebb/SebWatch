//! App icons from the desktop's icon theme: an absolute path as is, else the name looked up in
//! the user's theme (kdeglobals) and the themes it inherits, Breeze, hicolor and pixmaps, else a
//! generic app icon shipped in the app. Core's generic service icon also gets the shipped one:
//! themes draw it as a file type (Breeze: a pale box with "!"), not an app. A bitmap is taken at
//! the size the row shows in device pixels, as Plasma does, and shrunk to it by averaging: the
//! renderer samples it bilinearly without mipmaps, so a 256 px icon drawn at 22 px looks harsh.
//! Loaded once per name and scale.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sebwatch_core::naming::SERVICE_ICON;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

/// The row's icon size in logical pixels (`main.slint`).
const ROW_ICON: f32 = 22.0;

pub struct Icons {
    loaded: HashMap<String, Image>,
    /// The row's icon size in device pixels.
    px: u32,
}

impl Default for Icons {
    fn default() -> Self {
        Icons {
            loaded: HashMap::new(),
            px: ROW_ICON as u32,
        }
    }
}

impl Icons {
    /// Follows the window's scale factor, loading the icons again when it changes.
    pub fn scale(&mut self, scale_factor: f32) {
        let px = (ROW_ICON * scale_factor).round() as u32;
        if px != self.px {
            self.px = px;
            self.loaded.clear();
        }
    }

    pub fn get(&mut self, icon: &str) -> Image {
        let px = self.px;
        self.loaded
            .entry(icon.to_string())
            .or_insert_with(|| {
                find(icon, px)
                    .and_then(|path| load(&path, px))
                    .unwrap_or_else(generic)
            })
            .clone()
    }
}

/// The image at `path`, a bitmap larger than `px` square shrunk to fit it.
fn load(path: &Path, px: u32) -> Option<Image> {
    let image = Image::load_from_path(path).ok()?;
    if path.extension().is_some_and(|e| e == "svg") {
        return Some(image);
    }
    let size = image.size();
    let scale = px as f32 / size.width.max(size.height) as f32;
    if scale >= 1.0 {
        return Some(image);
    }
    let width = ((size.width as f32 * scale).round() as u32).max(1);
    let height = ((size.height as f32 * scale).round() as u32).max(1);
    let pixels = image.to_rgba8_premultiplied()?;
    Some(Image::from_rgba8_premultiplied(shrink(
        &pixels, width, height,
    )))
}

/// `pixels` shrunk to `width` × `height`, each pixel the average of the area it covers.
/// Premultiplied, so transparent pixels don't darken the edges.
fn shrink(
    pixels: &SharedPixelBuffer<Rgba8Pixel>,
    width: u32,
    height: u32,
) -> SharedPixelBuffer<Rgba8Pixel> {
    let (src_w, src_h) = (pixels.width() as usize, pixels.height() as usize);
    let src: Vec<[f32; 4]> = pixels
        .as_slice()
        .iter()
        .map(|p| [p.r, p.g, p.b, p.a].map(f32::from))
        .collect();
    // Rows first, then columns.
    let wide = average(&src, src_w, src_h, width as usize, 1, src_w);
    let out = average(
        &wide,
        src_h,
        width as usize,
        height as usize,
        width as usize,
        1,
    );
    let mut buffer = SharedPixelBuffer::new(width, height);
    for (p, [r, g, b, a]) in buffer.make_mut_slice().iter_mut().zip(out) {
        *p = Rgba8Pixel::new(
            r.round() as u8,
            g.round() as u8,
            b.round() as u8,
            a.round() as u8,
        );
    }
    buffer
}

/// Shrinks `lines` lines of `len` values each to `new_len` values, by area. A line's values are
/// `step` apart and lines `stride` apart; the result keeps the layout with `new_len` in place of
/// `len`.
fn average(
    src: &[[f32; 4]],
    len: usize,
    lines: usize,
    new_len: usize,
    step: usize,
    stride: usize,
) -> Vec<[f32; 4]> {
    let ratio = len as f32 / new_len as f32;
    let (out_step, out_stride) = if step == 1 { (1, new_len) } else { (step, 1) };
    let mut out = vec![[0.0; 4]; new_len * lines];
    for line in 0..lines {
        for i in 0..new_len {
            let (start, end) = (i as f32 * ratio, (i + 1) as f32 * ratio);
            let mut sum = [0.0; 4];
            for j in start as usize..(end.ceil() as usize).min(len) {
                let weight = (end.min(j as f32 + 1.0) - start.max(j as f32)).max(0.0);
                let value = src[line * stride + j * step];
                for c in 0..4 {
                    sum[c] += value[c] * weight;
                }
            }
            out[line * out_stride + i * out_step] = sum.map(|v| v / ratio);
        }
    }
    out
}

/// A gear on a grey square, for apps and services without a usable icon.
fn generic() -> Image {
    Image::load_from_svg_data(include_bytes!("../icons/generic-app.svg")).expect("a valid SVG")
}

fn find(icon: &str, px: u32) -> Option<PathBuf> {
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
        inherited(&theme, &bases, &mut themes);
    }
    for theme in ["breeze", "hicolor"] {
        if !themes.iter().any(|t| t == theme) {
            themes.push(theme.into());
        }
    }
    lookup(icon, px, &bases, &themes)
}

/// `theme` and the themes its `index.theme` inherits, depth first, each once.
fn inherited(theme: &str, bases: &[PathBuf], themes: &mut Vec<String>) {
    if themes.iter().any(|t| t == theme) {
        return;
    }
    themes.push(theme.into());
    let index = bases
        .iter()
        .find_map(|base| std::fs::read_to_string(base.join(theme).join("index.theme")).ok());
    let parents = index.as_deref().and_then(|text| {
        text.lines()
            .find_map(|line| line.strip_prefix("Inherits="))
            .map(str::to_string)
    });
    for parent in parents.iter().flat_map(|p| p.split(',')) {
        inherited(parent.trim(), bases, themes);
    }
}

/// `icon` at `px` device pixels in the themes under the bases, in order, then in pixmaps; `None`
/// for core's service icon, so an app without an icon of its own gets the shipped one.
fn lookup(icon: &str, px: u32, bases: &[PathBuf], themes: &[String]) -> Option<PathBuf> {
    if icon == SERVICE_ICON {
        return None;
    }
    if icon.starts_with('/') {
        return Some(PathBuf::from(icon)).filter(|p| usable(p));
    }
    for theme in themes {
        for base in bases {
            if let Some(path) = in_theme(&base.join(theme), icon, px) {
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

/// The icon in a theme laid out as size/category or category/size, as Plasma would draw it at
/// `px` device pixels: one from an `apps` folder first, then scalable, then the smallest bitmap
/// at least `px`, then the largest smaller one.
fn in_theme(theme: &Path, icon: &str, px: u32) -> Option<PathBuf> {
    let mut best: Option<((bool, i64), PathBuf)> = None;
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
                let size = size_of(&path) as i64;
                let fit = match () {
                    _ if ext == "svg" => i64::MAX,
                    _ if size >= px as i64 => i64::MAX / 2 - size,
                    _ => size,
                };
                let rank = (apps, fit);
                if best.as_ref().is_none_or(|(r, _)| rank > *r) {
                    best = Some((rank, path));
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

/// The size in device pixels from a directory such as `48x48`, `48` or `48@2x` (96), 0 if none.
fn size_of(path: &Path) -> u32 {
    path.components()
        .filter_map(|c| {
            let name = c.as_os_str().to_str()?;
            let (size, scale) = name.split_once('@').unwrap_or((name, "1x"));
            let size: u32 = size.split('x').next()?.parse().ok()?;
            Some(size * scale.strip_suffix('x')?.parse::<u32>().ok()?)
        })
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
            in_theme(&theme, "term", 22),
            Some(theme.join("scalable/apps/term.svg"))
        );
        assert_eq!(in_theme(&theme, "dark", 22), None);
        assert_eq!(in_theme(&theme, "game", 22), None);
        std::fs::remove_dir_all(&theme).unwrap();
    }

    #[test]
    fn takes_the_bitmap_plasma_would_draw_at_the_rows_size() {
        // Steam's hicolor PNGs, and a 16 px one for a 2x screen.
        let theme = std::env::temp_dir().join(format!("sebwatch-sizes-{}", std::process::id()));
        for dir in ["16x16", "24x24", "256x256", "16x16@2x"] {
            let path = theme.join(dir).join("apps/steam.png");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        let found = |px| in_theme(&theme, "steam", px).unwrap();
        // The smallest at least as large; the largest when all are smaller.
        assert_eq!(found(22), theme.join("24x24/apps/steam.png"));
        assert_eq!(found(32), theme.join("16x16@2x/apps/steam.png"));
        assert_eq!(found(44), theme.join("256x256/apps/steam.png"));
        assert_eq!(found(512), theme.join("256x256/apps/steam.png"));
        std::fs::remove_dir_all(&theme).unwrap();
    }

    #[test]
    fn shrinks_by_averaging() {
        // 4 × 2: two opaque white pixels, two clear ones, then two opaque black, two clear.
        let white = Rgba8Pixel::new(255, 255, 255, 255);
        let clear = Rgba8Pixel::new(0, 0, 0, 0);
        let black = Rgba8Pixel::new(0, 0, 0, 255);
        let mut pixels = SharedPixelBuffer::new(4, 2);
        pixels
            .make_mut_slice()
            .copy_from_slice(&[white, white, clear, clear, black, black, clear, clear]);
        let shrunk = shrink(&pixels, 2, 1);
        let grey = Rgba8Pixel::new(128, 128, 128, 255);
        assert_eq!(shrunk.as_slice(), [grey, clear]);
    }

    #[test]
    fn an_app_without_its_own_icon_gets_the_shipped_one() {
        // Breeze has the service icon, but as a file type: a pale box with "!".
        let base = std::env::temp_dir().join(format!("sebwatch-service-{}", std::process::id()));
        let file = base.join("breeze/mimetypes/32/application-x-executable.svg");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "<svg/>").unwrap();
        let found = lookup(
            SERVICE_ICON,
            22,
            std::slice::from_ref(&base),
            &["breeze".into()],
        );
        std::fs::remove_dir_all(&base).unwrap();
        assert_eq!(found, None);
    }
}
