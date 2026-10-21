use std::path::PathBuf;

pub struct AppPaths;

impl AppPaths {
    /// Config directory: ~/.config/arclight/
    #[must_use]
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("arclight")
    }

    /// Config file: ~/.config/arclight/config.toml
    #[must_use]
    pub fn config_file() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Data directory: ~/.local/share/arclight/
    #[must_use]
    pub fn data_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("arclight")
    }

    /// Runners downloaded by Arclight (e.g. Proton-GE)
    #[must_use]
    pub fn runners_dir() -> PathBuf {
        Self::data_dir().join("runners")
    }

    /// Default wine prefix location (used for Battle.net)
    #[must_use]
    pub fn default_prefix() -> PathBuf {
        Self::data_dir().join("prefix")
    }

    /// Default `WoW` wine prefix location
    #[must_use]
    pub fn default_wow_prefix() -> PathBuf {
        Self::data_dir().join("wow_prefix")
    }

    /// Default Wine prefix location for a configured game client.
    #[must_use]
    pub fn default_game_prefix(client_id: &str) -> PathBuf {
        if client_id == "retail" {
            Self::default_wow_prefix()
        } else {
            Self::data_dir().join(format!("wow_prefix_{client_id}"))
        }
    }

    /// Log directory
    #[must_use]
    pub fn log_dir() -> PathBuf {
        Self::data_dir().join("logs")
    }
}
