pub mod bnet;
pub mod detect;
pub mod process;

use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::{
    config::{Config, RunnerConfig, RunnerType},
    ui::logs::LogEntry,
};

/// A detected Wine/Proton runner
#[derive(Debug, Clone)]
pub struct Runner {
    pub name: String,
    pub path: PathBuf,
    pub runner_type: RunnerType,
}

/// Get the effective runner based on config (auto-detect if not specified).
///
/// # Errors
/// Returns an error if no runner is configured and auto-detection fails.
pub fn get_runner(config: &RunnerConfig) -> anyhow::Result<Runner> {
    if let Some(ref binary) = config.binary {
        Ok(Runner {
            name: binary
                .file_name()
                .map_or_else(|| "custom".into(), |n| n.to_string_lossy().to_string()),
            path: binary.clone(),
            runner_type: config.runner_type,
        })
    } else {
        // Auto-detect
        detect::find_best_runner()
    }
}

/// Launch `WoW` with the given config.
/// `use_launcherlogin`: whether to pass -launcherlogin -uid wow (requires ACCOUNT in registry)
/// `log_sender`: if provided, subprocess stdout/stderr will be piped and sent through this channel.
///
/// # Errors
/// Returns an error if the game path is not configured, the executable is not found, or launch fails.
pub async fn launch(
    config: &Config,
    use_launcherlogin: bool,
    gamemode_available: bool,
    log_sender: Option<mpsc::UnboundedSender<LogEntry>>,
) -> anyhow::Result<()> {
    let game_path = config
        .game_path
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Game path not configured"))?;

    // Try multiple possible exe locations
    let candidates = [
        game_path.join("_retail_/Wow.exe"),
        game_path.join("Wow.exe"),
        game_path.join("_retail_/WoW.exe"),
        game_path.join("WoW.exe"),
    ];

    let wow_exe = candidates
        .iter()
        .find(|p| p.exists())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Wow.exe not found. Tried:\n{}",
                candidates
                    .iter()
                    .map(|p| format!("  {}", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })?
        .clone();

    let runner = get_runner(&config.runner)?;
    tracing::info!("Using runner: {} ({})", runner.name, runner.path.display());

    process::launch_game(
        &runner,
        &wow_exe,
        config,
        use_launcherlogin,
        gamemode_available,
        log_sender,
    )
    .await
}
