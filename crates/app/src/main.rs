slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let window = MainWindow::new()?;
    window.set_version(env!("CARGO_PKG_VERSION").into());
    // Wayland compositors pick the taskbar icon from the desktop entry whose name matches the app id.
    slint::set_xdg_app_id("sebwatch")?;
    window.run()
}
