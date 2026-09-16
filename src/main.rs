pub mod background;
pub mod config;
pub mod launcher;
pub mod ui;

fn main() -> iced::Result {
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
