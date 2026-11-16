# Design

## Who it is for

One user on a 16 GB laptop (KDE Plasma 6 on Nobara/Fedora, Wayland, Intel iGPU + NVIDIA dGPU,
zswap + large swapfile) who keeps running out of RAM and could never see why: the built-in
monitors' per-app numbers add up to less than half of "used", and background things like
Android emulators never appear.

## Technology

- **Rust + Slint**, as in SebChat's PC app: Slint's winit backend with the femtovg renderer
  and accessibility, far lighter than a web view. A Cargo workspace: `crates/core`
  (`sebwatch-core`, all reading and accounting, no UI dependencies, tested headless) and
  `crates/app` (the `sebwatch` binary and its `ui/*.slint`).
- **Three themes** drawn in Slint: Breeze Light and Breeze Dark, which reproduce the look of
  the KDE apps beside it (colors, spacing, controls), and one close to SebChat's theme. The
  default follows the system's light/dark. Slint does not pick up the desktop's style by
  itself, so the Breeze look is ours to draw and keep close.
- Charts are custom-painted (a few bars), not a charting library.
- NVIDIA data through NVML loaded at runtime with `libloading`, never by spawning `nvidia-smi`.
  No NVIDIA driver means no NVIDIA section, not an error.
- No daemon, no root. Everything comes from files a normal user can read
  (`/sys/fs/cgroup`, `/proc`, `/sys/class/drm`).

## Main window

Top to bottom:

1. **RAM bar** that sums to total RAM: the top apps in fixed colors, then other apps, zswap
   pool, file cache (hatched: freed on demand), kernel and drivers, free. Hovering a segment
   names it.
2. **Swap bar**: how much of each app sits in swap, naming the five biggest by swap.
3. **VRAM bar** (NVIDIA only): each app's VRAM, naming the five biggest by VRAM, driver and
   other, free. Empty with "GPU asleep"
   while the GPU sleeps.
4. **App table**: name and icon, RAM in use, swapped out, file cache, CPU (cores), disk, GPU
   and VRAM. Sorted by RAM, largest first, by default; clicking a numeric column's header sorts
   by it (the thin chevron marks it, down for largest first) and clicking again reverses. The
   order is remembered in the settings file; SebChat's "Apps" title row says "sorted by …".
   Ties keep RAM's order. Expanding an app shows its processes and child groups.
   An app has one colour in every bar and on its row (seven series colours).
   A search field above the table (magnifier, "Ctrl+F" hint; in SebChat in the "Apps" title
   row) filters it by part of an app's name or group name, or a whole PID. Ctrl+F focuses it,
   Escape or its clear button empties it. While it filters, every app found shows (none folded
   into "N more apps") and a chip reads "Filtered: N of M apps · the bars show the whole system";
   the bars and tiles don't change. "Closed apps (cache)" stays only when the search finds it
   or one of the apps it lists.
5. Small headline tiles for CPU, GPU, VRAM and disk beside the bars, under the menu button.

It refreshes every 2 s while visible and does nothing while minimised or hidden.

**Keyboard and screen readers.** Tab goes from the menu button to the search field to the
table. In the table, Up and Down move the focus, Home and End go to the ends, Right and Left (or
Enter and Space) expand and fold apps, the fold and Closed apps; the focus follows its row when
a refresh reorders the rows or a search narrows them. A focused row with a tooltip (an app's
"Not attributed to processes" row) grows to show it as a second line. The menu opens
with Enter, Space or Down; its items take the arrows, Enter and Escape. The focused item has
Breeze's 1 px accent frame inside it (2 px and square in SebChat), shown after a key, not after
a click. Rows, bar segments, tiles, the menu button and the menu's items have accessible roles
and names; because AT-SPI through AccessKit 0.24 carries no text values or expanded state, the
values, a tooltip and "expanded" or "collapsed" are in the description.
`scripts/a11y` checks the AT-SPI tree on a headless KWin.

## Footprint

Measured 2026-11-16 on Seb's laptop (i7-11800H, RTX 3070 Laptop GPU on driver 620.70.06,
16 GB, kernel 7.3.4, Nobara) with `scripts/bench`: the release app on a headless KWin
(`kwin_wayland --virtual`), drawing through the NVIDIA GPU via EGL, 60 s visible, then 60 s
minimised; CPU from `/proc/<pid>/stat`, PSS from `/proc/<pid>/smaps_rollup`.

- Visible: about **2.3 % of one core**, **100 to 125 MB PSS**. Minimised: **0.00 %**. The PSS
  of the same binary moves between those figures from run to run with the NVIDIA driver's own
  mappings; the app's heap is about 13 MB of it.
- CPU per refresh is about 28 ms on the reader thread and 16 ms on the UI thread. The reader's
  biggest part is NVML, 19 ms: initialising it, attaching the device (7 ms, the same by index or
  by PCI address) and shutting it down take 11 ms of that, kept so a laptop dGPU can sleep
  between refreshes. Memory takes 4.3 ms, CPU and disk 1.2 ms, the cgroup walk they share 2 ms.
  On the UI thread the frame takes 8 ms; building the table and bars 0.2 ms, so updating rows in
  place instead of rebuilding them saves nothing measurable. One frame is drawn per refresh.
- About 65 MB of the PSS is the GL driver every GL window loads (NVIDIA's EGL libraries and
  device mappings, and Mesa's LLVM, which glvnd loads alongside). Slint's software renderer
  measured 57 MB and 1.9 % visible (#8), but it has no frame notifications, so it kept reading
  while minimised (1.1 %); not used.
- The optimisation pass in #30 kept what `scripts/bench` showed, run interleaved with main
  (main 2.42 / 2.52 %, after 2.32 / 2.27 %): one cgroup walk per refresh instead of two, and no
  NVML power reading (shown nowhere, 1.2 ms). Measured and dropped as no gain: `lto`,
  `codegen-units = 1` and `strip` (CPU 2.55 / 2.80 %, PSS unchanged; only the file shrank,
  28 to 18 MB) and glibc `arena_max=2` (anonymous memory unchanged). Keeping NVML open between
  refreshes would save about 4 ms more, but on this laptop the dGPU drives the display and never
  suspends, so it can't be shown that the GPU still sleeps with it open; not done.
- No fixed targets: best effort, without hurting stability.

Visual changes are shown as a rendered mockup and approved before they are built.

## Not planned

- Recording history or a timeline (decided: live only; the tiles keep only their last minute in
  memory).
- Replacing every feature of a full system monitor (sensors, network per connection, process
  editing). SebWatch explains usage; it is not a general task manager.
