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
- **Live only, no recorded history.** Seb, 2026-11-15. Exception: the CPU, GPU, VRAM and Disk
  tiles show the last minute as a small graph, kept in memory only, never saved, and paused while
  minimised (Seb chose history graphs for the tiles, 2026-11-16).
- **Core question the app answers:** "here is where your 12 GB of 16 GB RAM went", with app
  sums that add up and background things such as Android emulators visible. Seb, 2026-11-15.
- **Patch notes with every release:** three sections, New, Improved, Fixes; for users only,
  no system changes; most important items first, each a short bold sentence then a few
  concise explanatory sentences in regular text; other items just concise sentences. Format
  and steps in `docs/release.md`. Seb, 2026-11-15.
- **Main window layout (mockup approved, issue #5):** no summary sentences; the window starts
  with the RAM, swap and VRAM bars, headline tiles (CPU, GPU, VRAM, disk) to their left (two by
  two in a wide window, all four the same size; Seb, 2026-11-16, #58); the theme
  switcher (System, Light, Dark, SebChat; System follows the desktop's light/dark) in a menu
  behind a menu button, remembered between runs; the SebChat theme keeps the desktop's own title
  bar. Seb, 2026-11-15; summary removed and VRAM bar added by Seb, 2026-11-16. The app table
  sorts by any numeric column (chevron on the sorted column, click again to reverse,
  remembered), and the swap and VRAM bars name their own top five, an app keeping one colour
  everywhere (mockup approved, #33). A search field above the table (Ctrl+F, Escape clears)
  filters it by app name, group or PID while the bars stay system-wide (mockup approved, #34).
  Seb, 2026-11-16.
- **0.2 mockups (Seb, 2026-11-16):** the window is laid out as proper cards (#66): tiles, one card
  for the bars and one Apps card holding the search line and the table, all with the same outer
  margins; the top and bottom cards line up on both sides, so the menu button no longer takes
  space to the right of the top card; it sits in the top-right corner inside the bars card. Selecting a row (a click; only the arrow expands it) shows
  "End <name>" and "Kill <name>" buttons beside the search box, and the row's right-click menu
  has both too (#63, #42). Pause freezes the whole window (#42). Swap and memory pressure are
  explained in an ⓘ popup by the Swap title, with a "Memory pressure high" chip beside it (#11).
  The Intel iGPU shares the GPU tile with NVIDIA, and the table has "Intel" and "NVIDIA" GPU
  columns (#10). The Name column stays fixed while the value columns scroll sideways (#56).
  Explanations on headers and tiles are one short sentence each, concise and clear, without
  "not X" asides such as "not disk space" (#41). VRAM is "dedicated GPU memory", never
  "NVIDIA GPU memory".
- **Win+Esc binds through KGlobalAccel's `setShortcut` with SetPresent (#85):** a foreign
  shortcut (`setForeignShortcut`) is recorded but stays inactive, so the key does nothing until
  the next login; marking it present makes it work at once. Not `X-KDE-Shortcuts` in the desktop
  file: that sets a default that clashes with System Monitor's and that unbinding can't take back.
