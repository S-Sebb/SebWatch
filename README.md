# SebWatch

A resource monitor for Linux that answers one question exactly: **where did my RAM (and CPU,
GPU, disk) go?**

Most monitors list processes and their private memory. Those numbers leave out shared memory,
swap, the compressed swap pool, file cache and the kernel, and they hide anything without a
window (an Android emulator started as a background service, a build an agent spawned). Their
totals often explain less than half of what the system reports as used.

SebWatch reads the kernel's per-app accounting instead (cgroup v2: every app a desktop starts
runs in its own systemd scope or service), so its breakdown adds up to the RAM you have:

```
16 GB  ███████████ apps ███████████▏zswap▏cache ▏kernel▏free
       Claude agents 3.8  emulator 2.9  Zen 1.0  …
```

Status: planning. The backlog is in the [issues](https://github.com/S-Sebb/SebWatch/issues);
the direction is in [docs/design.md](docs/design.md), the accounting method in
[docs/accounting.md](docs/accounting.md).

## Goals

- **Every MB accounted for.** The RAM bar sums to total RAM: apps, zswap pool, file cache,
  kernel and drivers, free. Nothing is left as an unexplained gap.
- **Every app, window or not.** Background services, emulators, agents and their child
  processes count toward the app that started them.
- **Live.** It shows the present, refreshed every couple of seconds. No history is recorded.
- **Light.** A native Qt app: small memory footprint, near-zero CPU, no work while hidden.
- **Looks native.** On KDE Plasma it should be indistinguishable from the long-established
  apps beside it (System Settings, Dolphin, Kate): Breeze style, system colors, fonts and icons.

## License

MIT
