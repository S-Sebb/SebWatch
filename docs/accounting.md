# How usage is accounted

The method was checked against a live system (15.3 GB laptop, 2026-11-15): the per-app sums
plus the system rows below matched `MemTotal` to within the kernel's untracked allocations.

## Apps are cgroups

On a systemd desktop every launched app runs in its own cgroup v2 group: a scope
(`app-<launcher>-<desktop id>-<random>.scope`, `app-flatpak-<app id>-<n>.scope`) or a service
(`app-steam@autostart.service`, `emulator-guide35.service`). Child processes stay in their
parent's group, so a build an agent starts is charged to that agent.

- **Leaves**: walk `/sys/fs/cgroup` and keep every populated group with `memory.stat` whose
  children have none, skipping the root (its counters are system-wide; its own processes are
  kernel threads). A child without the memory controller is charged to its parent, so these
  leaves count every process outside the root once.
- **Grouping**: several groups can belong to one app (each Claude agent gets its own scope,
  T3 Code has an AppImage service and a scope). Group them by a cleaned name: strip the
  `.scope/.service/.slice` suffix, the template instance (`@autostart`, `@<id>`), the `app-`,
  `flatpak-` and `dbus-:<bus>-` prefixes and the random tail (a number or a hex id of 8+
  digits), and unescape `\x2d`. Generated names that say nothing (`appimagekit_<hash>`)
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

Apps plus Free, Zswap pool and Kernel and drivers sum exactly to `MemTotal`; swap is outside RAM.

| Row | Value |
|---|---|
| Free | `MemFree` |
| Zswap pool | `Zswap` minus every app's `zswap`: only the part of the compressed pool no app accounts for (each app's share is already in its RAM in use; `Zswapped` is how much the pool holds) |
| Kernel and drivers | `MemTotal - MemFree - Zswap pool - sum(apps' RAM in use + file cache)`: memory no cgroup is charged for (boot allocations, root page cache, GPU driver memory) |
| Available | `MemAvailable`, for the summary sentence |
| Pressure | `/proc/pressure/memory`, `some avg10` |

## CPU and disk

Two reads per leaf, one refresh interval apart (`crates/core/src/cpu_io.rs`); each rate is the
counter's increase divided by the interval. Results are keyed by the group's path relative to
the cgroup root, the same key the memory numbers use.

| Column | Source |
|---|---|
| CPU | `cpu.stat` `usage_usec`, shown in cores (1.00 = one core busy) |
| Disk read | `io.stat` `rbytes`, summed over devices, bytes/s |
| Disk write | `io.stat` `wbytes`, summed over devices, bytes/s |

- A group with no `io.stat` (io controller not enabled for its subtree) shows no disk use.
- A group new since the last read shows zero for one refresh: its counters cover its whole
  life, not the interval. A group that vanished is dropped. A counter that went down (a new
  group reusing the path) counts as zero.

## GPU

- **NVIDIA**: NVML `nvmlDeviceGetComputeRunningProcesses` / `GraphicsRunningProcesses` for
  VRAM per PID, `nvmlDeviceGetProcessUtilization` for SM %, device memory, utilization and
  power for the totals. PIDs map to apps through their cgroup.
- **Intel / AMD**: DRM fdinfo (`/proc/<pid>/fdinfo/*` with `drm-driver`): `drm-engine-*`
  busy time and `drm-memory-*` / `drm-resident-*` per client, deduplicated by `drm-client-id`.
