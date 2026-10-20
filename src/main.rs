pub mod background;
pub mod config;
pub mod launcher;
pub mod ui;

/// Works around Iced's UI intermittently freezing (input still handled, no redraws) on
/// Wayland with the default vsync present mode: <https://github.com/iced-rs/iced/issues/3496>.
/// An explicit `ICED_PRESENT_MODE` from the user is left untouched.
///
/// This runs before the tracing subscriber exists, so it reports via `eprintln!`.
#[cfg(target_os = "linux")]
fn apply_wayland_present_mode_workaround() {
    let on_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value == "wayland");
    if on_wayland && std::env::var_os("ICED_PRESENT_MODE").is_none() {
        // SAFETY: called first in `main`, before any other threads are spawned.
        unsafe { std::env::set_var("ICED_PRESENT_MODE", "no_vsync") };
        eprintln!(
            "Wayland detected: set ICED_PRESENT_MODE=no_vsync to avoid frozen redraws (iced#3496)"
        );
    }
}

fn main() -> iced::Result {
    #[cfg(target_os = "linux")]
    apply_wayland_present_mode_workaround();

    // A slow terminal must not block event handling (including Iced's own logs).
    let (writer, _log_guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .buffered_lines_limit(1024)
        .lossy(true)
        .finish(std::io::stderr());
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("arclight=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Arclight");

    iced::daemon(ui::App::new, ui::App::update, ui::App::view)
        .executor::<background::Executor>()
        .subscription(ui::App::subscription)
        .title(ui::App::title)
        .theme(ui::theme)
        .font(include_bytes!("../assets/NotoEmoji-Regular.ttf"))
        .run()
}
