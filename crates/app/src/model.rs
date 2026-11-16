//! What the window shows, in bytes and rates, and how it becomes the Slint view model.
//! Filled from `live.rs` (sebwatch-core) in the app and from `sample.rs` in tests and screenshots.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use slint::{ModelRc, SharedString, VecModel};

use crate::icons::Icons;
use crate::{Bar, Row, RowKind, Segment, SegmentKind, Tile};

/// Apps each bar names; the rest are its "Other apps".
const NAMED: usize = 5;
/// Series colours: the RAM bar's five apps and two for the swap and VRAM bars' own top apps.
const SERIES: usize = 7;
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

/// A sortable table column, in the table's order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Column {
    #[default]
    Ram,
    Swap,
    Cache,
    Cpu,
    Disk,
    Gpu,
    Vram,
}

impl Column {
    pub const ALL: [Column; 7] = [
        Column::Ram,
        Column::Swap,
        Column::Cache,
        Column::Cpu,
        Column::Disk,
        Column::Gpu,
        Column::Vram,
    ];

    /// The app's figure in this column; a missing one sorts below zero.
    fn value(self, app: &App) -> f64 {
        match self {
            Column::Ram => app.ram as f64,
            Column::Swap => app.swap as f64,
            Column::Cache => app.file_cache as f64,
            Column::Cpu => app.cpu as f64,
            Column::Disk => app.disk_per_s as f64,
            Column::Gpu => app.gpu_percent.map_or(-1.0, |p| p as f64),
            Column::Vram => app.vram.map_or(-1.0, |b| b as f64),
        }
    }
}

/// The table's order: largest first unless `ascending`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sort {
    pub column: Column,
    pub ascending: bool,
}

impl Sort {
    /// After a click on a column's header: that column largest first, or the other way round
    /// when it already sorts the table.
    pub fn click(self, column: Column) -> Sort {
        Sort {
            column,
            ascending: column == self.column && !self.ascending,
        }
    }

    /// Ties keep the reading's order (by RAM), so rows don't swap places between refreshes.
    fn apply(self, apps: &[App]) -> Vec<&App> {
        let mut sorted: Vec<&App> = apps.iter().collect();
        sorted.sort_by(|a, b| {
            let order = self.column.value(a).total_cmp(&self.column.value(b));
            if self.ascending {
                order
            } else {
                order.reverse()
            }
        });
        sorted
    }
}

/// Gives each named app one of the series colours and keeps it while the app stays named in
/// some bar, so a colour follows its app rather than its rank.
#[derive(Default)]
pub struct Colours {
    slots: [Option<String>; SERIES],
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
                            .expect("a free slot per named app");
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
    pub sort: Sort,
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
    pub sort: Sort,
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
        let ram = top(&s.apps, |a| a.ram);
        let swap = top(&s.apps, |a| a.swap);
        let vram = top(&s.apps, |a| a.vram.unwrap_or(0));
        // RAM's five first, then the swap and VRAM bars' apps rank by rank, so each bar's
        // biggest app always has a colour.
        let mut named: Vec<&str> = ram.iter().map(|a| a.key.as_str()).collect();
        for i in 0..NAMED {
            for app in [swap.get(i), vram.get(i)].into_iter().flatten() {
                if named.len() < SERIES && !named.contains(&app.key.as_str()) {
                    named.push(&app.key);
                }
            }
        }
        let slots = self.colours.assign(&named);
        let series: Series = named.into_iter().zip(slots).collect();
        Shown {
            memory: memory_bar(s, &series, &ram),
            swap: swap_bar(s, &series, &coloured(swap, &series)),
            vram: vram_bar(s, &series, &coloured(vram, &series)),
            tiles: tiles(s),
            rows: self.rows(s, &series),
            sort: self.sort,
        }
    }

    fn rows(&mut self, s: &Snapshot, series: &Series) -> Vec<Row> {
        let mut rows = Vec::new();
        let sorted = self.sort.apply(&s.apps);
        let colour = |app: &App| series.get(app.key.as_str()).map_or(-1, |&c| c as i32);
        let listed = sorted.len().min(LISTED);
        for app in &sorted[..listed] {
            self.app_rows(&mut rows, app, colour(app));
        }
        let more = &sorted[listed..];
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
                    self.app_rows(&mut rows, app, colour(app));
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

/// Each named app's colour slot, by key.
type Series<'a> = HashMap<&'a str, usize>;

/// A bar's five biggest apps by `measure`, leaving out those with none; ties keep RAM's order.
fn top(apps: &[App], measure: impl Fn(&App) -> u64) -> Vec<&App> {
    let mut sorted: Vec<&App> = apps.iter().filter(|a| measure(a) > 0).collect();
    sorted.sort_by_key(|a| std::cmp::Reverse(measure(a)));
    sorted.truncate(NAMED);
    sorted
}

/// A bar names its apps down to the first without a colour.
fn coloured<'a>(apps: Vec<&'a App>, series: &Series) -> Vec<&'a App> {
    apps.into_iter()
        .take_while(|a| series.contains_key(a.key.as_str()))
        .collect()
}

/// The "Other apps" segment: the apps a bar doesn't name, with some of `measure`.
fn others(
    apps: &[App],
    named: &[&App],
    measure: impl Fn(&App) -> u64,
    total: u64,
) -> Option<Segment> {
    let rest: Vec<&App> = apps
        .iter()
        .filter(|a| measure(a) > 0 && !named.iter().any(|n| n.key == a.key))
        .collect();
    (!rest.is_empty()).then(|| {
        segment(
            SegmentKind::Other,
            "Other apps",
            rest.iter().map(|a| measure(a)).sum(),
            total,
            &[format!("{} apps", rest.len())],
        )
    })
}

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

fn memory_bar(s: &Snapshot, series: &Series, named: &[&App]) -> Bar {
    let total = s.ram_total;
    let mut segments: Vec<Segment> = named
        .iter()
        .map(|app| {
            let mut notes = Vec::new();
            if let Some(zswap) = app.zswap.filter(|&z| z > 0) {
                notes.push(format!("includes {} compressed in zswap", bytes(zswap)));
            }
            if app.swap > 0 {
                notes.push(format!("{} more is swapped out", bytes(app.swap)));
            }
            app_segment(
                app,
                series[app.key.as_str()],
                app.ram,
                total,
                "in RAM",
                notes,
            )
        })
        .collect();
    segments.extend(others(&s.apps, named, |a| a.ram, total));
    segments.extend([
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

fn swap_bar(s: &Snapshot, series: &Series, named: &[&App]) -> Bar {
    let total = s.swap_total;
    let mut segments: Vec<Segment> = named
        .iter()
        .map(|app| {
            let c = series[app.key.as_str()];
            app_segment(app, c, app.swap, total, "in swap", Vec::new())
        })
        .collect();
    let used: u64 = s.apps.iter().map(|a| a.swap).sum();
    segments.extend(others(&s.apps, named, |a| a.swap, total));
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
fn vram_bar(s: &Snapshot, series: &Series, named: &[&App]) -> Option<Bar> {
    let bar = |total: String, segments: Vec<Segment>| Bar {
        title: "VRAM".into(),
        total: total.into(),
        segments: ModelRc::from(Rc::new(VecModel::from(segments))),
    };
    let Some((used, total)) = s.vram else {
        return s.gpu_asleep.then(|| bar("GPU asleep".into(), Vec::new()));
    };
    let vram = |app: &App| app.vram.unwrap_or(0);
    let mut segments: Vec<Segment> = named
        .iter()
        .map(|app| {
            let notes: Vec<String> = app
                .gpu_percent
                .map(|p| format!("{} of the GPU's time", percent(p)))
                .into_iter()
                .collect();
            app_segment(
                app,
                series[app.key.as_str()],
                vram(app),
                total,
                "in VRAM",
                notes,
            )
        })
        .collect();
    segments.extend(others(&s.apps, named, vram, total));
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
    // A sleeping GPU keeps its tiles, saying so in place of the figures not read (no value: the
    // words need the tile's width).
    if let Some(nvidia) = s.nvidia_percent {
        let (value, nvidia_unit) = if s.gpu_asleep {
            (String::new(), "NVIDIA asleep")
        } else {
            (percent(nvidia), "NVIDIA")
        };
        tiles.push(Tile {
            name: "GPU".into(),
            value: value.into(),
            unit: match s.intel_percent {
                Some(intel) => format!("{nvidia_unit} · {} Intel", percent(intel)),
                None => nvidia_unit.into(),
            }
            .into(),
            fraction: if s.gpu_asleep { 0.0 } else { nvidia / 100.0 },
        });
    }
    if let Some((used, total)) = s.vram {
        tiles.push(Tile {
            name: "VRAM".into(),
            value: format!("{:.1}", used as f64 / GB).into(),
            unit: format!("of {:.1} GB", total as f64 / GB).into(),
            fraction: share(used, total),
        });
    } else if s.gpu_asleep {
        tiles.push(Tile {
            name: "VRAM".into(),
            value: "".into(),
            unit: "GPU asleep".into(),
            fraction: 0.0,
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
            ["Zen Browser", "T3 Code", "KWin", "Driver and other", "Free"]
        );
        let colour =
            |segs: &[Segment], name: &str| segs.iter().find(|seg| seg.name == name).unwrap().series;
        assert_eq!(colour(&vram, "Zen Browser"), colour(&ram, "Zen Browser"));
        let sum: f32 = vram.iter().map(|seg| seg.fraction).sum();
        assert!((sum - 1.0).abs() < 1e-4, "VRAM bar sums to {sum}");

        let asleep = View::default().update(crate::sample::asleep());
        let bar = asleep.vram.unwrap();
        assert_eq!(bar.total, "GPU asleep");
        assert_eq!(slint::Model::row_count(&bar.segments), 0);
        // Both GPU tiles stay, saying so; the table shows no NVIDIA figures.
        let tiles: Vec<_> = asleep
            .tiles
            .iter()
            .map(|t| (t.name.as_str(), t.value.as_str(), t.unit.as_str()))
            .collect();
        assert!(tiles.contains(&("GPU", "", "NVIDIA asleep · 3% Intel")));
        assert!(tiles.contains(&("VRAM", "", "GPU asleep")));
        assert!(
            asleep
                .rows
                .iter()
                .all(|r| r.gpu.as_str() == NONE && r.vram.as_str() == NONE)
        );
        let none = Snapshot {
            vram: None,
            nvidia_percent: None,
            ..crate::sample::snapshot()
        };
        assert!(View::default().update(none).vram.is_none());
    }

    fn names(bar: &Bar) -> Vec<String> {
        slint::Model::iter(&bar.segments)
            .map(|seg| seg.name.to_string())
            .collect()
    }

    /// Every app row, with the "N more apps" fold open.
    fn app_rows(view: &mut View) -> Vec<Row> {
        view.toggle(MORE);
        let rows = view.render().rows;
        view.toggle(MORE);
        rows.into_iter()
            .filter(|r| r.kind == RowKind::App)
            .collect()
    }

    #[test]
    fn table_sorts_by_each_column_both_ways_ties_in_ram_order() {
        let s = crate::sample::snapshot();
        let by_name = |name: &str| s.apps.iter().position(|a| a.name == name).unwrap();
        let mut view = View::default();
        view.update(crate::sample::snapshot());
        for column in Column::ALL {
            for ascending in [false, true] {
                view.sort = Sort { column, ascending };
                let rows = app_rows(&mut view);
                assert_eq!(rows.len(), s.apps.len());
                for pair in rows.windows(2) {
                    let (a, b) = (by_name(&pair[0].name), by_name(&pair[1].name));
                    let (va, vb) = (column.value(&s.apps[a]), column.value(&s.apps[b]));
                    let ordered = if ascending { va <= vb } else { va >= vb };
                    assert!(ordered, "{column:?} {ascending}: {va} then {vb}");
                    if va == vb {
                        assert!(a < b, "{column:?} tie out of RAM order");
                    }
                }
            }
        }
        // The fold holds whatever the order puts last.
        view.sort = Sort {
            column: Column::Swap,
            ascending: false,
        };
        let rows = view.render().rows;
        assert_eq!(rows[0].name, "Android emulator");
        assert_eq!(rows[LISTED].kind, RowKind::More);
    }

    #[test]
    fn a_click_sorts_largest_first_and_again_reverses() {
        let ram = Sort::default();
        assert!(!ram.ascending);
        let swap = ram.click(Column::Swap);
        assert_eq!((swap.column, swap.ascending), (Column::Swap, false));
        assert!(swap.click(Column::Swap).ascending);
        assert!(!swap.click(Column::Swap).click(Column::Swap).ascending);
        assert!(!swap.click(Column::Swap).click(Column::Cpu).ascending);
    }

    #[test]
    fn swap_and_vram_bars_name_their_own_top_five_in_one_colour_per_app() {
        let mut view = View::default();
        let shown = view.update(crate::sample::snapshot());
        assert_eq!(
            names(&shown.swap)[..6],
            [
                "Android emulator",
                "Claude agents",
                "Steam",
                "Zen Browser",
                "WeChat",
                "Other apps"
            ]
        );
        // Each app's colour is the same in every bar and in its table row.
        let mut colours: HashMap<String, i32> = HashMap::new();
        for bar in [&shown.memory, &shown.swap, shown.vram.as_ref().unwrap()] {
            for seg in slint::Model::iter(&bar.segments).filter(|s| s.kind == SegmentKind::App) {
                let c = *colours.entry(seg.name.to_string()).or_insert(seg.series);
                assert_eq!(c, seg.series, "{} changes colour", seg.name);
            }
        }
        assert_eq!(colours.len(), SERIES);
        let mut seen: Vec<i32> = colours.values().copied().collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), SERIES, "two apps share a colour");
        for row in app_rows(&mut view) {
            let expected = colours.get(row.name.as_str()).copied().unwrap_or(-1);
            assert_eq!(row.series, expected, "{}'s row", row.name);
        }

        // WeChat climbs to the top of the swap bar: it keeps its colour.
        let mut s = crate::sample::snapshot();
        s.apps.iter_mut().find(|a| a.name == "WeChat").unwrap().swap = 6 << 30;
        let shown = view.update(s);
        let wechat = slint::Model::row_data(&shown.swap.segments, 0).unwrap();
        assert_eq!(wechat.name, "WeChat");
        assert_eq!(wechat.series, colours["WeChat"]);
    }

    #[test]
    fn each_bar_keeps_a_colour_for_its_biggest_app() {
        // Swap and VRAM held by ten other apps than RAM's top five: two colours are left.
        let mut s = crate::sample::snapshot();
        for (i, app) in s.apps.iter_mut().enumerate() {
            let (swap, vram) = match i {
                0..5 => (0, None),
                5..10 => ((20 - i as u64) << 20, None),
                _ => (0, Some((40 - i as u64) << 20)),
            };
            (app.swap, app.vram) = (swap, vram);
        }
        let shown = View::default().update(s);
        let swap = names(&shown.swap);
        let vram = names(&shown.vram.unwrap());
        assert_eq!(swap[1..], ["Other apps", "Free swap"]);
        assert_eq!(vram[1], "Other apps");
        assert_ne!(swap[0], vram[0]);
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
