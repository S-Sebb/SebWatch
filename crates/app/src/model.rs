//! What the window shows, in bytes and rates, and how it becomes the Slint view model.
//! Filled from `live.rs` (sebwatch-core) in the app and from `sample.rs` in tests and screenshots.

use std::collections::HashSet;
use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

use crate::icons::Icons;
use crate::{Bar, Row, RowKind, Segment, SegmentKind, Tile};

/// Apps with their own colour in the bars.
const COLOURED: usize = 5;
/// Segments at least this share of their bar show their size inside; smaller ones in the legend.
/// About 58 px at the default window width, room for "16.6 GB".
const LABELLED: f32 = 0.075;
/// Apps listed before the "N more apps" fold.
const LISTED: usize = 11;

pub struct Snapshot {
    pub ram_total: u64,
    pub swap_total: u64,
    /// Compressed pages no app is charged for.
    pub zswap_pool: u64,
    pub file_cache: u64,
    pub kernel: u64,
    pub free: u64,
    /// Sorted by RAM in use, largest first.
    pub apps: Vec<App>,
    pub cpu_used: f32,
    pub cpu_cores: u32,
    pub nvidia_percent: Option<f32>,
    pub intel_percent: Option<f32>,
    /// Used and total VRAM of the NVIDIA GPU, if it is awake.
    pub vram: Option<(u64, u64)>,
    /// The NVIDIA GPU sleeps: no VRAM figures, as reading them would wake it.
    pub gpu_asleep: bool,
    pub disk_per_s: u64,
}

pub struct App {
    /// Stable across refreshes: the core's grouping name.
    pub key: String,
    pub name: String,
    /// Muted text after the name, e.g. "17 agents".
    pub detail: String,
    /// Icon theme name or absolute path.
    pub icon: String,
    /// RAM in use, including its compressed share in zswap.
    pub ram: u64,
    /// That compressed share, when known.
    pub zswap: Option<u64>,
    pub swap: u64,
    pub file_cache: u64,
    pub cpu: f32,
    pub disk_per_s: u64,
    pub gpu_percent: Option<f32>,
    pub vram: Option<u64>,
    pub parts: Vec<Part>,
    /// Kernel memory and zswap charged to the app but to none of its processes.
    pub unattributed_ram: u64,
    pub unattributed_swap: u64,
}

/// A row under an expanded app: one of its groups (when it has several), or a process.
pub struct Part {
    pub group: bool,
    pub name: String,
    /// The PID, for a process.
    pub detail: String,
    /// None where the process's memory can't be read (another user's process).
    pub ram: Option<u64>,
    pub swap: Option<u64>,
    pub cpu: f32,
    /// Known per group, not per process.
    pub disk_per_s: Option<u64>,
}

/// Gives each of the top apps one of the five series colours and keeps it while the app stays in
/// the top five, so a colour follows its app rather than its rank.
#[derive(Default)]
pub struct Colours {
    slots: [Option<String>; COLOURED],
}

impl Colours {
    /// The colour slot of each name, in order.
    pub fn assign(&mut self, top: &[&str]) -> Vec<usize> {
        for slot in &mut self.slots {
            if slot
                .as_ref()
                .is_some_and(|name| !top.contains(&name.as_str()))
            {
                *slot = None;
            }
        }
        top.iter()
            .map(
                |name| match self.slots.iter().position(|s| s.as_deref() == Some(*name)) {
                    Some(i) => i,
                    None => {
                        let i = self
                            .slots
                            .iter()
                            .position(Option::is_none)
                            .expect("a free slot per top app");
                        self.slots[i] = Some(name.to_string());
                        i
                    }
                },
            )
            .collect()
    }
}

/// The window's state across refreshes: colours, expanded rows and loaded icons.
#[derive(Default)]
pub struct View {
    last: Option<Snapshot>,
    colours: Colours,
    expanded: HashSet<String>,
    icons: Icons,
}

pub struct Shown {
    pub memory: Bar,
    pub swap: Bar,
    /// `None` without an NVIDIA GPU.
    pub vram: Option<Bar>,
    pub tiles: Vec<Tile>,
    pub rows: Vec<Row>,
}

/// The key of the "N more apps" row.
const MORE: &str = "\u{0}more";

impl View {
    /// The keys of the expanded apps, whose processes the next reading includes.
    pub fn expanded(&self) -> &HashSet<String> {
        &self.expanded
    }

    pub fn toggle(&mut self, key: &str) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_string());
        }
    }

    /// Shows a new reading.
    pub fn update(&mut self, snapshot: Snapshot) -> Shown {
        self.last = Some(snapshot);
        self.render()
    }

    /// Shows the last reading again, e.g. after a row was expanded.
    pub fn render(&mut self) -> Shown {
        let s = self.last.take().expect("a reading before rendering");
        let shown = self.show(&s);
        self.last = Some(s);
        shown
    }

    fn show(&mut self, s: &Snapshot) -> Shown {
        let top: Vec<&str> = s
            .apps
            .iter()
            .take(COLOURED)
            .map(|a| a.key.as_str())
            .collect();
        let series = self.colours.assign(&top);
        let rest = s.apps.get(COLOURED..).unwrap_or_default();
        Shown {
            memory: memory_bar(s, &series, rest),
            swap: swap_bar(s, &series, rest),
            vram: vram_bar(s, &series, rest),
            tiles: tiles(s),
            rows: self.rows(s, &series),
        }
    }

    fn rows(&mut self, s: &Snapshot, series: &[usize]) -> Vec<Row> {
        let mut rows = Vec::new();
        let listed = s.apps.len().min(LISTED);
        for (i, app) in s.apps[..listed].iter().enumerate() {
            self.app_rows(&mut rows, app, series.get(i).map_or(-1, |&c| c as i32));
        }
        let more = &s.apps[listed..];
        if !more.is_empty() {
            let expanded = self.expanded.contains(MORE);
            rows.push(Row {
                kind: RowKind::More,
                key: MORE.into(),
                name: format!("{} more apps", more.len()).into(),
                expanded,
                ram: bytes(more.iter().map(|a| a.ram).sum()).into(),
                ..empty_row()
            });
            if expanded {
                for app in more {
                    self.app_rows(&mut rows, app, -1);
                }
            }
        }
        rows
    }

    fn app_rows(&mut self, rows: &mut Vec<Row>, app: &App, series: i32) {
        let expanded = self.expanded.contains(&app.key);
        rows.push(Row {
            kind: RowKind::App,
            depth: 0,
            key: app.key.as_str().into(),
            name: app.name.as_str().into(),
            detail: app.detail.as_str().into(),
            series,
            icon: self.icons.get(&app.icon),
            expanded,
            ram: bytes(app.ram).into(),
            swap: bytes(app.swap).into(),
            cache: bytes(app.file_cache).into(),
            cpu: cores(app.cpu).into(),
            disk: rate(app.disk_per_s).into(),
            gpu: app.gpu_percent.map_or(NONE.into(), percent).into(),
            vram: app.vram.map_or(NONE.into(), bytes).into(),
        });
        if !expanded {
            return;
        }
        let grouped = app.parts.iter().any(|p| p.group);
        for part in &app.parts {
            rows.push(Row {
                kind: if part.group {
                    RowKind::Group
                } else {
                    RowKind::Process
                },
                depth: if grouped && !part.group { 2 } else { 1 },
                name: part.name.as_str().into(),
                detail: part.detail.as_str().into(),
                ram: part.ram.map_or(NONE.into(), bytes).into(),
                swap: part.swap.map_or(NONE.into(), bytes).into(),
                cpu: cores(part.cpu).into(),
                disk: part.disk_per_s.map_or(NONE.into(), rate).into(),
                ..empty_row()
            });
        }
        if app.unattributed_ram + app.unattributed_swap > 0 {
            rows.push(Row {
                kind: RowKind::Kernel,
                depth: 1,
                name: "Kernel memory and zswap (not per process)".into(),
                ram: bytes(app.unattributed_ram).into(),
                swap: bytes(app.unattributed_swap).into(),
                ..empty_row()
            });
        }
    }
}

const NONE: &str = "—";

fn empty_row() -> Row {
    let none = SharedString::from(NONE);
    Row {
        kind: RowKind::App,
        depth: 0,
        key: "".into(),
        name: "".into(),
        detail: "".into(),
        series: -1,
        icon: Default::default(),
        expanded: false,
        ram: none.clone(),
        swap: none.clone(),
        cache: none.clone(),
        cpu: none.clone(),
        disk: none.clone(),
        gpu: none.clone(),
        vram: none,
    }
}

/// A name cut to fit the legend and a tooltip: a long one (a unit described by its
/// whole command line) would widen the window past the screen.
fn short(name: &str) -> String {
    const MAX: usize = 40;
    match name.char_indices().nth(MAX) {
        Some((end, _)) => format!("{}…", name[..end].trim_end()),
        None => name.into(),
    }
}

fn share(part: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        part as f32 / total as f32
    }
}

fn segment(kind: SegmentKind, name: &str, size: u64, total: u64, notes: &[String]) -> Segment {
    Segment {
        kind,
        series: -1,
        name: name.into(),
        detail: "".into(),
        fraction: share(size, total),
        start: 0.0,
        size: bytes(size).into(),
        labelled: share(size, total) >= LABELLED,
        tip: format!(
            "{}, {:.0}% of {}",
            bytes(size),
            share(size, total) * 100.0,
            bytes(total)
        )
        .into(),
        notes: strings(notes),
    }
}

fn strings(lines: &[String]) -> ModelRc<SharedString> {
    ModelRc::from(Rc::new(VecModel::from(
        lines.iter().map(SharedString::from).collect::<Vec<_>>(),
    )))
}

fn app_segment(
    app: &App,
    series: usize,
    size: u64,
    total: u64,
    place: &str,
    notes: Vec<String>,
) -> Segment {
    Segment {
        kind: SegmentKind::App,
        series: series as i32,
        detail: app.detail.as_str().into(),
        tip: format!(
            "{} {place}, {:.0}% of {}",
            bytes(size),
            share(size, total) * 100.0,
            bytes(total)
        )
        .into(),
        notes: strings(&notes),
        ..segment(SegmentKind::App, &short(&app.name), size, total, &[])
    }
}

/// Each segment starts where the ones before it end.
fn stack(segments: &mut [Segment]) {
    let mut start = 0.0;
    for segment in segments {
        segment.start = start;
        start += segment.fraction;
    }
}

fn memory_bar(s: &Snapshot, series: &[usize], rest: &[App]) -> Bar {
    let total = s.ram_total;
    let mut segments: Vec<Segment> = s
        .apps
        .iter()
        .zip(series)
        .map(|(app, &c)| {
            let mut notes = Vec::new();
            if let Some(zswap) = app.zswap.filter(|&z| z > 0) {
                notes.push(format!("includes {} compressed in zswap", bytes(zswap)));
            }
            if app.swap > 0 {
                notes.push(format!("{} more is swapped out", bytes(app.swap)));
            }
            app_segment(app, c, app.ram, total, "in RAM", notes)
        })
        .collect();
    let other: u64 = rest.iter().map(|a| a.ram).sum();
    segments.extend([
        segment(
            SegmentKind::Other,
            "Other apps",
            other,
            total,
            &[format!("{} apps", rest.len())],
        ),
        segment(
            SegmentKind::Zswap,
            "Zswap pool",
            s.zswap_pool,
            total,
            &["compressed memory no app is charged for".into()],
        ),
        segment(
            SegmentKind::Cache,
            "File cache",
            s.file_cache,
            total,
            &["freed when apps need the room".into()],
        ),
        segment(
            SegmentKind::Kernel,
            "Kernel and drivers",
            s.kernel,
            total,
            &["everything not counted above".into()],
        ),
        segment(SegmentKind::Free, "Free", s.free, total, &[]),
    ]);
    stack(&mut segments);
    Bar {
        title: "Memory".into(),
        total: format!("{} used of {}", bytes(total - s.free), bytes(total)).into(),
        segments: ModelRc::from(Rc::new(VecModel::from(segments))),
    }
}

fn swap_bar(s: &Snapshot, series: &[usize], rest: &[App]) -> Bar {
    let total = s.swap_total;
    let mut segments: Vec<Segment> = s
        .apps
        .iter()
        .zip(series)
        .map(|(app, &c)| app_segment(app, c, app.swap, total, "in swap", Vec::new()))
        .collect();
    let other: u64 = rest.iter().map(|a| a.swap).sum();
    let used: u64 = s.apps.iter().map(|a| a.swap).sum();
    segments.push(segment(
        SegmentKind::Other,
        "Other apps",
        other,
        total,
        &[format!("{} apps", rest.len())],
    ));
    segments.push(segment(
        SegmentKind::Free,
        "Free swap",
        total.saturating_sub(used),
        total,
        &[],
    ));
    stack(&mut segments);
    Bar {
        title: "Swap".into(),
        total: format!("{} used of {}", bytes(used), bytes(total)).into(),
        segments: ModelRc::from(Rc::new(VecModel::from(segments))),
    }
}

/// VRAM by app from NVML's per-process figures, the rest of the used VRAM as "Driver and other".
/// Empty while the GPU sleeps.
fn vram_bar(s: &Snapshot, series: &[usize], rest: &[App]) -> Option<Bar> {
    let bar = |total: String, segments: Vec<Segment>| Bar {
        title: "VRAM".into(),
        total: total.into(),
        segments: ModelRc::from(Rc::new(VecModel::from(segments))),
    };
    let Some((used, total)) = s.vram else {
        return s.gpu_asleep.then(|| bar("GPU asleep".into(), Vec::new()));
    };
    let vram = |app: &App| app.vram.unwrap_or(0);
    // Apps without VRAM are left out, so the legend lists only the GPU's users.
    let mut segments: Vec<Segment> = s
        .apps
        .iter()
        .zip(series)
        .filter(|(app, _)| vram(app) > 0)
        .map(|(app, &c)| {
            let notes: Vec<String> = app
                .gpu_percent
                .map(|p| format!("{} of the GPU's time", percent(p)))
                .into_iter()
                .collect();
            app_segment(app, c, vram(app), total, "in VRAM", notes)
        })
        .collect();
    let others: Vec<&App> = rest.iter().filter(|a| vram(a) > 0).collect();
    if !others.is_empty() {
        segments.push(segment(
            SegmentKind::Other,
            "Other apps",
            others.iter().map(|a| vram(a)).sum(),
            total,
            &[format!("{} apps", others.len())],
        ));
    }
    let apps: u64 = s.apps.iter().map(vram).sum();
    let unattributed = used.saturating_sub(apps);
    if unattributed > 0 {
        segments.push(segment(
            SegmentKind::Kernel,
            "Driver and other",
            unattributed,
            total,
            &["used, but by no app's process".into()],
        ));
    }
    segments.push(segment(
        SegmentKind::Free,
        "Free",
        total.saturating_sub(used.max(apps)),
        total,
        &[],
    ));
    stack(&mut segments);
    Some(bar(
        format!("{} used of {}", bytes(used), bytes(total)),
        segments,
    ))
}

fn tiles(s: &Snapshot) -> Vec<Tile> {
    let mut tiles = vec![Tile {
        name: "CPU".into(),
        value: format!("{:.1}", s.cpu_used).into(),
        unit: format!("of {} cores", s.cpu_cores).into(),
        fraction: s.cpu_used / s.cpu_cores.max(1) as f32,
    }];
    if let Some(nvidia) = s.nvidia_percent {
        tiles.push(Tile {
            name: "GPU".into(),
            value: percent(nvidia).into(),
            unit: match s.intel_percent {
                Some(intel) => format!("NVIDIA · {} Intel", percent(intel)),
                None => "NVIDIA".into(),
            }
            .into(),
            fraction: nvidia / 100.0,
        });
    }
    if let Some((used, total)) = s.vram {
        tiles.push(Tile {
            name: "VRAM".into(),
            value: format!("{:.1}", used as f64 / GB).into(),
            unit: format!("of {:.1} GB", total as f64 / GB).into(),
            fraction: share(used, total),
        });
    }
    tiles.push(Tile {
        name: "Disk".into(),
        value: format!("{:.1}", s.disk_per_s as f64 / MB).into(),
        unit: "MB/s read + write".into(),
        fraction: -1.0,
    });
    tiles
}

const KB: f64 = 1024.0;
const MB: f64 = KB * 1024.0;
const GB: f64 = MB * 1024.0;

/// Binary sizes with KDE's short unit names: "2.7 GB", "441 MB", "5.8 MB", "102 KB".
pub fn bytes(n: u64) -> String {
    let n = n as f64;
    // A unit steps up at 1000 of the one below, so no figure needs four digits.
    let (value, unit) = if n >= 1000.0 * MB {
        (n / GB, "GB")
    } else if n >= 1000.0 * KB {
        (n / MB, "MB")
    } else if n >= 1000.0 {
        (n / KB, "KB")
    } else if n == 0.0 {
        return "0".into();
    } else {
        (n, "B")
    };
    if unit == "GB" || value < 10.0 && unit != "B" {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

fn rate(n: u64) -> String {
    match n {
        0 => "0".into(),
        n => bytes(n) + "/s",
    }
}

fn cores(n: f32) -> String {
    format!("{n:.2}")
}

fn percent(n: f32) -> String {
    format!("{n:.0}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_kde() {
        assert_eq!(bytes(0), "0");
        assert_eq!(bytes(104_448), "102 KB");
        assert_eq!(bytes(6_081_741), "5.8 MB");
        assert_eq!(bytes(462_422_016), "441 MB");
        assert_eq!(bytes(2_899_102_924), "2.7 GB");
        assert_eq!(bytes(16_428_249_088), "15.3 GB");
        assert_eq!(bytes(1013 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn long_names_are_cut() {
        assert_eq!(short("Steam"), "Steam");
        let long = "[systemd-run] /home/s_sebb/.local/bin/claude-t3 --output-format stream-json";
        assert_eq!(short(long), "[systemd-run] /home/s_sebb/.local/bin/cl…");
    }

    #[test]
    fn colour_follows_its_app_not_its_rank() {
        let mut colours = Colours::default();
        assert_eq!(colours.assign(&["a", "b", "c", "d", "e"]), [0, 1, 2, 3, 4]);
        // b overtakes a; f replaces c, which left the top five, and takes c's colour.
        assert_eq!(colours.assign(&["b", "a", "f", "d", "e"]), [1, 0, 2, 3, 4]);
        assert_eq!(colours.assign(&["e", "d"]), [4, 3]);
        assert_eq!(colours.assign(&["g", "e", "h"]), [0, 4, 1]);
    }

    #[test]
    fn ram_bar_adds_up_to_total_ram() {
        let s = crate::sample::snapshot();
        let shown = View::default().update(s);
        let sum: f32 = slint::Model::iter(&shown.memory.segments)
            .map(|seg| seg.fraction)
            .sum();
        assert!((sum - 1.0).abs() < 1e-4, "RAM bar sums to {sum}");
        let sum: f32 = slint::Model::iter(&shown.swap.segments)
            .map(|seg| seg.fraction)
            .sum();
        assert!((sum - 1.0).abs() < 1e-4, "swap bar sums to {sum}");
    }

    #[test]
    fn vram_bar_keeps_ram_colours_and_sleeps_empty() {
        let s = crate::sample::snapshot();
        let shown = View::default().update(s);
        let ram: Vec<_> = slint::Model::iter(&shown.memory.segments).collect();
        let vram: Vec<_> = slint::Model::iter(&shown.vram.unwrap().segments).collect();
        let names: Vec<&str> = vram.iter().map(|seg| seg.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Zen Browser",
                "T3 Code",
                "Other apps",
                "Driver and other",
                "Free"
            ]
        );
        let colour =
            |segs: &[Segment], name: &str| segs.iter().find(|seg| seg.name == name).unwrap().series;
        assert_eq!(colour(&vram, "Zen Browser"), colour(&ram, "Zen Browser"));
        let sum: f32 = vram.iter().map(|seg| seg.fraction).sum();
        assert!((sum - 1.0).abs() < 1e-4, "VRAM bar sums to {sum}");

        let asleep = Snapshot {
            vram: None,
            gpu_asleep: true,
            ..crate::sample::snapshot()
        };
        let bar = View::default().update(asleep).vram.unwrap();
        assert_eq!(bar.total, "GPU asleep");
        assert_eq!(slint::Model::row_count(&bar.segments), 0);
        let none = Snapshot {
            vram: None,
            nvidia_percent: None,
            ..crate::sample::snapshot()
        };
        assert!(View::default().update(none).vram.is_none());
    }

    #[test]
    fn expanded_app_lists_its_processes_and_kernel_share() {
        let mut view = View::default();
        let collapsed = view.update(crate::sample::snapshot()).rows.len();
        view.toggle("Android emulator");
        let rows = view.render().rows;
        assert_eq!(rows.len(), collapsed + 4);
        assert_eq!(rows[2].name, "qemu-system-x86_64");
        assert_eq!(rows[5].kind, RowKind::Kernel);
        assert_eq!(rows[5].depth, 1);
        view.toggle("Android emulator");
        assert_eq!(view.render().rows.len(), collapsed);
    }
}
