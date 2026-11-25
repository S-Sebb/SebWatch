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
- **Win+Esc on KDE Plasma** opens SebWatch in place of System Monitor (Seb, #12): KGlobalAccel
  launches `sebwatch.desktop`, and System Monitor's own Win+Esc is set to none, given back on
  uninstall (`sebwatch --bind-win-esc`, `--unbind-win-esc`). One SebWatch at a time: it owns
  the D-Bus name `io.github.S_Sebb.SebWatch`, and a second start raises the first one's window
  through a KWin script (Wayland lets no app raise another's window) and exits.
- No daemon, no root. Everything comes from files a normal user can read
  (`/sys/fs/cgroup`, `/proc`, `/sys/class/drm`).

## Main window

From the top:

1. **RAM bar** that sums to total RAM: the top apps in fixed colors, then other apps, zswap
   pool, file cache (hatched: freed on demand), kernel and drivers, free. Hovering a segment
   names it.
2. **Swap bar**: how much of each app sits in swap, naming the five biggest by swap.
3. **VRAM bar** (NVIDIA only): each app's VRAM, naming the five biggest by VRAM, driver and
   other, free. Empty with "GPU asleep"
   while the GPU sleeps.
4. **App table**: name and icon, RAM in use, swapped out, file cache, CPU (cores), disk, GPU
   and VRAM; with an Intel or AMD GPU beside the NVIDIA one, GPU is two columns, "Intel" (or
   "AMD") and "NVIDIA" (#10). Sorted by RAM, largest first, by default; clicking a numeric column's header sorts
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
5. Headline tiles for CPU, GPU, VRAM and disk to the left of the bars: the value large, its
   unit below (the GPU tile shows the Intel or AMD GPU and the NVIDIA one side by side, a
   graph line each, #10), and the last minute as a line with a light fill along the tile's bottom, under
   the words (tile style T3, #52). All four are the same size, graph or not. From 1400 px wide
   they sit two by two, so the top area is only as tall as the bars and the table gets the rest;
   narrower windows stack them in one column (#58). The top area is as short as the taller of
   the tiles and the bars allows, and the bars share any spare height in equal gaps.

**Cards** (#66). The window is a set of matching cards, drawn like the tiles (Breeze: the view
colour in a 1 px frame with rounded corners; SebChat: the notched pixel frame): the tiles, one
card for the bars and one Apps card holding the search line and the table, clipped to its
corners. Every card keeps a 16 px margin to the window's edge and the tiles' gap (Breeze 8 px,
SebChat 14 px) to its neighbours, and the bars card and the Apps card line up on both sides. The
menu button sits in the bars card's top-right corner, after Memory's total.

**Selecting, pausing, End and Kill** (#42, #63). A click selects a row (the theme's selection
colour); only its arrow, or Right and Left, Enter and Space, expands it. The selection follows
its row's key across refreshes and searches, and the table keeps its scroll position. A click
on an app's section in a bar selects its row too and scrolls to it, first clearing a search
that hides it and opening the fold it is in; on its selected section, or Escape, deselects it.
Other apps opens the fold; other sections do nothing (#88). With a
row selected, "End <name>" and "Kill <name>" buttons sit beside the search field, sharing the
room up to Pause and ending a long name in an ellipsis (the tooltip has it whole, with the PID
of a process). The row's menu (right click, the menu key or Shift+F10) has the same two, with
Delete and Shift+Delete as their keys. A row with nothing to end (Not attributed, Closed apps,
the "N more apps" fold, another user's processes) shows no buttons, and its menu says why.
Kill asks first, with Cancel focused; End asks first only for a part of the desktop (Plasma,
KWin, the login session). After End the row says "ending…"; 5 s later, if it still runs, it
says "still running" and a strip offers Kill. A failed End or Kill shows a red strip with the
reason. Pause, at the right of the search line, freezes the whole window: SebWatch stops reading
until Resume, a strip says "Paused at HH:MM:SS", and End and Kill still work, an ended row
greyed with "ended" until Resume. What End and Kill do is in `docs/accounting.md`.

**Small windows** (#36). The window is at least 760 × 480 and remembers its size in the
settings file (not when maximised or full screen). A narrow window keeps the name column at
220 px and in view, while the value columns, header and rows together, scroll sideways under it
(a shadow on its edge while scrolled; the scrollbar runs under the values only). A short one keeps about 8
table rows and scrolls the whole page instead.

It refreshes every 2 s while visible and does nothing while minimised or hidden.
Readings that fail say so rather than showing 0: "unavailable" in a tile, "n/a" in a table
cell, and a KDE-style inline warning above the bars when the figures stop updating, giving the
cause and the time of the reading shown. A system that can't split memory by app shows a plain
message in place of the table (#38).

**Keyboard and screen readers.** Tab goes from the menu button to the tiles, the search field,
End, Kill and Pause, the column headers and the table. The tiles are one Tab stop and the headers another: the arrows
move inside each, and Enter or Space on a header sorts by it. Every tile and column header says
in one sentence what its number measures (#41): in the bars' tooltip style on hover, at once
when it has the keyboard focus, and as its accessible description. In the table, Up and Down
move the focus, Home and End go to the ends, Right and Left (or Enter and Space) expand and fold
apps, the fold and Closed apps; the focus follows its row when a refresh reorders the rows or a
search narrows them. A focused row with a tooltip (an app's "Not attributed to processes" row)
grows to show it as a second line. The menu opens with Enter, Space or Down; its items take the
arrows, Enter and Escape. The focused item has Breeze's 1 px accent frame inside it (2 px and
square in SebChat), shown after a key, not after a click. Rows, bar segments, tiles, column
headers, the menu button and the menu's items have accessible roles and names; because AT-SPI
through AccessKit 0.24 carries no text values or expanded state, the values, a tooltip and
"expanded" or "collapsed" are in the description.
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
- An expanded app of thousands of processes (#43), measured with `scripts/bench processes`:
  it reads and shows one refresh, folded and expanded, with an app of 1000 to 8000 sleeping
  processes in a throwaway user scope; CPU of the bench thread, software renderer for the frame.
  Expanded, it was 15 % of one core with 1000 processes and 102 % with 8000: the table made
  and laid out every row's elements each frame, in sight or not. A list view makes only the
  rows in sight, and processes are matched to the last sample by lookup, not by a scan of the
  group: now 3.0 % with 1000, 8.0 % with 4000 and 15 % with 8000, linear. What's left is the
  kernel's `stat` and `smaps_rollup` of each process, about 33 µs each.
- The Intel GPU (#10), measured 2026-11-17 with `scripts/bench` on the same laptop: main 2.28 %,
  with it 3.05 / 3.02 % of one core visible, 0.00 % minimised either way. Reading DRM fdinfo
  costs 4.3 ms of CPU per refresh averaged over 30 refreshes (0.2 %): a full search of every
  process's fds is 27 ms, so `drm::Scanner` keeps each process's DRM fds and searches all only
  every 30 s. The rest of the difference is the window drawing the extra column and graph line.
- **Start time** to the first frame, measured 2026-11-16 (#12) on the same laptop from exec
  to Slint's first drawn frame: warm about **200 to 450 ms**; cold, with the app's files dropped
  from the page cache, about 20 ms more; under memory pressure (`stress-ng --vm` taking all but
  about 1 GB, PSI full about 20 %) **0.5 to 2.6 s**. Most of it is Slint opening the window and
  loading the NVIDIA GL driver (250 ms warm, 1.4 to 2 s under pressure); reading takes 45 ms
  warm and 100 to 300 ms under pressure, and icon lookups about 100 ms. Reading on a thread
  while the window is built gained nothing measurable against that noise; not done. A second
  start only raises the open window, in under 10 ms.
- No fixed targets: best effort, without hurting stability.

Visual changes are shown as a rendered mockup and approved before they are built.

## Not planned

- Recording history or a timeline (decided: live only; the tiles keep only their last minute in
  memory).
- Replacing every feature of a full system monitor (sensors, network per connection, process
  editing). SebWatch explains usage; it is not a general task manager.
