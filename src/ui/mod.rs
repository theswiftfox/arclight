use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

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
    config::{
        BNET_SCALE_OPTIONS, Config, GameClient, NewsLocale, OnLaunchBehavior, ProtonFlag,
        RunnerOverride, RunnerType,
    },
    launcher,
    launcher::{
        artwork,
        bnet::{self, AuthStatus},
        detect::DetectedRunner,
        flavors::{self, DetectedFlavor},
        news::{self, NewsItem},
        version::{self, ClientVersion, Latest, LatestKey},
    },
};

pub mod logs;
mod main_view;
pub mod theme;

pub use theme::wow_theme;

/// Per-client choice for a Proton toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtonChoice {
    UseGlobal,
    On,
    Off,
}

impl ProtonChoice {
    const ALL: [Self; 3] = [Self::UseGlobal, Self::On, Self::Off];
}

impl From<Option<bool>> for ProtonChoice {
    fn from(value: Option<bool>) -> Self {
        match value {
            None => Self::UseGlobal,
            Some(true) => Self::On,
            Some(false) => Self::Off,
        }
    }
}

impl From<ProtonChoice> for Option<bool> {
    fn from(value: ProtonChoice) -> Self {
        match value {
            ProtonChoice::UseGlobal => None,
            ProtonChoice::On => Some(true),
            ProtonChoice::Off => Some(false),
        }
    }
}

impl std::fmt::Display for ProtonChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UseGlobal => "Use global",
            Self::On => "On",
            Self::Off => "Off",
        })
    }
}

const SETTINGS_SCROLL_ID: &str = "settings-scroll";
const SETTINGS_CONTENT_ID: &str = "settings-content";

/// Where to scroll when Settings is opened from another screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTarget {
    Section(SettingsSection),
    /// The card of the game client at this index
    Client(usize),
}

impl SettingsTarget {
    fn id(self) -> String {
        match self {
            Self::Section(section) => section.id().to_owned(),
            Self::Client(idx) => client_card_id(idx),
        }
    }
}

fn client_card_id(idx: usize) -> String {
    format!("settings-client-{idx}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Clients,
    Drives,
    Performance,
    Battlenet,
}

impl SettingsSection {
    const ALL: [Self; 5] = [
        Self::General,
        Self::Clients,
        Self::Drives,
        Self::Performance,
        Self::Battlenet,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Clients => "Game Clients",
            Self::Drives => "Drive Mappings",
            Self::Performance => "Performance",
            Self::Battlenet => "Battle.net",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::General => "settings-section-general",
            Self::Clients => "settings-section-clients",
            Self::Drives => "settings-section-drives",
            Self::Performance => "settings-section-performance",
            Self::Battlenet => "settings-section-battlenet",
        }
    }
}

#[derive(Debug, Clone)]
struct SettingsSnapshot {
    config: Config,
    game_path_input: String,
    wow_args_input: String,
    wow_env_vars: Vec<(String, String)>,
    bnet_args_input: String,
    bnet_env_vars: Vec<(String, String)>,
    drive_mappings: Vec<(String, String)>,
    selected_runner: Option<DetectedRunner>,
    selected_bnet_runner: Option<DetectedRunner>,
}

#[derive(Debug, Clone, Copy)]
pub enum PathTarget {
    GamePath,
    ClientExecutable(usize),
    ClientPrefix(usize),
    DriveMapping(usize),
    Background,
    ClientBackground(usize),
}

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

/// A client tab being dragged to a new position.
#[derive(Debug, Clone)]
struct TabDrag {
    id: String,
    moved: bool,
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
    OpenSettings(SettingsTarget),
    JumpTo(SettingsTarget),
    GoToMain,
    // Launch
    Launch,
    GameClientSelected(GameClient),
    GameClientSelectionSaved(Result<(), String>),
    ClientTabPressed(String),
    ClientTabEntered(String),
    ClientTabExited(String),
    ClientTabReleased,
    ClientOrderSaved(Result<(), String>),
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
    BrowsePath(PathTarget),
    PathPicked(PathTarget, Option<std::path::PathBuf>),
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
    GlobalEnvKeyChanged(usize, String),
    GlobalEnvValueChanged(usize, String),
    AddGlobalEnvVar,
    RemoveGlobalEnvVar(usize),
    BnetUseGlobalEnvToggled(bool),
    GlobalProtonToggled(ProtonFlag, bool),
    GameClientProtonChanged(usize, ProtonFlag, ProtonChoice),
    RemoveGameClientEnvVar(usize, usize),
    AddGameClient,
    RemoveGameClient(usize),
    GameClientBackgroundChanged(usize, String),
    BackgroundChanged(String),
    AddDetectedFlavor(String),
    RescanFlavors,
    CheckVersionsToggled(bool),
    OfficialArtworkToggled(bool),
    NewsThumbnailsToggled(bool),
    ToggleNews,
    RetryNews,
    NewsLocaleChanged(NewsLocale),
    NewsLoaded(u64, Result<Vec<NewsItem>, String>),
    OpenUrl(String),
    BnetRunnerSelected(DetectedRunner),
    BnetRunnerReset,
    OnLaunchChanged(OnLaunchBehavior),
    BnetScaleChanged(u32),
    FocusNext,
    FocusPrevious,
    JumpToSection(SettingsSection),
    SectionScrollTarget(Option<f32>),
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
    // Versions and detected clients
    VersionsChecked(u64, Result<version::Report, String>),
    FlavorsDetected(u64, Result<Vec<DetectedFlavor>, String>),
    ArtworkLoaded(u64, Result<HashMap<String, std::path::PathBuf>, String>),
    BnetIconFound(Option<std::path::PathBuf>),
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
    settings_snapshot: Option<SettingsSnapshot>,
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
    // Versions and detected clients
    client_versions: HashMap<String, ClientVersion>,
    latest_cache: HashMap<LatestKey, (Instant, Latest)>,
    version_generation: u64,
    detected_flavors: Vec<DetectedFlavor>,
    flavor_generation: u64,
    official_art: HashMap<String, std::path::PathBuf>,
    bnet_icon: Option<std::path::PathBuf>,
    tab_drag: Option<TabDrag>,
    hovered_tab: Option<String>,
    art_generation: u64,
    art_applied: u64,
    news_open: bool,
    news: NewsState,
    news_generation: u64,
}

#[derive(Debug, Clone, Default)]
enum NewsState {
    #[default]
    Idle,
    Loading,
    Loaded(Vec<NewsItem>, Instant),
    Failed(String),
}

/// News older than this is fetched again when the overlay is opened.
const NEWS_MAX_AGE: Duration = Duration::from_mins(15);

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
            settings_snapshot: None,
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
            client_versions: HashMap::new(),
            latest_cache: HashMap::new(),
            version_generation: 0,
            detected_flavors: Vec::new(),
            flavor_generation: 0,
            official_art: HashMap::new(),
            bnet_icon: None,
            tab_drag: None,
            hovered_tab: None,
            art_generation: 0,
            art_applied: 0,
            news_open: false,
            news: NewsState::Idle,
            news_generation: 0,
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
            size: iced::Size::new(1100.0, 700.0),
            min_size: Some(iced::Size::new(900.0, 600.0)),
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

    fn check_versions_task(&mut self) -> Task<Message> {
        self.version_generation += 1;
        let generation = self.version_generation;
        let Some(root) = self.config.game_path.clone() else {
            self.client_versions.clear();
            return Task::none();
        };
        let now = Instant::now();
        let fresh: std::collections::HashMap<_, _> = self
            .latest_cache
            .iter()
            .filter(|(_, (fetched_at, _))| now.duration_since(*fetched_at) < version::CACHE_TTL)
            .map(|(key, (_, latest))| (key.clone(), latest.clone()))
            .collect();
        let clients = self.config.game_clients.clone();
        let region = self.config.region;
        let network = self.config.check_versions;
        Task::perform(
            async move {
                version::check(root, clients, region, network, fresh)
                    .await
                    .map_err(|error| format!("{error:#}"))
            },
            move |result| Message::VersionsChecked(generation, result),
        )
    }

    fn news_task(&mut self) -> Task<Message> {
        self.news_generation += 1;
        let generation = self.news_generation;
        self.news = NewsState::Loading;
        let thumbnails = self.config.news_thumbnails;
        let locale = self.config.news_locale;
        Task::perform(
            async move {
                news::fetch(locale, thumbnails)
                    .await
                    .map_err(|error| format!("{error:#}"))
            },
            move |result| Message::NewsLoaded(generation, result),
        )
    }

    fn artwork_task(&mut self) -> Task<Message> {
        let Some(root) = self.config.game_path.clone() else {
            self.official_art.clear();
            return Task::none();
        };
        if !self.config.official_artwork {
            self.official_art.clear();
            return Task::none();
        }
        let clients = self.config.game_clients.clone();
        self.art_generation += 1;
        let cached_generation = self.art_generation;
        self.art_generation += 1;
        let refreshed_generation = self.art_generation;
        let (cached_root, cached_clients) = (root.clone(), clients.clone());
        Task::perform(
            async move {
                artwork::cached(cached_root, cached_clients)
                    .await
                    .map_err(|error| format!("{error:#}"))
            },
            move |result| Message::ArtworkLoaded(cached_generation, result),
        )
        .chain(Task::perform(
            async move {
                artwork::refresh(root, clients)
                    .await
                    .map_err(|error| format!("{error:#}"))
            },
            move |result| Message::ArtworkLoaded(refreshed_generation, result),
        ))
    }

    fn rescan_flavors_task(&mut self) -> Task<Message> {
        let input = self.game_path_input.trim();
        let root = (!input.is_empty()).then(|| std::path::PathBuf::from(input));
        self.detect_flavors_task(root)
    }

    fn detect_flavors_task(&mut self, root: Option<std::path::PathBuf>) -> Task<Message> {
        self.flavor_generation += 1;
        let generation = self.flavor_generation;
        let Some(root) = root else {
            self.detected_flavors.clear();
            return Task::none();
        };
        Task::perform(
            async move {
                background::blocking(
                    "Scanning for installed clients",
                    DISCOVERY_TIMEOUT,
                    move || Ok(flavors::detect_flavors(&root)),
                )
                .await
                .map_err(|error| format!("{error:#}"))
            },
            move |result| Message::FlavorsDetected(generation, result),
        )
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
        // Only listen for the end of a drag while one is running. Leaving the window
        // also ends it, because the release may never be delivered there.
        let drag_end = if self.tab_drag.is_some() {
            iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Mouse(
                    mouse::Event::ButtonReleased(mouse::Button::Left) | mouse::Event::CursorLeft,
                ) => Some(Message::ClientTabReleased),
                _ => None,
            })
        } else {
            Subscription::none()
        };
        Subscription::batch([
            drag_end,
            iced::window::close_requests().map(Message::WindowCloseRequested),
            iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                    modifiers,
                    ..
                }) if !modifiers.control() && !modifiers.alt() => Some(if modifiers.shift() {
                    Message::FocusPrevious
                } else {
                    Message::FocusNext
                }),
                _ => None,
            }),
        ])
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
                self.settings_snapshot = Some(self.snapshot_settings());
                self.screen = Screen::Settings;
                Task::none()
            }
            Message::OpenSettings(target) if self.loaded => {
                self.settings_snapshot = Some(self.snapshot_settings());
                self.screen = Screen::Settings;
                // The target only exists once the Settings screen has been drawn.
                Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
                    },
                    move |()| Message::JumpTo(target),
                )
            }
            Message::GoToMain => {
                if !self.saving
                    && let Some(snapshot) = self.settings_snapshot.take()
                {
                    self.restore_settings(snapshot);
                }
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
            Message::ClientTabPressed(id) if self.loaded => {
                self.tab_drag = Some(TabDrag {
                    id: id.clone(),
                    moved: false,
                });
                let client = self
                    .config
                    .game_clients
                    .iter()
                    .find(|client| client.id == id && client.id != self.config.selected_game_client)
                    .cloned();
                client.map_or_else(Task::none, |client| {
                    self.update(Message::GameClientSelected(client))
                })
            }
            Message::ClientTabEntered(id) => {
                if let Some(drag) = &mut self.tab_drag
                    && move_client(&mut self.config.game_clients, &drag.id, &id)
                {
                    drag.moved = true;
                }
                self.hovered_tab = Some(id);
                Task::none()
            }
            Message::ClientTabExited(id) => {
                if self.hovered_tab.as_ref() == Some(&id) {
                    self.hovered_tab = None;
                }
                Task::none()
            }
            Message::ClientTabReleased => match self.tab_drag.take() {
                Some(drag) if drag.moved => {
                    let config = self.config.clone();
                    Task::perform(
                        async move {
                            background::mutation("Saving client order", move || config.save())
                                .await
                                .map_err(|error| error.to_string())
                        },
                        Message::ClientOrderSaved,
                    )
                }
                _ => Task::none(),
            },
            Message::ClientOrderSaved(result) => {
                if let Err(error) = result {
                    self.status = format!("Could not save client order: {error}");
                }
                Task::none()
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
                Task::batch([self.refresh_auth_task(), self.check_versions_task()])
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
                    Ok(status) => {
                        self.auth_status = status;
                        return self.bnet_icon_task();
                    }
                    Err(error) => self.status = format!("Auth check failed: {error}"),
                }
                Task::none()
            }
            Message::BnetIconFound(icon) => {
                self.bnet_icon = icon;
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
                self.settings_snapshot = None;
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
                        // `background::blocking` rejects work beyond its worker limit, so
                        // the follow-up jobs run in sequential chains.
                        let flavors = self.detect_flavors_task(self.config.game_path.clone());
                        Task::batch([
                            self.refresh_auth_task(),
                            self.check_versions_task().chain(flavors),
                            self.artwork_task(),
                        ])
                    }
                    Err(error) => {
                        self.status = format!("Runner discovery failed: {error}");
                        Task::none()
                    }
                }
            }
            Message::VersionsChecked(generation, result)
                if generation == self.version_generation =>
            {
                match result {
                    Ok(report) => {
                        let now = Instant::now();
                        self.latest_cache.extend(
                            report
                                .fetched
                                .into_iter()
                                .map(|(key, latest)| (key, (now, latest))),
                        );
                        self.client_versions = report.clients;
                    }
                    Err(error) => tracing::warn!("Version check failed: {error}"),
                }
                Task::none()
            }
            Message::FlavorsDetected(generation, result)
                if generation == self.flavor_generation =>
            {
                match result {
                    Ok(flavors) => self.detected_flavors = flavors,
                    Err(error) => tracing::warn!("Scanning for installed clients failed: {error}"),
                }
                Task::none()
            }
            Message::ArtworkLoaded(generation, result) if generation > self.art_applied => {
                match result {
                    Ok(paths) => {
                        self.art_applied = generation;
                        self.official_art = paths;
                    }
                    Err(error) => tracing::warn!("Artwork lookup failed: {error}"),
                }
                Task::none()
            }
            Message::VersionsChecked(..)
            | Message::FlavorsDetected(..)
            | Message::ArtworkLoaded(..) => Task::none(),
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

    fn snapshot_settings(&self) -> SettingsSnapshot {
        SettingsSnapshot {
            config: self.config.clone(),
            game_path_input: self.game_path_input.clone(),
            wow_args_input: self.wow_args_input.clone(),
            wow_env_vars: self.wow_env_vars.clone(),
            bnet_args_input: self.bnet_args_input.clone(),
            bnet_env_vars: self.bnet_env_vars.clone(),
            drive_mappings: self.drive_mappings.clone(),
            selected_runner: self.selected_runner.clone(),
            selected_bnet_runner: self.selected_bnet_runner.clone(),
        }
    }

    fn restore_settings(&mut self, snapshot: SettingsSnapshot) {
        self.config = snapshot.config;
        self.game_path_input = snapshot.game_path_input;
        self.wow_args_input = snapshot.wow_args_input;
        self.wow_env_vars = snapshot.wow_env_vars;
        self.bnet_args_input = snapshot.bnet_args_input;
        self.bnet_env_vars = snapshot.bnet_env_vars;
        self.drive_mappings = snapshot.drive_mappings;
        self.selected_runner = snapshot.selected_runner;
        self.selected_bnet_runner = snapshot.selected_bnet_runner;
    }

    fn handle_save_settings(&mut self) -> Task<Message> {
        if !self.loaded || self.saving {
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
        self.config
            .global_env_vars
            .retain(|(key, _)| !key.is_empty());
        self.config.ensure_selected_client();

        let config = self.config.clone();
        let apply_scale = config.bnet_installed()
            && self
                .settings_snapshot
                .as_ref()
                .is_some_and(|s| s.config.bnet_scale_percent != config.bnet_scale_percent);
        Task::perform(
            async move {
                let scale_config = config.clone();
                background::mutation("Saving settings", move || {
                    let _setup = background::prefix_setup()?;
                    config.apply_drive_mappings()?;
                    config.save()
                })
                .await
                .map_err(|e| e.to_string())?;
                if apply_scale {
                    crate::launcher::bnet::apply_bnet_scaling(&scale_config)
                        .await
                        .map_err(|e| {
                            format!("settings were saved, but applying scaling failed: {e}")
                        })?;
                }
                Ok(())
            },
            Message::SettingsSaved,
        )
    }

    fn browse_path(&self, target: PathTarget) -> Task<Message> {
        let game_root = std::path::PathBuf::from(self.game_path_input.trim());
        let is_image = matches!(
            target,
            PathTarget::Background | PathTarget::ClientBackground(_)
        );
        let (title, pick_files, current) = match target {
            PathTarget::GamePath => (
                "Select WoW install folder",
                false,
                self.game_path_input.clone(),
            ),
            PathTarget::ClientExecutable(idx) => (
                "Select game executable",
                true,
                self.config
                    .game_clients
                    .get(idx)
                    .map_or_else(String::new, |c| {
                        game_root.join(&c.executable).to_string_lossy().into_owned()
                    }),
            ),
            PathTarget::ClientPrefix(idx) => (
                "Select Wine prefix folder",
                false,
                self.config
                    .game_clients
                    .get(idx)
                    .map_or_else(String::new, |c| {
                        c.prefix_path().to_string_lossy().into_owned()
                    }),
            ),
            PathTarget::DriveMapping(idx) => (
                "Select drive folder",
                false,
                self.drive_mappings
                    .get(idx)
                    .map_or_else(String::new, |(_, p)| p.clone()),
            ),
            PathTarget::Background => (
                "Select background image",
                true,
                self.config
                    .background_image
                    .as_ref()
                    .map_or_else(String::new, |p| p.to_string_lossy().into_owned()),
            ),
            PathTarget::ClientBackground(idx) => (
                "Select background image",
                true,
                self.config
                    .game_clients
                    .get(idx)
                    .and_then(|c| c.background.as_ref())
                    .map_or_else(String::new, |p| p.to_string_lossy().into_owned()),
            ),
        };
        let mut start = std::path::PathBuf::from(current.trim());
        while !start.as_os_str().is_empty() && !start.is_dir() {
            start.pop();
        }
        if start.as_os_str().is_empty() && matches!(target, PathTarget::ClientExecutable(_)) {
            start = game_root;
        }
        Task::perform(
            async move {
                let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
                if is_image {
                    dialog = dialog.add_filter("Images", &["jpg", "jpeg", "png", "webp"]);
                }
                if start.is_dir() {
                    dialog = dialog.set_directory(start);
                }
                let handle = if pick_files {
                    dialog.pick_file().await
                } else {
                    dialog.pick_folder().await
                };
                handle.map(|h| h.path().to_path_buf())
            },
            move |path| Message::PathPicked(target, path),
        )
    }

    fn apply_picked_path(&mut self, target: PathTarget, path: &std::path::Path) {
        match target {
            PathTarget::GamePath => self.game_path_input = path.to_string_lossy().into_owned(),
            PathTarget::ClientExecutable(idx) => {
                let root = std::path::PathBuf::from(self.game_path_input.trim());
                match path.strip_prefix(&root) {
                    Ok(relative) if !relative.as_os_str().is_empty() => {
                        if let Some(client) = self.config.game_clients.get_mut(idx) {
                            client.executable = relative.to_path_buf();
                        }
                    }
                    _ => {
                        self.status =
                            "Executable must be inside the WoW install folder".to_string();
                    }
                }
            }
            PathTarget::ClientPrefix(idx) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.prefix = Some(path.to_path_buf());
                }
            }
            PathTarget::DriveMapping(idx) => {
                if let Some(entry) = self.drive_mappings.get_mut(idx) {
                    entry.1 = path.to_string_lossy().into_owned();
                }
            }
            PathTarget::Background => self.config.background_image = Some(path.to_path_buf()),
            PathTarget::ClientBackground(idx) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.background = Some(path.to_path_buf());
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_settings_input(&mut self, message: Message) -> Task<Message> {
        if !self.loaded || self.saving {
            return Task::none();
        }
        match message {
            Message::GamePathChanged(path) => self.game_path_input = path,
            Message::BrowsePath(target) => return self.browse_path(target),
            Message::PathPicked(target, Some(path)) => {
                self.apply_picked_path(target, &path);
                if matches!(target, PathTarget::GamePath) {
                    return self.rescan_flavors_task();
                }
            }
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
            Message::GameClientBackgroundChanged(idx, path) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.background = optional_path(&path);
                }
            }
            Message::BackgroundChanged(path) => self.config.background_image = optional_path(&path),
            Message::AddDetectedFlavor(dir) => {
                let flavor =
                    flavors::unconfigured(&self.detected_flavors, &self.config.game_clients)
                        .into_iter()
                        .find(|flavor| flavor.dir == dir)
                        .cloned();
                if let Some(flavor) = flavor {
                    let client = flavor.to_client(&self.config.game_clients);
                    self.config.game_clients.push(client);
                }
            }
            Message::RescanFlavors => return self.rescan_flavors_task(),
            Message::CheckVersionsToggled(enabled) => self.config.check_versions = enabled,
            Message::OfficialArtworkToggled(enabled) => self.config.official_artwork = enabled,
            Message::NewsThumbnailsToggled(enabled) => {
                self.config.news_thumbnails = enabled;
                self.news = NewsState::Idle;
                self.news_open = false;
            }
            Message::NewsLocaleChanged(locale) => {
                self.config.news_locale = locale;
                self.news = NewsState::Idle;
                self.news_open = false;
            }
            Message::RetryNews => return self.news_task(),
            Message::ToggleNews => {
                self.news_open = !self.news_open;
                let stale = match &self.news {
                    NewsState::Idle | NewsState::Failed(_) => true,
                    NewsState::Loading => false,
                    NewsState::Loaded(_, fetched_at) => fetched_at.elapsed() >= NEWS_MAX_AGE,
                };
                if self.news_open && stale {
                    return self.news_task();
                }
            }
            Message::NewsLoaded(generation, result) if generation == self.news_generation => {
                self.news = match result {
                    Ok(items) => NewsState::Loaded(items, Instant::now()),
                    Err(error) => {
                        tracing::warn!("Loading news failed: {error}");
                        NewsState::Failed(error)
                    }
                };
            }
            Message::OpenUrl(url) => {
                if !url.starts_with(news::SITE) {
                    return Task::none();
                }
                return Task::future(async move {
                    let spawned = tokio::process::Command::new("xdg-open")
                        .arg(&url)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                    match spawned {
                        Ok(mut child) => {
                            let _ = child.wait().await;
                        }
                        Err(error) => tracing::warn!("Could not open {url}: {error}"),
                    }
                })
                .discard();
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
            Message::FocusNext if matches!(self.screen, Screen::Settings) => {
                return iced::widget::operation::focus_next().chain(reveal_focused());
            }
            Message::FocusPrevious if matches!(self.screen, Screen::Settings) => {
                return iced::widget::operation::focus_previous().chain(reveal_focused());
            }
            Message::JumpToSection(section) => {
                return scroll_to_target(SettingsTarget::Section(section));
            }
            Message::JumpTo(target) if matches!(self.screen, Screen::Settings) => {
                return scroll_to_target(target);
            }
            Message::SectionScrollTarget(Some(y)) => {
                return iced::widget::operation::scroll_to(
                    SETTINGS_SCROLL_ID,
                    iced::widget::scrollable::AbsoluteOffset {
                        x: None,
                        y: Some(y.max(0.0)),
                    },
                );
            }
            Message::BnetScaleChanged(percent) => self.config.bnet_scale_percent = percent,
            Message::BnetUseGlobalEnvToggled(enabled) => {
                self.config.bnet_use_global_env = enabled;
            }
            Message::GlobalProtonToggled(flag, enabled) => {
                self.config.global_proton.set(flag, enabled);
            }
            Message::GameClientProtonChanged(idx, flag, choice) => {
                if let Some(client) = self.config.game_clients.get_mut(idx) {
                    client.proton.set(flag, choice.into());
                }
            }
            Message::GlobalEnvKeyChanged(idx, key) => {
                if let Some(entry) = self.config.global_env_vars.get_mut(idx) {
                    entry.0 = key;
                }
            }
            Message::GlobalEnvValueChanged(idx, value) => {
                if let Some(entry) = self.config.global_env_vars.get_mut(idx) {
                    entry.1 = value;
                }
            }
            Message::AddGlobalEnvVar => {
                self.config
                    .global_env_vars
                    .push((String::new(), String::new()));
            }
            Message::RemoveGlobalEnvVar(idx) => {
                if idx < self.config.global_env_vars.len() {
                    self.config.global_env_vars.remove(idx);
                }
            }
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

    /// Look up the installed Battle.net's icon (and drop it once Battle.net is gone).
    fn bnet_icon_task(&mut self) -> Task<Message> {
        if self.auth_status == AuthStatus::NotInstalled {
            self.bnet_icon = None;
            return Task::none();
        }
        let config = self.config.clone();
        Task::perform(
            async move {
                background::blocking("Finding the Battle.net icon", IO_TIMEOUT, move || {
                    Ok(bnet::find_icon(&config))
                })
                .await
                .unwrap_or_default()
            },
            Message::BnetIconFound,
        )
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
    fn view_settings(&self) -> Element<'_, Message> {
        // Top bar
        let title = text("Settings").size(28);
        let save_btn = button("Save")
            .padding([8, 20])
            .on_press_maybe((!self.saving).then_some(Message::SaveSettings));
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
            .padding(8)
            .width(Length::Fill);
        let game_path_field = row![game_path_field, browse_button(PathTarget::GamePath)]
            .spacing(4)
            .align_y(iced::Alignment::Center);

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

        let bnet_scale_label = text("Battle.net scaling (%, applied on save):").size(14);
        let bnet_scale_picker = pick_list(
            BNET_SCALE_OPTIONS.to_vec(),
            Some(self.config.bnet_scale_percent),
            Message::BnetScaleChanged,
        )
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

        let news_locale_label = text("News language and region:").size(14);
        let news_locale_picker = pick_list(
            NewsLocale::ALL,
            Some(self.config.news_locale),
            Message::NewsLocaleChanged,
        )
        .padding(8)
        .width(Length::Fill);

        let background_label = text("Default background image (optional):").size(14);
        let background_field = row![
            text_input(
                "/path/to/image.jpg",
                &path_text(self.config.background_image.as_ref())
            )
            .on_input(Message::BackgroundChanged)
            .padding(8)
            .width(Length::Fill),
            browse_button(PathTarget::Background),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center);

        let check_versions_checkbox = checkbox(self.config.check_versions)
            .label("Check for newer game versions")
            .on_toggle(Message::CheckVersionsToggled);
        let official_artwork_checkbox = checkbox(self.config.official_artwork)
            .label("Show official artwork from the WoW game pages")
            .on_toggle(Message::OfficialArtworkToggled);
        let news_thumbnails_checkbox = checkbox(self.config.news_thumbnails)
            .label("Show thumbnails in the news overlay")
            .on_toggle(Message::NewsThumbnailsToggled);

        let global_env_label = text("Global environment variables:").size(14);
        let mut global_env_col = column![].spacing(4);
        for (idx, (key, value)) in self.config.global_env_vars.iter().enumerate() {
            let key_input = text_input("KEY", key)
                .on_input(move |v| Message::GlobalEnvKeyChanged(idx, v))
                .padding(6)
                .width(Length::FillPortion(2));
            let value_input = text_input("VALUE", value)
                .on_input(move |v| Message::GlobalEnvValueChanged(idx, v))
                .padding(6)
                .width(Length::FillPortion(3));
            let remove_btn = button("x")
                .padding([4, 8])
                .on_press(Message::RemoveGlobalEnvVar(idx));
            global_env_col =
                global_env_col.push(row![key_input, value_input, remove_btn].spacing(4));
        }
        let add_global_env_btn = button("+ Add Variable")
            .padding([4, 12])
            .on_press(Message::AddGlobalEnvVar);
        let bnet_use_global_env_checkbox = checkbox(self.config.bnet_use_global_env)
            .label("Use global environment variables and Proton toggles")
            .on_toggle(Message::BnetUseGlobalEnvToggled);
        let mut global_proton_toggles = row![].spacing(24);
        for flag in ProtonFlag::ALL {
            global_proton_toggles = global_proton_toggles.push(
                checkbox(self.config.global_proton.get(flag))
                    .label(flag.label())
                    .on_toggle(move |enabled| Message::GlobalProtonToggled(flag, enabled)),
            );
        }
        let global_proton_col = column![
            text("Proton toggles (ignored when using Wine):").size(14),
            global_proton_toggles
        ]
        .spacing(6);

        let clients_header = section_header(SettingsSection::Clients);
        let mut clients_column = column![].spacing(12);
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
                .padding(8)
                .width(Length::Fill);
            let executable = row![executable, browse_button(PathTarget::ClientExecutable(idx))]
                .spacing(4)
                .align_y(iced::Alignment::Center);
            let prefix = client
                .prefix
                .as_ref()
                .map_or_else(String::new, |path| path.to_string_lossy().into_owned());
            let prefix = text_input(
                &format!("Default: {}", client.prefix_path().display()),
                &prefix,
            )
            .on_input(move |value| Message::GameClientPrefixChanged(idx, value))
            .padding(8)
            .width(Length::Fill);
            let prefix = row![prefix, browse_button(PathTarget::ClientPrefix(idx))]
                .spacing(4)
                .align_y(iced::Alignment::Center);
            let background = row![
                text_input("Default background", &path_text(client.background.as_ref()))
                    .on_input(move |value| Message::GameClientBackgroundChanged(idx, value))
                    .padding(8)
                    .width(Length::Fill),
                browse_button(PathTarget::ClientBackground(idx)),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center);

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
            let mut proton_column =
                column![text("Proton toggles (ignored when using Wine):").size(14)].spacing(4);
            for flag in ProtonFlag::ALL {
                proton_column = proton_column.push(
                    row![
                        text(flag.label()).size(12).width(Length::FillPortion(2)),
                        pick_list(
                            ProtonChoice::ALL,
                            Some(ProtonChoice::from(client.proton.get(flag))),
                            move |choice| Message::GameClientProtonChanged(idx, flag, choice),
                        )
                        .padding(6)
                        .width(Length::FillPortion(3)),
                    ]
                    .spacing(4)
                    .align_y(iced::Alignment::Center),
                );
            }
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
                container(
                    column![
                        row![name, remove].spacing(8),
                        text("Relative executable:").size(14),
                        executable,
                        text("Prefix (blank uses the client default):").size(14),
                        prefix,
                        text("Background image (blank uses the default):").size(14),
                        background,
                        text("Runner override:").size(14),
                        row![runner_picker, runner_reset].spacing(8),
                        args,
                        proton_column,
                        env_column,
                    ]
                    .spacing(6),
                )
                .id(client_card_id(idx))
                .padding(12)
                .width(Length::Fill)
                .style(container::bordered_box),
            );
        }
        let add_client = button("+ Add Client")
            .padding([6, 12])
            .on_press(Message::AddGameClient);
        let rescan_clients = button("Rescan install")
            .padding([6, 12])
            .style(theme::surface_button)
            .on_press(Message::RescanFlavors);
        let mut detected_clients = column![].spacing(6);
        let missing = flavors::unconfigured(&self.detected_flavors, &self.config.game_clients);
        if !missing.is_empty() {
            detected_clients = detected_clients.push(text("Found in the install folder:").size(14));
            for flavor in missing {
                detected_clients = detected_clients.push(
                    button(
                        text(format!(
                            "+ Add {} ({})",
                            flavor.name,
                            flavor.executable.display()
                        ))
                        .size(13),
                    )
                    .padding([4, 12])
                    .on_press(Message::AddDetectedFlavor(flavor.dir.clone())),
                );
            }
        }

        // Drive Mappings Section
        let drive_header = section_header(SettingsSection::Drives);
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
                row![
                    letter_input,
                    colon,
                    path_input,
                    browse_button(PathTarget::DriveMapping(idx)),
                    remove_btn
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            );
        }
        let add_drive_btn = button("+ Add Drive")
            .padding([4, 12])
            .on_press(Message::AddDriveMapping);

        // Performance Section
        let perf_header = section_header(SettingsSection::Performance);

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
        let bnet_header = section_header(SettingsSection::Battlenet);

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

        let nav = SettingsSection::ALL.iter().fold(
            row![].spacing(6).align_y(iced::Alignment::Center),
            |nav, &section| {
                nav.push(
                    button(text(section.title()).size(13))
                        .padding([4, 12])
                        .style(theme::surface_button)
                        .on_press(Message::JumpToSection(section)),
                )
            },
        );
        let header = container(column![top_bar, text(&self.status).size(12), nav].spacing(8))
            .padding([20, 40])
            .width(Length::Fill);

        let mut content = column![
            section_header(SettingsSection::General),
            game_path_label,
            game_path_field,
            space().height(8),
            runner_label,
            row![runner_picker, runner_reset, download_ge].spacing(8),
            space().height(8),
            on_launch_label,
            on_launch_picker,
            space().height(8),
            news_locale_label,
            news_locale_picker,
            space().height(8),
            background_label,
            background_field,
            space().height(8),
            check_versions_checkbox,
            official_artwork_checkbox,
            news_thumbnails_checkbox,
            space().height(8),
            global_proton_col,
            space().height(8),
            global_env_label,
            global_env_col,
            add_global_env_btn,
            space().height(16),
            clients_header,
            clients_column,
            space().height(4),
            row![add_client, rescan_clients].spacing(8),
            detected_clients,
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
        .padding([0, 40])
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
            .push(bnet_scale_label)
            .push(bnet_scale_picker)
            .push(bnet_args_label)
            .push(bnet_args_field)
            .push(bnet_use_global_env_checkbox)
            .push(bnet_env_label)
            .push(bnet_env_col)
            .push(add_bnet_env_btn)
            .push(space().height(8))
            .push(install_bnet_btn);

        content = content.push(space().height(40));

        column![
            header,
            container(
                iced::widget::scrollable(
                    container(content)
                        .id(SETTINGS_CONTENT_ID)
                        .width(Length::Fill)
                        .height(Length::Shrink),
                )
                .id(SETTINGS_SCROLL_ID)
                .height(Length::Fill),
            )
            .padding(iced::Padding {
                bottom: 10.0,
                ..iced::Padding::ZERO
            })
            .height(Length::Fill),
        ]
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

/// Scrolls the settings just enough to bring the focused widget into view.
fn reveal_focused() -> Task<Message> {
    use iced::widget::selector::{self, Target};
    const MARGIN: f32 = 16.0;

    selector::find(selector::is_focused())
        .then(|focused| {
            let Some(focused) = focused else {
                return Task::done(None);
            };
            selector::find(selector::id(SETTINGS_CONTENT_ID)).then(move |top| {
                let (focused, Some(top)) = (focused.clone(), top) else {
                    return Task::done(None);
                };
                selector::find(selector::id(SETTINGS_SCROLL_ID)).map(move |scroll| {
                    let Some(Target::Scrollable {
                        bounds,
                        translation,
                        ..
                    }) = scroll
                    else {
                        return None;
                    };
                    let top_y = focused.bounds().y - top.bounds().y;
                    let bottom_y = top_y + focused.bounds().height;
                    if top_y < translation.y + MARGIN {
                        Some(top_y - MARGIN)
                    } else if bottom_y > translation.y + bounds.height - MARGIN {
                        Some(bottom_y - bounds.height + MARGIN)
                    } else {
                        None
                    }
                })
            })
        })
        .map(Message::SectionScrollTarget)
}

/// Scroll the settings page so the target's top edge is at the top of the view.
fn scroll_to_target(target: SettingsTarget) -> Task<Message> {
    use iced::widget::selector;
    selector::find(selector::id(target.id()))
        .then(|target| match target {
            Some(target) => selector::find(selector::id(SETTINGS_CONTENT_ID))
                .map(move |top| top.map(|top| target.bounds().y - top.bounds().y)),
            None => Task::done(None),
        })
        .map(Message::SectionScrollTarget)
}

fn section_header<'a>(section: SettingsSection) -> Element<'a, Message> {
    let bold = Font {
        weight: iced::font::Weight::Bold,
        ..Font::DEFAULT
    };
    container(
        column![
            space().height(12),
            text(section.title()).size(24).font(bold),
            iced::widget::rule::horizontal(2),
        ]
        .spacing(6),
    )
    .id(section.id())
    .width(Length::Fill)
    .into()
}

fn optional_path(input: &str) -> Option<std::path::PathBuf> {
    let input = input.trim();
    (!input.is_empty()).then(|| std::path::PathBuf::from(input))
}

fn path_text(path: Option<&std::path::PathBuf>) -> String {
    path.map_or_else(String::new, |path| path.to_string_lossy().into_owned())
}

fn browse_button<'a>(target: PathTarget) -> Element<'a, Message> {
    button("...")
        .padding([6, 12])
        .on_press(Message::BrowsePath(target))
        .into()
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

/// Move the client `id` to the position of `target`. Returns whether anything moved.
fn move_client(clients: &mut Vec<GameClient>, id: &str, target: &str) -> bool {
    let (Some(from), Some(to)) = (
        clients.iter().position(|client| client.id == id),
        clients.iter().position(|client| client.id == target),
    ) else {
        return false;
    };
    if from == to {
        return false;
    }
    let client = clients.remove(from);
    clients.insert(to, client);
    true
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
    fn opening_settings_from_the_main_screen_switches_screens_and_keeps_the_target() {
        let (mut app, startup) = App::new();
        drop(startup);
        app.loaded = true;

        let _ = app.update(Message::OpenSettings(SettingsTarget::Client(1)));

        assert!(matches!(app.screen, Screen::Settings));
        assert!(app.settings_snapshot.is_some());
        assert_eq!(SettingsTarget::Client(1).id(), "settings-client-1");
        assert_eq!(
            SettingsTarget::Section(SettingsSection::Clients).id(),
            "settings-section-clients"
        );
    }

    fn client(id: &str) -> GameClient {
        GameClient {
            id: id.into(),
            name: id.into(),
            ..GameClient::default()
        }
    }

    fn ids(clients: &[GameClient]) -> Vec<&str> {
        clients.iter().map(|client| client.id.as_str()).collect()
    }

    #[test]
    fn moving_a_client_takes_the_position_of_the_target() {
        let mut clients = vec![client("a"), client("b"), client("c"), client("d")];

        assert!(move_client(&mut clients, "a", "c"));
        assert_eq!(ids(&clients), ["b", "c", "a", "d"]);
        assert!(move_client(&mut clients, "d", "b"));
        assert_eq!(ids(&clients), ["d", "b", "c", "a"]);
        assert!(!move_client(&mut clients, "b", "b"));
        assert!(!move_client(&mut clients, "b", "missing"));
        assert!(!move_client(&mut clients, "missing", "b"));
        assert_eq!(ids(&clients), ["d", "b", "c", "a"]);
    }

    #[test]
    fn dragging_a_tab_reorders_selects_and_ends_on_release() {
        let (mut app, startup) = App::new();
        drop(startup);
        app.loaded = true;
        app.config.game_clients = vec![client("a"), client("b"), client("c")];
        app.config.selected_game_client = "a".into();

        let _ = app.update(Message::ClientTabPressed("c".into()));
        assert_eq!(app.config.selected_game_client, "c");
        assert!(app.tab_drag.is_some());

        let _ = app.update(Message::ClientTabEntered("a".into()));
        assert_eq!(ids(&app.config.game_clients), ["c", "a", "b"]);

        let _ = app.update(Message::ClientTabReleased);
        assert!(app.tab_drag.is_none());
        let _ = app.update(Message::ClientTabEntered("b".into()));
        assert_eq!(ids(&app.config.game_clients), ["c", "a", "b"]);
    }

    #[test]
    fn blank_image_paths_clear_the_background() {
        assert_eq!(optional_path("  "), None);
        assert_eq!(
            optional_path(" /art/a.jpg "),
            Some(PathBuf::from("/art/a.jpg"))
        );
    }

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
