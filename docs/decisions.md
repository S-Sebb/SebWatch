# Decisions Seb has made

Settled; don't ask again. One line each with the reason. Add a line when Seb settles something new.

- **Build our own, don't wrap an existing monitor:** no existing window app shows a breakdown
  that adds up to total RAM and includes windowless apps (checked: Mission Center, Resources,
  umv; below and systemd-cgtop do it only in a terminal). Seb, 2026-11-15.
- **Name:** SebWatch. Public GitHub repo, `/mnt/Storage/Repos/SebWatch`. Seb, 2026-11-15.
- **Robust and light-weight, elegant and modern, with three themes:** Breeze Light and Breeze
  Dark (looking at home next to the long-established KDE apps on Seb's PC), and one close to
  SebChat's theme. The default follows the system's light/dark. Seb, 2026-11-15.
- **Rust, not C++/Qt:** UI in Slint, like SebChat's PC app. Seb, 2026-11-15.
- **Live only, no history.** Seb, 2026-11-15.
- **Core question the app answers:** "here is where your 12 GB of 16 GB RAM went", with app
  sums that add up and background things such as Android emulators visible. Seb, 2026-11-15.
- **Patch notes with every release:** three sections, New, Improved, Fixes; for users only,
  no system changes; most important items first, each a short bold sentence then a few
  concise explanatory sentences in regular text; other items just concise sentences. Format
  and steps in `docs/release.md`. Seb, 2026-11-15.
- **Main window layout (mockup approved, issue #5):** no summary sentences; the window starts
  with the RAM, swap and VRAM bars, headline tiles (CPU, GPU, VRAM, disk) beside them; the theme
  switcher (System, Light, Dark, SebChat; System follows the desktop's light/dark) in a menu
  behind a menu button, remembered between runs; the SebChat theme keeps the desktop's own title
  bar. Seb, 2026-11-15; summary removed and VRAM bar added by Seb, 2026-11-16. The app table
  sorts by any numeric column (chevron on the sorted column, click again to reverse,
  remembered), and the swap and VRAM bars name their own top five, an app keeping one colour
  everywhere (mockup approved, #33). A search field above the table (Ctrl+F, Escape clears)
  filters it by app name, group or PID while the bars stay system-wide (mockup approved, #34).
  Seb, 2026-11-16.
