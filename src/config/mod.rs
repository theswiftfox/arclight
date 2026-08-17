use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod paths;
pub use paths::AppPaths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Config {
    /// Path to the `WoW` installation directory
    pub game_path: Option<PathBuf>,
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
    pub wow_args: Vec<String>,
    /// Environment variables for `WoW`
    pub wow_env_vars: Vec<(String, String)>,
    /// Skip default args (-launcherlogin -uid wow)
    pub wow_skip_default_args: bool,

    // Battle.net settings
    /// Launch arguments for Battle.net
    pub bnet_args: Vec<String>,
    /// Environment variables for Battle.net
    pub bnet_env_vars: Vec<(String, String)>,

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

    // Legacy field (migrated to wow_env_vars on load)
    #[serde(default)]
    pub env_vars: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RunnerConfig {
    /// Path to wine/proton binary (None = auto-detect)
    pub binary: Option<PathBuf>,
    /// Wine prefix path (None = use default)
    pub prefix: Option<PathBuf>,
    /// Runner type
    pub runner_type: RunnerType,
    /// Enable DXVK/VKD3D
    pub dxvk: bool,
    /// Enable esync
    pub esync: bool,
    /// Enable fsync
    pub fsync: bool,
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
            runner: RunnerConfig::default(),
            region: Region::EU,
            on_launch: OnLaunchBehavior::Minimize,
            auto_update: true,
            wow_args: Vec::new(),
            wow_env_vars: Vec::new(),
            wow_skip_default_args: false,
            bnet_args: vec!["--in-process-gpu".to_string()],
            bnet_env_vars: Vec::new(),
            drive_mappings: Vec::new(),
            mangohud_enabled: false,
            mangohud_config: String::new(),
            gamescope_enabled: false,
            gamescope_args: String::new(),
            env_vars: Vec::new(),
        }
    }
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            binary: None,
            prefix: None,
            runner_type: RunnerType::Wine,
            dxvk: true,
            esync: true,
            fsync: true,
        }
    }
}

impl Config {
    /// Load config from disk, or return default if not found.
    /// Migrates legacy `env_vars` to `wow_env_vars`.
    ///
    /// # Errors
    /// Returns an error if the config file exists but cannot be read or parsed.
    pub fn load() -> anyhow::Result<Self> {
        let path = AppPaths::config_file();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let mut config: Self = toml::from_str(&content)?;
            // Migrate legacy env_vars
            if !config.env_vars.is_empty() && config.wow_env_vars.is_empty() {
                config.wow_env_vars = std::mem::take(&mut config.env_vars);
            }
            config.env_vars.clear();
            Ok(config)
        } else {
            Ok(Self::default())
        }
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

    /// Get the effective Wine prefix path.
    /// For Proton, the actual prefix is under `pfx/` within the compat data path.
    pub fn prefix_path(&self) -> PathBuf {
        self.runner
            .prefix
            .clone()
            .unwrap_or_else(AppPaths::default_prefix)
    }

    /// Get the actual Wine prefix root (where `drive_c` lives).
    /// Proton uses `<path>/pfx/drive_c`, Wine uses `<path>/drive_c` directly.
    #[must_use]
    pub fn wine_prefix_root(&self) -> PathBuf {
        let base = self.prefix_path();
        let pfx = base.join("pfx");
        if pfx.join("drive_c").exists() {
            pfx
        } else if base.join("drive_c").exists() {
            base
        } else {
            // Default: check runner type
            match self.runner.runner_type {
                RunnerType::Proton => pfx,
                RunnerType::Wine => base,
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

    /// Apply drive letter mappings to the prefix dosdevices directory.
    ///
    /// # Errors
    /// Returns an error if the dosdevices directory cannot be created or symlinks cannot be set.
    pub fn apply_drive_mappings(&self) -> anyhow::Result<()> {
        let dosdevices = self.wine_prefix_root().join("dosdevices");
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

        Ok(())
    }
}
