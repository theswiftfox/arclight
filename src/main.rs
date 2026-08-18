pub mod config;
pub mod launcher;
pub mod ui;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("arclight=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Arclight");

    iced::daemon(ui::App::title, ui::App::update, ui::App::view)
        .subscription(ui::App::subscription)
        .theme(|_, _| ui::theme::wow_theme())
        .font(include_bytes!("../assets/NotoEmoji-Regular.ttf"))
        .run_with(|| {
            let (app, task) = ui::App::new();
            let open_main = {
                let (id, open) = iced::window::open(iced::window::Settings {
                    size: iced::Size::new(800.0, 500.0),
                    decorations: false,
                    platform_specific: iced::window::settings::PlatformSpecific {
                        application_id: "arclight".to_string(),
                        ..Default::default()
                    },
                    ..Default::default()
                });
                open.map(move |_| ui::Message::MainWindowOpened(id))
            };
            (app, iced::Task::batch([task, open_main]))
        })
}
