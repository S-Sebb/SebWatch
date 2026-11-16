//! Prints the Intel or AMD GPU's busy % per app over 2 s, headless, and how long a read takes:
//! `cargo run --release -p sebwatch-core --example drm`.

use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, Instant};

use sebwatch_core::drm::{Scanner, find_device, usage};
use sebwatch_core::memory::PROC_ROOT;

fn main() {
    let Some(device) = find_device(Path::new("/sys/class/drm")) else {
        println!("no Intel or AMD GPU");
        return;
    };
    let proc = Path::new(PROC_ROOT);
    let uid = std::fs::metadata(proc.join("self")).unwrap().uid();
    let mut scanner = Scanner::default();
    let start = Instant::now();
    let first = scanner.read(proc, uid, &device);
    println!("{device:?}; first read {:?}", start.elapsed());
    std::thread::sleep(Duration::from_secs(2));
    let again = Instant::now();
    let second = scanner.read(proc, uid, &device);
    println!("next read {:?}", again.elapsed());
    let u = usage(&first, &second, start.elapsed());
    println!(
        "device {:.1}% busy ({})",
        u.busy_percent,
        if u.whole_device {
            "whole GPU, from the driver"
        } else {
            "clients read"
        }
    );
    for (group, app) in &u.apps {
        let mark = if second.unreadable.contains(group) {
            " (some processes unreadable)"
        } else {
            ""
        };
        println!(
            "{:5.1}% {:>8} KiB {}{mark}",
            app.busy_percent,
            app.memory_bytes / 1024,
            group.display()
        );
    }
    for group in second
        .unreadable
        .iter()
        .filter(|g| !u.apps.contains_key(*g))
    {
        println!("  n/a              {}", group.display());
    }
}
