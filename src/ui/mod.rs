use std::sync::Arc;

use iced::{
    Element, Font, Length, Subscription, Task,
    widget::{
        button, checkbox, column, container, horizontal_space, mouse_area, pick_list, row,
        scrollable, text, text_input, vertical_space,
    },
    window,
};
use tokio::sync::mpsc;

use crate::{
    config::{Config, OnLaunchBehavior},
    launcher,
    launcher::{
        bnet::{self, AuthStatus},
        detect::DetectedRunner,
    },
};

pub mod logs;
pub mod theme;

pub use theme::wow_theme;

/// Font for emoji/symbol characters (`NotoEmoji`)
const EMOJI_FONT: Font = Font {
    family: iced::font::Family::Name("Noto Emoji"),
    weight: iced::font::Weight::Normal,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};

#[derive(Debug, Clone)]
pub enum Screen {
    Main,
    Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
    // Window controls
    WindowDrag(window::Id),
    WindowMinimize(window::Id),
    WindowMaximize(window::Id),
    WindowClose(window::Id),
    // Navigation
    GoToSettings,
    GoToMain,
    // Launch
    Launch,
    LaunchResult(Result<(), String>),
    // Battle.net
    LaunchBnet,
    LaunchBnetResult(Result<(), String>),
    KillBnet,
    KillBnetResult(Result<(), String>),
    InstallBnet,
    InstallBnetResult(Result<(), String>),
    RefreshAuthStatus,
    AuthStatusChecked(AuthStatus),
    // Settings
    GamePathChanged(String),
    RunnerSelected(DetectedRunner),
    OnLaunchChanged(OnLaunchBehavior),
    // WoW settings
    WowArgsChanged(String),
    WowSkipDefaultArgsToggled(bool),
    WowEnvKeyChanged(usize, String),
    WowEnvValueChanged(usize, String),
    AddWowEnvVar,
    RemoveWowEnvVar(usize),
    // Battle.net settings
    BnetArgsChanged(String),
    BnetEnvKeyChanged(usize, String),
    BnetEnvValueChanged(usize, String),
    AddBnetEnvVar,
    RemoveBnetEnvVar(usize),
    // Drive mappings
    DriveLetterChanged(usize, String),
    DrivePathChanged(usize, String),
    AddDriveMapping,
    RemoveDriveMapping(usize),
    // Performance
    MangoHudToggled(bool),
    MangoHudConfigChanged(String),
    GamescopeToggled(bool),
    GamescopeArgsChanged(String),
    // Save
    SaveSettings,
    SettingsSaved(Result<(), String>),
    // Runner detection
    RunnersDetected(Vec<DetectedRunner>),
    // Logs
    OpenLogs,
    LogsWindowOpened(window::Id),
    MainWindowOpened(window::Id),
    LogLine(logs::LogEntry),
    CopyLogs,
    ClearLogs,
    // Window events
    WindowCloseRequested(window::Id),
}

pub struct App {
    screen: Screen,
    config: Config,
    status: String,
    launching: bool,
    maximized: bool,
    auth_status: AuthStatus,
    bnet_busy: bool,
    detected_runners: Vec<DetectedRunner>,
    selected_runner: Option<DetectedRunner>,
    // Settings state
    game_path_input: String,
    wow_args_input: String,
    wow_env_vars: Vec<(String, String)>,
    bnet_args_input: String,
    bnet_env_vars: Vec<(String, String)>,
    drive_mappings: Vec<(String, String)>,
    // Logs
    log_buffer: logs::LogBuffer,
    log_sender: mpsc::UnboundedSender<logs::LogEntry>,
    log_receiver: Arc<std::sync::Mutex<Option<mpsc::UnboundedReceiver<logs::LogEntry>>>>,
    logs_window_id: Option<window::Id>,
    // Window IDs
    main_window_id: Option<window::Id>,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let config = Config::load().unwrap_or_default();
        let game_path_input = config
            .game_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let wow_args_input = config.wow_args.join(" ");
        let wow_env_vars = config.wow_env_vars.clone();
        let bnet_args_input = config.bnet_args.join(" ");
        let bnet_env_vars = config.bnet_env_vars.clone();
        let drive_mappings = config.drive_mappings.clone();
        let auth_status = bnet::check_auth_status(&config);

        let (log_sender, log_receiver) = logs::create_log_channel();

        let app = Self {
            screen: Screen::Main,
            config,
            status: "Ready".into(),
            launching: false,
            maximized: false,
            auth_status,
            bnet_busy: false,
            detected_runners: Vec::new(),
            selected_runner: None,
            game_path_input,
            wow_args_input,
            wow_env_vars,
            bnet_args_input,
            bnet_env_vars,
            drive_mappings,
            log_buffer: logs::LogBuffer::default(),
            log_sender,
            log_receiver: Arc::new(std::sync::Mutex::new(Some(log_receiver))),
            logs_window_id: None,
            main_window_id: None,
        };

        let task = Task::perform(async { detect_runners_async() }, Message::RunnersDetected);
        (app, task)
    }

    #[must_use]
    pub fn title(&self, window: window::Id) -> String {
        if Some(window) == self.logs_window_id {
            "Arclight - Logs".to_string()
        } else {
            "Arclight - WoW Launcher".to_string()
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let log_rx = self.log_receiver.clone();
        let log_sub = Subscription::run_with_id(
            "log-reader",
            iced::futures::stream::unfold(log_rx, |rx| async move {
                let mut receiver = rx.lock().ok()?.take()?;
                let entry = receiver.recv().await?;
                Some((
                    Message::LogLine(entry),
                    Arc::new(std::sync::Mutex::new(Some(receiver))),
                ))
            }),
        );

        let close_sub = iced::window::close_requests().map(Message::WindowCloseRequested);

        Subscription::batch([log_sub, close_sub])
    }

    #[allow(clippy::too_many_lines)]
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowDrag(id) => iced::window::drag(id),
            Message::WindowMinimize(id) => iced::window::minimize(id, true),
            Message::WindowMaximize(id) => {
                if Some(id) == self.main_window_id {
                    self.maximized = !self.maximized;
                    iced::window::maximize(id, self.maximized)
                } else {
                    Task::none()
                }
            }
            Message::WindowClose(id) => {
                if Some(id) == self.logs_window_id {
                    self.logs_window_id = None;
                    iced::window::close(id)
                } else {
                    // Main window close — exit app
                    iced::exit()
                }
            }
            Message::WindowCloseRequested(id) => {
                if Some(id) == self.logs_window_id {
                    self.logs_window_id = None;
                    iced::window::close(id)
                } else {
                    iced::exit()
                }
            }
            Message::GoToSettings => {
                self.screen = Screen::Settings;
                Task::none()
            }
            Message::GoToMain => {
                self.screen = Screen::Main;
                Task::none()
            }
            Message::Launch => self.handle_launch(),
            Message::LaunchResult(result) => {
                self.launching = false;
                match result {
                    Ok(()) => self.status = "Game launched!".into(),
                    Err(e) => self.status = format!("Launch failed: {e}"),
                }
                Task::none()
            }
            Message::LaunchBnet => self.handle_launch_bnet(),
            Message::LaunchBnetResult(result) => {
                self.bnet_busy = false;
                match result {
                    Ok(()) => self.status = "Battle.net closed.".into(),
                    Err(e) => self.status = format!("Battle.net failed: {e}"),
                }
                self.refresh_auth_task()
            }
            Message::KillBnet => {
                let config = self.config.clone();
                Task::perform(
                    async move { bnet::kill_bnet(&config).await.map_err(|e| e.to_string()) },
                    Message::KillBnetResult,
                )
            }
            Message::KillBnetResult(_result) => {
                self.bnet_busy = false;
                self.status = "Battle.net killed.".into();
                self.refresh_auth_task()
            }
            Message::InstallBnet => {
                self.bnet_busy = true;
                self.status = "Installing Battle.net...".into();
                let config = self.config.clone();
                Task::perform(
                    async move {
                        bnet::install_bnet(&config, None)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::InstallBnetResult,
                )
            }
            Message::InstallBnetResult(result) => {
                self.bnet_busy = false;
                match result {
                    Ok(()) => self.status = "Battle.net installed!".into(),
                    Err(e) => self.status = format!("Install failed: {e}"),
                }
                self.refresh_auth_task()
            }
            Message::RefreshAuthStatus => self.refresh_auth_task(),
            Message::AuthStatusChecked(status) => {
                self.auth_status = status;
                Task::none()
            }
            Message::SaveSettings => self.handle_save_settings(),
            Message::SettingsSaved(result) => {
                match result {
                    Ok(()) => self.status = "Settings saved".into(),
                    Err(e) => self.status = format!("Failed to save: {e}"),
                }
                self.screen = Screen::Main;
                self.refresh_auth_task()
            }
            Message::RunnersDetected(runners) => {
                if let Some(ref binary) = self.config.runner.binary {
                    self.selected_runner = runners.iter().find(|r| &r.path == binary).cloned();
                }
                self.detected_runners = runners;
                Task::none()
            }
            Message::MainWindowOpened(id) => {
                self.main_window_id = Some(id);
                Task::none()
            }
            Message::LogsWindowOpened(id) => {
                self.logs_window_id = Some(id);
                Task::none()
            }
            Message::OpenLogs => {
                if self.logs_window_id.is_some() {
                    // Already open
                    return Task::none();
                }
                let (id, task) = window::open(window::Settings {
                    size: iced::Size::new(700.0, 400.0),
                    decorations: false,
                    ..Default::default()
                });
                self.logs_window_id = Some(id);
                task.discard()
            }
            Message::LogLine(entry) => {
                self.log_buffer.push(entry);
                Task::none()
            }
            Message::CopyLogs => {
                let text = self.log_buffer.to_text();
                std::thread::spawn(move || {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(text);
                    }
                });
                Task::none()
            }
            Message::ClearLogs => {
                self.log_buffer.clear();
                Task::none()
            }
            _ => self.handle_settings_input(message),
        }
    }

    fn handle_launch(&mut self) -> Task<Message> {
        self.launching = true;
        self.status = "Launching WoW...".into();
        if let Err(e) = self.config.apply_drive_mappings() {
            tracing::error!("Failed to apply drive mappings: {}", e);
        }
        let config = self.config.clone();
        let on_launch = self.config.on_launch;

        // Only pipe logs if we're not closing on launch
        let log_sender = if on_launch == OnLaunchBehavior::Close {
            None
        } else {
            Some(self.log_sender.clone())
        };

        let use_launcherlogin = false;
        let launch_task = Task::perform(
            async move {
                launcher::launch(&config, use_launcherlogin, log_sender)
                    .await
                    .map_err(|e| e.to_string())
            },
            Message::LaunchResult,
        );
        let window_task = match on_launch {
            OnLaunchBehavior::Minimize => {
                if let Some(id) = self.main_window_id {
                    iced::window::minimize(id, true)
                } else {
                    iced::window::get_oldest().and_then(|id| iced::window::minimize(id, true))
                }
            }
            OnLaunchBehavior::Close => {
                if let Some(id) = self.main_window_id {
                    iced::window::close(id)
                } else {
                    iced::window::get_oldest().and_then(iced::window::close)
                }
            }
            OnLaunchBehavior::DoNothing => Task::none(),
        };
        Task::batch([launch_task, window_task])
    }

    fn handle_launch_bnet(&mut self) -> Task<Message> {
        self.bnet_busy = true;
        self.status = "Battle.net running... Close it when done.".into();
        if let Err(e) = self.config.apply_drive_mappings() {
            tracing::error!("Failed to apply drive mappings: {}", e);
        }
        let config = self.config.clone();
        let log_sender = Some(self.log_sender.clone());
        Task::perform(
            async move {
                bnet::launch_bnet(&config, log_sender)
                    .await
                    .map_err(|e| e.to_string())
            },
            Message::LaunchBnetResult,
        )
    }

    fn handle_save_settings(&mut self) -> Task<Message> {
        if self.game_path_input.is_empty() {
            self.config.game_path = None;
        } else {
            self.config.game_path = Some(self.game_path_input.clone().into());
        }
        self.config.wow_args = self
            .wow_args_input
            .split_whitespace()
            .map(std::string::ToString::to_string)
            .collect();
        self.config.wow_env_vars = self
            .wow_env_vars
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .cloned()
            .collect();
        self.config.bnet_args = self
            .bnet_args_input
            .split_whitespace()
            .map(std::string::ToString::to_string)
            .collect();
        self.config.bnet_env_vars = self
            .bnet_env_vars
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .cloned()
            .collect();
        self.config.drive_mappings = self
            .drive_mappings
            .iter()
            .filter(|(l, p)| !l.is_empty() && !p.is_empty())
            .cloned()
            .collect();

        if let Err(e) = self.config.apply_drive_mappings() {
            tracing::error!("Failed to apply drive mappings: {}", e);
        }

        let config = self.config.clone();
        Task::perform(
            async move { config.save().map_err(|e| e.to_string()) },
            Message::SettingsSaved,
        )
    }

    fn handle_settings_input(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::GamePathChanged(path) => self.game_path_input = path,
            Message::RunnerSelected(runner) => {
                self.config.runner.binary = Some(runner.path.clone());
                self.config.runner.runner_type = runner.runner_type;
                self.selected_runner = Some(runner);
            }
            Message::OnLaunchChanged(behavior) => self.config.on_launch = behavior,
            Message::WowArgsChanged(args) => self.wow_args_input = args,
            Message::WowSkipDefaultArgsToggled(val) => self.config.wow_skip_default_args = val,
            Message::WowEnvKeyChanged(idx, key) => {
                if let Some(entry) = self.wow_env_vars.get_mut(idx) {
                    entry.0 = key;
                }
            }
            Message::WowEnvValueChanged(idx, value) => {
                if let Some(entry) = self.wow_env_vars.get_mut(idx) {
                    entry.1 = value;
                }
            }
            Message::AddWowEnvVar => self.wow_env_vars.push((String::new(), String::new())),
            Message::RemoveWowEnvVar(idx) => {
                if idx < self.wow_env_vars.len() {
                    self.wow_env_vars.remove(idx);
                }
            }
            Message::BnetArgsChanged(args) => self.bnet_args_input = args,
            Message::BnetEnvKeyChanged(idx, key) => {
                if let Some(entry) = self.bnet_env_vars.get_mut(idx) {
                    entry.0 = key;
                }
            }
            Message::BnetEnvValueChanged(idx, value) => {
                if let Some(entry) = self.bnet_env_vars.get_mut(idx) {
                    entry.1 = value;
                }
            }
            Message::AddBnetEnvVar => self.bnet_env_vars.push((String::new(), String::new())),
            Message::RemoveBnetEnvVar(idx) => {
                if idx < self.bnet_env_vars.len() {
                    self.bnet_env_vars.remove(idx);
                }
            }
            Message::DriveLetterChanged(idx, letter) => {
                if let Some(entry) = self.drive_mappings.get_mut(idx) {
                    entry.0 = letter;
                }
            }
            Message::DrivePathChanged(idx, path) => {
                if let Some(entry) = self.drive_mappings.get_mut(idx) {
                    entry.1 = path;
                }
            }
            Message::AddDriveMapping => self.drive_mappings.push((String::new(), String::new())),
            Message::RemoveDriveMapping(idx) if idx < self.drive_mappings.len() => {
                self.drive_mappings.remove(idx);
            }
            Message::MangoHudToggled(val) => self.config.mangohud_enabled = val,
            Message::MangoHudConfigChanged(val) => self.config.mangohud_config = val,
            Message::GamescopeToggled(val) => self.config.gamescope_enabled = val,
            Message::GamescopeArgsChanged(val) => self.config.gamescope_args = val,
            _ => {}
        }
        Task::none()
    }

    fn refresh_auth_task(&self) -> Task<Message> {
        let config = self.config.clone();
        Task::perform(
            async move { bnet::check_auth_status(&config) },
            Message::AuthStatusChecked,
        )
    }

    #[must_use]
    pub fn view(&self, window: window::Id) -> Element<'_, Message> {
        if Some(window) == self.logs_window_id {
            return self.view_logs(window);
        }

        let content = match self.screen {
            Screen::Main => self.view_main(),
            Screen::Settings => self.view_settings(),
        };

        column![self.view_titlebar(window), content].into()
    }

    fn view_titlebar(&self, window: window::Id) -> Element<'_, Message> {
        let title_text = if Some(window) == self.logs_window_id {
            "Arclight - Logs"
        } else {
            "Arclight"
        };
        let title = text(title_text).size(13);

        let min_btn = button(
            container(text("\u{2014}").size(14))
                .width(28)
                .height(22)
                .center_x(28)
                .center_y(22),
        )
        .padding(0)
        .on_press(Message::WindowMinimize(window));

        let max_btn = button(
            container(text("\u{25FB}").font(EMOJI_FONT).size(11))
                .width(28)
                .height(22)
                .center_x(28)
                .center_y(22),
        )
        .padding(0)
        .on_press(Message::WindowMaximize(window));

        let close_btn = button(
            container(text("\u{2716}").font(EMOJI_FONT).size(11))
                .width(28)
                .height(22)
                .center_x(28)
                .center_y(22),
        )
        .padding(0)
        .on_press(Message::WindowClose(window));

        let controls = row![min_btn, max_btn, close_btn].spacing(2);

        let bar = row![title, horizontal_space(), controls]
            .spacing(8)
            .padding([6, 12])
            .align_y(iced::Alignment::Center);

        let styled_bar = container(bar)
            .width(Length::Fill)
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(theme::COLOR_TITLEBAR)),
                ..Default::default()
            });

        mouse_area(styled_bar)
            .on_press(Message::WindowDrag(window))
            .into()
    }

    fn view_logs(&self, window: window::Id) -> Element<'_, Message> {
        let copy_btn = button("Copy All")
            .padding([6, 16])
            .on_press(Message::CopyLogs);
        let clear_btn = button("Clear")
            .padding([6, 16])
            .on_press(Message::ClearLogs);

        let top_bar = row![
            text("Subprocess Logs").size(16),
            horizontal_space(),
            copy_btn,
            clear_btn,
        ]
        .spacing(8)
        .padding([8, 16])
        .align_y(iced::Alignment::Center);

        let mut log_col = column![].spacing(2).padding([4, 16]);
        for entry in &self.log_buffer.lines {
            log_col = log_col.push(text(entry.formatted()).size(12).font(Font::MONOSPACE));
        }

        let log_scroll = scrollable(log_col).height(Length::Fill).width(Length::Fill);

        column![self.view_titlebar(window), top_bar, log_scroll,].into()
    }

    #[allow(clippy::too_many_lines)]
    fn view_main(&self) -> Element<'_, Message> {
        let title = text("ARCLIGHT").size(40);
        let subtitle = text("World of Warcraft Launcher").size(16);

        // Auth status indicator
        let (auth_color, auth_text) = match &self.auth_status {
            AuthStatus::NotInstalled => {
                (theme::COLOR_DANGER, "Battle.net not installed".to_string())
            }
            AuthStatus::NotLoggedIn => (theme::COLOR_DANGER, "Not logged in".to_string()),
            AuthStatus::BnetLoggedIn => (theme::COLOR_SUCCESS, "Battle.net logged in".to_string()),
            AuthStatus::WowReady { game_account } => (
                theme::COLOR_SUCCESS,
                format!("Battle.net logged in ({game_account})"),
            ),
        };

        let auth_row = {
            let indicator = container(text("").size(1)).width(12).height(12).style(
                move |_theme: &iced::Theme| container::Style {
                    background: Some(iced::Background::Color(auth_color)),
                    border: iced::Border::default().rounded(6),
                    ..Default::default()
                },
            );
            let status_text = text(auth_text).size(12);

            let mut btn_row = row![].spacing(8);

            if self.bnet_busy {
                btn_row = btn_row.push(button("Battle.net running...").padding([6, 16]));
                btn_row = btn_row.push(
                    button("Force Quit")
                        .padding([6, 16])
                        .on_press(Message::KillBnet),
                );
            } else if !self.config.bnet_installed() {
                btn_row = btn_row.push(
                    button("Install Battle.net")
                        .padding([6, 16])
                        .on_press(Message::InstallBnet),
                );
            } else {
                btn_row = btn_row.push(
                    button("Launch Battle.net")
                        .padding([6, 16])
                        .on_press(Message::LaunchBnet),
                );
            }

            let refresh_btn = button("Refresh")
                .padding([6, 12])
                .on_press(Message::RefreshAuthStatus);

            row![
                indicator,
                status_text,
                horizontal_space(),
                btn_row,
                refresh_btn
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
        };

        let status = text(&self.status).size(14);

        let can_play = self.config.game_path.is_some() && !self.launching;

        let launch_btn = if can_play {
            button(text("Play").size(24))
                .padding([12, 48])
                .on_press(Message::Launch)
        } else {
            button(text("Play").size(24)).padding([12, 48])
        };

        let settings_btn = button(
            row![text("⚙").font(EMOJI_FONT).size(16), text("Settings")]
                .spacing(6)
                .align_y(iced::Alignment::Center),
        )
        .padding([8, 20])
        .on_press(Message::GoToSettings);

        let logs_btn = button("View Logs")
            .padding([8, 20])
            .on_press(Message::OpenLogs);

        let runner_info = if let Some(ref runner) = self.selected_runner {
            text(format!("Runner: {}", runner.name)).size(12)
        } else if let Ok(runner) = launcher::get_runner(&self.config.runner) {
            text(format!("Runner: {} (auto)", runner.name)).size(12)
        } else {
            text("No runner detected".to_string()).size(12)
        };

        let game_path_info = if let Some(ref path) = self.config.game_path {
            text(format!("Game: {}", path.display())).size(12)
        } else {
            text("Game path not set — configure in Settings".to_string()).size(12)
        };

        let content = column![
            title,
            subtitle,
            vertical_space().height(20),
            auth_row,
            vertical_space().height(20),
            launch_btn,
            vertical_space().height(12),
            status,
            vertical_space().height(Length::Fill),
            game_path_info,
            runner_info,
            row![settings_btn, logs_btn].spacing(8),
        ]
        .align_x(iced::Alignment::Center)
        .spacing(8)
        .padding(40)
        .width(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    #[allow(clippy::too_many_lines)]
    fn view_settings(&self) -> Element<'_, Message> {
        // Top bar
        let title = text("Settings").size(28);
        let save_btn = button("Save")
            .padding([8, 20])
            .on_press(Message::SaveSettings);
        let back_btn = button("Cancel")
            .padding([8, 20])
            .on_press(Message::GoToMain);
        let top_bar = row![title, horizontal_space(), back_btn, save_btn]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        // Game path
        let game_path_label = text("WoW Install Path:").size(14);
        let game_path_field = text_input("/path/to/World of Warcraft", &self.game_path_input)
            .on_input(Message::GamePathChanged)
            .padding(8);

        // Runner selection
        let runner_label = text("Runner:").size(14);
        let runner_picker = pick_list(
            self.detected_runners.clone(),
            self.selected_runner.clone(),
            Message::RunnerSelected,
        )
        .placeholder("Auto-detect")
        .padding(8)
        .width(Length::Fill);

        // On launch behavior
        let on_launch_label = text("On game launch:").size(14);
        let on_launch_picker = pick_list(
            vec![
                OnLaunchBehavior::DoNothing,
                OnLaunchBehavior::Minimize,
                OnLaunchBehavior::Close,
            ],
            Some(self.config.on_launch),
            Message::OnLaunchChanged,
        )
        .padding(8)
        .width(Length::Fill);

        // WoW Section
        let wow_header = text("WoW Launch Settings").size(18);

        let wow_args_label = text("Additional launch args:").size(14);
        let wow_args_field = text_input("e.g. -windowed", &self.wow_args_input)
            .on_input(Message::WowArgsChanged)
            .padding(8);

        let wow_default_args_checkbox = checkbox(
            "Skip default args (-launcherlogin -uid wow)",
            self.config.wow_skip_default_args,
        )
        .on_toggle(Message::WowSkipDefaultArgsToggled);

        let wow_env_label = text("Environment Variables:").size(14);
        let mut wow_env_col = column![].spacing(4);
        for (idx, (key, value)) in self.wow_env_vars.iter().enumerate() {
            let key_input = text_input("KEY", key)
                .on_input(move |v| Message::WowEnvKeyChanged(idx, v))
                .padding(6)
                .width(Length::FillPortion(2));
            let value_input = text_input("VALUE", value)
                .on_input(move |v| Message::WowEnvValueChanged(idx, v))
                .padding(6)
                .width(Length::FillPortion(3));
            let remove_btn = button("x")
                .padding([4, 8])
                .on_press(Message::RemoveWowEnvVar(idx));
            wow_env_col = wow_env_col.push(row![key_input, value_input, remove_btn].spacing(4));
        }
        let add_wow_env_btn = button("+ Add Variable")
            .padding([4, 12])
            .on_press(Message::AddWowEnvVar);

        // Drive Mappings Section
        let drive_header = text("Drive Mappings").size(18);
        let mut drive_col = column![].spacing(4);
        for (idx, (letter, path)) in self.drive_mappings.iter().enumerate() {
            let letter_input = text_input("d", letter)
                .on_input(move |v| Message::DriveLetterChanged(idx, v))
                .padding(6)
                .width(Length::Fixed(50.0));
            let colon = text(":").size(14);
            let path_input = text_input("/media/ssd", path)
                .on_input(move |v| Message::DrivePathChanged(idx, v))
                .padding(6)
                .width(Length::Fill);
            let remove_btn = button("x")
                .padding([4, 8])
                .on_press(Message::RemoveDriveMapping(idx));
            drive_col = drive_col.push(
                row![letter_input, colon, path_input, remove_btn]
                    .spacing(4)
                    .align_y(iced::Alignment::Center),
            );
        }
        let add_drive_btn = button("+ Add Drive")
            .padding([4, 12])
            .on_press(Message::AddDriveMapping);

        // Performance Section
        let perf_header = text("Performance").size(18);

        let mangohud_checkbox = checkbox("Enable MangoHud", self.config.mangohud_enabled)
            .on_toggle(Message::MangoHudToggled);
        let mangohud_config_field =
            text_input("e.g. fps,frametime,cpu_temp", &self.config.mangohud_config)
                .on_input(Message::MangoHudConfigChanged)
                .padding(8);

        let gamescope_checkbox = checkbox("Enable Gamescope", self.config.gamescope_enabled)
            .on_toggle(Message::GamescopeToggled);
        let gamescope_args_field =
            text_input("e.g. -w 2560 -h 1440 -f", &self.config.gamescope_args)
                .on_input(Message::GamescopeArgsChanged)
                .padding(8);

        // Battle.net Section
        let bnet_header = text("Battle.net Settings").size(18);

        let bnet_status = if self.config.bnet_installed() {
            text("Status: Installed ✓").size(12)
        } else {
            text("Status: Not installed").size(12)
        };

        let bnet_args_label = text("Launch args:").size(14);
        let bnet_args_field = text_input("e.g. --in-process-gpu", &self.bnet_args_input)
            .on_input(Message::BnetArgsChanged)
            .padding(8);

        let bnet_env_label = text("Environment Variables:").size(14);
        let mut bnet_env_col = column![].spacing(4);
        for (idx, (key, value)) in self.bnet_env_vars.iter().enumerate() {
            let key_input = text_input("KEY", key)
                .on_input(move |v| Message::BnetEnvKeyChanged(idx, v))
                .padding(6)
                .width(Length::FillPortion(2));
            let value_input = text_input("VALUE", value)
                .on_input(move |v| Message::BnetEnvValueChanged(idx, v))
                .padding(6)
                .width(Length::FillPortion(3));
            let remove_btn = button("x")
                .padding([4, 8])
                .on_press(Message::RemoveBnetEnvVar(idx));
            bnet_env_col = bnet_env_col.push(row![key_input, value_input, remove_btn].spacing(4));
        }
        let add_bnet_env_btn = button("+ Add Variable")
            .padding([4, 12])
            .on_press(Message::AddBnetEnvVar);

        let install_bnet_btn = if self.config.bnet_installed() {
            button("Battle.net Installed").padding([8, 16])
        } else {
            button("Install Battle.net")
                .padding([8, 16])
                .on_press(Message::InstallBnet)
        };

        let content = column![
            top_bar,
            vertical_space().height(12),
            game_path_label,
            game_path_field,
            vertical_space().height(8),
            runner_label,
            runner_picker,
            vertical_space().height(8),
            on_launch_label,
            on_launch_picker,
            vertical_space().height(16),
            // WoW section
            wow_header,
            wow_args_label,
            wow_args_field,
            wow_default_args_checkbox,
            wow_env_label,
            wow_env_col,
            add_wow_env_btn,
            vertical_space().height(16),
            // Drive mappings section
            drive_header,
            drive_col,
            add_drive_btn,
            vertical_space().height(16),
            // Performance section
            perf_header,
            mangohud_checkbox,
            mangohud_config_field,
            gamescope_checkbox,
            gamescope_args_field,
            vertical_space().height(16),
            // Battle.net section
            bnet_header,
            bnet_status,
            bnet_args_label,
            bnet_args_field,
            bnet_env_label,
            bnet_env_col,
            add_bnet_env_btn,
            vertical_space().height(8),
            install_bnet_btn,
        ]
        .spacing(6)
        .padding(40)
        .width(Length::Fill);

        iced::widget::scrollable(
            container(content)
                .width(Length::Fill)
                .height(Length::Shrink),
        )
        .height(Length::Fill)
        .into()
    }
}

fn detect_runners_async() -> Vec<DetectedRunner> {
    launcher::detect::detect_runners()
}
