pub mod bnet;
pub mod detect;
pub mod process;
pub mod steam;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use tokio::sync::mpsc;

use crate::{
    config::{Config, RunnerConfig, RunnerOverride, RunnerType},
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
        Ok(configured_runner(binary, config.runner_type))
    } else {
        // Auto-detect
        detect::find_best_runner()
    }
}

/// Get Battle.net's runner, falling back to the configured `WoW` runner.
///
/// # Errors
/// Returns an error if neither the Battle.net override nor the `WoW` runner can be resolved.
pub fn get_bnet_runner(config: &RunnerConfig) -> anyhow::Result<Runner> {
    if let Some(override_runner) = &config.bnet_runner {
        Ok(configured_override_runner(override_runner))
    } else {
        get_runner(config)
    }
}

fn configured_runner(binary: &Path, runner_type: RunnerType) -> Runner {
    Runner {
        name: binary
            .file_name()
            .map_or_else(|| "custom".into(), |n| n.to_string_lossy().to_string()),
        path: binary.to_path_buf(),
        runner_type,
    }
}

fn configured_override_runner(override_runner: &RunnerOverride) -> Runner {
    configured_runner(&override_runner.binary, override_runner.runner_type)
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

/// Resolve the wine64 binary for a runner, accounting for Proton's directory layout.
#[must_use]
pub fn wine_bin_path(runner: &Runner) -> PathBuf {
    match runner.runner_type {
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
    }
}

/// Resolve the `wineserver` binary alongside the runner's wine64 binary, falling back to the
/// system `wineserver` if it isn't found there.
#[must_use]
pub fn wineserver_bin_path(runner: &Runner) -> PathBuf {
    let candidate = wine_bin_path(runner)
        .parent()
        .unwrap_or(Path::new("."))
        .join("wineserver");
    if candidate.exists() {
        candidate
    } else {
        PathBuf::from("wineserver")
    }
}

/// `wineserver --wait` only waits on an *already-running* server for the prefix. Grace period
/// during which we retry if it returns suspiciously fast (see [`wait_for_prefix_idle`]).
const WINESERVER_STARTUP_GRACE: Duration = Duration::from_secs(20);
/// Minimum duration a "real" wait is expected to take; a faster return means there was no
/// server up yet rather than the prefix genuinely already being idle.
const WINESERVER_MIN_REAL_WAIT: Duration = Duration::from_millis(200);
/// How long to sleep between retries while waiting for the server to start.
const WINESERVER_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Block until every process running inside the given Wine prefix has exited.
///
/// `wineserver --wait` means "wait until the *currently running* wineserver for this prefix
/// terminates" — if we call it before the just-launched process has had a chance to start its
/// own wineserver, there is no server to wait on and it returns immediately, incorrectly
/// reporting the prefix as idle before the app has even opened a window. To avoid that race we
/// retry while it keeps returning near-instantly, for a short startup grace period, giving the
/// launched process time to actually come up.
///
/// # Errors
/// Returns an error if `wineserver` cannot be spawned.
pub async fn wait_for_prefix_idle(runner: &Runner, prefix: &Path) -> anyhow::Result<()> {
    let wineserver = wineserver_bin_path(runner);
    let deadline = Instant::now() + WINESERVER_STARTUP_GRACE;

    loop {
        let started = Instant::now();
        let status = tokio::process::Command::new(&wineserver)
            .arg("--wait")
            .env("WINEPREFIX", prefix)
            .status()
            .await?;

        if started.elapsed() >= WINESERVER_MIN_REAL_WAIT || Instant::now() >= deadline {
            tracing::info!(
                "All processes in {} exited (wineserver status: {status})",
                prefix.display()
            );
            return Ok(());
        }

        tracing::debug!(
            "wineserver --wait returned instantly for {}, server may not be up yet; retrying",
            prefix.display()
        );
        tokio::time::sleep(WINESERVER_POLL_INTERVAL).await;
    }
}

/// Force-kill every process running inside the given Wine prefix.
///
/// # Errors
/// Returns an error if `wineserver` cannot be spawned.
pub async fn kill_prefix(runner: &Runner, prefix: &Path) -> anyhow::Result<()> {
    let status = tokio::process::Command::new(wineserver_bin_path(runner))
        .arg("--kill")
        .env("WINEPREFIX", prefix)
        .status()
        .await?;
    tracing::info!(
        "Killed all processes in {} (wineserver status: {status})",
        prefix.display()
    );
    Ok(())
}

/// Wait until `WoW` (and everything else in its prefix) has exited.
///
/// # Errors
/// Returns an error if no runner can be resolved or `wineserver` fails.
pub async fn wait_for_wow_exit(config: &Config) -> anyhow::Result<()> {
    let runner = get_runner(&config.runner)?;
    // `wow_prefix_path()` is the outer compat-data dir; for Proton the actual Wine prefix
    // `wineserver` operates on lives at `<compat_data_path>/pfx`, so we must resolve the real
    // root or `wineserver --wait` will find nothing there and return immediately.
    wait_for_prefix_idle(&runner, &config.wow_wine_prefix_root()).await
}

/// Force-kill `WoW` and everything else running in its prefix.
///
/// # Errors
/// Returns an error if no runner can be resolved or `wineserver` fails.
pub async fn kill_wow(config: &Config) -> anyhow::Result<()> {
    let runner = get_runner(&config.runner)?;
    kill_prefix(&runner, &config.wow_wine_prefix_root()).await
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn bnet_runner_falls_back_to_wow_runner() {
        let config = RunnerConfig {
            binary: Some(PathBuf::from("/opt/proton-wow/proton")),
            runner_type: RunnerType::Proton,
            ..RunnerConfig::default()
        };

        let runner = get_bnet_runner(&config).unwrap();

        assert_eq!(runner.path, PathBuf::from("/opt/proton-wow/proton"));
        assert_eq!(runner.runner_type, RunnerType::Proton);
    }

    #[test]
    fn bnet_runner_uses_configured_override() {
        let config = RunnerConfig {
            binary: Some(PathBuf::from("/usr/bin/wine")),
            runner_type: RunnerType::Wine,
            bnet_runner: Some(RunnerOverride {
                binary: PathBuf::from("/opt/proton-bnet/proton"),
                runner_type: RunnerType::Proton,
            }),
            ..RunnerConfig::default()
        };

        let runner = get_bnet_runner(&config).unwrap();

        assert_eq!(runner.path, PathBuf::from("/opt/proton-bnet/proton"));
        assert_eq!(runner.runner_type, RunnerType::Proton);
    }
}
