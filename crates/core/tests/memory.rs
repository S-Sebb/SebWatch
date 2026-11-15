use std::path::Path;

use sebwatch_core::memory::read;

#[test]
fn fixture_sums_exactly_to_mem_total() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/memory");
    let m = read(&fixtures.join("cgroup"), &fixtures.join("proc")).unwrap();

    let apps: Vec<_> = m
        .apps
        .iter()
        .map(|a| (a.name.as_str(), a.ram, a.file_cache, a.swap))
        .collect();
    // Discord's autostart service and its scope are one app; the unpopulated group and the
    // non-leaf groups' counters are left out; the AppImage is named by its main process.
    assert_eq!(
        apps,
        [
            ("emulator-guide35", 11_500, 2_000, 5_000),
            ("t3code", 4_000, 0, 0),
            ("discord", 3_350, 450, 3_000),
        ]
    );
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
    assert_eq!(m.kernel_and_drivers, 102_400 - 51_200 - 21_300 - 848);
    let apps_sum: u64 = m.apps.iter().map(|a| a.ram + a.file_cache).sum();
    assert_eq!(
        apps_sum + m.free + m.zswap_pool + m.kernel_and_drivers,
        m.total
    );
    assert_eq!(m.pressure_avg10, 1.92);
}
