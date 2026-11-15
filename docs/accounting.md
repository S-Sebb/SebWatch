# How usage is accounted

The method was checked against a live system (15.3 GB laptop, 2026-11-15): the per-app sums
plus the system rows below matched `MemTotal` to within the kernel's untracked allocations.

## Apps are cgroups

On a systemd desktop every launched app runs in its own cgroup v2 group: a scope
(`app-<launcher>-<desktop id>-<random>.scope`, `app-flatpak-<app id>-<n>.scope`) or a service
(`app-steam@autostart.service`, `emulator-guide35.service`). Child processes stay in their
parent's group, so a build an agent starts is charged to that agent.

- **Leaves**: walk `/sys/fs/cgroup` and keep every group whose `cgroup.procs` is non-empty,
  skipping the root (its counters are system-wide; its own processes are kernel threads).
- **Grouping**: several groups can belong to one app (each Claude agent gets its own scope,
  T3 Code has an AppImage service and a scope). Group them by a cleaned name: strip the
  `.scope/.service/.slice` suffix, the `app-` and `flatpak-` prefixes and the random or
  numeric tail, and unescape `\x2d`. Generated names that say nothing (`appimagekit_<hash>`)
  fall back to the most common process name in the group.
- **Display name and icon** (`crates/core/src/naming.rs`): a group whose main process is
  `qemu-system*` is "Android emulator · <instance>" (its Description is a long shell command);
  else the matching `.desktop` file's `Name`/`Icon` (desktop id or Flatpak id, searched in the
  XDG data dirs and Flatpak exports), else the unit file's `Description=` (read from disk, also
  from the `foo@.service` template), else the cleaned name. Without a desktop file the icon is
  the generic `application-x-executable`. Results are cached per leaf name.

## Memory per app (`memory.stat`, bytes)

| Column | Source |
|---|---|
| RAM in use | `anon + shmem + kernel + zswap` (the zswap share is the compressed copy that lives in RAM) |
| File cache | `file - shmem` (freed on demand, shown apart) |
| Swapped out | `memory.swap.current` |

Per-process numbers in the drill-down use PSS from `/proc/<pid>/smaps_rollup`, so shared pages
are split rather than counted twice.

## System rows (`/proc/meminfo`, kB)

| Row | Value |
|---|---|
| Free | `MemFree` |
| Zswap pool | `Zswap` (compressed size in RAM; `Zswapped` is how much it holds) |
| Kernel and drivers | `MemTotal - MemFree - sum(apps' RAM in use + file cache)`: memory no cgroup is charged for (boot allocations, root page cache, GPU driver memory) |
| Available | `MemAvailable`, for the summary sentence |
| Pressure | `/proc/pressure/memory`, `some avg10` |

## CPU and disk

Two reads of `cpu.stat` (`usage_usec`) and `io.stat` (`rbytes`, `wbytes`) per leaf, one
refresh interval apart. CPU is shown in cores (1.00 = one core busy).

## GPU

- **NVIDIA**: NVML `nvmlDeviceGetComputeRunningProcesses` / `GraphicsRunningProcesses` for
  VRAM per PID, `nvmlDeviceGetProcessUtilization` for SM %, device memory, utilization and
  power for the totals. PIDs map to apps through their cgroup.
- **Intel / AMD**: DRM fdinfo (`/proc/<pid>/fdinfo/*` with `drm-driver`): `drm-engine-*`
  busy time and `drm-memory-*` / `drm-resident-*` per client, deduplicated by `drm-client-id`.
