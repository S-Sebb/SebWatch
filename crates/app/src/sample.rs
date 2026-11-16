//! A fixed reading with the approved mockup's numbers (this PC, 2026-11-15), shown until the
//! window reads `sebwatch-core`.

use crate::model::{App, Part, Snapshot};

const K: f64 = 1024.0;
const M: f64 = K * K;
const G: f64 = M * K;

fn b(n: f64, unit: f64) -> u64 {
    (n * unit) as u64
}

#[allow(clippy::too_many_arguments)]
fn app(
    name: &str,
    detail: &str,
    icon: &str,
    ram: u64,
    swap: u64,
    cache: u64,
    cpu: f32,
    disk: u64,
) -> App {
    App {
        key: name.into(),
        name: name.into(),
        detail: detail.into(),
        icon: icon.into(),
        ram,
        zswap: None,
        swap,
        file_cache: cache,
        cpu,
        disk_per_s: disk,
        gpu_percent: None,
        vram: None,
        parts: Vec::new(),
        unattributed_ram: 0,
        unattributed_swap: 0,
    }
}

fn part(name: &str, pid: u32, ram: u64, swap: u64, cpu: f32, disk: u64) -> Part {
    Part {
        group: false,
        name: name.into(),
        detail: pid.to_string(),
        ram: Some(ram),
        swap: Some(swap),
        cpu,
        disk_per_s: Some(disk),
    }
}

pub fn snapshot() -> Snapshot {
    let mut apps = vec![
        app(
            "Claude agents",
            "17 agents",
            "utilities-terminal",
            b(3.3, G),
            b(1.8, G),
            b(308.0, M),
            1.12,
            b(41.0, K),
        ),
        App {
            zswap: Some(b(461.0, M)),
            parts: vec![
                part(
                    "qemu-system-x86_64",
                    4130176,
                    b(1.5, G),
                    b(2.6, G),
                    0.85,
                    b(1.9, M),
                ),
                part("netsimd", 4130291, b(5.8, M), b(4.2, M), 0.01, 0),
                part(
                    "crashpad_handler",
                    4130195,
                    b(102.0, K),
                    b(307.0, K),
                    0.0,
                    0,
                ),
                // #37: a process SebWatch may not read.
                Part {
                    ram: None,
                    swap: None,
                    disk_per_s: None,
                    ..part("adb", 4130302, 0, 0, 0.0, 0)
                },
            ],
            unattributed_ram: b(1.2, G),
            unattributed_swap: b(296.0, M),
            ..app(
                "Android emulator",
                "· guide35",
                // As core names it: no desktop file, so the service icon.
                sebwatch_core::naming::SERVICE_ICON,
                b(2.7, G),
                b(2.9, G),
                b(26.0, M),
                0.86,
                b(1.9, M),
            )
        },
        App {
            gpu_percent: Some(2.0),
            vram: Some(b(118.0, M)),
            ..app(
                "Zen Browser",
                "",
                "app.zen_browser.zen",
                b(1.5, G),
                b(1.5, G),
                b(183.0, M),
                0.12,
                b(3.5, M),
            )
        },
        App {
            gpu_percent: Some(1.0),
            vram: Some(b(64.0, M)),
            ..app(
                "T3 Code",
                "2 groups",
                "/home/s_sebb/.local/share/icons/com.t3tools.T3Code.desktop.png",
                b(1.3, G),
                b(654.0, M),
                b(544.0, M),
                0.20,
                b(3.5, M),
            )
        },
        app(
            "Steam",
            "",
            "steam",
            b(441.0, M),
            b(1.6, G),
            b(74.0, M),
            0.24,
            0,
        ),
        app(
            "OpenCode agents",
            "",
            "utilities-terminal",
            b(410.0, M),
            b(249.0, M),
            b(80.0, M),
            0.01,
            0,
        ),
        app(
            "Plasma Workspace",
            "",
            "plasmashell",
            b(293.0, M),
            b(379.0, M),
            b(6.0, M),
            0.0,
            0,
        ),
        app(
            "WeChat",
            "",
            "/opt/wechat/icons/wechat.png",
            b(212.0, M),
            b(1.4, G),
            b(58.0, M),
            0.0,
            0,
        ),
        app(
            "Discord",
            "",
            "discord",
            b(190.0, M),
            b(642.0, M),
            b(68.0, M),
            0.0,
            b(10.0, K),
        ),
        app(
            "Zscaler",
            "",
            "/opt/zscaler/Images/ZscalerAppTop.png",
            b(161.0, M),
            b(126.0, M),
            b(11.0, M),
            0.02,
            0,
        ),
        App {
            gpu_percent: Some(1.0),
            vram: Some(b(11.0, M)),
            ..app(
                "KWin",
                "",
                "kwin",
                b(127.0, M),
                b(301.0, M),
                b(11.0, M),
                0.03,
                0,
            )
        },
        // The rest, under "7 more apps": 702 MB as in the mockup.
        app(
            "Dolphin",
            "",
            "system-file-manager",
            b(120.0, M),
            b(1.1, G),
            b(20.0, M),
            0.0,
            0,
        ),
        app(
            "Konsole",
            "",
            "utilities-terminal",
            b(110.0, M),
            b(0.9, G),
            b(12.0, M),
            0.0,
            0,
        ),
        app(
            "PipeWire",
            "",
            "audio-card",
            b(100.0, M),
            b(0.6, G),
            b(2.0, M),
            0.01,
            0,
        ),
        app(
            "Akonadi",
            "",
            "akonadi",
            b(98.0, M),
            b(0.5, G),
            b(9.0, M),
            0.0,
            0,
        ),
        app("Kate", "", "kate", b(95.0, M), b(0.3, G), b(4.0, M), 0.0, 0),
        app(
            "Spectacle",
            "",
            "spectacle",
            b(90.0, M),
            b(0.4, G),
            b(9.0, M),
            0.0,
            0,
        ),
        app(
            "Baloo",
            "",
            "baloo",
            b(89.0, M),
            b(0.25, G),
            b(30.0, M),
            0.0,
            0,
        ),
    ];
    apps.sort_by_key(|a| std::cmp::Reverse(a.ram));

    let ram_total = b(15.3, G);
    let (zswap_pool, file_cache, free) = (b(50.0, M), b(2.1, G), b(868.0, M));
    let apps_ram: u64 = apps.iter().map(|a| a.ram).sum();
    Snapshot {
        ram_total,
        swap_total: b(32.0, G),
        zswap_pool,
        file_cache,
        // The remainder, as in docs/accounting.md.
        kernel: ram_total - apps_ram - zswap_pool - file_cache - free,
        free,
        apps,
        cpu_used: 2.6,
        cpu_cores: 16,
        nvidia_percent: Some(0.0),
        intel_percent: Some(3.0),
        vram: Some((b(0.3, G), b(8.0, G))),
        gpu_asleep: false,
        disk_per_s: b(9.0, M),
    }
}

/// The same reading with the NVIDIA GPU asleep, as `live.rs` gives it: the busy figure 0%, and
/// no VRAM or per-app NVIDIA figures, as reading them would wake it.
pub fn asleep() -> Snapshot {
    let mut s = snapshot();
    s.nvidia_percent = Some(0.0);
    s.vram = None;
    s.gpu_asleep = true;
    for app in &mut s.apps {
        app.gpu_percent = None;
        app.vram = None;
    }
    s
}

// The 0.1 mockups (#33 to #40): states faked on top of today's view model, no behaviour.
pub mod mock {
    use std::rc::Rc;

    use slint::{Model, ModelRc, SharedString, VecModel};

    use super::{M, b};
    use crate::model::{App, Shown, Snapshot, View, bytes};
    use crate::{Bar, Row, RowKind, Segment, SegmentKind, Tile};

    /// #40: memory still charged to groups whose processes exited.
    pub const CLOSED: [(&str, f64, f64); 3] = [
        ("Okular", 214.0, 205.0),
        ("Firefox (Flatpak)", 151.0, 148.0),
        ("dnf makecache", 47.0, 45.0),
    ];

    pub fn closed() -> u64 {
        CLOSED.iter().map(|c| b(c.1, M)).sum()
    }

    /// The sample with the closed apps' memory taken out of the kernel leftover.
    pub fn snapshot() -> Snapshot {
        let mut s = super::snapshot();
        s.kernel -= closed();
        s
    }

    /// One colour per app across all bars: the RAM top five, then WeChat (swap top five) and
    /// KWin (VRAM).
    fn colour(name: &str) -> i32 {
        [
            "Claude agents",
            "Android emulator",
            "Zen Browser",
            "T3 Code",
            "Steam",
            "WeChat",
            "KWin",
        ]
        .iter()
        .position(|n| *n == name)
        .map_or(-1, |i| i as i32)
    }

    fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
        ModelRc::from(Rc::new(VecModel::from(items)))
    }

    fn items<T: Clone + 'static>(m: &ModelRc<T>) -> Vec<T> {
        m.iter().collect()
    }

    fn seg(
        kind: SegmentKind,
        series: i32,
        name: &str,
        size: u64,
        total: u64,
        tip: &str,
        notes: &[&str],
    ) -> Segment {
        let fraction = size as f32 / total as f32;
        Segment {
            kind,
            series,
            name: name.into(),
            detail: "".into(),
            fraction,
            start: 0.0,
            size: bytes(size).into(),
            labelled: fraction >= 0.075,
            tip: format!(
                "{} {tip}, {:.0}% of {}",
                bytes(size),
                fraction * 100.0,
                bytes(total)
            )
            .into(),
            notes: model(notes.iter().map(|n| SharedString::from(*n)).collect()),
        }
    }

    fn stack(segments: &mut [Segment]) {
        let mut start = 0.0;
        for s in segments {
            s.start = start;
            start += s.fraction;
        }
    }

    /// #33: a bar naming its own top five apps by `size`.
    fn top_five(apps: &[App], size: impl Fn(&App) -> u64, total: u64, place: &str) -> Vec<Segment> {
        let mut ranked: Vec<&App> = apps.iter().filter(|a| size(a) > 0).collect();
        ranked.sort_by_key(|a| std::cmp::Reverse(size(a)));
        let mut segments: Vec<Segment> = ranked
            .iter()
            .take(5)
            .map(|a| {
                seg(
                    SegmentKind::App,
                    colour(&a.name),
                    &a.name,
                    size(a),
                    total,
                    place,
                    &[],
                )
            })
            .collect();
        let rest = &ranked[ranked.len().min(5)..];
        if !rest.is_empty() {
            let n = format!("{} apps", rest.len());
            segments.push(seg(
                SegmentKind::Other,
                -1,
                "Other apps",
                rest.iter().map(|a| size(a)).sum(),
                total,
                place,
                &[n.as_str()],
            ));
        }
        segments
    }

    /// The proposed main view: RAM bar with the renamed leftover and closed apps, swap and VRAM
    /// bars with their own top fives, rows sorted by `sort` (0 RAM, 1 swap), `expanded` open.
    pub fn shown(make: fn() -> Snapshot, sort: usize, expanded: &[&str]) -> Shown {
        let s = make();
        let mut view = View::default();
        for key in expanded {
            view.toggle(key);
        }
        let apps = make().apps;
        let mut sorted = make();
        if sort == 1 {
            sorted.apps.sort_by_key(|a| std::cmp::Reverse(a.swap));
        }
        let mut shown = view.update(sorted);

        // RAM bar: one colour per app, the leftover renamed (#37), closed apps (#40).
        let mut memory = items(&shown.memory.segments);
        for seg in &mut memory {
            if seg.kind == SegmentKind::App {
                seg.series = colour(&seg.name);
            }
            if seg.kind == SegmentKind::Kernel {
                seg.name = "Kernel, drivers and unattributed".into();
                seg.notes = model(vec!["includes memory no app could be charged for".into()]);
            }
        }
        if sort == 1 {
            // The RAM bar keeps its own top five whatever the table's sort.
            memory = items(&View::default().update(make()).memory.segments);
            for seg in &mut memory {
                if seg.kind == SegmentKind::App {
                    seg.series = colour(&seg.name);
                }
                if seg.kind == SegmentKind::Kernel {
                    seg.name = "Kernel, drivers and unattributed".into();
                }
            }
        }
        let other = memory
            .iter()
            .position(|s| s.kind == SegmentKind::Other)
            .unwrap()
            + 1;
        memory.insert(
            other,
            seg(
                SegmentKind::Closed,
                -1,
                "Closed apps (cache)",
                closed(),
                s.ram_total,
                "in RAM",
                &[
                    "still charged to 3 apps that have exited",
                    "mostly cache, freed when apps need the room",
                ],
            ),
        );
        stack(&mut memory);
        shown.memory.segments = model(memory);

        let mut swap = top_five(&apps, |a| a.swap, s.swap_total, "in swap");
        let used: u64 = apps.iter().map(|a| a.swap).sum();
        swap.push(seg(
            SegmentKind::Free,
            -1,
            "Free swap",
            s.swap_total - used,
            s.swap_total,
            "",
            &[],
        ));
        stack(&mut swap);
        shown.swap.segments = model(swap);

        if let Some((used, total)) = s.vram {
            let mut vram = top_five(&apps, |a| a.vram.unwrap_or(0), total, "in VRAM");
            let by_apps: u64 = apps.iter().filter_map(|a| a.vram).sum();
            vram.push(seg(
                SegmentKind::Kernel,
                -1,
                "Driver and other",
                used - by_apps,
                total,
                "",
                &["used, but by no app's process"],
            ));
            vram.push(seg(
                SegmentKind::Free,
                -1,
                "Free",
                total - used,
                total,
                "",
                &[],
            ));
            stack(&mut vram);
            shown.vram.as_mut().unwrap().segments = model(vram);
        }

        // Rows: one colour per app, the renamed remainder with its mark (#37), closed apps (#40).
        let mut rows = Vec::new();
        for mut row in shown.rows.drain(..) {
            if row.kind == RowKind::App {
                row.series = colour(&row.name);
            }
            if row.kind == RowKind::Kernel {
                row.name = "Memory not attributed to processes".into();
                row.detail = "· incl. adb, unreadable".into();
            }
            rows.push(row);
        }
        let closed_open = expanded.contains(&"closed");
        rows.push(Row {
            kind: RowKind::More,
            key: "closed".into(),
            name: "Closed apps (cache)".into(),
            detail: "3 apps".into(),
            expanded: closed_open,
            series: -1,
            ram: bytes(closed()).into(),
            swap: bytes(b(21.0, M)).into(),
            cache: bytes(CLOSED.iter().map(|c| b(c.2, M)).sum()).into(),
            ..blank()
        });
        if closed_open {
            for (name, ram, cache) in CLOSED {
                rows.push(Row {
                    kind: RowKind::Process,
                    depth: 1,
                    name: name.into(),
                    detail: "exited".into(),
                    ram: bytes(b(ram, M)).into(),
                    cache: bytes(b(cache, M)).into(),
                    ..blank()
                });
            }
        }
        shown.rows = rows;
        shown
    }

    fn blank() -> Row {
        let none = SharedString::from("—");
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

    /// #34: the table filtered by "k": matching apps, folded ones included, no fold row.
    pub fn filtered() -> (Shown, String) {
        let total = snapshot().apps.len();
        let mut shown = shown(snapshot, 0, &[]);
        // Kate, Konsole and Akonadi sit in the fold; matching brings them out.
        let mut view = View::default();
        let mut found = snapshot();
        found.apps.retain(|a| a.name.to_lowercase().contains('k'));
        let n = found.apps.len();
        let mut rows = view.update(found).rows;
        for r in &mut rows {
            r.series = colour(&r.name);
        }
        shown.rows = rows;
        (
            shown,
            format!("Filtered: {n} of {total} apps · the bars show the whole system"),
        )
    }

    /// #38: no io.stat (disk) and NVML not answering (GPU, VRAM).
    pub fn unavailable() -> Shown {
        let mut shown = shown(snapshot, 0, &[]);
        shown.tiles = items(&model(shown.tiles))
            .into_iter()
            .map(|t: Tile| match t.name.as_str() {
                "GPU" => Tile {
                    value: "".into(),
                    unit: "unavailable · 3% Intel".into(),
                    fraction: -1.0,
                    ..t
                },
                "VRAM" => Tile {
                    value: "".into(),
                    unit: "unavailable".into(),
                    fraction: -1.0,
                    ..t
                },
                "Disk" => Tile {
                    value: "".into(),
                    unit: "unavailable".into(),
                    ..t
                },
                _ => t,
            })
            .collect();
        let na = SharedString::from("n/a");
        for r in &mut shown.rows {
            if r.kind == RowKind::App {
                r.disk = na.clone();
                r.gpu = na.clone();
                r.vram = na.clone();
            }
        }
        shown.vram = Some(Bar {
            title: "VRAM".into(),
            total: "unavailable".into(),
            segments: model(Vec::new()),
        });
        shown
    }

    /// #38: no per-app memory accounting: totals only, the leftover named for what it is.
    pub fn unaccounted() -> Shown {
        let mut s = snapshot();
        let apps_ram: u64 = s.apps.iter().map(|a| a.ram).sum();
        s.kernel += apps_ram + closed();
        let total_swap: u64 = s.apps.iter().map(|a| a.swap).sum();
        s.apps.clear();
        let (ram_total, swap_total, zswap, cache, kernel, free) = (
            s.ram_total,
            s.swap_total,
            s.zswap_pool,
            s.file_cache,
            s.kernel,
            s.free,
        );
        let (vu, vt) = s.vram.unwrap();
        let mut shown = View::default().update(s);
        let mut memory = vec![
            seg(
                SegmentKind::Kernel,
                -1,
                "Used, not split by app",
                kernel,
                ram_total,
                "in RAM",
                &["apps, kernel and drivers together: no per-app accounting here"],
            ),
            seg(
                SegmentKind::Zswap,
                -1,
                "Zswap pool",
                zswap,
                ram_total,
                "",
                &[],
            ),
            seg(
                SegmentKind::Cache,
                -1,
                "File cache",
                cache,
                ram_total,
                "",
                &[],
            ),
            seg(SegmentKind::Free, -1, "Free", free, ram_total, "", &[]),
        ];
        stack(&mut memory);
        shown.memory.segments = model(memory);
        let mut swap = vec![
            seg(
                SegmentKind::Other,
                -1,
                "Used swap",
                total_swap,
                swap_total,
                "",
                &[],
            ),
            seg(
                SegmentKind::Free,
                -1,
                "Free swap",
                swap_total - total_swap,
                swap_total,
                "",
                &[],
            ),
        ];
        stack(&mut swap);
        shown.swap.segments = model(swap);
        shown.swap.total = format!("{} used of {}", bytes(total_swap), bytes(swap_total)).into();
        let mut vram = vec![
            seg(SegmentKind::Kernel, -1, "Used", vu, vt, "", &[]),
            seg(SegmentKind::Free, -1, "Free", vt - vu, vt, "", &[]),
        ];
        stack(&mut vram);
        shown.vram = Some(Bar {
            title: "VRAM".into(),
            total: format!("{} used of {}", bytes(vu), bytes(vt)).into(),
            segments: model(vram),
        });
        shown
    }
}
