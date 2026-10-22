use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod paths;
pub use paths::AppPaths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Config {
    /// Path to the `WoW` installation directory
    pub game_path: Option<PathBuf>,
    /// Configured game clients below the shared installation directory
    pub game_clients: Vec<GameClient>,
    /// ID of the game client selected on the main screen
    pub selected_game_client: String,
    /// Wine/Proton runner configuration
    pub runner: RunnerConfig,
    /// Game region
    pub region: Region,
    /// What to do with the launcher window when game starts
    pub on_launch: OnLaunchBehavior,
    /// Auto-check for updates on launch
    pub auto_update: bool,

    // WoW launch settings
    /// Additional launch arguments for `WoW`
    #[serde(skip_serializing)]
    pub wow_args: Vec<String>,
    /// Environment variables for `WoW`
    #[serde(skip_serializing)]
    pub wow_env_vars: Vec<(String, String)>,
    /// Skip default args (-launcherlogin -uid wow)
    #[serde(skip_serializing)]
    pub wow_skip_default_args: bool,

    // Battle.net settings
    /// Launch arguments for Battle.net
    pub bnet_args: Vec<String>,
    /// Environment variables for Battle.net
    pub bnet_env_vars: Vec<(String, String)>,
    /// Battle.net UI scaling in percent (100 = no scaling), applied as Wine DPI
    pub bnet_scale_percent: u32,

    // Prefix settings
    /// Drive letter mappings (letter, linux path) e.g. ("d", "/media/ssd")
    pub drive_mappings: Vec<(String, String)>,

    // Performance wrappers
    /// Enable `MangoHud` overlay for the game
    #[serde(default)]
    pub mangohud_enabled: bool,
    /// `MangoHud` config string (e.g. `fps,frametime,cpu_temp`)
    #[serde(default)]
    pub mangohud_config: String,
    /// Enable Gamescope compositor for the game
    #[serde(default)]
    pub gamescope_enabled: bool,
    /// Gamescope arguments (e.g. "-w 2560 -h 1440 -f")
    #[serde(default)]
    pub gamescope_args: String,
    /// Enable `GameMode` (performance optimizations + inhibit sleep)
    #[serde(default)]
    pub gamemode_enabled: bool,

    // Legacy field (migrated to wow_env_vars on load)
    #[serde(default)]
    pub env_vars: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RunnerConfig {
    /// Path to wine/proton binary (None = auto-detect)
    pub binary: Option<PathBuf>,
    /// Optional Wine/Proton runner override for Battle.net (None = use the `WoW` runner)
    pub bnet_runner: Option<RunnerOverride>,
    /// Wine prefix path for Battle.net (None = use default)
    pub prefix: Option<PathBuf>,
    /// Wine prefix path for `WoW` (None = use default, separate from Battle.net prefix)
    #[serde(skip_serializing)]
    pub wow_prefix: Option<PathBuf>,
    /// Runner type
    pub runner_type: RunnerType,
    /// Enable DXVK/VKD3D
    pub dxvk: bool,
    /// Enable esync
    pub esync: bool,
    /// Enable fsync
    pub fsync: bool,
}

/// A specific Wine/Proton runner selection for one application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerOverride {
    /// Path to the Wine or Proton executable
    pub binary: PathBuf,
    /// Type of executable at `binary`
    pub runner_type: RunnerType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameClient {
    /// Stable identifier used for selection and the default prefix path
    pub id: String,
    /// User-facing client name
    pub name: String,
    /// Executable path relative to the shared `game_path`
    pub executable: PathBuf,
    /// Optional runner override; `None` uses the global runner
    pub runner: Option<RunnerOverride>,
    /// Wine/Proton prefix; `None` uses Arclight's client-specific default
    pub prefix: Option<PathBuf>,
    /// Additional launch arguments
    pub args: Vec<String>,
    /// Environment variables applied when launching this client
    pub env_vars: Vec<(String, String)>,
    /// Skip Battle.net launcher-login arguments when they are available
    pub skip_default_args: bool,
}

impl GameClient {
    #[must_use]
    pub fn retail() -> Self {
        Self {
            id: "retail".into(),
            name: "Retail".into(),
            executable: PathBuf::from("_retail_/Wow.exe"),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn classic() -> Self {
        Self {
            id: "classic".into(),
            name: "Classic".into(),
            executable: PathBuf::from("_classic_/WowClassic.exe"),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn prefix_path(&self) -> PathBuf {
        self.prefix
            .clone()
            .unwrap_or_else(|| AppPaths::default_game_prefix(&self.id))
    }
}

impl Default for GameClient {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            executable: PathBuf::new(),
            runner: None,
            prefix: None,
            args: Vec::new(),
            env_vars: Vec::new(),
            skip_default_args: false,
        }
    }
}

impl std::fmt::Display for GameClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunnerType {
    Wine,
    Proton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
    US,
    EU,
    KR,
    TW,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnLaunchBehavior {
    DoNothing,
    Minimize,
    Close,
}

impl std::fmt::Display for OnLaunchBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OnLaunchBehavior::DoNothing => write!(f, "Do nothing"),
            OnLaunchBehavior::Minimize => write!(f, "Minimize"),
            OnLaunchBehavior::Close => write!(f, "Close"),
        }
    }
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Region::US => write!(f, "US"),
            Region::EU => write!(f, "EU"),
            Region::KR => write!(f, "KR"),
            Region::TW => write!(f, "TW"),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            game_path: None,
            game_clients: vec![GameClient::retail(), GameClient::classic()],
            selected_game_client: "retail".into(),
            runner: RunnerConfig::default(),
            region: Region::EU,
            on_launch: OnLaunchBehavior::Minimize,
            auto_update: true,
            wow_args: Vec::new(),
            wow_env_vars: Vec::new(),
            wow_skip_default_args: false,
            bnet_args: vec!["--in-process-gpu".to_string()],
            bnet_env_vars: Vec::new(),
            bnet_scale_percent: 100,
            drive_mappings: Vec::new(),
            mangohud_enabled: false,
            mangohud_config: String::new(),
            gamescope_enabled: false,
            gamescope_args: String::new(),
            gamemode_enabled: false,
            env_vars: Vec::new(),
        }
    }
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            binary: None,
            bnet_runner: None,
            prefix: None,
            wow_prefix: None,
            runner_type: RunnerType::Wine,
            dxvk: true,
            esync: true,
            fsync: true,
        }
    }
}

/// Scaling choices offered for the Battle.net client.
pub const BNET_SCALE_OPTIONS: [u32; 5] = [100, 125, 150, 175, 200];

impl Config {
    /// Wine `LogPixels` value for the configured Battle.net scale (96 DPI = 100%).
    #[must_use]
    pub fn bnet_dpi(&self) -> u32 {
        (96 * self.bnet_scale_percent.clamp(100, 400)).div_ceil(100)
    }

    /// Load config from disk, or return default if not found.
    /// Migrates legacy `env_vars` to `wow_env_vars`.
    ///
    /// # Errors
    /// Returns an error if the config file exists but cannot be read or parsed.
    pub fn load() -> anyhow::Result<Self> {
        let path = AppPaths::config_file();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Self::from_toml(&content)
        } else {
            Ok(Self::default())
        }
    }

    fn from_toml(content: &str) -> anyhow::Result<Self> {
        let has_game_clients = toml::from_str::<toml::Value>(content)?
            .get("game_clients")
            .is_some();
        let mut config: Self = toml::from_str(content)?;
        if !config.env_vars.is_empty() && config.wow_env_vars.is_empty() {
            config.wow_env_vars = std::mem::take(&mut config.env_vars);
        }
        config.env_vars.clear();
        if !has_game_clients {
            let mut retail = GameClient::retail();
            retail.runner = config.runner.binary.take().map(|binary| RunnerOverride {
                binary,
                runner_type: config.runner.runner_type,
            });
            retail.prefix.clone_from(&config.runner.wow_prefix);
            retail.args = std::mem::take(&mut config.wow_args);
            retail.env_vars = std::mem::take(&mut config.wow_env_vars);
            retail.skip_default_args = config.wow_skip_default_args;
            config.game_clients = vec![retail, GameClient::classic()];
            config.selected_game_client = "retail".into();
        }
        config.ensure_selected_client();
        Ok(config)
    }

    /// Save config to disk.
    ///
    /// # Errors
    /// Returns an error if the config directory cannot be created or the file cannot be written.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = AppPaths::config_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Get the effective Wine prefix path (used for Battle.net).
    /// For Proton, the actual prefix is under `pfx/` within the compat data path.
    pub fn prefix_path(&self) -> PathBuf {
        self.runner
            .prefix
            .clone()
            .unwrap_or_else(AppPaths::default_prefix)
    }

    /// Get the effective Wine prefix path for `WoW` (separate from Battle.net).
    pub fn wow_prefix_path(&self) -> PathBuf {
        self.runner
            .wow_prefix
            .clone()
            .unwrap_or_else(AppPaths::default_wow_prefix)
    }

    #[must_use]
    pub fn selected_client(&self) -> Option<&GameClient> {
        self.game_clients
            .iter()
            .find(|client| client.id == self.selected_game_client)
    }

    pub fn ensure_selected_client(&mut self) {
        if self.selected_client().is_none() {
            self.selected_game_client = self
                .game_clients
                .first()
                .map_or_else(String::new, |client| client.id.clone());
        }
    }

    #[must_use]
    pub fn game_wine_prefix_root(&self, client: &GameClient, runner_type: RunnerType) -> PathBuf {
        Self::resolve_wine_root_for_type(&client.prefix_path(), runner_type)
    }

    /// Get the actual Wine prefix root (where `drive_c` lives).
    /// Proton uses `<path>/pfx/drive_c`, Wine uses `<path>/drive_c` directly.
    #[must_use]
    pub fn wine_prefix_root(&self) -> PathBuf {
        Self::resolve_wine_root(&self.prefix_path(), &self.runner)
    }

    /// Get the actual Wine prefix root for the `WoW` prefix.
    #[must_use]
    pub fn wow_wine_prefix_root(&self) -> PathBuf {
        Self::resolve_wine_root(&self.wow_prefix_path(), &self.runner)
    }

    /// Resolve the Wine prefix root for a given base path.
    fn resolve_wine_root(base: &Path, runner: &RunnerConfig) -> PathBuf {
        Self::resolve_wine_root_for_type(base, runner.runner_type)
    }

    fn resolve_wine_root_for_type(base: &Path, runner_type: RunnerType) -> PathBuf {
        let pfx = base.join("pfx");
        if pfx.join("drive_c").exists() {
            pfx
        } else if base.join("drive_c").exists() {
            base.to_path_buf()
        } else {
            match runner_type {
                RunnerType::Proton => pfx,
                RunnerType::Wine => base.to_path_buf(),
            }
        }
    }

    /// Get the Battle.net executable path within the prefix
    #[must_use]
    pub fn bnet_exe_path(&self) -> PathBuf {
        self.wine_prefix_root()
            .join("drive_c/Program Files (x86)/Battle.net/Battle.net Launcher.exe")
    }

    /// Check if Battle.net is installed in the prefix
    #[must_use]
    pub fn bnet_installed(&self) -> bool {
        self.bnet_exe_path().exists()
    }

    /// Apply drive letter mappings to both prefix dosdevices directories.
    ///
    /// # Errors
    /// Returns an error if the dosdevices directory cannot be created or symlinks cannot be set.
    pub fn apply_drive_mappings(&self) -> anyhow::Result<()> {
        let mut prefixes = vec![self.wine_prefix_root()];
        prefixes.extend(self.game_clients.iter().map(|client| {
            let runner_type = client
                .runner
                .as_ref()
                .map_or(self.runner.runner_type, |runner| runner.runner_type);
            self.game_wine_prefix_root(client, runner_type)
        }));
        for prefix_root in &prefixes {
            let dosdevices = prefix_root.join("dosdevices");
            if !dosdevices.exists() {
                std::fs::create_dir_all(&dosdevices)?;
            }

            for (letter, path) in &self.drive_mappings {
                if letter.is_empty() || path.is_empty() {
                    continue;
                }
                let link_name = dosdevices.join(format!("{}:", letter.to_lowercase()));
                let target = std::path::Path::new(path);

                // Remove existing symlink if it exists
                if link_name.exists() || link_name.is_symlink() {
                    std::fs::remove_file(&link_name)?;
                }

                std::os::unix::fs::symlink(target, &link_name)?;
                tracing::info!("Drive mapping: {}: -> {}", letter, path);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bnet_dpi_follows_scale() {
        let mut config = Config::default();
        assert_eq!(config.bnet_dpi(), 96);
        config.bnet_scale_percent = 150;
        assert_eq!(config.bnet_dpi(), 144);
        config.bnet_scale_percent = 200;
        assert_eq!(config.bnet_dpi(), 192);
    }

    #[test]
    fn defaults_include_retail_and_classic() {
        let config = Config::default();

        assert_eq!(config.selected_game_client, "retail");
        assert_eq!(config.game_clients[0], GameClient::retail());
        assert_eq!(config.game_clients[1], GameClient::classic());
    }

    #[test]
    fn legacy_game_settings_migrate_to_retail() {
        let config = Config::from_toml(
            r#"
game_path = "/games/World of Warcraft"
wow_args = ["-windowed"]
wow_env_vars = [["FOO", "bar"]]
wow_skip_default_args = true

[runner]
binary = "/opt/proton/proton"
wow_prefix = "/prefixes/wow"
runner_type = "Proton"
"#,
        )
        .unwrap();
        let retail = &config.game_clients[0];

        assert_eq!(retail.name, "Retail");
        assert_eq!(retail.args, ["-windowed"]);
        assert_eq!(retail.env_vars, [("FOO".into(), "bar".into())]);
        assert!(retail.skip_default_args);
        assert_eq!(retail.prefix.as_deref(), Some(Path::new("/prefixes/wow")));
        assert_eq!(
            retail.runner.as_ref().map(|runner| runner.binary.as_path()),
            Some(Path::new("/opt/proton/proton"))
        );
        assert!(config.runner.binary.is_none());
    }
}
