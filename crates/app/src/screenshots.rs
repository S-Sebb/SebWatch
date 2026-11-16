//! The window drawn offscreen by Slint's software renderer, in each theme, with the Android
//! emulator's bar segment hovered and its row expanded, then with the NVIDIA GPU asleep, then
//! sorted by swap both ways, then searched for "k" from the keyboard, plus Breeze Light with the menu open, and one of this PC's live
//! reading.
//! Nothing shows on the screen. Run with `scripts/screenshots <dir>`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

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
        crate::present(&ui, crate::sample::asleep());
        shot.save(&shot.render(), &dir.join(format!("app-{name}-asleep.png")));

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

        if choice == ThemeChoice::Light {
            shot.pointer(1000.0 - 16.0 - 15.0, 14.0 + 15.0, true);
            shot.save(&shot.render(), &dir.join("app-light-menu.png"));
        }
        ui.hide().unwrap();
    }

    // The keyboard focus, reached by Tab (menu button, search field, table): the Android
    // emulator's row, its leftover row with the tooltip, the search field, then the menu on Dark.
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
        ui.invoke_pick(1);
        for _ in 0..3 {
            shot.tap(Key::Tab);
        }
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
        if choice == ThemeChoice::Light {
            shot.save(&shot.render(), &dir.join("focus-search-light.png"));
        }
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
