//! The window drawn offscreen by Slint's software renderer, in each theme, with the Android
//! emulator's bar segment hovered and its row expanded, then with the NVIDIA GPU asleep, plus
//! Breeze Light with the menu open, and one of this PC's live reading.
//! Nothing shows on the screen. Run with `scripts/screenshots <dir>`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

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
        crate::setup(&ui, crate::sample::snapshot(), |_| {});
        ui.show().unwrap();
        let shot = Shot {
            window: window.clone(),
            now: now.clone(),
            width: 1000,
            height,
        };
        window.set_size(PhysicalSize::new(shot.width, shot.height));

        ui.set_selected("Android emulator".into());
        ui.invoke_toggle("Android emulator".into());
        let first = shot.render();
        let android = ui.global::<Theme>().get_series().row_data(1).unwrap();
        let (x, y) = find(&first, shot.width, android);
        shot.pointer(x, y, false);
        shot.wait(Duration::from_millis(800));
        shot.save(&shot.render(), &dir.join(format!("app-{name}.png")));

        shot.pointer(4.0, 4.0, false);
        crate::present(&ui, crate::sample::asleep());
        shot.save(&shot.render(), &dir.join(format!("app-{name}-asleep.png")));

        if choice == ThemeChoice::Light {
            crate::present(&ui, crate::sample::snapshot());
            shot.pointer(1000.0 - 16.0 - 15.0, 14.0 + 15.0, true);
            shot.save(&shot.render(), &dir.join("app-light-menu.png"));
        }
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
        |_| {},
    );
    let biggest = ui.get_rows().row_data(0).unwrap().key;
    ui.set_selected(biggest.clone());
    ui.invoke_toggle(biggest.clone());
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

/// The 0.1 mockups (#33 to #40), from `sample::mock`. Run with
/// `SEBWATCH_SHOTS=<dir> cargo test -p sebwatch --bin sebwatch mockups -- --ignored`.
#[test]
#[ignore = "writes PNG files"]
fn mockups() {
    use crate::sample::mock;
    let dir = std::path::PathBuf::from(std::env::var("SEBWATCH_SHOTS").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let now = Rc::new(Cell::new(Duration::ZERO));
    slint::platform::set_platform(Box::new(Offscreen {
        window: window.clone(),
        now: now.clone(),
    }))
    .unwrap();
    let open = |choice, width, height, shown: crate::model::Shown| {
        let ui = MainWindow::new().unwrap();
        ui.global::<Theme>().set_choice(choice);
        crate::setup(&ui, mock::snapshot(), |_| {});
        crate::show(&ui, shown);
        ui.show().unwrap();
        window.set_size(PhysicalSize::new(width, height));
        let shot = Shot {
            window: window.clone(),
            now: now.clone(),
            width,
            height,
        };
        (ui, shot)
    };
    let save =
        |shot: &Shot, name: &str| shot.save(&shot.render(), &dir.join(format!("{name}.png")));

    // Main view in each theme: the RAM bar's renamed leftover hovered, Android emulator open.
    for (choice, name, height) in [
        (ThemeChoice::Light, "light", 844),
        (ThemeChoice::Dark, "dark", 844),
        (ThemeChoice::Sebchat, "sebchat", 1024),
    ] {
        let (ui, shot) = open(
            choice,
            1000,
            height,
            mock::shown(mock::snapshot, 0, &["Android emulator"]),
        );
        ui.set_selected("Android emulator".into());
        if choice != ThemeChoice::Sebchat {
            let kernel = ui.global::<Theme>().get_kernel();
            let (x, y) = find(&shot.render(), shot.width, kernel);
            shot.pointer(x, y, false);
            shot.wait(Duration::from_millis(800));
        }
        save(&shot, &format!("main-{name}"));
        ui.hide().unwrap();
    }

    let light = ThemeChoice::Light;
    // #33: sorted by swap.
    let (ui, shot) = open(light, 1000, 844, mock::shown(mock::snapshot, 1, &[]));
    ui.set_sort_column(1);
    save(&shot, "sort-swap-light");
    ui.hide().unwrap();

    // #34: searching "k".
    let (shown, note) = mock::filtered();
    let (ui, shot) = open(light, 1000, 844, shown);
    ui.set_search("k".into());
    ui.set_search_focused(true);
    ui.set_filter_note(note.into());
    save(&shot, "search-light");
    ui.hide().unwrap();

    // #35: keyboard focus on a row, then in the theme menu.
    for (choice, name) in [(light, "light"), (ThemeChoice::Dark, "dark")] {
        let (ui, shot) = open(
            choice,
            1000,
            844,
            mock::shown(mock::snapshot, 0, &["Android emulator"]),
        );
        ui.set_selected("Android emulator".into());
        ui.set_focus_row(1);
        save(&shot, &format!("focus-row-{name}"));
        ui.hide().unwrap();
    }
    let (ui, shot) = open(light, 1000, 844, mock::shown(mock::snapshot, 0, &[]));
    ui.set_menu_focus(2);
    shot.render();
    shot.pointer(1000.0 - 16.0 - 15.0, 14.0 + 15.0, true);
    save(&shot, "focus-menu-light");
    ui.hide().unwrap();

    // #36: the minimum width, and a short window.
    for (name, width, height) in [("narrow", 760, 844), ("short", 1000, 480)] {
        let (ui, shot) = open(light, width, height, mock::shown(mock::snapshot, 0, &[]));
        save(&shot, &format!("{name}-light"));
        ui.hide().unwrap();
    }
    let (ui, shot) = open(
        ThemeChoice::Dark,
        760,
        844,
        mock::shown(mock::snapshot, 0, &[]),
    );
    save(&shot, "narrow-dark");
    ui.hide().unwrap();

    // #38: unavailable readings, not updating, no per-app accounting.
    let (ui, shot) = open(light, 1000, 844, mock::unavailable());
    ui.set_vram_note("The NVIDIA driver is not answering".into());
    save(&shot, "unavailable-light");
    ui.hide().unwrap();
    let (ui, shot) = open(light, 1000, 844, mock::shown(mock::snapshot, 0, &[]));
    ui.set_message("Not updating: the last 3 readings failed (cannot read /sys/fs/cgroup/user.slice/memory.stat). Showing the reading from 14:02:16.".into());
    save(&shot, "stale-light");
    ui.hide().unwrap();
    let (ui, shot) = open(light, 1000, 844, mock::unaccounted());
    ui.set_placeholder_title("SebWatch can't split memory by app on this system".into());
    ui.set_placeholder_text("It needs cgroup v2 memory accounting and the per-app groups a systemd user session creates; this system has neither. The bars above still show the totals.".into());
    save(&shot, "unaccounted-light");
    ui.hide().unwrap();

    // #40: closed apps, open in the table, hovered in the RAM bar.
    let (ui, shot) = open(
        light,
        1000,
        844,
        mock::shown(mock::snapshot, 0, &["closed"]),
    );
    let other = ui.global::<Theme>().get_other();
    let pixels = shot.render();
    let (x, y) = find(&pixels, shot.width, other);
    let i = (y as u32 - 12) * shot.width + x as u32;
    let run = pixels[i as usize..]
        .iter()
        .take_while(|p| (p.r, p.g, p.b) == (other.red(), other.green(), other.blue()))
        .count();
    shot.pointer(x + run as f32 + 12.0, y, false);
    shot.wait(Duration::from_millis(800));
    save(&shot, "closed-light");
    ui.hide().unwrap();
}
