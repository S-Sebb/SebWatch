//! The cgroup v2 walk shared by every per-app reading.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Every group under a root, as paths relative to it. The root itself is skipped: its counters
/// are system-wide and its own processes are kernel threads.
#[derive(Debug, Default)]
pub struct Groups {
    pub with_processes: Vec<PathBuf>,
    /// Slices and groups whose processes all exited; memory can stay charged to them.
    pub without_processes: Vec<PathBuf>,
}

/// Walks `root` once. A group can vanish at any point during the walk; whatever is gone is
/// simply left out.
pub fn walk(root: &Path) -> io::Result<Groups> {
    let mut groups = Groups::default();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            visit(root, &entry.path(), &mut groups);
        }
    }
    Ok(groups)
}

fn visit(root: &Path, dir: &Path, groups: &mut Groups) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            visit(root, &entry.path(), groups);
        }
    }
    let Ok(procs) = fs::read_to_string(dir.join("cgroup.procs")) else {
        return;
    };
    let rel = dir
        .strip_prefix(root)
        .expect("walk stays under root")
        .to_path_buf();
    if procs.trim().is_empty() {
        groups.without_processes.push(rel);
    } else {
        groups.with_processes.push(rel);
    }
}
