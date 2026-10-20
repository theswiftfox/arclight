use std::{path::Path, process::Stdio};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use super::Runner;
use crate::{
    background::{self, IO_TIMEOUT},
    config::{Config, GameClient, RunnerType},
    ui::logs::{LogEntry, LogSender, MAX_LINE_BYTES},
};

/// Launch the game using the specified runner.
///
/// # Errors
/// Returns an error if the prefix cannot be created or the game process fails to launch.
pub async fn launch_game(
    runner: &Runner,
    wow_exe: &Path,
    config: &Config,
    client: &GameClient,
    use_launcherlogin: bool,
    gamemode_available: bool,
    log_sender: Option<LogSender>,
) -> anyhow::Result<()> {
    let probe_config = config.clone();
    let probe_client = client.clone();
    let runner_type = runner.runner_type;
    let (prefix, exists) = background::blocking("Checking WoW prefix", IO_TIMEOUT, move || {
        let prefix = probe_client.prefix_path();
        let exists = probe_config
            .game_wine_prefix_root(&probe_client, runner_type)
            .join("drive_c")
            .is_dir();
        Ok((prefix, exists))
    })
    .await?;

    // Ensure prefix exists
    if !exists {
        tracing::info!("Creating Wine prefix at {}", prefix.display());
        super::create_prefix(runner, &prefix, "Initializing game prefix").await?;
    }
    background::apply_drive_mappings(config.clone()).await?;

    let runner = runner.clone();
    let wow_exe = wow_exe.to_path_buf();
    let config = config.clone();
    let client = client.clone();
    let client_name = client.name.clone();
    let mut cmd = background::blocking("Building WoW command", IO_TIMEOUT, move || {
        Ok(build_game_command(
            &runner,
            &wow_exe,
            &prefix,
            &config,
            &client,
            use_launcherlogin,
            gamemode_available,
        ))
    })
    .await?;

    if let Some(sender) = log_sender {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = background::spawn_game(cmd).await?;
        tracing::info!("{} launched with PID: {:?}", client_name, child.id());
        spawn_log_readers(child, sender, "WoW");
    } else {
        // Accepted games survive launcher shutdown; late spawns are cleaned up.
        let mut child = background::spawn_game(cmd).await?;
        tracing::info!("{} launched with PID: {:?}", client_name, child.id());
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
    }

    Ok(())
}

fn build_game_command(
    runner: &Runner,
    wow_exe: &Path,
    prefix: &Path,
    config: &Config,
    client: &GameClient,
    use_launcherlogin: bool,
    gamemode_available: bool,
) -> Command {
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

    if use_launcherlogin && !client.skip_default_args {
        cmd.args(["-launcherlogin", "-uid", "wow"]);
    }
    cmd.args(&client.args).env("WINEPREFIX", prefix);

    if runner.runner_type == RunnerType::Proton {
        cmd.env("STEAM_COMPAT_DATA_PATH", prefix).env(
            "STEAM_COMPAT_CLIENT_INSTALL_PATH",
            dirs::home_dir().unwrap_or_default().join(".steam/steam"),
        );
    }
    if config.runner.esync {
        cmd.env("WINEESYNC", "1");
    }
    if config.runner.fsync {
        cmd.env("WINEFSYNC", "1");
    }
    if config.runner.dxvk && runner.runner_type == RunnerType::Wine {
        cmd.env("WINEDLLOVERRIDES", "d3d11,dxgi=n,b");
    }
    for (key, value) in &client.env_vars {
        cmd.env(key, value);
    }
    if config.mangohud_enabled {
        cmd.env("MANGOHUD", "1");
        if !config.mangohud_config.is_empty() {
            cmd.env("MANGOHUD_CONFIG", &config.mangohud_config);
        }
    }
    if let Some(parent) = wow_exe.parent() {
        cmd.current_dir(parent);
    }

    tracing::info!("Launching {}: {:?}", client.name, cmd);
    let cmd = if config.gamescope_enabled {
        with_gamescope_cmd(&cmd, config)
    } else {
        cmd
    };
    if config.gamemode_enabled && gamemode_available {
        with_gamemode_cmd(&cmd)
    } else {
        cmd
    }
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
        } else {
            gamescope_cmd.env_remove(key);
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
        } else {
            gamemode_cmd.env_remove(key);
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
pub(crate) fn spawn_log_readers(
    mut child: tokio::process::Child,
    sender: LogSender,
    source: &'static str,
) {
    let mut stdout_lines = child.stdout.take().map(BoundedLines::new);
    let mut stderr_lines = child.stderr.take().map(BoundedLines::new);

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
                        Ok(Some(line)) => sender.send(LogEntry::new(source, line)),
                        _ => stdout_lines = None,
                    }
                }
                result = read_line(&mut stderr_lines) => {
                    match result {
                        Ok(Some(line)) => sender.send(LogEntry::new(source, line)),
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
async fn read_line<R: tokio::io::AsyncRead + Unpin>(
    lines: &mut Option<BoundedLines<R>>,
) -> std::io::Result<Option<String>> {
    match lines {
        Some(l) => l.next_line().await,
        None => std::future::pending().await,
    }
}

/// Cancellation-safe line reader that drains oversized lines without retaining them.
struct BoundedLines<R> {
    reader: BufReader<R>,
    line: Vec<u8>,
    truncated: bool,
}

impl<R: tokio::io::AsyncRead + Unpin> BoundedLines<R> {
    fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            line: Vec::new(),
            truncated: false,
        }
    }

    async fn next_line(&mut self) -> std::io::Result<Option<String>> {
        loop {
            let bytes = self.reader.fill_buf().await?;
            if bytes.is_empty() {
                return if self.line.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(self.finish()))
                };
            }
            let newline = bytes.iter().position(|&b| b == b'\n');
            let length = newline.unwrap_or(bytes.len());
            let retained = length.min(MAX_LINE_BYTES.saturating_sub(self.line.len()));
            self.line.extend_from_slice(&bytes[..retained]);
            self.truncated |= retained < length;
            self.reader.consume(length + usize::from(newline.is_some()));
            if newline.is_some() {
                return Ok(Some(self.finish()));
            }
            // Even a producer with a continuously readable, newline-free pipe
            // must let the child-exit arm and other runtime tasks run.
            tokio::task::yield_now().await;
        }
    }

    fn finish(&mut self) -> String {
        let mut text = String::from_utf8_lossy(&self.line)
            .trim_end_matches('\r')
            .to_string();
        if self.truncated {
            text.push_str(" [truncated]");
        }
        self.line.clear();
        self.truncated = false;
        text
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;

    use super::*;

    #[tokio::test]
    async fn oversized_pipe_line_is_drained_and_following_line_survives() {
        let (mut writer, reader) = tokio::io::duplex(128);
        let producer = tokio::spawn(async move {
            writer
                .write_all(&vec![b'x'; MAX_LINE_BYTES * 4])
                .await
                .unwrap();
            writer.write_all(b"\nok\n").await.unwrap();
        });
        let mut lines = BoundedLines::new(reader);
        let line = lines.next_line().await.unwrap().unwrap();
        assert!(line.ends_with("[truncated]"));
        assert!(line.len() < MAX_LINE_BYTES + 32);
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("ok"));
        producer.await.unwrap();
        assert!(lines.next_line().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn cancelling_partial_read_does_not_lose_bytes() {
        let (mut writer, reader) = tokio::io::duplex(128);
        writer.write_all(b"partial").await.unwrap();
        let mut lines = BoundedLines::new(reader);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), lines.next_line())
                .await
                .is_err()
        );
        writer.write_all(b" line\n").await.unwrap();
        assert_eq!(
            lines.next_line().await.unwrap().as_deref(),
            Some("partial line")
        );
    }
}
