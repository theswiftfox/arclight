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

    /// Default wine prefix location
    #[must_use]
    pub fn default_prefix() -> PathBuf {
        Self::data_dir().join("prefix")
    }

    /// Log directory
    #[must_use]
    pub fn log_dir() -> PathBuf {
        Self::data_dir().join("logs")
    }
}
