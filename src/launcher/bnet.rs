use std::{path::Path, process::Stdio};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc,
};

use crate::{
    config::{AppPaths, Config, RunnerType},
    launcher::{Runner, get_runner},
    ui::logs::LogEntry,
};

const BNET_INSTALLER_URL: &str = "https://www.battle.net/download/getInstallerForGame?os=win&version=LIVE&gameProgram=BATTLENET_APP";

/// Auth status based on registry contents
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthStatus {
    /// No Battle.net installed in prefix
    NotInstalled,
    /// Battle.net installed but no login token found
    NotLoggedIn,
    /// Battle.net is logged in but `WoW` hasn't been launched from bnet yet
    /// (no ACCOUNT blob — `WoW` will show its own login or user needs to launch from bnet once)
    BnetLoggedIn,
    /// Full `WoW` session token in registry — auto-login will work
    WowReady { game_account: String },
}

/// Check auth status by parsing the Wine registry
#[must_use]
pub fn check_auth_status(config: &Config) -> AuthStatus {
    if !config.bnet_installed() {
        return AuthStatus::NotInstalled;
    }

    let wine_root = config.wine_prefix_root();
    let user_reg = wine_root.join("user.reg");

    if !user_reg.exists() {
        return AuthStatus::NotLoggedIn;
    }

    match std::fs::read_to_string(&user_reg) {
        Ok(content) => parse_auth_from_registry(&content),
        Err(_) => AuthStatus::NotLoggedIn,
    }
}

/// Parse the Wine user.reg file for Battle.net login status
fn parse_auth_from_registry(content: &str) -> AuthStatus {
    let mut in_wow_section = false;
    let mut in_bnet_section = false;
    let mut has_account = false;
    let mut has_bnet_login = false;
    let mut game_account = String::new();

    for line in content.lines() {
        // Check for WoW-specific launch options (best case: ready to launch)
        if line.starts_with(
            "[Software\\\\Blizzard Entertainment\\\\Battle.net\\\\Launch Options\\\\WoW]",
        ) {
            in_wow_section = true;
            in_bnet_section = false;
            continue;
        }
        // Check for base Battle.net launch options (indicates bnet is logged in)
        if line.starts_with("[Software\\\\Blizzard Entertainment\\\\Battle.net\\\\Launch Options]")
            && !line.contains("\\\\WoW")
            && !line.contains("\\\\BNA")
        {
            in_bnet_section = true;
            in_wow_section = false;
            continue;
        }
        if (in_wow_section || in_bnet_section) && line.starts_with('[') {
            in_wow_section = false;
            in_bnet_section = false;
            continue;
        }

        if in_wow_section {
            if line.starts_with("\"ACCOUNT\"=hex:") {
                has_account = true;
            }
            if line.starts_with("\"GAME_ACCOUNT\"=")
                && let Some(val) = line.split('=').nth(1)
            {
                game_account = val.trim_matches('"').to_string();
            }
        }
        if in_bnet_section && line.starts_with("\"URI_TOKEN\"=") {
            has_bnet_login = true;
        }
    }

    if has_account {
        AuthStatus::WowReady { game_account }
    } else if has_bnet_login {
        AuthStatus::BnetLoggedIn
    } else {
        AuthStatus::NotLoggedIn
    }
}

/// Download the Battle.net installer.
///
/// # Errors
/// Returns an error if the HTTP request fails or the file cannot be written.
pub async fn download_installer(target_path: &Path) -> anyhow::Result<()> {
    tracing::info!("Downloading Battle.net installer...");

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent("Arclight/0.1")
        .build()?;

    let response = client.get(BNET_INSTALLER_URL).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("Failed to download installer: HTTP {}", response.status());
    }

    let bytes = response.bytes().await?;

    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(target_path, &bytes)?;

    tracing::info!(
        "Installer downloaded to {} ({} bytes)",
        target_path.display(),
        bytes.len()
    );
    Ok(())
}

/// Install Battle.net into the prefix.
///
/// # Errors
/// Returns an error if prefix creation, installer download, or the installer process fails.
pub async fn install_bnet(config: &Config, installer_path: Option<&Path>) -> anyhow::Result<()> {
    let prefix = config.prefix_path();
    let runner = get_runner(&config.runner)?;

    // Ensure prefix exists
    if !prefix.exists() {
        tracing::info!("Creating Wine prefix at {}", prefix.display());
        create_prefix(&runner, &prefix).await?;
    }

    // Download installer if no path provided
    let installer = if let Some(path) = installer_path {
        path.to_path_buf()
    } else {
        let dl_path = AppPaths::data_dir().join("Battle.net-Setup.exe");
        download_installer(&dl_path).await?;
        dl_path
    };

    tracing::info!("Running Battle.net installer...");

    let mut cmd = build_wine_command(&runner, &prefix, &installer, config);
    let status = cmd.status().await?;

    if !status.success() {
        anyhow::bail!("Battle.net installer exited with status: {status}");
    }

    tracing::info!("Battle.net installation complete");
    Ok(())
}

/// Launch Battle.net client and wait for it to exit.
///
/// # Errors
/// Returns an error if Battle.net is not installed, the runner cannot be resolved, or the process fails to spawn.
pub async fn launch_bnet(
    config: &Config,
    log_sender: Option<mpsc::UnboundedSender<LogEntry>>,
) -> anyhow::Result<()> {
    let bnet_exe = config.bnet_exe_path();
    if !bnet_exe.exists() {
        anyhow::bail!(
            "Battle.net not found at {}. Please install it first.",
            bnet_exe.display()
        );
    }

    let prefix = config.prefix_path();
    let runner = get_runner(&config.runner)?;

    tracing::info!("Launching Battle.net...");

    let mut cmd = build_wine_command(&runner, &prefix, &bnet_exe, config);

    // Add Battle.net specific args
    for arg in &config.bnet_args {
        cmd.arg(arg);
    }

    // Add Battle.net specific env vars
    for (key, value) in &config.bnet_env_vars {
        cmd.env(key, value);
    }

    if let Some(sender) = log_sender {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch Battle.net: {e}"))?;
        spawn_log_readers(child, sender);
    } else {
        let _child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch Battle.net: {e}"))?;
    }

    tracing::info!("Battle.net launched, waiting for all prefix processes to exit...");

    // The initial Battle.net process exits quickly after spawning the real client.
    // Use wineserver --wait to block until ALL processes in the prefix have exited.
    //
    // `prefix` here is the outer compat-data dir; for Proton the actual Wine prefix
    // `wineserver` operates on lives at `<compat_data_path>/pfx`, so we must resolve the real
    // root or `wineserver --wait` will find nothing there and return immediately.
    let runner = get_runner(&config.runner)?;
    crate::launcher::wait_for_prefix_idle(&runner, &config.wine_prefix_root()).await
}

/// Kill all Battle.net processes in the prefix.
///
/// # Errors
/// Returns an error if the runner cannot be resolved.
pub async fn kill_bnet(config: &Config) -> anyhow::Result<()> {
    let runner = get_runner(&config.runner)?;
    crate::launcher::kill_prefix(&runner, &config.wine_prefix_root()).await
}

/// Build a Wine/Proton command with common env vars
fn build_wine_command(runner: &Runner, prefix: &Path, exe: &Path, config: &Config) -> Command {
    let mut cmd = match runner.runner_type {
        RunnerType::Wine => {
            let mut cmd = Command::new(&runner.path);
            cmd.arg(exe);
            cmd
        }
        RunnerType::Proton => {
            let mut cmd = Command::new(&runner.path);
            cmd.arg("run").arg(exe);
            cmd
        }
    };

    cmd.env("WINEPREFIX", prefix);

    if runner.runner_type == RunnerType::Proton {
        cmd.env("STEAM_COMPAT_DATA_PATH", prefix);
        cmd.env(
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

    if let Some(parent) = exe.parent() {
        cmd.current_dir(parent);
    }

    cmd
}

/// Create a new Wine prefix
async fn create_prefix(runner: &Runner, prefix: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(prefix)?;

    let wine_bin = crate::launcher::wine_bin_path(runner);

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

/// Spawn a background task that reads stdout/stderr from a child process and sends log
/// entries, stopping as soon as the direct child process exits.
///
/// We deliberately tie the readers' lifetime to the child's exit rather than to EOF on the
/// pipes: Wine/Proton keep long-lived helper processes (e.g. `wineserver`) running in the
/// background for the shared prefix, and those helpers inherit our piped stdout/stderr file
/// descriptors. If we only stopped on EOF, the reader tasks — and the pipe file descriptors
/// they hold open — would never be released for the lifetime of the app, leaking two file
/// descriptors on every launch until the process runs out of them.
fn spawn_log_readers(mut child: tokio::process::Child, sender: mpsc::UnboundedSender<LogEntry>) {
    let mut stdout_lines = child.stdout.take().map(|s| BufReader::new(s).lines());
    let mut stderr_lines = child.stderr.take().map(|s| BufReader::new(s).lines());

    tokio::spawn(async move {
        loop {
            tokio::select! {
                status = child.wait() => {
                    if let Err(e) = status {
                        tracing::warn!("Error waiting for BNet process: {e}");
                    }
                    break;
                }
                result = read_line(&mut stdout_lines) => {
                    match result {
                        Ok(Some(line)) => { let _ = sender.send(LogEntry::new("BNet", line)); }
                        _ => stdout_lines = None,
                    }
                }
                result = read_line(&mut stderr_lines) => {
                    match result {
                        Ok(Some(line)) => { let _ = sender.send(LogEntry::new("BNet", line)); }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auth_logged_in_wow() {
        let reg = r#"[Software\\Blizzard Entertainment\\Battle.net\\Launch Options\\WoW] 1786951062
#time=1dd2e187e8d4138
"ACCOUNT"=hex:01,00,00,00
"GAME_ACCOUNT"="WoW2"
"REGION"="EU"
"#;
        let status = parse_auth_from_registry(reg);
        assert_eq!(
            status,
            AuthStatus::WowReady {
                game_account: "WoW2".to_string()
            }
        );
    }

    #[test]
    fn test_parse_auth_logged_in_bnet() {
        let reg = r#"[Software\\Blizzard Entertainment\\Battle.net\\Launch Options] 1786964615
#time=1dd2e380cc9a0ee
"URI_TOKEN"="9ce9c3ec"

[Software\\Blizzard Entertainment\\Battle.net\\Launch Options\\BNA] 1786343422
"CONNECTION_STRING_EU"="eu.actual.battle.net"
"#;
        let status = parse_auth_from_registry(reg);
        assert_eq!(status, AuthStatus::BnetLoggedIn);
    }

    #[test]
    fn test_parse_auth_not_logged_in() {
        let reg = r#"[Software\\Blizzard Entertainment\\Battle.net\\Launch Options\\BNA] 1786343422
"CONNECTION_STRING_EU"="eu.actual.battle.net"
"#;
        let status = parse_auth_from_registry(reg);
        assert_eq!(status, AuthStatus::NotLoggedIn);
    }
}
