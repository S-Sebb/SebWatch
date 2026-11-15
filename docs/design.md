# Design

## Who it is for

One user on a 16 GB laptop (KDE Plasma 6 on Nobara/Fedora, Wayland, Intel iGPU + NVIDIA dGPU,
zswap + large swapfile) who keeps running out of RAM and could never see why: the built-in
monitors' per-app numbers add up to less than half of "used", and background things like
Android emulators never appear.

## Technology

- **C++20 + Qt 6 Widgets**, built with CMake. Widgets pick up the system's Breeze style,
  color scheme, font and icon theme for free and cost far less memory than QML or a web view.
- KDE Frameworks only where they add real integration (about dialog, icons, desktop file
  handling). The app must still run on other desktops with plain Qt styling.
- Charts are custom-painted widgets (a few bars), not Qt Charts.
- NVIDIA data through NVML loaded at runtime with `dlopen`, never by spawning `nvidia-smi`.
  No NVIDIA driver means no NVIDIA section, not an error.
- No daemon, no root. Everything comes from files a normal user can read
  (`/sys/fs/cgroup`, `/proc`, `/sys/class/drm`).

## Main window

Top to bottom:

1. **Summary in plain words**, e.g. "RAM is nearly full: 1.3 GB available of 15.3 GB. The
   biggest user is Claude agents (3.8 GB), then Android emulator (2.9 GB). Apps waited for
   memory 14% of the last 10 s."
2. **RAM bar** that sums to total RAM: the top apps in fixed colors, then other apps, zswap
   pool, file cache (hatched: freed on demand), kernel and drivers, free. Hovering a segment
   names it.
3. **Swap bar**: how much of each app sits in swap.
4. **App table**: name and icon, RAM in use, swapped out, file cache, CPU (cores), disk, GPU
   and VRAM. Sorted by RAM. Expanding an app shows its processes and child groups.
5. A row of small headline tiles for CPU, GPU and disk may sit beside or above the bars.

It refreshes every 2 s while visible and does nothing while minimised or hidden.

Visual changes are shown as a rendered mockup and approved before they are built.

## Not planned

- Recording history or a timeline (decided: live only).
- Replacing every feature of a full system monitor (sensors, network per connection, process
  editing). SebWatch explains usage; it is not a general task manager.
