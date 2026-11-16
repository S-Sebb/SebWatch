mod art;
mod desktop;
mod icons;
mod live;
mod model;
#[cfg(test)]
mod sample;
#[cfg(test)]
mod screenshots;
mod settings;

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, RenderingState, VecModel, Weak};

slint::include_modules!();

const REFRESH: Duration = Duration::from_secs(2);

thread_local! {
    /// The window's view. It lives on the UI thread; readings come from the reader thread.
    static VIEW: RefCell<model::View> = RefCell::default();
}

fn show(window: &MainWindow, shown: model::Shown) {
    window.set_memory(shown.memory);
    window.set_swap(shown.swap);
    window.set_has_vram(shown.vram.is_some());
    window.set_vram(shown.vram.unwrap_or_default());
    window.set_tiles(ModelRc::from(Rc::new(VecModel::from(shown.tiles))));
    window.set_rows(ModelRc::from(Rc::new(VecModel::from(shown.rows))));
    window.set_sort_column(shown.sort.column as i32);
    window.set_sort_ascending(shown.sort.ascending);
    window.set_filtered(shown.filtered.into());
}

/// Shows a new reading.
fn present(window: &MainWindow, snapshot: model::Snapshot) {
    VIEW.with_borrow_mut(|view| show(window, view.update(snapshot)));
}

/// Art, the first reading, expanding rows, sorting and searching: everything but the desktop and the reader
/// thread. `toggled` gets the expanded apps' keys whenever an app is expanded or folded, `sorted`
/// the new order after a click on a column's header.
fn setup(
    window: &MainWindow,
    first: model::Snapshot,
    sort: model::Sort,
    toggled: impl Fn(HashSet<String>) + 'static,
    sorted: impl Fn(model::Sort) + 'static,
) {
    let art = window.global::<Art>();
    art.set_cloth(art::cloth());
    art.set_hatch(art::hatch());
    VIEW.set(model::View::default());
    VIEW.with_borrow_mut(|view| view.sort = sort);
    present(window, first);
    let weak = window.as_weak();
    window.on_sort_clicked(move |column| {
        let sort = VIEW.with_borrow_mut(|view| {
            view.sort = view.sort.click(model::Column::ALL[column as usize]);
            show(&weak.unwrap(), view.render());
            view.sort
        });
        sorted(sort);
    });
    let weak = window.as_weak();
    window.on_searched(move |text| {
        VIEW.with_borrow_mut(|view| {
            view.search = text.into();
            show(&weak.unwrap(), view.render());
        });
    });
    let weak = window.as_weak();
    window.on_toggle(move |key| {
        let expanded = VIEW.with_borrow_mut(|view| {
            view.toggle(&key);
            show(&weak.unwrap(), view.render());
            view.expanded().clone()
        });
        toggled(expanded);
    });
}

/// The reader thread: reads every 2 s while the window draws (`drawn`, set on each frame; `None`
/// reads always), and at once when an app is expanded or folded. Ends with the window.
fn read_loop(
    mut reader: live::Reader,
    toggles: Receiver<HashSet<String>>,
    drawn: Option<Arc<AtomicBool>>,
    window: Weak<MainWindow>,
) {
    let mut expanded = HashSet::new();
    loop {
        match toggles.recv_timeout(REFRESH) {
            Ok(now) => expanded = now,
            // A minimised or hidden window draws no frames (Wayland reports no minimised state,
            // but stops frames), so it reads nothing.
            Err(RecvTimeoutError::Timeout) => {
                if drawn
                    .as_ref()
                    .is_some_and(|d| !d.swap(false, Ordering::Relaxed))
                {
                    continue;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
        match reader.read(REFRESH, &expanded) {
            Ok(snapshot) => {
                let shown = window.upgrade_in_event_loop(move |window| {
                    present(&window, snapshot);
                    // A frame even when nothing changed, so the next reading isn't skipped.
                    window.window().request_redraw();
                });
                if shown.is_err() {
                    return;
                }
            }
            // A transient failure (a group vanishing mid-read): keep the last reading.
            Err(error) => eprintln!("sebwatch: reading failed, showing the last one: {error}"),
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let window = MainWindow::new()?;
    window.set_version(env!("CARGO_PKG_VERSION").into());
    // Wayland compositors pick the taskbar icon from the desktop entry whose name matches the app id.
    slint::set_xdg_app_id("sebwatch")?;
    let theme = window.global::<Theme>();
    theme.set_choice(settings::theme());
    theme.set_desktop_dark(desktop::dark());
    let weak = window.as_weak();
    desktop::watch(move |dark| {
        let _ = weak
            .upgrade_in_event_loop(move |window| window.global::<Theme>().set_desktop_dark(dark));
    });
    window.on_theme_chosen(settings::save_theme);

    let mut reader = live::Reader::new();
    let first = reader
        .read(REFRESH, &HashSet::new())
        .unwrap_or_else(|error| {
            eprintln!(
                "sebwatch: cannot read memory use (needs cgroup v2 at /sys/fs/cgroup): {error}"
            );
            std::process::exit(1);
        });
    let (toggled, toggles) = mpsc::channel();
    setup(
        &window,
        first,
        settings::sort(),
        move |expanded| {
            let _ = toggled.send(expanded);
        },
        settings::save_sort,
    );

    let drawn = Arc::new(AtomicBool::new(false));
    let notified = window.window().set_rendering_notifier({
        let drawn = drawn.clone();
        move |state, _| {
            if let RenderingState::AfterRendering = state {
                drawn.store(true, Ordering::Relaxed);
            }
        }
    });
    if let Err(error) = &notified {
        eprintln!("sebwatch: refreshing even while hidden, no frame notifications: {error}");
    }
    let drawn = notified.is_ok().then_some(drawn);
    let weak = window.as_weak();
    std::thread::spawn(move || read_loop(reader, toggles, drawn, weak));

    window.run()
}
