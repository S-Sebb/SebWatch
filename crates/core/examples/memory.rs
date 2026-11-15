//! Prints this machine's memory accounting and checks that it sums to `MemTotal`.
//! `cargo run -p sebwatch-core --example memory`

use std::path::Path;

use sebwatch_core::memory::{read, CGROUP_ROOT, PROC_ROOT};

fn main() -> std::io::Result<()> {
    let m = read(Path::new(CGROUP_ROOT), Path::new(PROC_ROOT))?;
    let mib = |b: u64| b as f64 / 1048576.0;
    println!(
        "{:<44} {:>9} {:>9} {:>9}",
        "app", "RAM MiB", "cache", "swap"
    );
    for a in &m.apps {
        println!(
            "{:<44} {:>9.1} {:>9.1} {:>9.1}",
            a.name,
            mib(a.ram),
            mib(a.file_cache),
            mib(a.swap)
        );
    }
    let apps: u64 = m.apps.iter().map(|a| a.ram + a.file_cache).sum();
    for (row, v) in [
        ("apps (RAM in use + file cache)", apps),
        ("free", m.free),
        ("zswap pool (unattributed)", m.zswap_pool),
        ("kernel and drivers", m.kernel_and_drivers),
    ] {
        println!("{row:<44} {:>9.1}", mib(v));
    }
    let sum = apps + m.free + m.zswap_pool + m.kernel_and_drivers;
    println!(
        "sum {sum} B, MemTotal {} B: {}",
        m.total,
        if sum == m.total { "match" } else { "MISMATCH" }
    );
    println!(
        "available {:.1} MiB, pressure some avg10 {}%",
        mib(m.available),
        m.pressure_avg10
    );
    Ok(())
}
