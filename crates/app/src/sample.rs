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
        groups: Vec::new(),
        pids: Vec::new(),
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
            ],
            unattributed_ram: b(1.2, G),
            unattributed_swap: b(296.0, M),
            groups: vec!["emulator-guide35.service".into()],
            pids: vec![4130176, 4130195, 4130291],
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
