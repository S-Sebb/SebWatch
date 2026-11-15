//! The cgroup v2 walk shared by every per-app reading.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Every group under `root` that has processes, as paths relative to `root`. The root itself is
/// skipped: its counters are system-wide and its own processes are kernel threads. A group can
/// vanish at any point during the walk; whatever is gone is simply left out.
pub fn groups_with_processes(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut groups = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            walk(root, &entry.path(), &mut groups);
        }
    }
    Ok(groups)
}

fn walk(root: &Path, dir: &Path, groups: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            walk(root, &entry.path(), groups);
        }
    }
    if fs::read_to_string(dir.join("cgroup.procs")).is_ok_and(|p| !p.trim().is_empty()) {
        let rel = dir.strip_prefix(root).expect("walk stays under root");
        groups.push(rel.to_path_buf());
    }
}
