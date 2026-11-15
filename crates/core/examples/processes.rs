//! Prints the drill-down of this machine's biggest apps and checks each sums to the app's RAM.
//! `cargo run -p sebwatch-core --example processes`

use std::path::Path;
use std::time::{Duration, Instant};

use sebwatch_core::memory::{CGROUP_ROOT, PROC_ROOT, read};
use sebwatch_core::processes::{drill_down, read_sample};

fn main() -> std::io::Result<()> {
    let (cgroup, proc) = (Path::new(CGROUP_ROOT), Path::new(PROC_ROOT));
    let mib = |b: u64| b as f64 / 1048576.0;
    for app in read(cgroup, proc)?.apps.iter().take(3) {
        let start = Instant::now();
        let first = read_sample(cgroup, proc, &app.cgroups);
        std::thread::sleep(Duration::from_secs(1));
        let second = read_sample(cgroup, proc, &app.cgroups);
        let d = drill_down(app.ram, &first, &second, start.elapsed());
        println!("{} ({:.1} MiB RAM in use)", app.name, mib(app.ram));
        for g in &d.groups {
            println!(
                "  {}: {:.1} MiB, swap {:.1} MiB, {:.2} cores",
                g.path.display(),
                mib(g.ram),
                mib(g.swap),
                g.cpu_cores
            );
            for p in g.processes.iter().take(5) {
                println!(
                    "    {:>7} {:<20} {:>9.1} MiB {:>5.2} cores",
                    p.pid,
                    p.name,
                    mib(p.ram.unwrap_or(0)),
                    p.cpu_cores
                );
            }
        }
        let rows: u64 = d.groups.iter().map(|g| g.ram).sum::<u64>() + d.kernel_and_zswap;
        println!(
            "  kernel memory and zswap: {:.1} MiB; rows sum: {}",
            mib(d.kernel_and_zswap),
            if rows == app.ram { "match" } else { "MISMATCH" }
        );
    }
    Ok(())
}
