use std::{path::Path, process::Stdio};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc,
};

use super::Runner;
use crate::{
    config::{Config, RunnerType},
    ui::logs::LogEntry,
};

/// Launch the game using the specified runner.
///
/// # Errors
/// Returns an error if the prefix cannot be created or the game process fails to launch.
pub async fn launch_game(
    runner: &Runner,
    wow_exe: &Path,
    config: &Config,
    use_launcherlogin: bool,
    gamemode_available: bool,
    log_sender: Option<mpsc::UnboundedSender<LogEntry>>,
) -> anyhow::Result<()> {
    let prefix = config.wow_prefix_path();

    // Ensure prefix exists
    if !prefix.exists() {
        tracing::info!("Creating Wine prefix at {}", prefix.display());
        create_prefix(runner, &prefix).await?;
    }

    let mut cmd = match runner.runner_type {
        RunnerType::Wine => {
            let mut cmd = Command::new(&runner.path);
            cmd.arg(wow_exe);
            cmd
        }
        RunnerType::Proton => {
            let mut cmd = Command::new(&runner.path);
            cmd.arg("run").arg(wow_exe);
            cmd
        }
    };

    // Default args: -launcherlogin -uid wow (only if ACCOUNT blob exists in registry)
    if use_launcherlogin && !config.wow_skip_default_args {
        cmd.arg("-launcherlogin");
        cmd.arg("-uid");
        cmd.arg("wow");
    }

    // Custom WoW args
    for arg in &config.wow_args {
        cmd.arg(arg);
    }

    // Set Wine prefix
    cmd.env("WINEPREFIX", &prefix);

    // Proton-specific env vars
    if runner.runner_type == RunnerType::Proton {
        cmd.env("STEAM_COMPAT_DATA_PATH", &prefix);
        cmd.env(
            "STEAM_COMPAT_CLIENT_INSTALL_PATH",
            dirs::home_dir().unwrap_or_default().join(".steam/steam"),
        );
    }

    // Esync/Fsync
    if config.runner.esync {
        cmd.env("WINEESYNC", "1");
    }
    if config.runner.fsync {
        cmd.env("WINEFSYNC", "1");
    }

    // DXVK (for Proton it's usually built-in)
    if config.runner.dxvk && runner.runner_type == RunnerType::Wine {
        cmd.env("WINEDLLOVERRIDES", "d3d11,dxgi=n,b");
    }

    // WoW-specific env vars
    for (key, value) in &config.wow_env_vars {
        cmd.env(key, value);
    }

    // MangoHud
    if config.mangohud_enabled {
        cmd.env("MANGOHUD", "1");
        if !config.mangohud_config.is_empty() {
            cmd.env("MANGOHUD_CONFIG", &config.mangohud_config);
        }
    }

    // Set working directory to game dir
    if let Some(parent) = wow_exe.parent() {
        cmd.current_dir(parent);
    }

    tracing::info!("Launching WoW: {:?}", cmd);

    // If gamescope is enabled, wrap the entire command
    let cmd = if config.gamescope_enabled {
        with_gamescope_cmd(&cmd, config)
    } else {
        cmd
    };

    // If gamemode is enabled, wrap as outermost layer
    let mut cmd = if config.gamemode_enabled && gamemode_available {
        with_gamemode_cmd(&cmd)
    } else {
        cmd
    };

    if let Some(sender) = log_sender {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn process (runner: {}): {}",
                runner.path.display(),
                e
            )
        })?;
        tracing::info!("WoW launched with PID: {:?}", child.id());
        spawn_log_readers(child, sender, "WoW");
    } else {
        let child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn process (runner: {}): {}",
                runner.path.display(),
                e
            )
        })?;
        tracing::info!("WoW launched with PID: {:?}", child.id());
    }

    Ok(())
}

/// wrap the command in a gamescope command, preserving args and env vars
fn with_gamescope_cmd(cmd: &Command, config: &Config) -> Command {
    let mut gamescope_cmd = Command::new("gamescope");
    for arg in config.gamescope_args.split_whitespace() {
        gamescope_cmd.arg(arg);
    }
    gamescope_cmd.arg("--");

    // Re-build the inner command as gamescope args
    let program = cmd.as_std().get_program().to_owned();
    gamescope_cmd.arg(&program);
    for arg in cmd.as_std().get_args() {
        gamescope_cmd.arg(arg);
    }

    // Copy env vars
    for (key, val) in cmd.as_std().get_envs() {
        if let Some(v) = val {
            gamescope_cmd.env(key, v);
        }
    }

    // Copy working directory
    if let Some(dir) = cmd.as_std().get_current_dir() {
        gamescope_cmd.current_dir(dir);
    }

    tracing::info!("Wrapping with gamescope: {:?}", gamescope_cmd);

    gamescope_cmd
}

/// Wrap a command with `gamemoderun` for performance optimizations and sleep inhibition.
fn with_gamemode_cmd(cmd: &Command) -> Command {
    let mut gamemode_cmd = Command::new("gamemoderun");

    let program = cmd.as_std().get_program().to_owned();
    gamemode_cmd.arg(&program);
    for arg in cmd.as_std().get_args() {
        gamemode_cmd.arg(arg);
    }

    for (key, val) in cmd.as_std().get_envs() {
        if let Some(v) = val {
            gamemode_cmd.env(key, v);
        }
    }

    if let Some(dir) = cmd.as_std().get_current_dir() {
        gamemode_cmd.current_dir(dir);
    }

    tracing::info!("Wrapping with gamemoderun: {:?}", gamemode_cmd);

    gamemode_cmd
}

/// Spawn a background task that reads stdout/stderr from a child process and sends log
/// entries, stopping as soon as the direct child process exits.
///
/// We deliberately tie the readers' lifetime to the child's exit rather than to EOF on the
/// pipes: Wine/Proton keep long-lived helper processes (e.g. `wineserver`) running in the
/// background for the shared prefix, and those helpers inherit our piped stdout/stderr file
/// descriptors. If we only stopped on EOF, the reader tasks — and the pipe file descriptors
/// they hold open — would never be released for the lifetime of the app, leaking two file
/// descriptors on every launch until the process runs out of them.
fn spawn_log_readers(
    mut child: tokio::process::Child,
    sender: mpsc::UnboundedSender<LogEntry>,
    source: &'static str,
) {
    let mut stdout_lines = child.stdout.take().map(|s| BufReader::new(s).lines());
    let mut stderr_lines = child.stderr.take().map(|s| BufReader::new(s).lines());

    tokio::spawn(async move {
        loop {
            tokio::select! {
                status = child.wait() => {
                    if let Err(e) = status {
                        tracing::warn!("Error waiting for {source} process: {e}");
                    }
                    break;
                }
                result = read_line(&mut stdout_lines) => {
                    match result {
                        Ok(Some(line)) => { let _ = sender.send(LogEntry::new(source, line)); }
                        _ => stdout_lines = None,
                    }
                }
                result = read_line(&mut stderr_lines) => {
                    match result {
                        Ok(Some(line)) => { let _ = sender.send(LogEntry::new(source, line)); }
                        _ => stderr_lines = None,
                    }
                }
            }
        }
    });
}

/// Read the next line from an optional line reader, resolving to a pending future forever if
/// the reader has already been exhausted (so the corresponding `select!` arm is effectively
/// disabled without needing separate `if` guards per branch).
async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(
    lines: &mut Option<tokio::io::Lines<R>>,
) -> std::io::Result<Option<String>> {
    match lines {
        Some(l) => l.next_line().await,
        None => std::future::pending().await,
    }
}

/// Create a new Wine prefix
async fn create_prefix(runner: &Runner, prefix: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(prefix)?;

    let wine_bin = super::wine_bin_path(runner);

    let status = Command::new(&wine_bin)
        .arg("wineboot")
        .arg("--init")
        .env("WINEPREFIX", prefix)
        .env("WINEARCH", "win64")
        .status()
        .await?;

    if !status.success() {
        anyhow::bail!("Failed to create Wine prefix");
    }

    Ok(())
}
