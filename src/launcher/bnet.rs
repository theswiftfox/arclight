use std::path::Path;

use tokio::process::Command;

use crate::{
    config::{AppPaths, Config, RunnerType},
    launcher::{Runner, get_runner},
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
pub async fn launch_bnet(config: &Config) -> anyhow::Result<()> {
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

    let _child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to launch Battle.net: {e}"))?;

    tracing::info!("Battle.net launched, waiting for all prefix processes to exit...");

    // The initial Battle.net process exits quickly after spawning the real client.
    // Use wineserver --wait to block until ALL processes in the prefix have exited.
    let runner = get_runner(&config.runner)?;
    let wine_bin = match runner.runner_type {
        RunnerType::Wine => runner.path.clone(),
        RunnerType::Proton => {
            let proton_dir = runner.path.parent().unwrap_or(Path::new("."));
            let wine = proton_dir.join("dist/bin/wine64");
            if wine.exists() {
                wine
            } else {
                proton_dir.join("files/bin/wine64")
            }
        }
    };

    let wineserver = wine_bin
        .parent()
        .unwrap_or(Path::new("."))
        .join("wineserver");
    let wineserver_bin = if wineserver.exists() {
        wineserver
    } else {
        std::path::PathBuf::from("wineserver")
    };

    let status = Command::new(&wineserver_bin)
        .arg("--wait")
        .env("WINEPREFIX", &prefix)
        .status()
        .await?;

    tracing::info!(
        "All prefix processes exited (wineserver status: {})",
        status
    );

    Ok(())
}

/// Kill all Battle.net processes in the prefix.
///
/// # Errors
/// Returns an error if the runner cannot be resolved.
pub async fn kill_bnet(config: &Config) -> anyhow::Result<()> {
    let prefix = config.prefix_path();
    let runner = get_runner(&config.runner)?;

    let wine_bin = match runner.runner_type {
        RunnerType::Wine => runner.path.clone(),
        RunnerType::Proton => {
            let proton_dir = runner.path.parent().unwrap_or(Path::new("."));
            let wine = proton_dir.join("dist/bin/wine64");
            if wine.exists() {
                wine
            } else {
                proton_dir.join("files/bin/wine64")
            }
        }
    };

    // Use wineserver --kill to terminate all processes in the prefix
    let wineserver = wine_bin
        .parent()
        .unwrap_or(Path::new("."))
        .join("wineserver");
    if wineserver.exists() {
        let _ = Command::new(&wineserver)
            .arg("--kill")
            .env("WINEPREFIX", &prefix)
            .status()
            .await;
    } else {
        // Fallback: try system wineserver
        let _ = Command::new("wineserver")
            .arg("--kill")
            .env("WINEPREFIX", &prefix)
            .status()
            .await;
    }

    tracing::info!("Battle.net killed");
    Ok(())
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

    let wine_bin = match runner.runner_type {
        RunnerType::Wine => runner.path.clone(),
        RunnerType::Proton => {
            let proton_dir = runner.path.parent().unwrap_or(Path::new("."));
            let wine = proton_dir.join("dist/bin/wine64");
            if wine.exists() {
                wine
            } else {
                proton_dir.join("files/bin/wine64")
            }
        }
    };

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
