use std::path::Path;

use sebwatch_core::memory::{CLOSED_APPS, read};

#[test]
fn fixture_sums_exactly_to_mem_total() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/memory");
    let m = read(&fixtures.join("cgroup"), &fixtures.join("proc")).unwrap();

    let apps: Vec<_> = m
        .apps
        .iter()
        .map(|a| (a.name.as_str(), a.ram, a.file_cache, a.swap))
        .collect();
    // Discord's autostart service and its scope are one app; the AppImage is named by its main
    // process. Groups without processes count only what their children don't: Steam's exited
    // helper (cache 1500) and its slice's own cache (200) go to Steam, the only app in that
    // slice; the exited scope in the shared app.slice (600 + 600) and the slices' own charges
    // (cache 3000 in app.slice, kernel 100 in user.slice) go to the closed-apps entry.
    assert_eq!(
        apps,
        [
            ("emulator-guide35", 11_500, 2_000, 5_000),
            ("t3code", 4_000, 0, 0),
            ("discord", 3_350, 450, 3_000),
            ("steam", 3_000, 1_700, 0),
            (CLOSED_APPS, 700, 3_600, 0),
        ]
    );
    assert!(m.apps[4].cgroups.is_empty());
    assert_eq!(m.apps[2].cgroups.len(), 2);
    // The emulator's processes sit in a child without the memory controller: charged to the
    // service, listed under the child (the key CPU and disk use).
    assert_eq!(
        m.apps[0].cgroups,
        [Path::new(
            "user.slice/app.slice/emulator-guide35.service/qemu"
        )]
    );

    assert_eq!(m.total, 102_400);
    assert_eq!(m.free, 51_200);
    assert_eq!(m.available, 71_680);
    // Zswap is 2048 bytes; the apps hold 1200 of it in their RAM in use.
    assert_eq!(m.zswap_pool, 848);
    assert_eq!(m.kernel_and_drivers, 102_400 - 51_200 - 30_300 - 848);
    let apps_sum: u64 = m.apps.iter().map(|a| a.ram + a.file_cache).sum();
    assert_eq!(
        apps_sum + m.free + m.zswap_pool + m.kernel_and_drivers,
        m.total
    );
    assert_eq!(m.pressure_avg10, 1.92);
}
