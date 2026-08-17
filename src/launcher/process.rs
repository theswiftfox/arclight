use std::path::Path;

use tokio::process::Command;

use super::Runner;
use crate::config::{Config, RunnerType};

/// Launch the game using the specified runner.
///
/// # Errors
/// Returns an error if the prefix cannot be created or the game process fails to launch.
pub async fn launch_game(
    runner: &Runner,
    wow_exe: &Path,
    config: &Config,
    use_launcherlogin: bool,
) -> anyhow::Result<()> {
    let prefix = config.prefix_path();

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
    if config.gamescope_enabled {
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

        let child = gamescope_cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn gamescope: {e}"))?;
        tracing::info!("WoW launched via gamescope with PID: {:?}", child.id());
        return Ok(());
    }

    let child = cmd.spawn().map_err(|e| {
        anyhow::anyhow!(
            "Failed to spawn process (runner: {}): {}",
            runner.path.display(),
            e
        )
    })?;
    tracing::info!("WoW launched with PID: {:?}", child.id());

    Ok(())
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
