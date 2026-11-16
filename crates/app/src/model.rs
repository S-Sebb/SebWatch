//! What the window shows, in bytes and rates, and how it becomes the Slint view model.
//! Filled from `live.rs` (sebwatch-core) in the app and from `sample.rs` in tests and screenshots.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use slint::{ModelRc, SharedString, VecModel};

use crate::history::{History, Measure};
use crate::icons::Icons;
use crate::{Bar, Row, RowKind, Segment, SegmentKind, TableKey, Tile};

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
    /// When it was read, for the tiles' graphs.
    pub taken: Instant,
    pub ram_total: u64,
    pub swap_total: u64,
    /// Compressed pages no app is charged for.
    pub zswap_pool: u64,
    pub file_cache: u64,
    pub kernel: u64,
    pub free: u64,
    /// Sorted by RAM in use, largest first.
    pub apps: Vec<App>,
    /// Memory still charged to groups whose processes all exited, outside any running app.
    pub closed: Closed,
    /// None when no group's CPU could be read.
    pub cpu_used: Option<f32>,
    pub cpu_cores: u32,
    pub nvidia_percent: Option<f32>,
    pub intel_percent: Option<f32>,
    /// Used and total VRAM of the NVIDIA GPU, if it is awake.
    pub vram: Option<(u64, u64)>,
    /// The NVIDIA GPU sleeps: no VRAM figures, as reading them would wake it.
    pub gpu_asleep: bool,
    /// The NVIDIA GPU is awake but its driver doesn't answer: no GPU or VRAM figures.
    pub nvidia_unavailable: bool,
    /// None when no group has `io.stat` (the io controller is off).
    pub disk_per_s: Option<u64>,
    /// Some on a system that can't split memory by app (no cgroup v2 memory accounting or no
    /// systemd user session): `apps` is empty and `kernel` holds all memory in use.
    pub unsplit: Option<Unsplit>,
}

pub struct Unsplit {
    pub swap_used: u64,
}

pub struct App {
    /// Stable across refreshes: the core's grouping name.
    pub key: String,
    pub name: String,
    /// Muted text after the name, e.g. "17 agents".
    pub detail: String,
    /// Icon theme name or absolute path.
    pub icon: String,
    /// Its cgroups' names, e.g. "app-steam@autostart.service", and their processes: what a search
    /// finds it by besides its name.
    pub groups: Vec<String>,
    pub pids: Vec<u32>,
    /// RAM in use, including its compressed share in zswap.
    pub ram: u64,
    /// That compressed share, when known.
    pub zswap: Option<u64>,
    pub swap: u64,
    pub file_cache: u64,
    pub cpu: f32,
    /// None when none of its groups has `io.stat`.
    pub disk_per_s: Option<u64>,
    pub gpu_percent: Option<f32>,
    pub vram: Option<u64>,
    pub parts: Vec<Part>,
    /// Charged to the app but to none of the processes read: kernel memory, zswap and the
    /// memory of `unreadable`.
    pub unattributed_ram: u64,
    pub unattributed_swap: u64,
    /// How far the processes' RAM exceeds the app's (`unattributed_ram` is 0 then).
    pub over_charge: u64,
    /// The processes whose memory couldn't be read, as `parts` lists them.
    pub unreadable: Vec<Unreadable>,
}

pub struct Unreadable {
    pub name: String,
    pub pid: u32,
    /// Refused: another user's process. Otherwise the read failed some other way.
    pub denied: bool,
}

/// "Closed apps (cache)": its own segment and the table's last row, outside the apps.
#[derive(Default)]
pub struct Closed {
    pub ram: u64,
    pub swap: u64,
    pub file_cache: u64,
    /// Biggest RAM first.
    pub exited: Vec<Exited>,
}

/// A closed app, or the slice that kept a closed app's charges.
pub struct Exited {
    pub name: String,
    pub ram: u64,
    pub swap: u64,
    pub file_cache: u64,
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
    /// Known per group, not per process: `None` for a process, or a group whose `io.stat`
    /// can't be read.
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
            Column::Disk => app.disk_per_s.map_or(-1.0, |d| d as f64),
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

/// The window's state across refreshes: colours, expanded rows, the keyboard focus and loaded
/// icons.
#[derive(Default)]
pub struct View {
    last: Option<Snapshot>,
    pub sort: Sort,
    /// The search field's text: the table lists only the apps it finds, none folded.
    pub search: String,
    colours: Colours,
    expanded: HashSet<String>,
    icons: Icons,
    history: History,
    /// Set from the reading for the rows: the NVIDIA driver didn't answer.
    gpu_unavailable: bool,
    /// Readings failed in a row since the last one shown, and the last error.
    failures: u32,
    error: String,
    /// When the reading shown was taken, as local "HH:MM:SS".
    shown_at: String,
    /// The last rendered rows, in order.
    tree: Vec<Node>,
    focus: Option<Focus>,
}

/// A rendered row's place in the tree.
struct Node {
    key: String,
    /// The row Left goes to: the app above a process, the fold above the apps in it.
    parent: Option<String>,
    /// An app, the "N more apps" fold or the closed apps.
    expandable: bool,
}

/// The row the keyboard is on. It follows its row's key, so it stays on the same app when a
/// refresh reorders the rows or a search narrows them; a row that went away hands it to its
/// parent, or to the row now at its place.
struct Focus {
    key: String,
    parent: Option<String>,
    index: usize,
}

pub struct Shown {
    pub memory: Bar,
    pub swap: Bar,
    /// `None` without an NVIDIA GPU.
    pub vram: Option<Bar>,
    pub tiles: Vec<Tile>,
    pub rows: Vec<Row>,
    pub sort: Sort,
    /// "Filtered: 5 of 18 apps · …" while searching, else empty.
    pub filtered: String,
    /// The focused row's index in `rows`, -1 for none.
    pub focus: i32,
    /// Why the figures aren't updating, empty while they are.
    pub warning: String,
    /// The system can't split memory by app: a message replaces the table.
    pub unsplit: bool,
}

/// The key of the "N more apps" row.
const MORE: &str = "\u{0}more";
/// The key of the closed apps' row.
const CLOSED: &str = "\u{0}closed";
use sebwatch_core::memory::CLOSED_APPS;

/// "1 app", "3 apps".
fn apps(n: usize) -> String {
    format!("{n} app{}", if n == 1 { "" } else { "s" })
}

/// The closed apps' segment in a bar, when they hold some of it; `note` adds a tooltip line.
fn closed_segment(
    closed: &Closed,
    size: u64,
    total: u64,
    place: &str,
    note: Option<String>,
) -> Option<Segment> {
    (size > 0).then(|| {
        let mut notes = vec![format!(
            "still charged to {} that have exited",
            apps(closed.exited.len())
        )];
        notes.extend(note);
        Segment {
            tip: format!(
                "{} {place}, {:.0}% of {}",
                bytes(size),
                share(size, total) * 100.0,
                bytes(total)
            )
            .into(),
            ..segment(SegmentKind::Closed, CLOSED_APPS, size, total, &notes)
        }
    })
}

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

    /// A click on the row at `index`: focuses it, and expands or folds an app. True when it did.
    pub fn pick(&mut self, index: usize) -> bool {
        let Some(node) = self.tree.get(index) else {
            return false;
        };
        let key = node.key.clone();
        let toggled = node.expandable;
        self.focus_on(index);
        if toggled {
            self.toggle(&key);
        }
        toggled
    }

    /// A key pressed in the table. True when it expanded or folded an app, which then needs a
    /// new reading for its processes.
    pub fn press(&mut self, key: TableKey) -> bool {
        let Some(last) = self.tree.len().checked_sub(1) else {
            return false;
        };
        let Some(i) = self.focus.as_ref().map(|f| f.index) else {
            // The first key only shows where the focus is.
            self.focus_on(if key == TableKey::End { last } else { 0 });
            return false;
        };
        let node = &self.tree[i];
        let expanded = self.expanded.contains(&node.key);
        match key {
            TableKey::Up => self.focus_on(i.saturating_sub(1)),
            TableKey::Down => self.focus_on((i + 1).min(last)),
            TableKey::Home => self.focus_on(0),
            TableKey::End => self.focus_on(last),
            TableKey::Right if node.expandable && !expanded => {
                let key = node.key.clone();
                self.toggle(&key);
                return true;
            }
            // On an open app, Right steps into its first row.
            TableKey::Right => {
                if self
                    .tree
                    .get(i + 1)
                    .is_some_and(|next| next.parent.as_ref() == Some(&node.key))
                {
                    self.focus_on(i + 1);
                }
            }
            TableKey::Left if node.expandable && expanded => {
                let key = node.key.clone();
                self.toggle(&key);
                return true;
            }
            TableKey::Left => {
                if let Some(up) = node
                    .parent
                    .as_ref()
                    .and_then(|p| self.tree.iter().position(|n| &n.key == p))
                {
                    self.focus_on(up);
                }
            }
            TableKey::Toggle if node.expandable => {
                let key = node.key.clone();
                self.toggle(&key);
                return true;
            }
            TableKey::Toggle => {}
        }
        false
    }

    fn focus_on(&mut self, index: usize) {
        let node = &self.tree[index];
        self.focus = Some(Focus {
            key: node.key.clone(),
            parent: node.parent.clone(),
            index,
        });
    }

    /// Finds the focused row in newly rendered rows.
    fn refocus(&mut self) -> i32 {
        let Some(focus) = &self.focus else {
            return -1;
        };
        let at = |key: &str| self.tree.iter().position(|n| n.key == key);
        let index = at(&focus.key)
            .or_else(|| focus.parent.as_deref().and_then(at))
            .or_else(|| self.tree.len().checked_sub(1).map(|l| focus.index.min(l)));
        match index {
            Some(i) => {
                self.focus_on(i);
                i as i32
            }
            None => {
                self.focus = None;
                -1
            }
        }
    }

    fn node(&mut self, key: &str, parent: Option<&str>, expandable: bool) {
        self.tree.push(Node {
            key: key.into(),
            parent: parent.map(String::from),
            expandable,
        });
    }

    /// Shows a new reading.
    pub fn update(&mut self, snapshot: Snapshot) -> Shown {
        self.history.push(snapshot.taken, readings(&snapshot));
        self.failures = 0;
        self.shown_at = crate::clock::now();
        self.last = Some(snapshot);
        self.render()
    }

    /// A reading failed: the last one stays, with a warning saying so.
    pub fn failed(&mut self, error: String) -> Shown {
        self.failures += 1;
        self.error = error;
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
        self.tree.clear();
        self.gpu_unavailable = s.nvidia_unavailable;
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
            tiles: tiles(s, &self.history),
            rows: self.rows(s, &series),
            focus: self.refocus(),
            warning: match self.failures {
                0 => String::new(),
                n => format!(
                    "Not updating: the last {} failed ({}). Showing the reading from {}.",
                    if n == 1 {
                        "reading".into()
                    } else {
                        format!("{n} readings")
                    },
                    self.error,
                    self.shown_at
                ),
            },
            unsplit: s.unsplit.is_some(),
            sort: self.sort,
            filtered: match self.searching() {
                Some(query) => format!(
                    "Filtered: {} of {} apps · the bars show the whole system",
                    s.apps.iter().filter(|a| matches(a, &query)).count(),
                    s.apps.len()
                ),
                None => String::new(),
            },
        }
    }

    /// The search in lower case, `None` for an empty field.
    fn searching(&self) -> Option<String> {
        let query = self.search.trim();
        (!query.is_empty()).then(|| query.to_lowercase())
    }

    fn rows(&mut self, s: &Snapshot, series: &Series) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut sorted = self.sort.apply(&s.apps);
        let colour = |app: &App| series.get(app.key.as_str()).map_or(-1, |&c| c as i32);
        // A search lists every app it finds, so none hides in the fold.
        let query = self.searching();
        let listed = match &query {
            Some(query) => {
                sorted.retain(|a| matches(a, query));
                sorted.len()
            }
            None => sorted.len().min(LISTED),
        };
        for app in &sorted[..listed] {
            self.app_rows(&mut rows, app, colour(app), None);
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
            self.node(MORE, None, true);
            if expanded {
                for app in more {
                    self.app_rows(&mut rows, app, colour(app), Some(MORE));
                }
            }
        }
        let found = |query: &str| {
            let found = |text: &str| text.to_lowercase().contains(query);
            found(CLOSED_APPS) || s.closed.exited.iter().any(|e| found(&e.name))
        };
        if query.as_deref().is_none_or(found) {
            self.closed_rows(&mut rows, &s.closed);
        }
        rows
    }

    /// Last, whatever the sort: it isn't an app, and it shrinks as the kernel reclaims it.
    fn closed_rows(&mut self, rows: &mut Vec<Row>, closed: &Closed) {
        if closed.exited.is_empty() {
            return;
        }
        let expanded = self.expanded.contains(CLOSED);
        rows.push(Row {
            kind: RowKind::More,
            key: CLOSED.into(),
            name: CLOSED_APPS.into(),
            detail: apps(closed.exited.len()).into(),
            expanded,
            ram: bytes(closed.ram).into(),
            swap: bytes(closed.swap).into(),
            cache: bytes(closed.file_cache).into(),
            ..empty_row()
        });
        self.node(CLOSED, None, true);
        if expanded {
            for e in &closed.exited {
                self.node(&format!("{CLOSED}\u{1}{}", e.name), Some(CLOSED), false);
            }
            rows.extend(closed.exited.iter().map(|e| Row {
                kind: RowKind::Process,
                depth: 1,
                key: format!("{CLOSED}\u{1}{}", e.name).into(),
                name: e.name.as_str().into(),
                detail: "exited".into(),
                ram: bytes(e.ram).into(),
                swap: bytes(e.swap).into(),
                cache: bytes(e.file_cache).into(),
                ..empty_row()
            }));
        }
    }

    fn app_rows(&mut self, rows: &mut Vec<Row>, app: &App, series: i32, parent: Option<&str>) {
        let expanded = self.expanded.contains(&app.key);
        self.node(&app.key, parent, true);
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
            disk: app.disk_per_s.map_or(NA.into(), rate).into(),
            gpu: match app.gpu_percent {
                _ if self.gpu_unavailable => NA.into(),
                Some(p) => percent(p),
                None => NONE.into(),
            }
            .into(),
            vram: match app.vram {
                _ if self.gpu_unavailable => NA.into(),
                Some(b) => bytes(b),
                None => NONE.into(),
            }
            .into(),
            tip: "".into(),
        });
        if !expanded {
            return;
        }
        let grouped = app.parts.iter().any(|p| p.group);
        // A process under a group steps back to its group, else to the app.
        let mut group: Option<String> = None;
        for part in &app.parts {
            let key = format!("{}\u{1}{}\u{1}{}", app.key, part.name, part.detail);
            if part.group {
                group = Some(key.clone());
            }
            let parent = match &group {
                Some(group) if !part.group => group.clone(),
                _ => app.key.clone(),
            };
            self.node(&key, Some(&parent), false);
            rows.push(Row {
                kind: if part.group {
                    RowKind::Group
                } else {
                    RowKind::Process
                },
                depth: if grouped && !part.group { 2 } else { 1 },
                key: key.into(),
                name: part.name.as_str().into(),
                detail: part.detail.as_str().into(),
                ram: part.ram.map_or(NONE.into(), bytes).into(),
                swap: part.swap.map_or(NONE.into(), bytes).into(),
                cpu: cores(part.cpu).into(),
                disk: part
                    .disk_per_s
                    .map_or(if part.group { NA } else { NONE }.into(), rate)
                    .into(),
                ..empty_row()
            });
        }
        if app.unattributed_ram + app.unattributed_swap + app.over_charge > 0
            || !app.unreadable.is_empty()
        {
            let key = format!("{}\u{1}leftover", app.key);
            self.node(&key, Some(&app.key), false);
            rows.push(Row {
                key: key.into(),
                ..leftover_row(app)
            });
        }
    }
}

/// The expanded app's last row: what is charged to it but to none of its processes it read.
fn leftover_row(app: &App) -> Row {
    let mut name = "Not attributed to processes".to_string();
    let mut tip = format!(
        "Memory charged to {} but to none of its processes",
        app.name
    );
    if !app.unreadable.is_empty() {
        name += &format!(" · {} unreadable", app.unreadable.len());
        tip += " it could read";
    }
    tip += ": kernel memory and zswap for the app";
    let (denied, failed): (Vec<_>, Vec<_>) = app.unreadable.iter().partition(|u| u.denied);
    let list = |procs: &[&Unreadable]| {
        procs
            .iter()
            .map(|u| format!("{} ({})", u.name, u.pid))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !denied.is_empty() {
        let (runs, it) = if denied.len() == 1 {
            ("runs", "it")
        } else {
            ("run", "them")
        };
        tip += &format!(
            ", and {}, which {runs} as another user so SebWatch may not read {it}",
            list(&denied)
        );
    }
    if !failed.is_empty() {
        tip += &format!(", and {}, whose memory could not be read", list(&failed));
    }
    tip += ".";
    if app.over_charge > 0 {
        name += &format!(" · processes show {} more", bytes(app.over_charge));
        tip += &format!(
            " Its processes add up to {} more than {} is charged for: they share pages with \
             other apps' processes, and the kernel charges each shared page to one app.",
            bytes(app.over_charge),
            app.name
        );
    }
    Row {
        kind: RowKind::Kernel,
        depth: 1,
        name: name.into(),
        ram: bytes(app.unattributed_ram).into(),
        swap: bytes(app.unattributed_swap).into(),
        tip: tip.into(),
        ..empty_row()
    }
}

/// A figure that doesn't apply ("0" is zero, "n/a" a reading that failed).
const NONE: &str = "—";
/// A figure that couldn't be read.
const NA: &str = "n/a";

/// Whether a search (in lower case) finds the app: part of its name or one of its groups' names,
/// or one of its PIDs in full.
fn matches(app: &App, query: &str) -> bool {
    let found = |text: &str| text.to_lowercase().contains(query);
    found(&app.name)
        || found(&app.key)
        || app.groups.iter().any(|g| found(g))
        || query.parse().is_ok_and(|pid: u32| app.pids.contains(&pid))
}

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
        tip: "".into(),
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
        description: "".into(),
    }
}

/// The tooltip's lines in one, for screen readers.
fn describe(segment: &Segment) -> SharedString {
    let mut text = segment.tip.to_string();
    for note in slint::Model::iter(&segment.notes) {
        text = format!("{text}; {note}");
    }
    text.into()
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
        segment.description = describe(segment);
        start += segment.fraction;
    }
}

fn bar(title: &str, total: String, segments: Vec<Segment>, note: &str) -> Bar {
    Bar {
        title: title.into(),
        total: total.into(),
        segments: ModelRc::from(Rc::new(VecModel::from(segments))),
        note: note.into(),
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
    // Its cache stays in File cache, as every app's does.
    let cache = (s.closed.file_cache > 0).then(|| {
        format!(
            "plus {} of file cache, freed when apps need the room",
            bytes(s.closed.file_cache)
        )
    });
    segments.extend(closed_segment(
        &s.closed,
        s.closed.ram,
        total,
        "in RAM",
        cache,
    ));
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
        if s.unsplit.is_some() {
            segment(
                SegmentKind::Kernel,
                "Used, not split by app",
                s.kernel,
                total,
                &["this system can't charge memory to apps".into()],
            )
        } else {
            segment(
                SegmentKind::Kernel,
                "Kernel, drivers and unattributed",
                s.kernel,
                total,
                &["includes memory no app could be charged for".into()],
            )
        },
        segment(SegmentKind::Free, "Free", s.free, total, &[]),
    ]);
    if s.unsplit.is_some() {
        // No apps: memory in use leads, where they would be.
        let used = segments.remove(segments.len() - 2);
        segments.insert(0, used);
    }
    stack(&mut segments);
    let used = format!("{} used of {}", bytes(total - s.free), bytes(total));
    bar("Memory", used, segments, "")
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
    let used = match &s.unsplit {
        Some(unsplit) => {
            segments.push(segment(
                SegmentKind::Other,
                "Used swap",
                unsplit.swap_used,
                total,
                &[],
            ));
            unsplit.swap_used
        }
        None => s.apps.iter().map(|a| a.swap).sum::<u64>() + s.closed.swap,
    };
    segments.extend(others(&s.apps, named, |a| a.swap, total));
    segments.extend(closed_segment(
        &s.closed,
        s.closed.swap,
        total,
        "in swap",
        None,
    ));
    segments.push(segment(
        SegmentKind::Free,
        "Free swap",
        total.saturating_sub(used),
        total,
        &[],
    ));
    stack(&mut segments);
    bar(
        "Swap",
        format!("{} used of {}", bytes(used), bytes(total)),
        segments,
        "",
    )
}

/// VRAM by app from NVML's per-process figures, the rest of the used VRAM as "Driver and other".
/// Empty while the GPU sleeps or its driver doesn't answer.
fn vram_bar(s: &Snapshot, series: &Series, named: &[&App]) -> Option<Bar> {
    let Some((used, total)) = s.vram else {
        if s.nvidia_unavailable {
            let note = "The NVIDIA driver is not answering";
            return Some(bar("VRAM", "unavailable".into(), Vec::new(), note));
        }
        return s
            .gpu_asleep
            .then(|| bar("VRAM", "GPU asleep".into(), Vec::new(), ""));
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
            if s.unsplit.is_some() {
                "Used"
            } else {
                "Driver and other"
            },
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
        "VRAM",
        format!("{} used of {}", bytes(used), bytes(total)),
        segments,
        "",
    ))
}

/// The graphed figures of a reading, in `Measure` order: CPU, GPU and VRAM as a share of the
/// whole, Disk in bytes/s. A sleeping GPU has none.
fn readings(s: &Snapshot) -> [Option<f32>; 4] {
    [
        s.cpu_used.map(|cpu| cpu / s.cpu_cores.max(1) as f32),
        s.nvidia_percent
            .filter(|_| !s.gpu_asleep)
            .map(|p| p / 100.0),
        s.vram.map(|(used, total)| share(used, total)),
        s.disk_per_s.map(|d| d as f32),
    ]
}

fn tile(name: &str, value: String, unit: String, graph: Option<(&History, Measure)>) -> Tile {
    let (line, area) = graph
        .map(|(h, measure)| h.graph(measure))
        .unwrap_or_default();
    Tile {
        name: name.into(),
        value: value.into(),
        unit: unit.into(),
        graphed: graph.is_some(),
        line: line.into(),
        area: area.into(),
    }
}

/// A tile whose reading failed: it says so, with no figure and no graph.
fn unavailable(name: &str) -> Tile {
    tile(name, String::new(), UNAVAILABLE.into(), None)
}

const UNAVAILABLE: &str = "unavailable";

fn tiles(s: &Snapshot, history: &History) -> Vec<Tile> {
    let mut tiles = vec![match s.cpu_used {
        Some(cpu) => tile(
            "CPU",
            format!("{cpu:.1}"),
            format!("of {} cores", s.cpu_cores),
            Some((history, Measure::Cpu)),
        ),
        None => unavailable("CPU"),
    }];
    // A sleeping or silent GPU keeps its tiles, saying so in place of the figures and graphs not
    // read (no value: the words need the tile's width).
    let intel = |unit: &str| match s.intel_percent {
        Some(intel) => format!("{unit} · {} Intel", percent(intel)),
        None => unit.into(),
    };
    if s.nvidia_unavailable {
        tiles.push(tile("GPU", String::new(), intel(UNAVAILABLE), None));
        tiles.push(unavailable("VRAM"));
    } else {
        if let Some(nvidia) = s.nvidia_percent {
            let (value, unit) = if s.gpu_asleep {
                (String::new(), "NVIDIA asleep")
            } else {
                (percent(nvidia), "NVIDIA")
            };
            let graph = (!s.gpu_asleep).then_some((history, Measure::Gpu));
            tiles.push(tile("GPU", value, intel(unit), graph));
        }
        if let Some((used, total)) = s.vram {
            tiles.push(tile(
                "VRAM",
                format!("{:.1}", used as f64 / GB),
                format!("of {:.1} GB", total as f64 / GB),
                Some((history, Measure::Vram)),
            ));
        } else if s.gpu_asleep {
            tiles.push(tile("VRAM", String::new(), "GPU asleep".into(), None));
        }
    }
    tiles.push(match s.disk_per_s {
        Some(disk) => tile(
            "Disk",
            format!("{:.1}", disk as f64 / MB),
            "MB/s read + write".into(),
            Some((history, Measure::Disk)),
        ),
        None => unavailable("Disk"),
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
        // Their graphs go; CPU and Disk keep theirs.
        let graphed: Vec<_> = asleep.tiles.iter().map(|t| t.graphed).collect();
        assert_eq!(graphed, [true, false, false, true]);
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
        s.closed.swap = 0;
        let shown = View::default().update(s);
        let swap = names(&shown.swap);
        let vram = names(&shown.vram.unwrap());
        assert_eq!(swap[1..], ["Other apps", "Free swap"]);
        assert_eq!(vram[1], "Other apps");
        assert_ne!(swap[0], vram[0]);
    }

    #[test]
    fn search_finds_apps_by_name_group_or_pid_folded_or_not() {
        let mut view = View::default();
        let all = view.update(crate::sample::snapshot());
        assert_eq!(all.filtered, "");
        let find = |view: &mut View, search: &str| {
            view.search = search.into();
            let shown = view.render();
            let names: Vec<String> = shown.rows.iter().map(|r| r.name.to_string()).collect();
            (names, shown.filtered)
        };
        // Konsole, Akonadi and Kate sit in the fold: found, they show without it. Okular has
        // exited, so its memory is found under Closed apps.
        let (found, note) = find(&mut view, "K");
        assert_eq!(
            found,
            [
                "Plasma Workspace",
                "KWin",
                "Konsole",
                "Akonadi",
                "Kate",
                "Closed apps (cache)"
            ]
        );
        assert_eq!(
            note,
            "Filtered: 5 of 18 apps · the bars show the whole system"
        );
        assert_eq!(find(&mut view, "guide35.service").0, ["Android emulator"]);
        assert_eq!(find(&mut view, "firefox").0, ["Closed apps (cache)"]);
        assert_eq!(find(&mut view, " 4130291 ").0, ["Android emulator"]);
        // A PID is matched whole, not by its digits.
        assert!(find(&mut view, "413029").0.is_empty());
        // Cleared: the listed apps, the fold and Closed apps.
        assert_eq!(find(&mut view, "").0.len(), LISTED + 2);
        // The bars don't change with the search.
        view.search = "kate".into();
        let shown = view.render();
        assert_eq!(names(&shown.memory), names(&all.memory));
    }

    #[test]
    fn expanded_app_lists_its_processes_and_kernel_share() {
        let mut view = View::default();
        let collapsed = view.update(crate::sample::snapshot()).rows.len();
        view.toggle("Android emulator");
        let rows = view.render().rows;
        assert_eq!(rows.len(), collapsed + 5);
        assert_eq!(rows[2].name, "qemu-system-x86_64");
        assert_eq!((rows[5].name.as_str(), rows[5].ram.as_str()), ("adb", NONE));
        assert_eq!(rows[6].kind, RowKind::Kernel);
        assert_eq!(rows[6].depth, 1);
        view.toggle("Android emulator");
        assert_eq!(view.render().rows.len(), collapsed);
    }

    #[test]
    fn keys_move_through_rows_and_expand_and_fold_apps() {
        let mut view = View::default();
        let rows = view.update(crate::sample::snapshot()).rows.len() as i32;
        let mut press = |key| {
            let toggled = view.press(key);
            (view.render().focus, toggled)
        };
        // The first key only shows the focus, on the first row.
        assert_eq!(press(TableKey::Down), (0, false));
        assert_eq!(press(TableKey::Down), (1, false));
        // Right opens the Android emulator, then steps into its processes.
        assert_eq!(press(TableKey::Right), (1, true));
        assert_eq!(press(TableKey::Right), (2, false));
        assert_eq!(press(TableKey::Down), (3, false));
        // Left from a process goes back to its app, then folds it.
        assert_eq!(press(TableKey::Left), (1, false));
        assert_eq!(press(TableKey::Left), (1, true));
        assert_eq!(press(TableKey::Up), (0, false));
        assert_eq!(press(TableKey::Up), (0, false));
        // Closed apps, last, open like an app; Left from an exited app returns to them.
        assert_eq!(press(TableKey::End), (rows - 1, false));
        assert_eq!(press(TableKey::Toggle), (rows - 1, true));
        assert_eq!(press(TableKey::Down), (rows, false));
        assert_eq!(press(TableKey::Left), (rows - 1, false));
        // The "N more apps" fold above them too.
        assert_eq!(press(TableKey::Up), (rows - 2, false));
        assert_eq!(press(TableKey::Right), (rows - 2, true));
        assert_eq!(press(TableKey::Right), (rows - 1, false));
        assert_eq!(press(TableKey::Left), (rows - 2, false));
        assert_eq!(press(TableKey::Home), (0, false));
        assert!(view.pick(1));
        assert_eq!(view.render().focus, 1);
        assert!(view.expanded().contains("Android emulator"));
    }

    #[test]
    fn focus_follows_its_row_across_refreshes_and_searches() {
        let mut view = View::default();
        view.update(crate::sample::snapshot());
        view.pick(1);
        view.render();
        // Android emulator's netsimd.
        view.press(TableKey::Right);
        view.press(TableKey::Down);
        assert_eq!(view.render().rows[3].name, "netsimd");

        // Android emulator overtakes Claude agents: the focus moves with its row.
        let mut s = crate::sample::snapshot();
        s.apps[1].ram = s.apps[0].ram * 2;
        let shown = view.update(s);
        assert_eq!(shown.focus, 2);
        assert_eq!(shown.rows[2].name, "netsimd");

        // netsimd ends: the focus goes to its app.
        let mut s = crate::sample::snapshot();
        s.apps[1].parts.retain(|p| p.name != "netsimd");
        let shown = view.update(s);
        assert_eq!(shown.focus, 1);
        assert_eq!(shown.rows[1].name, "Android emulator");

        // A search that keeps the app keeps the focus on it.
        view.search = "guide35.service".into();
        let shown = view.render();
        assert_eq!(shown.rows[shown.focus as usize].name, "Android emulator");
        view.search = "".into();
        assert_eq!(view.render().focus, 1);
    }

    #[test]
    fn closed_apps_come_last_hatched_and_list_what_they_hold() {
        // Sorted by swap, where it would otherwise sit among the apps.
        let mut view = View {
            sort: Sort::default().click(Column::Swap),
            ..Default::default()
        };
        let shown = view.update(crate::sample::snapshot());
        let last = shown.rows.last().unwrap();
        assert_eq!(
            (last.key.as_str(), last.name.as_str(), last.detail.as_str()),
            (CLOSED, CLOSED_APPS, "3 apps")
        );
        assert_eq!(
            (last.ram.as_str(), last.cache.as_str()),
            ("412 MB", "398 MB")
        );

        // In the RAM bar right after Other apps; its cache stays in File cache.
        let memory: Vec<_> = slint::Model::iter(&shown.memory.segments).collect();
        let kinds: Vec<_> = memory.iter().map(|seg| seg.kind).collect();
        let other = kinds.iter().position(|&k| k == SegmentKind::Other).unwrap();
        assert_eq!(kinds[other + 1], SegmentKind::Closed);
        let closed = &memory[other + 1];
        assert_eq!(
            (closed.name.as_str(), closed.size.as_str()),
            (CLOSED_APPS, "412 MB")
        );
        let notes: Vec<_> = slint::Model::iter(&closed.notes).collect();
        assert_eq!(
            notes,
            [
                "still charged to 3 apps that have exited",
                "plus 398 MB of file cache, freed when apps need the room"
            ]
        );

        view.toggle(CLOSED);
        let rows = view.render().rows;
        let children: Vec<_> = rows[rows.len() - 3..]
            .iter()
            .map(|r| (r.name.as_str(), r.detail.as_str(), r.ram.as_str(), r.depth))
            .collect();
        assert_eq!(
            children,
            [
                ("Okular", "exited", "214 MB", 1),
                ("Firefox", "exited", "151 MB", 1),
                ("dnf makecache", "exited", "47 MB", 1),
            ]
        );
        assert_eq!(rows[rows.len() - 4].key, CLOSED);

        // Nothing charged to closed apps: no row, no segment.
        let none = Snapshot {
            closed: Closed::default(),
            ..crate::sample::snapshot()
        };
        let shown = View::default().update(none);
        assert!(shown.rows.iter().all(|r| r.key != CLOSED));
        assert!(slint::Model::iter(&shown.memory.segments).all(|s| s.kind != SegmentKind::Closed));
    }

    /// The leftover row of the sample's Android emulator, changed by `edit`.
    fn leftover(edit: impl FnOnce(&mut App)) -> Row {
        let mut s = crate::sample::snapshot();
        let app = s
            .apps
            .iter_mut()
            .find(|a| a.key == "Android emulator")
            .unwrap();
        edit(app);
        let mut view = View::default();
        view.toggle("Android emulator");
        let rows = view.update(s).rows;
        rows.into_iter()
            .find(|r| r.kind == RowKind::Kernel)
            .unwrap()
    }

    #[test]
    fn leftover_row_says_what_it_holds() {
        let none = leftover(|a| a.unreadable.clear());
        assert_eq!(none.name, "Not attributed to processes");
        assert_eq!(none.ram, "1.2 GB");
        assert_eq!(
            none.tip,
            "Memory charged to Android emulator but to none of its processes: kernel memory and \
             zswap for the app."
        );

        let some = leftover(|a| {
            a.unreadable.push(Unreadable {
                name: "helper".into(),
                pid: 7,
                denied: false,
            })
        });
        assert_eq!(some.name, "Not attributed to processes · 2 unreadable");
        assert_eq!(
            some.tip,
            "Memory charged to Android emulator but to none of its processes it could read: \
             kernel memory and zswap for the app, and adb (4130302), which runs as another user \
             so SebWatch may not read it, and helper (7), whose memory could not be read."
        );

        let over = leftover(|a| {
            a.unreadable.clear();
            a.unattributed_ram = 0;
            a.over_charge = 12 * 1024 * 1024;
        });
        assert_eq!(
            over.name,
            "Not attributed to processes · processes show 12 MB more"
        );
        assert_eq!(over.ram, "0");
        assert!(over.tip.ends_with(
            "Its processes add up to 12 MB more than Android emulator is charged for: they \
             share pages with other apps' processes, and the kernel charges each shared page to \
             one app."
        ));
    }

    fn tile_texts(shown: &Shown) -> Vec<(String, String, bool)> {
        shown
            .tiles
            .iter()
            .map(|t| (t.name.to_string(), t.unit.to_string(), t.graphed))
            .collect()
    }

    #[test]
    fn failed_readings_say_unavailable_and_zero_stays_zero() {
        let mut s = crate::sample::unavailable();
        s.cpu_used = None;
        // The emulator expanded, with a group whose `io.stat` failed beside its processes.
        let emulator = &mut s.apps[1];
        emulator.parts.insert(
            0,
            Part {
                group: true,
                name: "app-guide35.scope".into(),
                detail: String::new(),
                ram: Some(1),
                swap: Some(1),
                cpu: 0.0,
                disk_per_s: None,
            },
        );
        let mut view = View::default();
        view.toggle(&emulator.key.clone());
        let shown = view.update(s);
        let tiles = tile_texts(&shown);
        let t = |name: &str, unit: &str| (name.to_string(), unit.to_string(), false);
        assert_eq!(
            tiles,
            [
                t("CPU", "unavailable"),
                t("GPU", "unavailable · 3% Intel"),
                t("VRAM", "unavailable"),
                t("Disk", "unavailable"),
            ]
        );
        assert!(shown.tiles.iter().all(|t| t.value.is_empty()));
        let vram = shown.vram.unwrap();
        assert_eq!(vram.total, "unavailable");
        assert_eq!(vram.note, "The NVIDIA driver is not answering");
        let apps: Vec<_> = shown
            .rows
            .iter()
            .filter(|r| r.kind == RowKind::App)
            .collect();
        assert!(
            apps.iter()
                .all(|r| r.disk == NA && r.gpu == NA && r.vram == NA)
        );
        let disk = |name: &str| {
            let row = shown.rows.iter().find(|r| r.name == name).unwrap();
            row.disk.to_string()
        };
        // A group's disk failed; a process's never applies (known per group only).
        assert_eq!(disk("app-guide35.scope"), NA);
        assert_eq!(disk("qemu-system-x86_64"), NONE);

        // A reading that worked: zero is "0", a figure that doesn't apply "—".
        let shown = View::default().update(crate::sample::snapshot());
        let steam = shown.rows.iter().find(|r| r.name == "Steam").unwrap();
        assert_eq!((steam.disk.as_str(), steam.gpu.as_str()), ("0", NONE));
    }

    #[test]
    fn failed_reads_keep_the_last_reading_with_a_warning_until_one_works() {
        let mut view = View::default();
        assert_eq!(view.update(crate::sample::snapshot()).warning, "");
        let once = view
            .failed("cannot read /proc/meminfo: gone".into())
            .warning;
        assert!(
            once.starts_with(
                "Not updating: the last reading failed (cannot read /proc/meminfo: gone). \
                 Showing the reading from "
            ),
            "{once}"
        );
        let shown = view.failed("cannot read /sys/fs/cgroup: gone".into());
        assert!(shown.warning.starts_with(
            "Not updating: the last 2 readings failed (cannot read /sys/fs/cgroup: gone)"
        ));
        // The time of the reading shown, "HH:MM:SS".
        let time = shown
            .warning
            .trim_end_matches('.')
            .rsplit(' ')
            .next()
            .unwrap();
        assert!(time.len() == 8 && time.split(':').count() == 3, "{time}");
        assert_eq!(names(&shown.memory), names(&view.render().memory));
        assert_eq!(view.update(crate::sample::snapshot()).warning, "");
    }

    #[test]
    fn a_system_that_cannot_split_memory_shows_totals_and_says_why() {
        let shown = View::default().update(crate::sample::unsplit());
        assert!(shown.unsplit);
        assert!(shown.rows.is_empty());
        assert_eq!(
            names(&shown.memory),
            ["Used, not split by app", "Zswap pool", "File cache", "Free"]
        );
        let sum: f32 = slint::Model::iter(&shown.memory.segments)
            .map(|s| s.fraction)
            .sum();
        assert!((sum - 1.0).abs() < 1e-4, "RAM bar sums to {sum}");
        assert_eq!(names(&shown.swap), ["Used swap", "Free swap"]);
        assert_eq!(shown.swap.total, "15.6 GB used of 32.0 GB");
        assert_eq!(names(&shown.vram.unwrap()), ["Used", "Free"]);
    }
}
