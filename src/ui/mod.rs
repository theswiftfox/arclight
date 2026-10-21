use std::collections::HashSet;

use iced::{
    Element, Font, Length, Subscription, Task, mouse,
    widget::{
        button, checkbox, column, container, mouse_area, pick_list, row, scrollable, space, text,
        text_input,
    },
    window,
};

use crate::{
    background::{self, DISCOVERY_TIMEOUT, IO_TIMEOUT},
    config::{Config, GameClient, OnLaunchBehavior, RunnerOverride, RunnerType},
    launcher,
    launcher::{
        bnet::{self, AuthStatus},
        detect::DetectedRunner,
    },
};

pub mod logs;
pub mod theme;

pub use theme::wow_theme;

#[must_use]
pub fn theme(_app: &App, _window: window::Id) -> iced::Theme {
    wow_theme()
}

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
pub struct RunnerDiscovery {
    runners: Vec<DetectedRunner>,
    automatic: Option<DetectedRunner>,
    selected: Option<DetectedRunner>,
    selected_bnet: Option<DetectedRunner>,
    gamemode: bool,
    available_clients: HashSet<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ConfigLoaded(Result<Config, String>),
    RetryStartup,
    // Window controls
    WindowDrag(window::Id),
    WindowResize(window::Id, window::Direction),
    WindowMinimize(window::Id),
    WindowMaximize(window::Id),
    WindowClose(window::Id),
    // Navigation
    GoToSettings,
    GoToMain,
    // Launch
    Launch,
    GameClientSelected(GameClient),
    GameClientSelectionSaved(Result<(), String>),
    LaunchResult(Result<(), String>),
    WowExited(u64, Result<(), String>),
    KillWow,
    KillWowResult(Result<(), String>),
    // Battle.net
    LaunchBnet,
    LaunchBnetResult(Result<(), String>),
    BnetExited(u64, Result<(), String>),
    KillBnet,
    KillBnetResult(Result<(), String>),
    InstallBnet,
    InstallBnetResult(u64, Result<(), String>),
    CancelInstall,
    RefreshAuthStatus,
    AuthStatusChecked(u64, Result<AuthStatus, String>),
    // Settings
    GamePathChanged(String),
    RunnerSelected(DetectedRunner),
    RunnerReset,
    GameClientNameChanged(usize, String),
    GameClientExecutableChanged(usize, String),
    GameClientPrefixChanged(usize, String),
    GameClientRunnerSelected(usize, DetectedRunner),
    GameClientRunnerReset(usize),
    GameClientArgsChanged(usize, String),
    GameClientEnvKeyChanged(usize, usize, String),
    GameClientEnvValueChanged(usize, usize, String),
    AddGameClientEnvVar(usize),
    RemoveGameClientEnvVar(usize, usize),
    AddGameClient,
    RemoveGameClient(usize),
    BnetRunnerSelected(DetectedRunner),
    BnetRunnerReset,
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
    GameModeToggled(bool),
    // Save
    SaveSettings,
    SettingsSaved(Result<(), String>),
    // Runner detection
    RunnersDetected(Result<RunnerDiscovery, String>),
    DownloadProtonGe,
    ProtonGeDownloaded(Result<std::path::PathBuf, String>),
    // Logs
    OpenLogs,
    LogsWindowOpened(window::Id),
    MainWindowOpened(window::Id),
    LogBatch(Vec<logs::LogEntry>),
    CopyLogs,
    LogsCopied(Result<(), String>),
    OlderLogs,
    NewerLogs,
    ClearLogs,
    // Window events
    WindowCloseRequested(window::Id),
}

#[allow(clippy::struct_excessive_bools)]
pub struct App {
    screen: Screen,
    config: Config,
    status: String,
    launching: bool,
    wow_running: bool,
    maximized: bool,
    auth_status: AuthStatus,
    available_clients: HashSet<String>,
    bnet_busy: bool,
    bnet_started: bool,
    detected_runners: Vec<DetectedRunner>,
    selected_runner: Option<DetectedRunner>,
    selected_bnet_runner: Option<DetectedRunner>,
    // Settings state
    game_path_input: String,
    wow_args_input: String,
    wow_env_vars: Vec<(String, String)>,
    bnet_args_input: String,
    bnet_env_vars: Vec<(String, String)>,
    drive_mappings: Vec<(String, String)>,
    // Logs
    log_buffer: logs::LogBuffer,
    log_sender: logs::LogSender,
    logs_window_id: Option<window::Id>,
    // Window IDs
    main_window_id: Option<window::Id>,
    // Tool availability
    gamemode_installed: bool,
    loaded: bool,
    loading: bool,
    saving: bool,
    copying: bool,
    killing_wow: bool,
    killing_bnet: bool,
    installing: bool,
    downloading_runner: bool,
    auth_busy: bool,
    auth_generation: u64,
    automatic_runner: Option<DetectedRunner>,
    wow_config: Option<Config>,
    wow_client: Option<GameClient>,
    bnet_config: Option<Config>,
    wow_generation: u64,
    bnet_generation: u64,
    wow_monitor: Option<iced::task::Handle>,
    bnet_monitor: Option<iced::task::Handle>,
    install_task: Option<iced::task::Handle>,
    install_generation: u64,
    log_page: usize,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let config = Config::default();
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
        let auth_status = AuthStatus::NotInstalled;

        let (log_sender, log_receiver) = logs::create_log_channel();

        let app = Self {
            screen: Screen::Main,
            config,
            status: "Loading configuration...".into(),
            launching: false,
            wow_running: false,
            maximized: false,
            auth_status,
            available_clients: HashSet::new(),
            bnet_busy: false,
            bnet_started: false,
            detected_runners: Vec::new(),
            selected_runner: None,
            selected_bnet_runner: None,
            game_path_input,
            wow_args_input,
            wow_env_vars,
            bnet_args_input,
            bnet_env_vars,
            drive_mappings,
            log_buffer: logs::LogBuffer::default(),
            log_sender,
            logs_window_id: None,
            main_window_id: None,
            gamemode_installed: false,
            loaded: false,
            loading: true,
            saving: false,
            copying: false,
            killing_wow: false,
            killing_bnet: false,
            installing: false,
            downloading_runner: false,
            auth_busy: false,
            auth_generation: 0,
            automatic_runner: None,
            wow_config: None,
            wow_client: None,
            bnet_config: None,
            wow_generation: 0,
            bnet_generation: 0,
            wow_monitor: None,
            bnet_monitor: None,
            install_task: None,
            install_generation: 0,
            log_page: 0,
        };

        // The stream owns its receiver for the app lifetime; UI callbacks never lock it.
        let log_task = Task::stream(iced::futures::stream::unfold(
            log_receiver,
            |mut receiver| async move {
                let entries = receiver.next_batch().await?;
                Some((Message::LogBatch(entries), receiver))
            },
        ));
        let (id, open) = iced::window::open(iced::window::Settings {
            size: iced::Size::new(800.0, 650.0),
            min_size: Some(iced::Size::new(700.0, 600.0)),
            resizable: true,
            decorations: false,
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "arclight".to_string(),
                ..Default::default()
            },
            ..Default::default()
        });
        let open_main = open.map(move |_| Message::MainWindowOpened(id));
        let task = Task::batch([Self::load_config_task(), log_task, open_main]);
        (app, task)
    }

    fn load_config_task() -> Task<Message> {
        Task::perform(
            async {
                background::blocking("Loading configuration", IO_TIMEOUT, Config::load)
                    .await
                    .map_err(|e| e.to_string())
            },
            Message::ConfigLoaded,
        )
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
        iced::window::close_requests().map(Message::WindowCloseRequested)
    }

    #[allow(clippy::too_many_lines)]
    pub fn update(&mut self, message: Message) -> Task<Message> {
        let _timing = CallbackTiming::new("update");
        match message {
            Message::RetryStartup if !self.loading && !self.loaded => {
                self.loading = true;
                self.status = "Loading configuration...".into();
                Self::load_config_task()
            }
            Message::ConfigLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(config) => {
                        self.game_path_input = config
                            .game_path
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.wow_args_input = config.wow_args.join(" ");
                        self.wow_env_vars.clone_from(&config.wow_env_vars);
                        self.bnet_args_input = config.bnet_args.join(" ");
                        self.bnet_env_vars.clone_from(&config.bnet_env_vars);
                        self.drive_mappings.clone_from(&config.drive_mappings);
                        self.config = config.clone();
                        self.status = "Detecting runners...".into();
                        self.loading = true;
                        Task::perform(
                            async move {
                                background::blocking(
                                    "Runner discovery",
                                    DISCOVERY_TIMEOUT,
                                    move || Ok(discover_runners(&config)),
                                )
                                .await
                                .map_err(|e| e.to_string())
                            },
                            Message::RunnersDetected,
                        )
                    }
                    Err(error) => {
                        self.status = format!("Could not load configuration: {error}");
                        Task::none()
                    }
                }
            }
            Message::WindowDrag(id) => iced::window::drag(id),
            Message::WindowResize(id, direction) => iced::window::drag_resize(id, direction),
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
            Message::GoToSettings if self.loaded => {
                self.screen = Screen::Settings;
                Task::none()
            }
            Message::GoToMain => {
                self.screen = Screen::Main;
                Task::none()
            }
            Message::Launch => self.handle_launch(),
            Message::GameClientSelected(client) => {
                self.config.selected_game_client = client.id;
                let config = self.config.clone();
                Task::perform(
                    async move {
                        background::mutation("Saving client selection", move || config.save())
                            .await
                            .map_err(|error| error.to_string())
                    },
                    Message::GameClientSelectionSaved,
                )
            }
            Message::GameClientSelectionSaved(result) => {
                if let Err(error) = result {
                    self.status = format!("Could not save client selection: {error}");
                }
                Task::none()
            }
            Message::LaunchResult(result) => {
                self.launching = false;
                match result {
                    Ok(()) => {
                        let Some(config) = self.wow_config.clone() else {
                            self.status = "Launch configuration is unavailable".into();
                            return Task::none();
                        };
                        self.wow_running = true;
                        self.status = "Game launched!".into();
                        let on_launch = config.on_launch;
                        let generation = self.wow_generation;
                        let (monitor, handle) = Task::perform(
                            async move {
                                let client = config
                                    .selected_client()
                                    .ok_or_else(|| "No game client selected".to_string())?;
                                launcher::wait_for_wow_exit(&config, client)
                                    .await
                                    .map_err(|e| e.to_string())
                            },
                            move |result| Message::WowExited(generation, result),
                        )
                        .abortable();
                        self.wow_monitor = Some(handle);
                        let window_task = match on_launch {
                            OnLaunchBehavior::Close => return iced::exit(),
                            OnLaunchBehavior::Minimize => self
                                .main_window_id
                                .map_or_else(Task::none, |id| window::minimize(id, true)),
                            OnLaunchBehavior::DoNothing => Task::none(),
                        };
                        return Task::batch([monitor, window_task]);
                    }
                    Err(e) => self.status = format!("Launch failed: {e}"),
                }
                Task::none()
            }
            Message::WowExited(generation, result) if generation == self.wow_generation => {
                self.wow_monitor = None;
                match result {
                    Ok(()) => {
                        self.wow_running = false;
                        self.status = "WoW closed.".into();
                    }
                    Err(e) => self.status = format!("Lost track of WoW: {e}"),
                }
                // Restore the launcher window now that we know the game has actually exited
                // (rather than just having been launched).
                let restore_task = if self
                    .wow_config
                    .as_ref()
                    .is_some_and(|c| c.on_launch == OnLaunchBehavior::Minimize)
                {
                    self.main_window_id
                        .map_or_else(Task::none, |id| iced::window::minimize(id, false))
                } else {
                    Task::none()
                };
                Task::batch([restore_task, self.refresh_auth_task()])
            }
            Message::KillWow if self.wow_running && !self.killing_wow => {
                let (Some(config), Some(client)) =
                    (self.wow_config.clone(), self.wow_client.clone())
                else {
                    self.status = "Running game configuration is unavailable".into();
                    return Task::none();
                };
                self.killing_wow = true;
                self.status = "Killing WoW...".into();
                Task::perform(
                    async move {
                        launcher::kill_wow(&config, &client)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::KillWowResult,
                )
            }
            Message::KillWowResult(result) => {
                self.killing_wow = false;
                if let Err(error) = result {
                    self.status = format!("Could not kill WoW: {error}");
                    return Task::none();
                }
                self.wow_generation += 1;
                if let Some(handle) = self.wow_monitor.take() {
                    handle.abort();
                }
                self.wow_running = false;
                self.status = "WoW killed.".into();
                self.refresh_auth_task()
            }
            Message::LaunchBnet => self.handle_launch_bnet(),
            Message::LaunchBnetResult(result) => {
                match result {
                    Ok(()) => {
                        let Some(config) = self.bnet_config.clone() else {
                            self.bnet_busy = false;
                            self.status = "Battle.net launch configuration is unavailable".into();
                            return Task::none();
                        };
                        self.bnet_started = true;
                        self.status = "Battle.net running... Close it when done.".into();
                        let generation = self.bnet_generation;
                        let (task, handle) = Task::perform(
                            async move {
                                bnet::wait_for_bnet_exit(&config)
                                    .await
                                    .map_err(|e| e.to_string())
                            },
                            move |result| Message::BnetExited(generation, result),
                        )
                        .abortable();
                        self.bnet_monitor = Some(handle);
                        return task;
                    }
                    Err(e) => {
                        self.bnet_busy = false;
                        self.status = format!("Battle.net failed: {e}");
                    }
                }
                self.refresh_auth_task()
            }
            Message::BnetExited(generation, result) if generation == self.bnet_generation => {
                self.bnet_monitor = None;
                self.status = match result {
                    Ok(()) => {
                        self.bnet_busy = false;
                        self.bnet_started = false;
                        "Battle.net closed.".into()
                    }
                    Err(error) => format!("Lost track of Battle.net: {error}"),
                };
                self.refresh_auth_task()
            }
            Message::KillBnet if self.bnet_started && !self.killing_bnet => {
                let Some(config) = self.bnet_config.clone() else {
                    self.status = "Running Battle.net configuration is unavailable".into();
                    return Task::none();
                };
                self.killing_bnet = true;
                Task::perform(
                    async move { bnet::kill_bnet(&config).await.map_err(|e| e.to_string()) },
                    Message::KillBnetResult,
                )
            }
            Message::KillBnetResult(result) => {
                self.killing_bnet = false;
                if let Err(error) = result {
                    self.status = format!("Could not kill Battle.net: {error}");
                    return Task::none();
                }
                self.bnet_generation += 1;
                if let Some(handle) = self.bnet_monitor.take() {
                    handle.abort();
                }
                self.bnet_busy = false;
                self.bnet_started = false;
                self.status = "Battle.net killed.".into();
                self.refresh_auth_task()
            }
            Message::InstallBnet
                if self.loaded
                    && !self.bnet_busy
                    && !self.saving
                    && !self.launching
                    && !self.killing_bnet =>
            {
                self.bnet_busy = true;
                self.installing = true;
                self.status = "Installing Battle.net...".into();
                let config = self.config.clone();
                self.install_generation += 1;
                let generation = self.install_generation;
                let (task, handle) = Task::perform(
                    async move {
                        bnet::install_bnet(&config, None)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    move |result| Message::InstallBnetResult(generation, result),
                )
                .abortable();
                self.install_task = Some(handle);
                task
            }
            Message::CancelInstall if self.installing => {
                if let Some(handle) = self.install_task.take() {
                    handle.abort();
                }
                self.install_generation += 1;
                self.installing = false;
                self.bnet_busy = false;
                self.status = "Installation cancelled.".into();
                self.refresh_auth_task()
            }
            Message::InstallBnetResult(generation, result)
                if generation == self.install_generation =>
            {
                self.install_task = None;
                self.bnet_busy = false;
                self.installing = false;
                match result {
                    Ok(()) => self.status = "Battle.net installed!".into(),
                    Err(e) => self.status = format!("Install failed: {e}"),
                }
                self.refresh_auth_task()
            }
            Message::RefreshAuthStatus => self.refresh_auth_task(),
            Message::AuthStatusChecked(generation, result)
                if generation == self.auth_generation =>
            {
                self.auth_busy = false;
                match result {
                    Ok(status) => self.auth_status = status,
                    Err(error) => self.status = format!("Auth check failed: {error}"),
                }
                Task::none()
            }
            Message::SaveSettings => self.handle_save_settings(),
            Message::SettingsSaved(result) => {
                self.saving = false;
                match result {
                    Ok(()) => self.status = "Settings saved".into(),
                    Err(e) => {
                        self.status = format!("Failed to save: {e}");
                        return Task::none();
                    }
                }
                self.screen = Screen::Main;
                // Supersede any auth read for the previous settings.
                self.auth_busy = false;
                let config = self.config.clone();
                Task::batch([
                    self.refresh_auth_task(),
                    Task::perform(
                        async move {
                            background::blocking(
                                "Refreshing game clients",
                                DISCOVERY_TIMEOUT,
                                move || Ok(discover_runners(&config)),
                            )
                            .await
                            .map_err(|error| error.to_string())
                        },
                        Message::RunnersDetected,
                    ),
                ])
            }
            Message::DownloadProtonGe if !self.downloading_runner => {
                self.downloading_runner = true;
                self.status = "Downloading Proton-GE (this can take a few minutes)...".into();
                Task::perform(
                    async {
                        launcher::proton_ge::install_latest()
                            .await
                            .map_err(|error| format!("{error:#}"))
                    },
                    Message::ProtonGeDownloaded,
                )
            }
            Message::ProtonGeDownloaded(result) => {
                self.downloading_runner = false;
                match result {
                    Ok(path) => {
                        tracing::info!("Installed Proton-GE at {}", path.display());
                        self.status = "Proton-GE installed".into();
                        let config = self.config.clone();
                        Task::perform(
                            async move {
                                background::blocking(
                                    "Runner discovery",
                                    DISCOVERY_TIMEOUT,
                                    move || Ok(discover_runners(&config)),
                                )
                                .await
                                .map_err(|error| error.to_string())
                            },
                            Message::RunnersDetected,
                        )
                    }
                    Err(error) => {
                        self.status = format!("Proton-GE download failed: {error}");
                        Task::none()
                    }
                }
            }
            Message::RunnersDetected(result) => {
                self.loading = false;
                match result {
                    Ok(discovery) => {
                        self.selected_runner = discovery.selected;
                        self.selected_bnet_runner = discovery.selected_bnet;
                        self.automatic_runner = discovery.automatic;
                        self.detected_runners = discovery.runners;
                        self.gamemode_installed = discovery.gamemode;
                        self.available_clients = discovery.available_clients;
                        self.loaded = true;
                        self.status = "Ready".into();
                        self.refresh_auth_task()
                    }
                    Err(error) => {
                        self.status = format!("Runner discovery failed: {error}");
                        Task::none()
                    }
                }
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
                    min_size: Some(iced::Size::new(500.0, 300.0)),
                    resizable: true,
                    decorations: false,
                    ..Default::default()
                });
                self.logs_window_id = Some(id);
                task.discard()
            }
            Message::LogBatch(entries) => {
                for entry in entries {
                    self.log_buffer.push(entry);
                }
                Task::none()
            }
            Message::CopyLogs if !self.copying => {
                self.copying = true;
                let buffer = self.log_buffer.clone();
                Task::perform(
                    async move {
                        background::blocking("Copying logs", IO_TIMEOUT, move || {
                            let text = buffer.to_text();
                            arboard::Clipboard::new()?.set_text(text)?;
                            Ok(())
                        })
                        .await
                        .map_err(|e| e.to_string())
                    },
                    Message::LogsCopied,
                )
            }
            Message::LogsCopied(result) => {
                self.copying = false;
                self.status = match result {
                    Ok(()) => "Logs copied".into(),
                    Err(error) => format!("Copy failed: {error}"),
                };
                Task::none()
            }
            Message::OlderLogs => {
                self.log_page = (self.log_page + 1)
                    .min(self.log_buffer.lines.len().saturating_sub(1) / logs::PAGE_SIZE);
                Task::none()
            }
            Message::NewerLogs => {
                self.log_page = self.log_page.saturating_sub(1);
                Task::none()
            }
            Message::ClearLogs => {
                self.log_buffer.clear();
                self.log_page = 0;
                Task::none()
            }
            _ => self.handle_settings_input(message),
        }
    }

    fn handle_launch(&mut self) -> Task<Message> {
        if !self.loaded
            || self.launching
            || self.wow_running
            || self.killing_wow
            || self.saving
            || self.installing
        {
            return Task::none();
        }
        self.launching = true;
        let config = self.config.clone();
        let Some(client) = config.selected_client().cloned() else {
            self.launching = false;
            self.status = "No game client selected".into();
            return Task::none();
        };
        self.status = format!("Launching {}...", client.name);
        self.wow_config = Some(config.clone());
        self.wow_client = Some(client.clone());
        self.wow_generation += 1;
        let on_launch = self.config.on_launch;

        // Only pipe logs if we're not closing on launch
        let log_sender = if on_launch == OnLaunchBehavior::Close {
            None
        } else {
            Some(self.log_sender.clone())
        };

        let use_launcherlogin = false;
        let has_gamemode = self.gamemode_installed;

        Task::perform(
            async move {
                launcher::launch(
                    &config,
                    &client,
                    use_launcherlogin,
                    has_gamemode,
                    log_sender,
                )
                .await
                .map_err(|e| e.to_string())
            },
            Message::LaunchResult,
        )
    }

    fn handle_launch_bnet(&mut self) -> Task<Message> {
        if !self.loaded || self.bnet_busy || self.killing_bnet || self.saving {
            return Task::none();
        }
        self.bnet_busy = true;
        self.status = "Launching Battle.net...".into();
        let config = self.config.clone();
        self.bnet_config = Some(config.clone());
        self.bnet_generation += 1;
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
        if !self.loaded || self.saving || self.launching || self.bnet_busy {
            return Task::none();
        }
        self.saving = true;
        self.status = "Saving settings...".into();
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

        if let Err(error) = validate_game_clients(&self.config.game_clients) {
            self.saving = false;
            self.status = error;
            return Task::none();
        }
        for client in &mut self.config.game_clients {
            client.env_vars.retain(|(key, _)| !key.is_empty());
        }
        self.config.ensure_selected_client();

        let config = self.config.clone();
        Task::perform(
            async move {
                background::mutation("Saving settings", move || {
                    let _setup = background::prefix_setup()?;
                    config.apply_drive_mappings()?;
                    config.save()
                })
                .await
                .map_err(|e| e.to_string())
            },
            Message::SettingsSaved,
        )
    }

    #[allow(clippy::too_many_lines)]
    fn handle_settings_input(&mut self, message: Message) -> Task<Message> {
        if !self.loaded || self.saving {
            return Task::none();
        }
        match message {
            Message::GamePathChanged(path) => self.game_path_input = path,
            Message::RunnerSelected(runner) => {
                self.config.runner.binary = Some(runner.path.clone());
                self.config.runner.runner_type = runner.runner_type;
                self.selected_runner = Some(runner);
            }
            Message::RunnerReset => {
                self.config.runner.binary = None;
                self.selected_runner = None;
            }
            Message::GameClientNameChanged(idx, name) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.name = name;
                }
            }
            Message::GameClientExecutableChanged(idx, executable) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.executable = executable.into();
                }
            }
            Message::GameClientPrefixChanged(idx, prefix) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.prefix = (!prefix.is_empty()).then(|| prefix.into());
                }
            }
            Message::GameClientRunnerSelected(idx, runner) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.runner = Some(RunnerOverride {
                        binary: runner.path,
                        runner_type: runner.runner_type,
                    });
                }
            }
            Message::GameClientRunnerReset(idx) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.runner = None;
                }
            }
            Message::GameClientArgsChanged(idx, args) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.args = args
                        .split_whitespace()
                        .map(std::string::ToString::to_string)
                        .collect();
                }
            }
            Message::GameClientEnvKeyChanged(idx, env_idx, key) => {
                if let Some(entry) = self
                    .config
                    .game_clients
                    .get_mut(idx)
                    .and_then(|client| client.env_vars.get_mut(env_idx))
                {
                    entry.0 = key;
                }
            }
            Message::GameClientEnvValueChanged(idx, env_idx, value) => {
                if let Some(entry) = self
                    .config
                    .game_clients
                    .get_mut(idx)
                    .and_then(|client| client.env_vars.get_mut(env_idx))
                {
                    entry.1 = value;
                }
            }
            Message::AddGameClientEnvVar(idx) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.env_vars.push((String::new(), String::new()));
                }
            }
            Message::RemoveGameClientEnvVar(idx, env_idx) => {
                if let Some(client) = self.config.game_clients.get_mut(idx)
                    && env_idx < client.env_vars.len()
                {
                    client.env_vars.remove(env_idx);
                }
            }
            Message::AddGameClient => {
                let id = next_client_id(&self.config.game_clients);
                let client = GameClient {
                    id,
                    name: "New Client".into(),
                    ..GameClient::default()
                };
                self.config.game_clients.push(client);
            }
            Message::RemoveGameClient(idx) => {
                if idx < self.config.game_clients.len() {
                    self.config.game_clients.remove(idx);
                    self.config.ensure_selected_client();
                }
            }
            Message::BnetRunnerSelected(runner) => {
                self.config.runner.bnet_runner = Some(RunnerOverride {
                    binary: runner.path.clone(),
                    runner_type: runner.runner_type,
                });
                self.selected_bnet_runner = Some(runner);
            }
            Message::BnetRunnerReset => {
                self.config.runner.bnet_runner = None;
                self.selected_bnet_runner = None;
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
            Message::GameModeToggled(val) => self.config.gamemode_enabled = val,
            _ => {}
        }
        Task::none()
    }

    fn refresh_auth_task(&mut self) -> Task<Message> {
        if self.auth_busy || !self.loaded {
            return Task::none();
        }
        self.auth_busy = true;
        self.auth_generation += 1;
        let generation = self.auth_generation;
        let config = self.config.clone();
        Task::perform(
            async move {
                background::blocking("Checking authentication", IO_TIMEOUT, move || {
                    Ok(bnet::check_auth_status(&config))
                })
                .await
                .map_err(|e| e.to_string())
            },
            move |result| Message::AuthStatusChecked(generation, result),
        )
    }

    #[must_use]
    pub fn view(&self, window: window::Id) -> Element<'_, Message> {
        let _timing = CallbackTiming::new("view");
        let content = if Some(window) == self.logs_window_id {
            self.view_logs(window)
        } else if !self.loaded {
            column![
                self.view_titlebar(window),
                container(
                    column![
                        text(&self.status),
                        button("Retry")
                            .on_press_maybe((!self.loading).then_some(Message::RetryStartup)),
                    ]
                    .spacing(12)
                )
                .padding(40)
            ]
            .into()
        } else {
            let body = match self.screen {
                Screen::Main => self.view_main(),
                Screen::Settings => self.view_settings(),
            };
            column![self.view_titlebar(window), body].into()
        };

        resize_frame(window, content)
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

        let bar = row![title, space().width(Length::Fill), controls]
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
            .on_press_maybe((!self.copying).then_some(Message::CopyLogs));
        let clear_btn = button("Clear")
            .padding([6, 16])
            .on_press(Message::ClearLogs);

        let top_bar = row![
            text("Subprocess Logs").size(16),
            space().width(Length::Fill),
            copy_btn,
            clear_btn,
        ]
        .spacing(8)
        .padding([8, 16])
        .align_y(iced::Alignment::Center);

        let mut log_col = column![].spacing(2).padding([4, 16]);
        let page = self
            .log_page
            .min(self.log_buffer.lines.len().saturating_sub(1) / logs::PAGE_SIZE);
        let end = self
            .log_buffer
            .lines
            .len()
            .saturating_sub(page * logs::PAGE_SIZE);
        let start = end.saturating_sub(logs::PAGE_SIZE);
        for entry in self.log_buffer.lines.iter().skip(start).take(end - start) {
            log_col = log_col.push(text(entry.formatted()).size(12).font(Font::MONOSPACE));
        }

        let log_scroll = scrollable(log_col).height(Length::Fill).width(Length::Fill);

        let paging = row![
            button("Older").on_press_maybe((start > 0).then_some(Message::OlderLogs)),
            text(format!(
                "Lines {}–{} of {}",
                if end == 0 { 0 } else { start + 1 },
                end,
                self.log_buffer.lines.len()
            )),
            button("Newer").on_press_maybe((page > 0).then_some(Message::NewerLogs)),
        ]
        .spacing(12)
        .padding(8);
        column![
            self.view_titlebar(window),
            top_bar,
            paging,
            log_scroll,
            text(&self.status).size(12)
        ]
        .into()
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

            if self.installing {
                btn_row = btn_row.push(
                    button("Cancel installation")
                        .padding([6, 16])
                        .on_press(Message::CancelInstall),
                );
            } else if self.bnet_busy {
                btn_row = btn_row.push(button("Battle.net running...").padding([6, 16]));
                btn_row = btn_row.push(button("Force Quit").padding([6, 16]).on_press_maybe(
                    (self.bnet_started && !self.killing_bnet).then_some(Message::KillBnet),
                ));
            } else if self.auth_status == AuthStatus::NotInstalled {
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
                .on_press_maybe((!self.auth_busy).then_some(Message::RefreshAuthStatus));

            row![
                indicator,
                status_text,
                space().width(Length::Fill),
                btn_row,
                refresh_btn
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
        };

        let status = text(&self.status).size(14);

        let selected_client = self.config.selected_client();
        let can_play = selected_client
            .is_some_and(|client| self.available_clients.contains(&client.id))
            && !self.launching
            && !self.wow_running
            && !self.killing_wow
            && !self.saving
            && !self.installing;

        let client_picker = pick_list(
            self.config.game_clients.clone(),
            selected_client.cloned(),
            Message::GameClientSelected,
        )
        .placeholder("Select a game client")
        .padding(8)
        .width(Length::Fixed(240.0));

        let launch_btn = if self.wow_running {
            row![
                button(
                    text(format!(
                        "{} is running",
                        self.wow_client
                            .as_ref()
                            .map_or("WoW", |client| &client.name)
                    ))
                    .size(16)
                )
                .padding([12, 32]),
                button("Force Quit")
                    .padding([12, 24])
                    .on_press_maybe((!self.killing_wow).then_some(Message::KillWow)),
            ]
            .spacing(8)
        } else {
            row![if can_play {
                button(text("Play").size(24))
                    .padding([12, 48])
                    .on_press(Message::Launch)
            } else {
                button(text("Play").size(24)).padding([12, 48])
            }]
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

        let runner_info = text(effective_runner_label(
            selected_client,
            self.selected_runner.as_ref(),
            self.automatic_runner.as_ref(),
            &self.detected_runners,
        ))
        .size(12);

        let game_path_info =
            if let (Some(path), Some(client)) = (&self.config.game_path, selected_client) {
                let executable = path.join(&client.executable);
                let availability = if self.available_clients.contains(&client.id) {
                    ""
                } else {
                    " (not found)"
                };
                text(format!("Game: {}{}", executable.display(), availability)).size(12)
            } else {
                text("Game path not set — configure in Settings".to_string()).size(12)
            };

        let content = column![
            title,
            subtitle,
            space().height(20),
            auth_row,
            space().height(20),
            client_picker,
            launch_btn,
            space().height(12),
            status,
            space().height(Length::Fill),
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
        let save_btn = button("Save").padding([8, 20]).on_press_maybe(
            (!self.saving && !self.launching && !self.bnet_busy).then_some(Message::SaveSettings),
        );
        let back_btn = button("Cancel")
            .padding([8, 20])
            .on_press(Message::GoToMain);
        let top_bar = row![title, space().width(Length::Fill), back_btn, save_btn]
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
        let runner_reset = if self.selected_runner.is_some() {
            button("Auto-detect")
                .padding([6, 12])
                .on_press(Message::RunnerReset)
        } else {
            button("Using auto-detect").padding([6, 12])
        };
        let download_ge = if self.downloading_runner {
            button("Downloading Proton-GE...").padding([6, 12])
        } else {
            button("Download Proton-GE")
                .padding([6, 12])
                .on_press(Message::DownloadProtonGe)
        };

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

        let clients_header = text("Game Clients").size(18);
        let mut clients_column = column![].spacing(16);
        for (idx, client) in self.config.game_clients.iter().enumerate() {
            let name = text_input("Client name", &client.name)
                .on_input(move |value| Message::GameClientNameChanged(idx, value))
                .padding(8)
                .width(Length::FillPortion(2));
            let remove = button("Remove")
                .padding([6, 12])
                .on_press(Message::RemoveGameClient(idx));
            let executable = client.executable.to_string_lossy();
            let executable = text_input("_retail_/Wow.exe", &executable)
                .on_input(move |value| Message::GameClientExecutableChanged(idx, value))
                .padding(8);
            let prefix = client
                .prefix
                .as_ref()
                .map_or_else(String::new, |path| path.to_string_lossy().into_owned());
            let prefix = text_input(
                &format!("Default: {}", client.prefix_path().display()),
                &prefix,
            )
            .on_input(move |value| Message::GameClientPrefixChanged(idx, value))
            .padding(8);
            let selected_override = client.runner.as_ref().and_then(|configured| {
                self.detected_runners.iter().find(|runner| {
                    same_path(&runner.path, &configured.binary)
                        && runner.runner_type == configured.runner_type
                })
            });
            let runner_picker = pick_list(
                self.detected_runners.clone(),
                selected_override.cloned(),
                move |runner| Message::GameClientRunnerSelected(idx, runner),
            )
            .placeholder("Use global runner")
            .padding(8)
            .width(Length::Fill);
            let runner_reset = button("Use global").padding([6, 12]).on_press_maybe(
                client
                    .runner
                    .is_some()
                    .then_some(Message::GameClientRunnerReset(idx)),
            );
            let args = client.args.join(" ");
            let args = text_input("Additional launch arguments", &args)
                .on_input(move |value| Message::GameClientArgsChanged(idx, value))
                .padding(8);
            let mut env_column = column![text("Environment Variables:").size(14)].spacing(4);
            for (env_idx, (key, value)) in client.env_vars.iter().enumerate() {
                let key_input = text_input("KEY", key)
                    .on_input(move |value| Message::GameClientEnvKeyChanged(idx, env_idx, value))
                    .padding(6)
                    .width(Length::FillPortion(2));
                let value_input = text_input("VALUE", value)
                    .on_input(move |value| Message::GameClientEnvValueChanged(idx, env_idx, value))
                    .padding(6)
                    .width(Length::FillPortion(3));
                let remove = button("x")
                    .padding([4, 8])
                    .on_press(Message::RemoveGameClientEnvVar(idx, env_idx));
                env_column = env_column.push(row![key_input, value_input, remove].spacing(4));
            }
            env_column = env_column.push(
                button("+ Add Variable")
                    .padding([4, 12])
                    .on_press(Message::AddGameClientEnvVar(idx)),
            );
            clients_column = clients_column.push(
                column![
                    row![name, remove].spacing(8),
                    text("Relative executable:").size(14),
                    executable,
                    text("Prefix (blank uses the client default):").size(14),
                    prefix,
                    text("Runner override:").size(14),
                    row![runner_picker, runner_reset].spacing(8),
                    args,
                    env_column,
                ]
                .spacing(6),
            );
        }
        let add_client = button("+ Add Client")
            .padding([6, 12])
            .on_press(Message::AddGameClient);

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

        let mangohud_checkbox = checkbox(self.config.mangohud_enabled)
            .label("Enable MangoHud")
            .on_toggle(Message::MangoHudToggled);
        let mangohud_config_field =
            text_input("e.g. fps,frametime,cpu_temp", &self.config.mangohud_config)
                .on_input(Message::MangoHudConfigChanged)
                .padding(8);

        let gamescope_checkbox = checkbox(self.config.gamescope_enabled)
            .label("Enable Gamescope")
            .on_toggle(Message::GamescopeToggled);
        let gamescope_args_field =
            text_input("e.g. -w 2560 -h 1440 -f", &self.config.gamescope_args)
                .on_input(Message::GamescopeArgsChanged)
                .padding(8);

        let gamemode_checkbox = checkbox(self.config.gamemode_enabled)
            .label("Enable GameMode (performance + inhibit sleep)")
            .on_toggle(Message::GameModeToggled);
        let gamemode_warning: Option<Element<'_, Message>> = if self.gamemode_installed {
            None
        } else {
            Some(
                text("⚠ gamemoderun not found — install gamemode to use this feature")
                    .size(12)
                    .color(iced::Color::from_rgb(1.0, 0.8, 0.2))
                    .into(),
            )
        };

        // Battle.net Section
        let bnet_header = text("Battle.net Settings").size(18);

        let bnet_status = if self.auth_status == AuthStatus::NotInstalled {
            text("Status: Not installed").size(12)
        } else {
            text("Status: Installed ✓").size(12)
        };

        let bnet_runner_label = text("Runner:").size(14);
        let bnet_runner_picker = pick_list(
            self.detected_runners.clone(),
            self.selected_bnet_runner.clone(),
            Message::BnetRunnerSelected,
        )
        .placeholder("Use global runner")
        .padding(8)
        .width(Length::Fill);
        let bnet_runner_reset = if self.selected_bnet_runner.is_some() {
            button("Use global runner")
                .padding([6, 12])
                .on_press(Message::BnetRunnerReset)
        } else {
            button("Using global runner").padding([6, 12])
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

        let install_bnet_btn = if self.installing {
            button("Cancel installation")
                .padding([8, 16])
                .on_press(Message::CancelInstall)
        } else if self.auth_status != AuthStatus::NotInstalled {
            button("Battle.net Installed").padding([8, 16])
        } else {
            button("Install Battle.net")
                .padding([8, 16])
                .on_press(Message::InstallBnet)
        };

        let mut content = column![
            top_bar,
            text(&self.status).size(12),
            space().height(12),
            game_path_label,
            game_path_field,
            space().height(8),
            runner_label,
            row![runner_picker, runner_reset, download_ge].spacing(8),
            space().height(8),
            on_launch_label,
            on_launch_picker,
            space().height(16),
            clients_header,
            clients_column,
            add_client,
            space().height(16),
            // Drive mappings section
            drive_header,
            drive_col,
            add_drive_btn,
            space().height(16),
            // Performance section
            perf_header,
            mangohud_checkbox,
            mangohud_config_field,
            gamescope_checkbox,
            gamescope_args_field,
            gamemode_checkbox,
        ]
        .spacing(6)
        .padding(40)
        .width(Length::Fill);

        if let Some(warning) = gamemode_warning {
            content = content.push(warning);
        }

        content = content
            .push(space().height(16))
            // Battle.net section
            .push(bnet_header)
            .push(bnet_status)
            .push(bnet_runner_label)
            .push(bnet_runner_picker)
            .push(bnet_runner_reset)
            .push(bnet_args_label)
            .push(bnet_args_field)
            .push(bnet_env_label)
            .push(bnet_env_col)
            .push(add_bnet_env_btn)
            .push(space().height(8))
            .push(install_bnet_btn);

        iced::widget::scrollable(
            container(content)
                .width(Length::Fill)
                .height(Length::Shrink),
        )
        .height(Length::Fill)
        .into()
    }
}

/// Diagnostics only: no synchronous output, and no logging on the normal path.
struct CallbackTiming {
    name: &'static str,
    started: std::time::Instant,
}

impl CallbackTiming {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            started: std::time::Instant::now(),
        }
    }
}

impl Drop for CallbackTiming {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed();
        if elapsed > std::time::Duration::from_millis(16) {
            tracing::warn!(
                callback = self.name,
                elapsed_ms = elapsed.as_millis(),
                "Slow UI callback"
            );
        }
    }
}

const RESIZE_BORDER: f32 = 6.0;

fn resize_handle(
    window: window::Id,
    direction: window::Direction,
    width: Length,
    height: Length,
    interaction: mouse::Interaction,
) -> Element<'static, Message> {
    mouse_area(container(space()).width(width).height(height))
        .on_press(Message::WindowResize(window, direction))
        .interaction(interaction)
        .into()
}

fn resize_frame(window: window::Id, content: Element<'_, Message>) -> Element<'_, Message> {
    use window::Direction;

    let north = resize_handle(
        window,
        Direction::North,
        Length::Fill,
        Length::Fixed(RESIZE_BORDER),
        mouse::Interaction::ResizingVertically,
    );
    let south = resize_handle(
        window,
        Direction::South,
        Length::Fill,
        Length::Fixed(RESIZE_BORDER),
        mouse::Interaction::ResizingVertically,
    );
    let west = resize_handle(
        window,
        Direction::West,
        Length::Fixed(RESIZE_BORDER),
        Length::Fill,
        mouse::Interaction::ResizingHorizontally,
    );
    let east = resize_handle(
        window,
        Direction::East,
        Length::Fixed(RESIZE_BORDER),
        Length::Fill,
        mouse::Interaction::ResizingHorizontally,
    );
    let north_west = resize_handle(
        window,
        Direction::NorthWest,
        Length::Fixed(RESIZE_BORDER),
        Length::Fixed(RESIZE_BORDER),
        mouse::Interaction::ResizingDiagonallyUp,
    );
    let north_east = resize_handle(
        window,
        Direction::NorthEast,
        Length::Fixed(RESIZE_BORDER),
        Length::Fixed(RESIZE_BORDER),
        mouse::Interaction::ResizingDiagonallyDown,
    );
    let south_west = resize_handle(
        window,
        Direction::SouthWest,
        Length::Fixed(RESIZE_BORDER),
        Length::Fixed(RESIZE_BORDER),
        mouse::Interaction::ResizingDiagonallyDown,
    );
    let south_east = resize_handle(
        window,
        Direction::SouthEast,
        Length::Fixed(RESIZE_BORDER),
        Length::Fixed(RESIZE_BORDER),
        mouse::Interaction::ResizingDiagonallyUp,
    );

    column![
        row![north_west, north, north_east].height(RESIZE_BORDER),
        row![west, content, east].height(Length::Fill),
        row![south_west, south, south_east].height(RESIZE_BORDER),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn configured_runner(
    runners: &mut Vec<DetectedRunner>,
    binary: &std::path::Path,
    runner_type: RunnerType,
) -> DetectedRunner {
    if let Some(runner) = runners
        .iter()
        .find(|runner| same_path(&runner.path, binary))
    {
        return runner.clone();
    }

    let runner = DetectedRunner {
        name: format!("Configured (not found): {}", binary.display()),
        path: binary.to_path_buf(),
        runner_type,
    };
    runners.push(runner.clone());
    runner
}

fn same_path(first: &std::path::Path, second: &std::path::Path) -> bool {
    first.canonicalize().unwrap_or_else(|_| first.to_path_buf())
        == second
            .canonicalize()
            .unwrap_or_else(|_| second.to_path_buf())
}

fn effective_runner_label(
    client: Option<&GameClient>,
    global: Option<&DetectedRunner>,
    automatic: Option<&DetectedRunner>,
    detected: &[DetectedRunner],
) -> String {
    if let Some(override_runner) = client.and_then(|client| client.runner.as_ref()) {
        let name = detected
            .iter()
            .find(|runner| {
                runner.runner_type == override_runner.runner_type
                    && same_path(&runner.path, &override_runner.binary)
            })
            .map_or_else(
                || format!("Configured: {}", override_runner.binary.display()),
                |runner| runner.name.clone(),
            );
        return format!("Runner: {name} (client override)");
    }

    if let Some(runner) = global {
        format!("Runner: {} (global)", runner.name)
    } else if let Some(runner) = automatic {
        format!("Runner: {} (auto)", runner.name)
    } else {
        "No Proton runner — download Proton-GE in Settings".into()
    }
}

fn next_client_id(clients: &[GameClient]) -> String {
    let mut suffix = clients.len() + 1;
    loop {
        let candidate = format!("client-{suffix}");
        if clients.iter().all(|client| client.id != candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn validate_game_clients(clients: &[GameClient]) -> Result<(), String> {
    if clients.is_empty() {
        return Err("Add at least one game client".into());
    }
    let mut names = HashSet::new();
    for client in clients {
        let name = client.name.trim();
        if name.is_empty() {
            return Err("Game client names cannot be empty".into());
        }
        if !names.insert(name.to_lowercase()) {
            return Err(format!("Game client name '{name}' is duplicated"));
        }
        if client.executable.as_os_str().is_empty() || client.executable.is_absolute() {
            return Err(format!(
                "{} needs an executable path relative to the WoW install root",
                client.name
            ));
        }
        if client.executable.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        }) {
            return Err(format!(
                "{} executable must stay inside the WoW install root",
                client.name
            ));
        }
    }
    Ok(())
}

fn discover_runners(config: &Config) -> RunnerDiscovery {
    let mut runners = launcher::detect::detect_runners();
    let automatic = launcher::detect::automatic_runner(&runners).cloned();
    let selected = config
        .runner
        .binary
        .as_ref()
        .map(|binary| configured_runner(&mut runners, binary, config.runner.runner_type));
    let selected_bnet = config
        .runner
        .bnet_runner
        .as_ref()
        .map(|runner| configured_runner(&mut runners, &runner.binary, runner.runner_type));
    for client in &config.game_clients {
        if let Some(override_runner) = &client.runner {
            configured_runner(
                &mut runners,
                &override_runner.binary,
                override_runner.runner_type,
            );
        }
    }
    let available_clients = config.game_path.as_ref().map_or_else(HashSet::new, |root| {
        config
            .game_clients
            .iter()
            .filter(|client| launcher::resolve_game_executable(root, client).is_ok())
            .map(|client| client.id.clone())
            .collect()
    });
    RunnerDiscovery {
        runners,
        automatic,
        selected,
        selected_bnet,
        gamemode: which::which("gamemoderun").is_ok(),
        available_clients,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn startup_and_pending_operations_do_not_block_navigation() {
        // No runtime or filesystem access is needed to construct/update/view.
        let (mut app, startup) = App::new();
        drop(startup);
        assert!(!app.loaded);
        let _ = app.view(window::Id::unique());
        app.loaded = true;
        app.saving = true;
        app.launching = true;
        let _ = app.update(Message::GoToSettings);
        assert!(matches!(app.screen, Screen::Settings));
        let _ = app.update(Message::GoToMain);
        assert!(matches!(app.screen, Screen::Main));
        let _ = app.view(window::Id::unique());
        let _ = app.update(Message::SettingsSaved(Err("timed out".into())));
        assert!(!app.saving);
        let _ = app.update(Message::LaunchResult(Err("timed out".into())));
        assert!(!app.launching);
    }

    #[test]
    fn stale_results_and_failed_kills_do_not_reset_live_state() {
        let (mut app, startup) = App::new();
        drop(startup);
        app.loaded = true;
        app.auth_generation = 2;
        app.auth_busy = true;
        let _ = app.update(Message::AuthStatusChecked(1, Ok(AuthStatus::BnetLoggedIn)));
        assert!(app.auth_busy);
        assert_eq!(app.auth_status, AuthStatus::NotInstalled);
        app.wow_running = true;
        app.wow_generation = 2;
        let _ = app.update(Message::WowExited(1, Ok(())));
        assert!(app.wow_running);
        app.killing_wow = true;
        let _ = app.update(Message::KillWowResult(Err("timed out".into())));
        assert!(app.wow_running);
        assert!(!app.killing_wow);
        let _ = app.update(Message::WowExited(2, Err("monitor failed".into())));
        assert!(app.wow_running); // Force Quit remains available.
        app.bnet_busy = true;
        app.bnet_started = true;
        app.killing_bnet = true;
        let _ = app.update(Message::KillBnetResult(Err("failed".into())));
        assert!(app.bnet_busy && app.bnet_started);
        assert!(!app.killing_bnet);
    }

    #[test]
    fn duplicate_requests_and_cancelled_install_results_are_ignored() {
        let (mut app, startup) = App::new();
        drop(startup);
        app.loaded = true;
        let first = app.update(Message::Launch);
        let generation = app.wow_generation;
        let duplicate = app.update(Message::Launch);
        assert_eq!(app.wow_generation, generation);
        drop((first, duplicate));
        app.installing = true;
        app.bnet_busy = true;
        app.install_generation = 4;
        let _ = app.update(Message::CancelInstall);
        let _ = app.update(Message::InstallBnetResult(4, Ok(())));
        assert!(!app.installing && !app.bnet_busy);
        assert_eq!(app.status, "Installation cancelled.");
    }

    fn runner(name: &str, path: &str) -> DetectedRunner {
        DetectedRunner {
            name: name.to_string(),
            path: PathBuf::from(path),
            runner_type: RunnerType::Proton,
        }
    }

    #[test]
    fn configured_runner_uses_matching_detected_runner() {
        let mut runners = vec![runner("Steam Proton 10.0", "/opt/proton/proton")];

        let selected = configured_runner(
            &mut runners,
            &PathBuf::from("/opt/proton/proton"),
            RunnerType::Proton,
        );

        assert_eq!(selected.name, "Steam Proton 10.0");
        assert_eq!(runners.len(), 1);
    }

    #[test]
    fn configured_runner_is_retained_when_not_detected() {
        let mut runners = vec![runner("Steam Proton 10.0", "/opt/proton-10/proton")];

        let selected = configured_runner(
            &mut runners,
            &PathBuf::from("/opt/proton-9/proton"),
            RunnerType::Proton,
        );

        assert_eq!(
            selected.name,
            "Configured (not found): /opt/proton-9/proton"
        );
        assert_eq!(selected.path, PathBuf::from("/opt/proton-9/proton"));
        assert_eq!(runners.len(), 2);
    }

    #[test]
    fn runner_label_prefers_selected_client_override() {
        let global = runner("Global Proton", "/opt/global/proton");
        let client_runner = runner("Client Proton", "/opt/client/proton");
        let mut client = GameClient::retail();
        client.runner = Some(RunnerOverride {
            binary: client_runner.path.clone(),
            runner_type: client_runner.runner_type,
        });
        let detected = [global.clone(), client_runner];

        let label = effective_runner_label(Some(&client), Some(&global), None, &detected);

        assert_eq!(label, "Runner: Client Proton (client override)");
    }

    #[test]
    fn runner_label_uses_global_then_automatic_fallbacks() {
        let global = runner("Global Proton", "/opt/global/proton");
        let automatic = runner("Automatic Proton", "/opt/auto/proton");

        assert_eq!(
            effective_runner_label(
                Some(&GameClient::retail()),
                Some(&global),
                Some(&automatic),
                &[]
            ),
            "Runner: Global Proton (global)"
        );
        assert_eq!(
            effective_runner_label(Some(&GameClient::retail()), None, Some(&automatic), &[]),
            "Runner: Automatic Proton (auto)"
        );
    }
}
