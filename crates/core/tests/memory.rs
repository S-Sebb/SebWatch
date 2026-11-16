use std::path::Path;

use sebwatch_core::memory::{CLOSED_APPS, Zswap, pressure, read, zswap};

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
    // The closed-apps entry keeps its groups, which sum to it; other apps keep none.
    let exited: Vec<_> = m.apps[4]
        .exited
        .iter()
        .map(|e| (e.group.to_str().unwrap(), e.ram, e.file_cache, e.swap))
        .collect();
    assert_eq!(
        exited,
        [
            ("user.slice/app.slice/app-gone-55.scope", 600, 600, 0),
            ("user.slice", 100, 0, 0),
            ("user.slice/app.slice", 0, 3_000, 0),
        ]
    );
    assert!(m.apps[..4].iter().all(|a| a.exited.is_empty()));
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
    let proc = fixtures.join("proc");
    assert_eq!(pressure(&proc), Some(1.92));
    // zswap holds pages here though no sysfs says it is on.
    let (stored, pool) = (10 * 1024, 2 * 1024);
    assert_eq!(zswap(&proc, &fixtures), Some(Zswap { stored, pool }));
    // A kernel without pressure tracking or zswap.
    assert_eq!(pressure(&fixtures), None);
    assert_eq!(zswap(&fixtures, &fixtures), None);
}

#[test]
fn splits_by_app_needs_the_memory_controller_and_a_user_session() {
    use sebwatch_core::memory::splits_by_app;
    let root = std::env::temp_dir().join(format!("sebwatch-split-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let session = root.join("user.slice/user-1000.slice/user@1000.service/app.slice");
    std::fs::create_dir_all(&session).unwrap();
    let controls = root.join("cgroup.subtree_control");

    std::fs::write(&controls, "cpu io memory pids\n").unwrap();
    assert!(splits_by_app(&root));
    // No memory accounting below the root (or no cgroup v2 at all).
    std::fs::write(&controls, "cpu io pids\n").unwrap();
    assert!(!splits_by_app(&root));
    std::fs::remove_file(&controls).unwrap();
    assert!(!splits_by_app(&root));
    // No systemd user session: no per-app groups.
    std::fs::write(&controls, "memory\n").unwrap();
    std::fs::remove_dir_all(root.join("user.slice/user-1000.slice")).unwrap();
    assert!(!splits_by_app(&root));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn totals_come_from_meminfo_and_errors_name_the_file() {
    use sebwatch_core::memory::totals;
    let dir = std::env::temp_dir().join(format!("sebwatch-totals-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("meminfo"),
        "MemTotal: 100 kB\nMemFree: 10 kB\nActive(file): 20 kB\nInactive(file): 5 kB\nZswap: 1 kB\n",
    )
    .unwrap();
    let t = totals(&dir).unwrap();
    assert_eq!(
        (t.total, t.free, t.file_cache, t.zswap_pool),
        (100 * 1024, 10 * 1024, 25 * 1024, 1024)
    );
    std::fs::remove_file(dir.join("meminfo")).unwrap();
    let error = read(&dir, &dir).unwrap_err().to_string();
    assert!(
        error.starts_with("cannot read ") && error.contains("meminfo"),
        "{error}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
