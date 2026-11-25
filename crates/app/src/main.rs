mod art;
mod clock;
mod desktop;
mod ending;
mod history;
mod icons;
mod kde;
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
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Model, ModelRc, RenderingState, VecModel, Weak};

slint::include_modules!();

/// How often the window reads, while it is shown.
const REFRESH: Duration = Duration::from_secs(2);

/// How long End waits before it offers Kill.
const END_GRACE: Duration = Duration::from_secs(5);

thread_local! {
    /// The window's view. It lives on the UI thread; readings come from the reader thread.
    static VIEW: RefCell<model::View> = RefCell::default();
    static ENDING: RefCell<Ending> = RefCell::new(Ending {
        system: ending::System::new(),
        last: None,
        watch: slint::Timer::default(),
    });
}

/// End and Kill: the system they act on and the last one done, watched until its row goes.
struct Ending {
    system: ending::System,
    last: Option<Ended>,
    watch: slint::Timer,
}

struct Ended {
    key: String,
    plan: ending::Plan,
    at: Instant,
    killed: bool,
}

/// End and Kill for the selected row, and End's news while it stays selected.
fn offer(window: &MainWindow, selected: Option<model::Selected>) {
    let plan = ENDING.with_borrow(|e| selected.as_ref().map(|s| e.system.plan(&s.endable)));
    let (key, name, full) = selected
        .as_ref()
        .map_or(("", "", ""), |s| (&*s.key, &*s.name, &*s.full));
    window.set_end_name(name.into());
    window.set_end_full(full.into());
    window.set_can_end(matches!(plan, Some(Ok(_))));
    window.set_end_asks(matches!(plan, Some(Ok(ref p)) if p.desktop));
    window.set_nothing(match plan {
        Some(Err(why)) => why.into(),
        _ => "".into(),
    });
    // The news is about one row: it goes when the selection moves on.
    if window.get_strip_key() != key {
        window.set_strip("".into());
    }
}

/// Ends or kills the selected row, then watches it go.
fn end(window: &MainWindow, kill: bool) {
    let Some(selected) = VIEW.with_borrow(|view| view.selected()) else {
        return;
    };
    let done = ENDING.with_borrow(|e| {
        let plan = e.system.plan(&selected.endable)?;
        match kill {
            true => e.system.kill(&plan),
            false => e.system.end(&plan),
        }
        .map(|()| plan)
    });
    window.set_strip_key(selected.key.as_str().into());
    match done {
        Ok(plan) => {
            window.set_strip("".into());
            VIEW.with_borrow_mut(|view| {
                view.note(&selected.key, model::Note::Ending);
                show(window, view.render());
            });
            let weak = window.as_weak();
            ENDING.with_borrow_mut(|e| {
                e.last = Some(Ended {
                    key: selected.key,
                    plan,
                    at: Instant::now(),
                    killed: kill,
                });
                e.watch.start(
                    slint::TimerMode::Repeated,
                    Duration::from_millis(250),
                    move || {
                        if let Some(window) = weak.upgrade() {
                            watch(&window);
                        }
                    },
                );
            });
        }
        Err(why) => {
            let verb = if kill { "kill" } else { "end" };
            window.set_strip(format!("Could not {verb} {}: {why}.", selected.name).into());
            window.set_strip_error(true);
        }
    }
}

/// The last End or Kill: gone, or still running after `END_GRACE`, when it offers Kill.
fn watch(window: &MainWindow) {
    let news = ENDING.with_borrow_mut(|e| {
        let last = e.last.as_ref()?;
        if !e.system.alive(&last.plan) {
            e.watch.stop();
            return e.last.take().map(|l| (l.key, model::Note::Ended));
        }
        (!last.killed && last.at.elapsed() >= END_GRACE).then(|| {
            e.watch.stop();
            (last.key.clone(), model::Note::Running)
        })
    });
    let Some((key, note)) = news else {
        return;
    };
    let gone = note == model::Note::Ended;
    // Running, the next reading drops the row; paused, it stays greyed until Resume.
    if !gone || window.get_paused() {
        VIEW.with_borrow_mut(|view| view.note(&key, note));
    }
    if window.get_strip_key() == key.as_str() {
        window.set_strip(match gone {
            true => "".into(),
            false => format!("Still running {} s after End.", END_GRACE.as_secs()).into(),
        });
        window.set_strip_error(false);
    }
    VIEW.with_borrow_mut(|view| show(window, view.render()));
}

fn show(window: &MainWindow, shown: model::Shown) {
    window.set_memory(rebar(window.get_memory(), shown.memory));
    window.set_swap(rebar(window.get_swap(), shown.swap));
    window.set_swap_info(shown.swap_info);
    window.set_has_vram(shown.vram.is_some());
    window.set_vram(rebar(window.get_vram(), shown.vram.unwrap_or_default()));
    window.global::<Columns>().set_igpu(shown.igpu.into());
    if let Some(new) = refill(&window.get_tiles(), shown.tiles) {
        window.set_tiles(new);
    }
    if let Some(new) = refill(&window.get_rows(), shown.rows) {
        window.set_rows(new);
    }
    window.set_sort_column(shown.sort.column as i32);
    window.set_sort_ascending(shown.sort.ascending);
    window.set_filtered(shown.filtered.into());
    window.set_current(shown.focus);
    window.set_warning(shown.warning.into());
    window.set_unsplit(shown.unsplit);
    offer(window, shown.selected);
}

/// Puts `items` into `model` in place, changing only the items that differ, so the window keeps
/// the items it made for them and a screen reader keeps its place (a new model rebuilds them all
/// on every refresh). A new model when `model` is the window's initial empty one.
fn refill<T: Clone + PartialEq + 'static>(model: &ModelRc<T>, items: Vec<T>) -> Option<ModelRc<T>> {
    let Some(model) = model.as_any().downcast_ref::<VecModel<T>>() else {
        return Some(ModelRc::from(Rc::new(VecModel::from(items))));
    };
    for _ in items.len()..model.row_count() {
        model.remove(model.row_count() - 1);
    }
    let kept = model.row_count();
    for (i, item) in items.iter().take(kept).enumerate() {
        if model.row_data(i).as_ref() != Some(item) {
            model.set_row_data(i, item.clone());
        }
    }
    model.extend(items.into_iter().skip(kept));
    None
}

/// Draws the icons again when the window's scale factor changed.
fn rescale(window: &MainWindow) {
    VIEW.with_borrow_mut(|view| {
        if view.scale_icons(window.window().scale_factor()) {
            show(window, view.render());
        }
    });
}

/// `new` with `old`'s segments model refilled, so a hovered segment keeps its ring and tooltip
/// across refreshes (#86).
fn rebar(old: Bar, mut new: Bar) -> Bar {
    let segments = std::mem::take(&mut new.segments).iter().collect();
    new.segments = refill(&old.segments, segments).unwrap_or(old.segments);
    new
}

/// Shows a new reading.
fn present(window: &MainWindow, snapshot: model::Snapshot) {
    VIEW.with_borrow_mut(|view| {
        view.scale_icons(window.window().scale_factor());
        show(window, view.update(snapshot))
    });
}

/// Art, the first reading, clicks and keys in the table, sorting, searching, pausing, End and
/// Kill on `system`: everything but the desktop and the reader thread. `toggled` gets the
/// expanded apps' keys whenever an app is expanded or folded, `sorted` the new order after a
/// click on a column's header, `paused` whether the window is now paused.
fn setup(
    window: &MainWindow,
    first: model::Snapshot,
    sort: model::Sort,
    system: ending::System,
    toggled: impl Fn(HashSet<String>) + 'static,
    sorted: impl Fn(model::Sort) + 'static,
    paused: impl Fn(bool) + 'static,
) {
    let art = window.global::<Art>();
    art.set_cloth(art::cloth());
    art.set_hatch(art::hatch());
    VIEW.set(model::View::default());
    ENDING.with_borrow_mut(|e| {
        e.system = system;
        e.last = None;
        e.watch.stop();
    });
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
    // Applies a click or key to the view and shows the result.
    let act = {
        let weak = window.as_weak();
        move |act: &dyn Fn(&mut model::View) -> bool| {
            let expanded = VIEW.with_borrow_mut(|view| {
                let changed = act(view);
                show(&weak.unwrap(), view.render());
                changed.then(|| view.expanded().clone())
            });
            if let Some(expanded) = expanded {
                toggled(expanded);
            }
        }
    };
    let act = Rc::new(act);
    window.on_pick({
        let act = act.clone();
        move |index| act(&|view| view.pick(index as usize))
    });
    window.on_expand({
        let act = act.clone();
        move |index| act(&|view| view.expand(index as usize))
    });
    window.on_bar_picked({
        let act = act.clone();
        let weak = window.as_weak();
        move |key| {
            act(&|view| view.reveal(&key));
            // A search that hid the app was cleared.
            if VIEW.with_borrow(|view| view.search.is_empty()) {
                weak.unwrap().set_search("".into());
            }
            VIEW.with_borrow(|view| view.revealed(&key))
        }
    });
    window.on_deselect({
        let act = act.clone();
        move || act(&|view| view.deselect())
    });
    window.on_key(move |key| act(&|view| view.press(key)));
    let weak = window.as_weak();
    window.on_end(move || end(&weak.unwrap(), false));
    let weak = window.as_weak();
    window.on_kill(move || end(&weak.unwrap(), true));
    let weak = window.as_weak();
    window.on_pause_clicked(move || {
        let window = weak.unwrap();
        let now = !window.get_paused();
        window.set_paused(now);
        window.set_paused_at(VIEW.with_borrow(|view| view.shown_at().into()));
        paused(now);
    });
}

/// The reader thread: reads every 2 s while the window draws (`drawn`, set on each frame; `None`
/// reads always), and at once when an app is expanded or folded or the window resumes; nothing
/// while it is paused. Ends with the window.
fn read_loop(
    mut reader: live::Reader,
    toggles: Receiver<HashSet<String>>,
    drawn: Option<Arc<AtomicBool>>,
    paused: Arc<AtomicBool>,
    window: Weak<MainWindow>,
) {
    let mut expanded = HashSet::new();
    loop {
        let event = toggles.recv_timeout(REFRESH);
        if paused.load(Ordering::Relaxed) {
            if let Ok(now) = event {
                expanded = now;
            }
            continue;
        }
        match event {
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
                    // Paused while it was read: the window keeps its moment.
                    if window.get_paused() {
                        return;
                    }
                    present(&window, snapshot);
                    // A frame even when nothing changed, so the next reading isn't skipped.
                    window.window().request_redraw();
                });
                if shown.is_err() {
                    return;
                }
            }
            // The last reading stays, with a warning saying it is not updating and why.
            Err(error) => {
                eprintln!("sebwatch: reading failed, showing the last one: {error}");
                let error = error.to_string();
                let shown = window.upgrade_in_event_loop(move |window| {
                    if window.get_paused() {
                        return;
                    }
                    VIEW.with_borrow_mut(|view| show(&window, view.failed(error)));
                });
                if shown.is_err() {
                    return;
                }
            }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let shortcut = match std::env::args().nth(1).as_deref() {
        Some("--bind-win-esc") => Some(kde::bind_win_esc()),
        Some("--unbind-win-esc") => Some(kde::unbind_win_esc()),
        _ => None,
    };
    if let Some(done) = shortcut {
        if let Err(error) = done {
            eprintln!("sebwatch: cannot change the Win+Esc shortcut: {error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if !kde::first_instance() {
        return Ok(());
    }
    let window = MainWindow::new()?;
    if let Some((width, height)) = settings::size() {
        window
            .window()
            .set_size(slint::LogicalSize::new(width, height));
    }
    // The size is saved on closing, unless maximised or full screen.
    let weak = window.as_weak();
    window.window().on_close_requested(move || {
        if let Some(window) = weak.upgrade() {
            let window = window.window();
            if !window.is_maximized() && !window.is_fullscreen() {
                let size = window.size().to_logical(window.scale_factor());
                settings::save_size((size.width, size.height));
            }
        }
        slint::CloseRequestResponse::HideWindow
    });
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
            eprintln!("sebwatch: cannot read memory use: {error}");
            std::process::exit(1);
        });
    let (toggled, toggles) = mpsc::channel();
    let paused = Arc::new(AtomicBool::new(false));
    let resumed = toggled.clone();
    setup(
        &window,
        first,
        settings::sort(),
        ending::System::new(),
        move |expanded| {
            let _ = toggled.send(expanded);
        },
        settings::save_sort,
        {
            let paused = paused.clone();
            move |now| {
                paused.store(now, Ordering::Relaxed);
                // Resumed: a reading at once, of the apps expanded now.
                if !now {
                    let _ = resumed.send(VIEW.with_borrow(|view| view.expanded().clone()));
                }
            }
        },
    );

    let drawn = Arc::new(AtomicBool::new(false));
    let notified = window.window().set_rendering_notifier({
        let drawn = drawn.clone();
        let weak = window.as_weak();
        let mut scale = 0.0;
        move |state, _| {
            if let RenderingState::AfterRendering = state {
                drawn.store(true, Ordering::Relaxed);
                // Moved to a screen of another scale: icons for it now, even while paused.
                let Some(window) = weak.upgrade() else { return };
                if window.window().scale_factor() != scale {
                    scale = window.window().scale_factor();
                    let weak = weak.clone();
                    slint::Timer::single_shot(Duration::ZERO, move || {
                        if let Some(window) = weak.upgrade() {
                            rescale(&window);
                        }
                    });
                }
            }
        }
    });
    if let Err(error) = &notified {
        eprintln!("sebwatch: refreshing even while hidden, no frame notifications: {error}");
    }
    let drawn = notified.is_ok().then_some(drawn);
    let weak = window.as_weak();
    std::thread::spawn(move || read_loop(reader, toggles, drawn, paused, weak));

    window.run()
}

#[cfg(test)]
mod tests {
    use slint::{Model, ModelRc};

    #[test]
    fn refill_keeps_the_model_across_growing_and_shrinking() {
        let model: ModelRc<i32> = super::refill(&ModelRc::default(), vec![1, 2]).unwrap();
        for items in [vec![1, 3, 4, 5], vec![6], vec![]] {
            assert!(super::refill(&model, items.clone()).is_none());
            assert_eq!(model.iter().collect::<Vec<_>>(), items);
        }
    }
}
