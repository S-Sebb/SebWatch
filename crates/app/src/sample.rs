//! A fixed reading with the approved mockup's numbers (this PC, 2026-11-15), shown until the
//! window reads `sebwatch-core`.

use std::time::{Duration, Instant};

use crate::model::{App, Closed, Exited, Igpu, Part, Snapshot, Unreadable};

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
        groups: Vec::new(),
        cgroups: vec![unit(&format!("app-{}.scope", name.replace(' ', "-")))],
        pids: Vec::new(),
        ram,
        zswap: None,
        swap,
        file_cache: cache,
        cpu,
        disk_per_s: Some(disk),
        gpu_percent: None,
        igpu_percent: None,
        igpu_unreadable: false,
        vram: None,
        parts: Vec::new(),
        unattributed_ram: 0,
        unattributed_swap: 0,
        over_charge: 0,
        unreadable: Vec::new(),
    }
}

/// A unit of this user's systemd, as End and Kill find it.
fn unit(leaf: &str) -> std::path::PathBuf {
    let uid = crate::ending::System::new().uid;
    format!("user.slice/user-{uid}.slice/user@{uid}.service/app.slice/{leaf}").into()
}

fn part(name: &str, pid: u32, ram: u64, swap: u64, cpu: f32) -> Part {
    Part {
        group: false,
        name: name.into(),
        detail: pid.to_string(),
        ram: Some(ram),
        swap: Some(swap),
        cpu,
        // Known per group only, as in `live`.
        disk_per_s: None,
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
                part("qemu-system-x86_64", 4130176, b(1.5, G), b(2.6, G), 0.85),
                part("netsimd", 4130291, b(5.8, M), b(4.2, M), 0.01),
                part("crashpad_handler", 4130195, b(102.0, K), b(307.0, K), 0.0),
                // Runs as another user: its memory can't be read.
                Part {
                    ram: None,
                    swap: None,
                    ..part("adb", 4130302, 0, 0, 0.0)
                },
            ],
            unattributed_ram: b(1.2, G),
            unattributed_swap: b(296.0, M),
            groups: vec!["emulator-guide35.service".into()],
            cgroups: vec![unit("emulator-guide35.service")],
            pids: vec![4130176, 4130195, 4130291, 4130302],
            unreadable: vec![Unreadable {
                name: "adb".into(),
                pid: 4130302,
                denied: true,
            }],
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
            igpu_percent: Some(4.0),
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
            igpu_percent: Some(1.0),
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
        App {
            igpu_percent: Some(0.0),
            ..app(
                "Steam",
                "",
                "steam",
                b(441.0, M),
                b(1.6, G),
                b(74.0, M),
                0.24,
                0,
            )
        },
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
        App {
            igpu_percent: Some(3.0),
            ..app(
                "Plasma Workspace",
                "",
                "plasmashell",
                b(293.0, M),
                b(379.0, M),
                b(6.0, M),
                0.0,
                0,
            )
        },
        App {
            igpu_percent: Some(1.0),
            ..app(
                "WeChat",
                "",
                "/opt/wechat/icons/wechat.png",
                b(212.0, M),
                b(1.4, G),
                b(58.0, M),
                0.0,
                0,
            )
        },
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
            // KWin's own GPU clients can't be read (`drm.rs`).
            igpu_unreadable: true,
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
    let closed = closed();
    Snapshot {
        taken: Instant::now(),
        ram_total,
        swap_total: b(32.0, G),
        zswap_pool,
        file_cache,
        // The remainder, as in docs/accounting.md.
        kernel: ram_total - apps_ram - closed.ram - zswap_pool - file_cache - free,
        free,
        apps,
        closed,
        cpu_used: Some(2.6),
        cpu_cores: 16,
        nvidia_percent: Some(0.0),
        igpu: Some(Igpu {
            vendor: "Intel".into(),
            percent: 14.0,
            whole_device: true,
        }),
        vram: Some((b(0.3, G), b(8.0, G))),
        gpu_asleep: false,
        nvidia_unavailable: false,
        disk_per_s: Some(b(9.0, M)),
        pressure: Some(0.0),
        zswap: Some(sebwatch_core::memory::Zswap {
            stored: b(6.1, G),
            pool: b(1.9, G),
        }),
        unsplit: None,
    }
}

/// Three apps that exited, as in the approved mockup (`closed-light.png`).
fn closed() -> Closed {
    let exited: Vec<Exited> = [
        ("Okular", 214.0, 12.0, 205.0),
        ("Firefox", 151.0, 9.0, 148.0),
        ("dnf makecache", 47.0, 0.0, 45.0),
    ]
    .into_iter()
    .map(|(name, ram, swap, cache)| Exited {
        name: name.into(),
        ram: b(ram, M),
        swap: b(swap, M),
        file_cache: b(cache, M),
    })
    .collect();
    Closed {
        ram: exited.iter().map(|e| e.ram).sum(),
        swap: exited.iter().map(|e| e.swap).sum(),
        file_cache: exited.iter().map(|e| e.file_cache).sum(),
        exited,
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

/// The same reading with the NVIDIA driver not answering and no `io.stat` (the io controller
/// off): GPU, VRAM and disk figures unavailable.
pub fn unavailable() -> Snapshot {
    let mut s = snapshot();
    s.nvidia_percent = None;
    s.vram = None;
    s.nvidia_unavailable = true;
    s.disk_per_s = None;
    for app in &mut s.apps {
        app.gpu_percent = None;
        app.vram = None;
        app.disk_per_s = None;
    }
    s
}

/// The same machine without cgroup v2 memory accounting or a systemd user session: totals only.
pub fn unsplit() -> Snapshot {
    let s = snapshot();
    let used: u64 = s.ram_total - s.free - s.file_cache - s.zswap_pool;
    let swap_used = s.apps.iter().map(|a| a.swap).sum::<u64>() + s.closed.swap;
    Snapshot {
        kernel: used,
        apps: Vec::new(),
        closed: Default::default(),
        unsplit: Some(crate::model::Unsplit { swap_used }),
        ..s
    }
}

/// A minute of readings at 2 s ending in `last`: CPU with a short burst, the GPU near idle, VRAM
/// steady, disk with two spikes (the tiles' graphs, mockup T3).
pub fn minute(last: fn() -> Snapshot, start: Instant) -> Vec<Snapshot> {
    let wave = |base: f32, swing: f32, seed: f32, i: usize| {
        let t = i as f32 + seed;
        (base + swing * ((t * 0.7).sin() * 0.6 + (t * 2.3).sin() * 0.4)).max(0.0)
    };
    (0..30)
        .map(|i| {
            let mut s = last();
            s.taken = start + Duration::from_secs(2 * i as u64);
            if i < 29 {
                let cpu = match i {
                    21 => 48.0,
                    22 => 37.0,
                    _ => wave(15.0, 6.0, 0.0, i),
                };
                s.cpu_used = Some(cpu / 100.0 * s.cpu_cores as f32);
                if let Some(igpu) = &mut s.igpu {
                    igpu.percent = wave(12.0, 6.0, 5.0, i);
                }
                if !s.gpu_asleep {
                    s.nvidia_percent = s.nvidia_percent.map(|_| wave(2.0, 2.0, 3.0, i));
                }
                let disk = match i {
                    8 => 12.0,
                    _ => wave(2.0, 2.0, 1.0, i),
                };
                s.disk_per_s = Some(b(disk as f64, M));
            }
            s
        })
        .collect()
}
