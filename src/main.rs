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

    iced::application("Arclight - WoW Launcher", ui::App::update, ui::App::view)
        .theme(|_| ui::theme::wow_theme())
        .decorations(false)
        .window(iced::window::Settings {
            size: iced::Size::new(800.0, 500.0),
            decorations: false,
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "arclight".to_string(),
                ..Default::default()
            },
            ..Default::default()
        })
        .font(include_bytes!("../assets/NotoEmoji-Regular.ttf"))
        .run_with(ui::App::new)
}
