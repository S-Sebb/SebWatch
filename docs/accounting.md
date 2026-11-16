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
- **Groups without processes**: the same walk lists them. Linux keeps charging a group for
  memory its exited processes left behind (mostly page cache, also tmpfs files and kernel
  objects), and removing a group moves its charges to its parent, so slices hold some too.
  Each such group with `memory.stat` counts only its own share: its `memory.stat` (and
  `memory.swap.current`) minus its memory children's, field by field, floored at 0 (parent and
  children are read moments apart). That share goes to the running app whose groups are the
  only ones in the nearest enclosing group that has any (a Steam slice with an exited helper
  scope: Steam), otherwise to one entry, **Closed apps (cache)** (`memory::CLOSED_APPS`), with
  no groups of processes; its `exited` lists each group it holds with that group's own share
  (they sum to the entry). The window names them as it names apps (`naming.rs`), merging
  groups of one name.
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
  from the `foo@.service` template), else the cleaned name. A `systemd-run` unit's Description
  is its command line (`[systemd-run] /home/…/claude-t3 --output-format …`), so it is named by
  the program it runs instead: the full name of its main process (the command may be a wrapper
  script), else the command's first word after `setsid`/`env`/`nohup`, as a cleaned base name.
  T3 Code runs each coding agent as such a unit inside `app-t3code-agents-<id>.slice`; those
  are "<Program> agents" (`Claude agents`; the program capitalised, or its product spelling from a
  small table in `naming.rs`, such as `OpenCode`) with the terminal icon, and the row's detail counts
  its groups as agents ("13 agents") rather than "groups". Without a desktop file the icon is
  the generic `application-x-executable`, which the window draws as its own gear icon
  (`crates/app/icons/generic-app.svg`). Results are cached per leaf name.

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
| Name, PID | `/proc/<pid>/stat` `comm`; a 15-byte `comm` may be cut by the kernel, so it is completed from the base name of `cmdline`'s `argv[0]`, else of `exe`, when that starts with it (else it stays) |
| RAM | `Pss_Anon + Pss_Shmem` (the per-process part of RAM in use; file pages are file cache) |
| Swapped out | `SwapPss` |
| CPU | `utime + stime` from `/proc/<pid>/stat`, two reads one refresh apart, in cores |

- An app of several groups shows each group with its sums over its processes.
- **Not attributed to processes**: the app's RAM in use minus its processes' RAM, so the rows
  sum to the app's total. It holds the group's `kernel` and `zswap`, which no process owns (on
  a live system the processes' PSS matched each group's `anon` within 1%), and the memory of
  processes whose `smaps_rollup` can't be read (`ProcessRow::unreadable` holds why:
  `PermissionDenied` for another user's process, such as a root helper). Floored at 0: PSS
  splits pages shared with other apps, which the cgroup charges to one, so the processes can
  add up to more than the app; `DrillDown::over_charge` holds by how much.
- A process that exits between reads is dropped; a new process, or a PID reused by a new
  process (different start time), shows zero CPU for one refresh. A process whose
  `smaps_rollup` can't be read shows CPU but no memory.

## System rows (`/proc/meminfo`, kB)

Apps plus Free, Zswap pool and Kernel, drivers and unattributed sum exactly to `MemTotal`; swap is outside RAM.

| Row | Value |
|---|---|
| Free | `MemFree` |
| Zswap pool | `Zswap` minus every app's `zswap`: only the part of the compressed pool no app accounts for (each app's share is already in its RAM in use; `Zswapped` is how much the pool holds) |
| Kernel, drivers and unattributed | `MemTotal - MemFree - Zswap pool - sum(apps' RAM in use + file cache)` (Closed apps (cache) included): memory no cgroup is charged for (boot allocations, root page cache, GPU driver memory), and whatever the per-cgroup counters miss |
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
  use), Closed apps (cache) (its RAM in use; grey and hatched like File cache), Zswap pool, File cache (every app's file cache), Kernel, drivers and unattributed and Free; the
  shares add up to `MemTotal`.
- **Closed apps (cache)** is not an app: it is never among a bar's named apps or Other apps,
  and its row is the table's last, whatever the sort. Its cache counts in File cache. Expanded,
  it lists the exited groups it holds, each with its RAM, swap and file cache. Its swap, when
  it has some, is its own segment in the swap bar too.
- **Swap bar**: the five biggest apps by swap, Other apps, and free swap (`SwapTotal` minus every
  app's swap).
- **VRAM bar** (NVIDIA only, from NVML, `nvidia.rs`): the five biggest apps by VRAM (each app's
  processes summed over its groups), Other apps, Driver and other (used VRAM minus every app's:
  the driver, contexts and processes NVML doesn't list) and Free (total minus used). Apps without
  VRAM are left out. A sleeping GPU isn't read, so its bar shows empty with "GPU asleep"; without
  an NVIDIA GPU there is no bar.
- **Colours**: seven series colours, one per app in every bar and on its table row. They go to
  the RAM bar's five, then to the swap and VRAM bars' apps alternately, rank by rank, so each
  bar's biggest app always has one; a bar names its apps down to the first left without a colour
  and counts the rest as Other apps. A colour stays with its app while some bar names it.
- A segment of at least 7.5% of its bar shows its size inside; a smaller one shows it in the
  legend.
- Sizes are binary with KDE's short names (1 GB = 1024 MB); a unit steps up at 1000 of the one
  below, so no figure has four digits.
- CPU tile: the apps' CPU summed, of the logical cores. GPU tile: the NVIDIA device's busy %.
  A sleeping NVIDIA GPU isn't read: the GPU and VRAM tiles say it is asleep, and the table's
  GPU and VRAM columns show "—". Disk tile: every app's read + write.
- **Search** matches an app's display name, its grouping name and its groups' names (part of
  them, any case), or a whole PID from its groups' `cgroup.procs`, read every refresh for every
  app (the first group's was already read to name the app).
- Each tile's graph is its last 30 readings (60 s at 2 s), kept in memory only: CPU and GPU as
  a share of the whole, VRAM of its total, Disk scaled to its own peak in that minute. A
  refresh that didn't happen (the window minimised, a failed reading) is a gap, not a line
  across it; a sleeping GPU's tiles have no graph.
- **Zero, unavailable and stale.** "0" is a reading of zero and "—" a figure that doesn't apply
  (a process's disk, an app's GPU when it uses none). A figure that couldn't be read says so: a
  group without `io.stat` (the io controller off) has disk "n/a" (on Seb's PC, the services in
  systemd's `session.slice`: plasmashell, KWin, the portals, WirePlumber); the Disk tile sums the
  groups that have it, and with none anywhere says "unavailable"; an NVIDIA GPU that is awake but whose NVML doesn't start or
  answer (the library is there, so it isn't "no NVIDIA driver") makes the GPU and VRAM tiles say
  "unavailable", the VRAM bar "The NVIDIA driver is not answering" and the GPU and VRAM cells
  "n/a"; no readable `cpu.stat` makes the CPU tile "unavailable". An unavailable tile has no
  graph. When a whole reading fails, the last one stays with a warning above the bars:
  "Not updating: the last N readings failed (<error>). Showing the reading from HH:MM:SS."
- **A system that can't split memory by app**: without the memory controller in the cgroup v2
  root's `cgroup.subtree_control`, or without a systemd user session
  (`user.slice/user-<uid>.slice/user@<uid>.service`), a message replaces the table. The bars
  show totals from `/proc/meminfo`: RAM as "Used, not split by app" (`MemTotal - MemFree -
  file cache - Zswap`), zswap pool, file cache (`Active(file) + Inactive(file)`) and free; swap
  as used (`SwapTotal - SwapFree`) and free; VRAM as used and free. CPU and disk come from the
  cgroup root's own `cpu.stat` and `io.stat`, which cover the whole system.
- An expanded app reads its processes (`processes.rs`) only while expanded; its rows plus
  "Not attributed to processes" sum to its RAM in use. That row's swap is the app's swap minus
  its processes' swap. It adds " · N unreadable" when N processes couldn't be read, and
  " · processes show X more" when their RAM exceeds the app's (its RAM is 0 then); hovering it
  explains what it holds and names the unreadable processes and why. An app of several groups lists each group, then its
  processes; disk is known per group, so a process shows none.
