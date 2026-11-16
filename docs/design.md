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
2. **Swap bar**: how much of each app sits in swap.
3. **VRAM bar** (NVIDIA only): each app's VRAM, driver and other, free. Empty with "GPU asleep"
   while the GPU sleeps.
4. **App table**: name and icon, RAM in use, swapped out, file cache, CPU (cores), disk, GPU
   and VRAM. Sorted by RAM. Expanding an app shows its processes and child groups.
5. Small headline tiles for CPU, GPU, VRAM and disk beside the bars, under the menu button.

It refreshes every 2 s while visible and does nothing while minimised or hidden.

Visual changes are shown as a rendered mockup and approved before they are built.

## Not planned

- Recording history or a timeline (decided: live only).
- Replacing every feature of a full system monitor (sensors, network per connection, process
  editing). SebWatch explains usage; it is not a general task manager.
