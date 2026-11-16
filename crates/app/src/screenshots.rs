//! The window drawn offscreen by Slint's software renderer, in each theme, with the Android
//! emulator's bar segment hovered and its row expanded, then with the NVIDIA GPU asleep, then
//! sorted by swap both ways, then searched for "k" from the keyboard, then with memory pressure
//! high and the swap popup open, plus Breeze Light with the menu open, and one of this PC's live
//! reading. Also `bench_refresh`, the window side of `scripts/bench processes`.
//! Nothing shows on the screen. Run with `scripts/screenshots <dir>`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::platform::Key;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Model, PhysicalSize, Rgb8Pixel};

use crate::{MainWindow, Theme, ThemeChoice};

struct Offscreen {
    window: Rc<MinimalSoftwareWindow>,
    now: Rc<Cell<Duration>>,
}

impl Platform for Offscreen {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.now.get()
    }
}

struct Shot {
    window: Rc<MinimalSoftwareWindow>,
    now: Rc<Cell<Duration>>,
    width: u32,
    height: u32,
}

impl Shot {
    /// Lets timers (the tooltip's delay) and animations run their course.
    fn wait(&self, time: Duration) {
        self.now.set(self.now.get() + time);
        slint::platform::update_timers_and_animations();
    }

    fn render(&self) -> Vec<Rgb8Pixel> {
        self.wait(Duration::from_millis(50));
        self.window.request_redraw();
        let mut pixels = vec![Rgb8Pixel::default(); (self.width * self.height) as usize];
        self.window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, self.width as usize);
        });
        pixels
    }

    fn pointer(&self, x: f32, y: f32, click: bool) {
        let position = LogicalPosition::new(x, y);
        self.window
            .dispatch_event(WindowEvent::PointerMoved { position });
        if click {
            let button = PointerEventButton::Left;
            self.window
                .dispatch_event(WindowEvent::PointerPressed { position, button });
            self.window
                .dispatch_event(WindowEvent::PointerReleased { position, button });
        }
    }

    fn key(&self, text: impl Into<slint::SharedString>, pressed: bool) {
        let text = text.into();
        self.window.dispatch_event(if pressed {
            WindowEvent::KeyPressed { text }
        } else {
            WindowEvent::KeyReleased { text }
        });
    }

    fn tap(&self, text: impl Into<slint::SharedString> + Clone) {
        self.key(text.clone(), true);
        self.key(text, false);
    }

    fn save(&self, pixels: &[Rgb8Pixel], path: &std::path::Path) {
        let file = std::fs::File::create(path).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        let bytes: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&bytes)
            .unwrap();
    }
}

fn same(p: Rgb8Pixel, colour: slint::Color) -> bool {
    (p.r, p.g, p.b) == (colour.red(), colour.green(), colour.blue())
}

/// The first run of this colour on pixel row `y`: its start and end.
fn run_in_row(pixels: &[Rgb8Pixel], width: u32, colour: slint::Color, y: f32) -> (f32, f32) {
    let line = &pixels[(y as u32 * width) as usize..][..width as usize];
    let start = line
        .iter()
        .position(|&p| same(p, colour))
        .expect("the colour is on the row");
    let run = line[start..]
        .iter()
        .take_while(|&&p| same(p, colour))
        .count();
    (start as f32, (start + run) as f32)
}

/// The middle of the first run of this colour on the topmost row that has it.
fn find(pixels: &[Rgb8Pixel], width: u32, colour: slint::Color) -> (f32, f32) {
    let i = pixels
        .iter()
        .position(|p| (p.r, p.g, p.b) == (colour.red(), colour.green(), colour.blue()))
        .expect("the colour is on screen");
    let run = pixels[i..]
        .iter()
        .take_while(|p| (p.r, p.g, p.b) == (colour.red(), colour.green(), colour.blue()))
        .count();
    let (x, y) = (i as u32 % width, i as u32 / width);
    (x as f32 + run as f32 / 2.0, y as f32 + 12.0)
}

/// Expands or folds a row by its key, as a click would but without focusing it.
fn toggle(ui: &MainWindow, key: &str) {
    crate::VIEW.with_borrow_mut(|view| {
        view.toggle(key);
        crate::show(ui, view.render());
    });
}

#[test]
#[ignore = "writes PNG files; run through scripts/screenshots"]
fn screenshots() {
    let dir = std::path::PathBuf::from(
        std::env::var("SEBWATCH_SHOTS").expect("SEBWATCH_SHOTS names the output folder"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let now = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(Offscreen {
        window: window.clone(),
        now: now.clone(),
    }))
    .unwrap();

    // The mockup's window sizes, without its title bar.
    for (choice, name, height) in [
        (ThemeChoice::Light, "light", 844),
        (ThemeChoice::Dark, "dark", 844),
        (ThemeChoice::Sebchat, "sebchat", 1024),
    ] {
        let ui = MainWindow::new().unwrap();
        ui.global::<Theme>().set_choice(choice);
        crate::setup(
            &ui,
            crate::sample::snapshot(),
            Default::default(),
            |_| {},
            |_| {},
        );
        let began = Instant::now();
        for reading in crate::sample::minute(crate::sample::snapshot, began) {
            crate::present(&ui, reading);
        }
        ui.show().unwrap();
        let shot = Shot {
            window: window.clone(),
            now: now.clone(),
            width: 1000,
            height,
        };
        window.set_size(PhysicalSize::new(shot.width, shot.height));

        ui.invoke_pick(1);
        let first = shot.render();
        let android = ui.global::<Theme>().get_series().row_data(1).unwrap();
        let (x, y) = find(&first, shot.width, android);
        shot.pointer(x, y, false);
        shot.wait(Duration::from_millis(800));
        shot.save(&shot.render(), &dir.join(format!("app-{name}.png")));

        // The RAM bar's leftover, then the expanded app's leftover row: five rows below the
        // app's row, found by its swatch (the lowest pixel of its colour near the left edge).
        let kernel = ui.global::<Theme>().get_kernel();
        shot.pointer(4.0, 4.0, false);
        let (start, end) = run_in_row(&first, shot.width, kernel, y);
        shot.pointer((start + end) / 2.0, y, false);
        shot.wait(Duration::from_millis(800));
        shot.save(
            &shot.render(),
            &dir.join(format!("leftover-bar-{name}.png")),
        );
        let swatch = (0..first.len())
            .rev()
            .find(|&i| i as u32 % shot.width < 60 && same(first[i], android))
            .expect("the app's swatch");
        let row = if choice == ThemeChoice::Sebchat {
            31.0
        } else {
            28.0
        };
        shot.pointer(4.0, 4.0, false);
        shot.pointer(
            300.0,
            (swatch as u32 / shot.width) as f32 + 5.0 * row,
            false,
        );
        shot.wait(Duration::from_millis(800));
        shot.save(
            &shot.render(),
            &dir.join(format!("leftover-row-{name}.png")),
        );
        // #41: a header hovered ("RAM in use", "Swapped out" in SebChat's wider table), two rows
        // and a header's half above the app's row.
        let header = (swatch as u32 / shot.width) as f32 - 5.0 - 1.5 * row - 14.0;
        shot.pointer(4.0, 4.0, false);
        shot.pointer(440.0, header, false);
        shot.wait(Duration::from_millis(800));
        shot.save(
            &shot.render(),
            &dir.join(format!("about-header-{name}.png")),
        );

        // Closed apps hovered: the segment after Other apps, grey too, so the rightmost pixel
        // of that grey; and its row expanded in a window tall enough for the whole table.
        toggle(&ui, "Android emulator");
        let other = ui.global::<Theme>().get_other();
        let line = &first[(y as u32 * shot.width) as usize..][..shot.width as usize];
        let closed = line.iter().rposition(|&p| same(p, other)).unwrap() as f32 - 3.0;
        toggle(&ui, "\u{0}closed");
        window.set_size(PhysicalSize::new(shot.width, 1300));
        let tall = Shot {
            window: window.clone(),
            now: now.clone(),
            width: shot.width,
            height: 1300,
        };
        tall.render();
        tall.pointer(4.0, 4.0, false);
        tall.pointer(closed, y, false);
        tall.wait(Duration::from_millis(800));
        tall.save(&tall.render(), &dir.join(format!("closed-{name}.png")));
        window.set_size(PhysicalSize::new(shot.width, shot.height));
        toggle(&ui, "\u{0}closed");
        toggle(&ui, "Android emulator");

        shot.pointer(4.0, 4.0, false);
        let later = began + Duration::from_secs(60);
        for reading in crate::sample::minute(crate::sample::asleep, later) {
            crate::present(&ui, reading);
        }
        shot.save(&shot.render(), &dir.join(format!("app-{name}-asleep.png")));

        // #38: readings that failed. The NVIDIA driver silent and no io.stat; then three failed
        // refreshes in a row; then a system that can't split memory by app.
        crate::present(&ui, crate::sample::unavailable());
        shot.save(&shot.render(), &dir.join(format!("unavailable-{name}.png")));
        crate::present(&ui, crate::sample::snapshot());
        let error = "cannot read /sys/fs/cgroup/user.slice/memory.stat: No such file or directory";
        for _ in 0..3 {
            crate::VIEW.with_borrow_mut(|view| crate::show(&ui, view.failed(error.into())));
        }
        shot.save(&shot.render(), &dir.join(format!("stale-{name}.png")));
        crate::present(&ui, crate::sample::unsplit());
        shot.save(&shot.render(), &dir.join(format!("unaccounted-{name}.png")));

        // #36: the narrowest and the shortest window, the minimum size, and the minimum at 2x;
        // and a window maximised on a 1920×1080 screen, where the legends wrap least.
        crate::present(&ui, crate::sample::snapshot());
        for (size, width, height, scale_factor) in [
            ("full", 1920, 1040, 1.0),
            ("narrow", 760, 844, 1.0),
            ("short", 1000, 480, 1.0),
            ("min", 760, 480, 1.0),
            ("min-2x", 1520, 960, 2.0),
        ] {
            window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
            window.set_size(PhysicalSize::new(width, height));
            let small = Shot {
                window: window.clone(),
                now: now.clone(),
                width,
                height,
            };
            small.save(&small.render(), &dir.join(format!("{size}-{name}.png")));
            // The table scrolled sideways (header and rows together), the short page scrolled
            // down to the table; then back.
            let (x, y, delta_x, delta_y) = match size {
                "narrow" => (300.0, 700.0, -300.0, 0.0),
                "short" => (300.0, 200.0, 0.0, -1000.0),
                _ => continue,
            };
            let scroll = |delta_x: f32, delta_y: f32| {
                let position = LogicalPosition::new(x, y);
                window.dispatch_event(WindowEvent::PointerScrolled {
                    position,
                    delta_x,
                    delta_y,
                });
                window.dispatch_event(WindowEvent::PointerExited);
            };
            scroll(delta_x, delta_y);
            small.save(
                &small.render(),
                &dir.join(format!("{size}-scrolled-{name}.png")),
            );
            scroll(-delta_x, -delta_y);
        }
        window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
        window.set_size(PhysicalSize::new(shot.width, shot.height));

        // Sorted by swap, largest first, then smallest first.
        crate::present(&ui, crate::sample::snapshot());
        toggle(&ui, "Android emulator");
        ui.invoke_sort_clicked(1);
        shot.save(&shot.render(), &dir.join(format!("sort-swap-{name}.png")));
        ui.invoke_sort_clicked(1);
        shot.save(
            &shot.render(),
            &dir.join(format!("sort-swap-up-{name}.png")),
        );
        ui.invoke_sort_clicked(0);

        // Ctrl+F, then "k": five apps, three of them from the fold. Escape clears it.
        shot.key(Key::Control, true);
        shot.tap("f");
        shot.key(Key::Control, false);
        shot.tap("k");
        assert_eq!(ui.get_search(), "k");
        shot.save(&shot.render(), &dir.join(format!("search-{name}.png")));
        shot.tap(Key::Escape);
        assert_eq!(ui.get_search(), "");
        assert_eq!(ui.get_filtered(), "");

        // KWin found, the pointer on its Intel cell: "n/a" and the tooltip saying why.
        shot.key(Key::Control, true);
        shot.tap("f");
        shot.key(Key::Control, false);
        for c in ["k", "w", "i", "n"] {
            shot.tap(c);
        }
        crate::present(&ui, crate::sample::snapshot());
        let kwin = ui.get_rows().row_data(0).unwrap();
        assert_eq!((kwin.name.as_str(), kwin.igpu.as_str()), ("KWin", "n/a"));
        let colour = ui
            .global::<Theme>()
            .get_series()
            .row_data(kwin.series as usize)
            .unwrap();
        let pixels = shot.render();
        let swatch = (0..pixels.len())
            .rev()
            .find(|&i| i as u32 % shot.width < 60 && same(pixels[i], colour))
            .expect("KWin's swatch");
        // The Intel column's middle: right of it NVIDIA (62 px), VRAM (60 px) and the margins.
        shot.pointer(
            1000.0 - 8.0 - 60.0 - 62.0 - 26.0 - 22.0,
            (swatch as u32 / shot.width) as f32,
            false,
        );
        shot.wait(Duration::from_millis(800));
        shot.save(&shot.render(), &dir.join(format!("igpu-na-{name}.png")));
        shot.pointer(4.0, 4.0, false);
        shot.tap(Key::Escape);
        assert_eq!(ui.get_search(), "");

        if choice == ThemeChoice::Light {
            shot.pointer(1000.0 - 16.0 - 15.0, 14.0 + 15.0, true);
            shot.save(&shot.render(), &dir.join("app-light-menu.png"));
        }
        ui.hide().unwrap();
    }

    // The keyboard focus, reached by Tab (menu button, swap popup's button, tiles, search field,
    // header, table): the swap popup, the tiles and the header with their tooltips open at once,
    // the Android emulator's row, its leftover row with the tooltip, the search field, then the
    // menu.
    for (choice, name, height) in [
        (ThemeChoice::Light, "light", 844),
        (ThemeChoice::Dark, "dark", 844),
        (ThemeChoice::Sebchat, "sebchat", 1024),
    ] {
        let ui = MainWindow::new().unwrap();
        ui.global::<Theme>().set_choice(choice);
        crate::setup(
            &ui,
            crate::sample::snapshot(),
            Default::default(),
            |_| {},
            |_| {},
        );
        ui.show().unwrap();
        let shot = Shot {
            window: window.clone(),
            now: now.clone(),
            width: 1000,
            height,
        };
        window.set_size(PhysicalSize::new(shot.width, shot.height));
        // #11: memory pressure high, the popup opened from the chip, the third Tab stop (after
        // the menu button and the ⓘ button), with Space; Escape closes it.
        let mut tight = crate::sample::snapshot();
        tight.pressure = Some(18.0);
        crate::present(&ui, tight);
        for _ in 0..3 {
            shot.tap(Key::Tab);
        }
        shot.tap(" ");
        shot.save(&shot.render(), &dir.join(format!("pressure-{name}.png")));
        shot.tap(Key::Escape);
        crate::present(&ui, crate::sample::snapshot());
        ui.invoke_pick(1);
        // Escape gave the focus back to the ⓘ button; Enter opens it.
        shot.tap(Key::Return);
        shot.save(&shot.render(), &dir.join(format!("focus-popup-{name}.png")));
        shot.tap(Key::Escape);
        // On to the tiles.
        shot.tap(Key::Tab);
        shot.save(&shot.render(), &dir.join(format!("focus-tile-{name}.png")));
        // The last tile, Disk.
        shot.tap(Key::End);
        shot.save(
            &shot.render(),
            &dir.join(format!("focus-tile-disk-{name}.png")),
        );
        shot.tap(Key::Tab);
        shot.tap(Key::Tab);
        // Left/Right to Disk, the fifth figure; Enter sorts by it.
        for _ in 0..5 {
            shot.tap(Key::RightArrow);
        }
        shot.save(
            &shot.render(),
            &dir.join(format!("focus-header-{name}.png")),
        );
        shot.tap(Key::RightArrow);
        shot.save(
            &shot.render(),
            &dir.join(format!("focus-header-intel-{name}.png")),
        );
        shot.tap(Key::LeftArrow);
        shot.tap(Key::Return);
        assert_eq!(ui.get_sort_column(), 4);
        ui.invoke_sort_clicked(0);
        shot.tap(Key::Tab);
        shot.save(&shot.render(), &dir.join(format!("focus-row-{name}.png")));
        for _ in 0..5 {
            shot.tap(Key::DownArrow);
        }
        assert_eq!(ui.get_current(), 6);
        shot.save(
            &shot.render(),
            &dir.join(format!("focus-leftover-{name}.png")),
        );
        shot.tap(Key::Backtab);
        shot.tap(Key::Backtab);
        if choice == ThemeChoice::Light {
            shot.save(&shot.render(), &dir.join("focus-search-light.png"));
        }
        // Back past the ⓘ button to the menu button.
        shot.tap(Key::Backtab);
        shot.tap(Key::Backtab);
        shot.tap(Key::Backtab);
        shot.tap(Key::Return);
        shot.tap(Key::DownArrow);
        shot.save(&shot.render(), &dir.join(format!("focus-menu-{name}.png")));
        ui.hide().unwrap();
    }

    // This PC's own reading through sebwatch-core, twice so CPU and disk have rates, with the
    // biggest app expanded into its processes.
    let mut reader = crate::live::Reader::new();
    let interval = Duration::from_secs(1);
    let ui = MainWindow::new().unwrap();
    ui.global::<Theme>().set_choice(ThemeChoice::Light);
    crate::setup(
        &ui,
        reader.read(interval, &Default::default()).unwrap(),
        Default::default(),
        |_| {},
        |_| {},
    );
    let biggest = ui.get_rows().row_data(0).unwrap().key;
    ui.invoke_pick(0);
    std::thread::sleep(interval);
    let expanded = [biggest.to_string()].into();
    crate::present(&ui, reader.read(interval, &expanded).unwrap());
    ui.show().unwrap();
    let shot = Shot {
        window: window.clone(),
        now: now.clone(),
        width: 1000,
        height: 844,
    };
    window.set_size(PhysicalSize::new(shot.width, shot.height));
    shot.save(&shot.render(), &dir.join("live-light.png"));
}

/// `scripts/bench processes`: the CPU one refresh costs, reading and showing it, with the app
/// that holds the group `SEBWATCH_BENCH_GROUP` (relative to the cgroup root) folded, then
/// expanded. Prints one line each.
#[test]
#[ignore = "needs a large group; run through scripts/bench processes"]
fn bench_refresh() {
    const READS: u32 = 10;
    let group = std::env::var("SEBWATCH_BENCH_GROUP").expect("SEBWATCH_BENCH_GROUP names a group");
    let leaf = group.rsplit('/').next().unwrap();
    // This thread's CPU time in seconds: schedstat's first field is in ns.
    let cpu = || {
        let stat = std::fs::read_to_string("/proc/thread-self/schedstat").unwrap();
        stat.split_whitespace()
            .next()
            .unwrap()
            .parse::<f64>()
            .unwrap()
            / 1e9
    };
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let now = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(Offscreen {
        window: window.clone(),
        now: now.clone(),
    }))
    .unwrap();
    let shot = Shot {
        window: window.clone(),
        now,
        width: 1000,
        height: 844,
    };

    let mut reader = crate::live::Reader::new();
    let first = reader.read(crate::REFRESH, &Default::default()).unwrap();
    let app = first
        .apps
        .iter()
        .find(|a| a.groups.iter().any(|g| g == leaf))
        .expect("the bench group belongs to an app");
    let (key, processes) = (app.key.clone(), app.pids.len());
    let ui = MainWindow::new().unwrap();
    crate::setup(&ui, first, Default::default(), |_| {}, |_| {});
    ui.show().unwrap();
    window.set_size(PhysicalSize::new(shot.width, shot.height));
    shot.render();

    for label in ["folded", "expanded"] {
        if label == "expanded" {
            crate::VIEW.with_borrow_mut(|view| view.toggle(&key));
        }
        let expanded = crate::VIEW.with_borrow(|view| view.expanded().clone());
        // The first reading of an expanded app has no CPU yet, and the first frame lays it out.
        crate::present(&ui, reader.read(crate::REFRESH, &expanded).unwrap());
        shot.render();
        let (mut read, mut show) = (0.0, 0.0);
        for _ in 0..READS {
            let start = cpu();
            let snapshot = reader.read(crate::REFRESH, &expanded).unwrap();
            let read_end = cpu();
            crate::present(&ui, snapshot);
            shot.render();
            read += read_end - start;
            show += cpu() - read_end;
        }
        let ms = |s: f64| s * 1e3 / f64::from(READS);
        let core = 100.0 * (read + show) / f64::from(READS) / crate::REFRESH.as_secs_f64();
        println!(
            "BENCH {processes} processes, {label}: read {:.1} ms + show {:.1} ms per refresh, \
             {core:.2} % of one core at {} s",
            ms(read),
            ms(show),
            crate::REFRESH.as_secs()
        );
    }
}
