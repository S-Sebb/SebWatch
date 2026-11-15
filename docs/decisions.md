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
