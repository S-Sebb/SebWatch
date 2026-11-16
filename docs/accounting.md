# How usage is accounted

The method was checked against a live system (15.3 GB laptop, 2026-11-15): the per-app sums
plus the system rows below matched `MemTotal` to within the kernel's untracked allocations.

## Apps are cgroups

On a systemd desktop every launched app runs in its own cgroup v2 group: a scope
(`app-<launcher>-<desktop id>-<random>.scope`, `app-flatpak-<app id>-<n>.scope`) or a service
(`app-steam@autostart.service`, `emulator-guide35.service`). Child processes stay in their
parent's group, so a build an agent starts is charged to that agent.

- **Leaves**: walk `/sys/fs/cgroup` once (`crates/core/src/cgroup.rs`, shared by memory, CPU
  and disk) and keep every group whose `cgroup.procs` is non-empty, skipping the root (its
  counters are system-wide; its own processes are kernel threads). Memory is read from the
  nearest of these groups or their ancestors that has `memory.stat` (a child without the memory
  controller, such as `systemd-udevd.service/udev`, is charged to its parent), each such group
  once.
- **Grouping**: several groups can belong to one app (each Claude agent gets its own scope,
  T3 Code has an AppImage service and a scope). Group them by `naming::clean_name`: strip the
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

## Drill-down: processes inside an app

Read only for expanded apps (`crates/core/src/processes.rs`), from each of the app's groups'
`cgroup.procs`. Per-process memory is PSS from `/proc/<pid>/smaps_rollup`, so shared pages are
split rather than counted twice.

| Column | Source |
|---|---|
| Name, PID | `/proc/<pid>/stat` `comm` |
| RAM | `Pss_Anon + Pss_Shmem` (the per-process part of RAM in use; file pages are file cache) |
| Swapped out | `SwapPss` |
| CPU | `utime + stime` from `/proc/<pid>/stat`, two reads one refresh apart, in cores |

- An app of several groups shows each group with its sums over its processes.
- **Kernel memory and zswap (not per process)**: the app's RAM in use minus its processes'
  RAM, so the rows sum to the app's total. It holds the group's `kernel` and `zswap`, which no
  process owns (on a live system the processes' PSS matched each group's `anon` within 1%).
  Floored at 0: PSS splits pages shared with other apps, which the cgroup charges to one.
- A process that exits between reads is dropped; a new process, or a PID reused by a new
  process (different start time), shows zero CPU for one refresh. Another user's process
  whose `smaps_rollup` can't be read shows CPU but no memory.

## System rows (`/proc/meminfo`, kB)

Apps plus Free, Zswap pool and Kernel and drivers sum exactly to `MemTotal`; swap is outside RAM.

| Row | Value |
|---|---|
| Free | `MemFree` |
| Zswap pool | `Zswap` minus every app's `zswap`: only the part of the compressed pool no app accounts for (each app's share is already in its RAM in use; `Zswapped` is how much the pool holds) |
| Kernel and drivers | `MemTotal - MemFree - Zswap pool - sum(apps' RAM in use + file cache)`: memory no cgroup is charged for (boot allocations, root page cache, GPU driver memory) |
| Available | `MemAvailable` |
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

- **NVIDIA** (`crates/core/src/nvidia.rs`): first the dGPU's runtime PM status
  (`/sys/bus/pci/devices/<addr>/power/runtime_status`); `suspended` shows "asleep" and NVML is
  not loaded, so the GPU stays asleep. Otherwise `libnvidia-ml.so.1` is loaded at runtime and
  NVML is initialised, read and shut down every refresh (an open session would keep the GPU
  awake). No NVIDIA GPU, no library or a failed init: no NVIDIA section.

  | Column | Source |
  |---|---|
  | VRAM per app | `nvmlDeviceGetComputeRunningProcesses_v3` and `GraphicsRunningProcesses_v3`; a PID in both counts once |
  | GPU % per app | `nvmlDeviceGetProcessUtilization` SM %, averaged per PID over the refresh interval |
  | Device totals | `nvmlDeviceGetMemoryInfo` used/total, `GetUtilizationRates` gpu %, `GetPowerUsage` mW |

  PIDs map to apps through `/proc/<pid>/cgroup` (`0::/<path>`), the same key as CPU and disk;
  each app's numbers are the sum over its processes.
- **Intel / AMD**: DRM fdinfo (`/proc/<pid>/fdinfo/*` with `drm-driver`): `drm-engine-*`
  busy time and `drm-memory-*` / `drm-resident-*` per client, deduplicated by `drm-client-id`.

## In the window (`crates/app/src/model.rs`, `live.rs`)

- **RAM bar**: the five biggest apps in their own colours, then Other apps (the rest's RAM in
  use), Zswap pool, File cache (every app's file cache), Kernel and drivers and Free; the
  shares add up to `MemTotal`. A colour stays with its app while it is in the top five.
- **Swap bar**: the same five apps' swap, Other apps, and free swap (`SwapTotal` minus every
  app's swap).
- **VRAM bar** (NVIDIA only, from NVML, `nvidia.rs`): the same five apps' VRAM (each app's
  processes summed over its groups), Other apps, Driver and other (used VRAM minus every app's:
  the driver, contexts and processes NVML doesn't list) and Free (total minus used). Apps without
  VRAM are left out. A sleeping GPU isn't read, so its bar shows empty with "GPU asleep"; without
  an NVIDIA GPU there is no bar.
- A segment of at least 7.5% of its bar shows its size inside; a smaller one shows it in the
  legend.
- Sizes are binary with KDE's short names (1 GB = 1024 MB); a unit steps up at 1000 of the one
  below, so no figure has four digits.
- CPU tile: the apps' CPU summed, of the logical cores. GPU tile: the NVIDIA device's busy %;
  a sleeping NVIDIA GPU shows 0% and no VRAM tile. Disk tile: every app's read + write.
- An expanded app reads its processes (`processes.rs`) only while expanded; its rows plus
  "Kernel memory and zswap (not per process)" sum to its RAM in use. That row's swap is the
  app's swap minus its processes' swap. An app of several groups lists each group, then its
  processes; disk is known per group, so a process shows none.
