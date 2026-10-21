pub mod bnet;
pub mod detect;
pub mod process;
pub mod proton_ge;
pub mod steam;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    background::{self, DISCOVERY_TIMEOUT, IO_TIMEOUT, SETUP_TIMEOUT},
    config::{Config, GameClient, RunnerConfig, RunnerOverride, RunnerType},
    ui::logs::LogSender,
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

/// Get a game client's runner, falling back to the global runner.
///
/// # Errors
/// Returns an error if no client override is configured and auto-detection fails.
pub fn get_game_runner(config: &RunnerConfig, client: &GameClient) -> anyhow::Result<Runner> {
    client.runner.as_ref().map_or_else(
        || get_runner(config),
        |runner| Ok(configured_override_runner(runner)),
    )
}

/// Resolve a configured client executable below the shared `WoW` installation root.
///
/// # Errors
/// Returns an error for unsafe paths or when the executable does not exist.
pub fn resolve_game_executable(root: &Path, client: &GameClient) -> anyhow::Result<PathBuf> {
    use std::path::Component;

    if client.executable.as_os_str().is_empty()
        || client.executable.is_absolute()
        || client.executable.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        anyhow::bail!(
            "{} executable must be a relative path inside the WoW install root",
            client.name
        );
    }
    let executable = root.join(&client.executable);
    if !executable.is_file() {
        anyhow::bail!(
            "{} executable not found at {}",
            client.name,
            executable.display()
        );
    }
    Ok(executable)
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
    client: &GameClient,
    use_launcherlogin: bool,
    gamemode_available: bool,
    log_sender: Option<LogSender>,
) -> anyhow::Result<()> {
    let _setup = background::prefix_setup()?;
    let probe_config = config.clone();
    let probe_client = client.clone();
    let (wow_exe, runner) = background::blocking("Preparing WoW", DISCOVERY_TIMEOUT, move || {
        let config = &probe_config;
        let game_path = config
            .game_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Game path not configured"))?;

        let wow_exe = resolve_game_executable(game_path, &probe_client)?;
        let runner = get_game_runner(&config.runner, &probe_client)?;
        Ok((wow_exe, runner))
    })
    .await?;
    tracing::info!("Using runner: {} ({})", runner.name, runner.path.display());

    process::launch_game(
        &runner,
        &wow_exe,
        config,
        client,
        use_launcherlogin,
        gamemode_available,
        log_sender,
    )
    .await
}

/// Resolve Wine inside a runner, accounting for old and current Proton layouts.
///
/// # Errors
/// Returns an error when a Proton installation contains no Wine executable.
pub fn wine_bin_path(runner: &Runner) -> anyhow::Result<PathBuf> {
    match runner.runner_type {
        RunnerType::Wine => Ok(runner.path.clone()),
        RunnerType::Proton => {
            let proton_dir = runner.path.parent().unwrap_or(Path::new("."));
            let candidates = [
                proton_dir.join("dist/bin/wine64"),
                proton_dir.join("dist/bin/wine"),
                proton_dir.join("files/bin/wine64"),
                proton_dir.join("files/bin/wine"),
            ];
            candidates
                .iter()
                .find(|path| path.is_file())
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No Wine executable found for Proton runner {}. Tried:\n{}",
                        runner.path.display(),
                        candidates
                            .iter()
                            .map(|path| format!("  {}", path.display()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                })
        }
    }
}

fn prefix_init_command(
    runner: &Runner,
    prefix: &Path,
) -> anyhow::Result<Option<tokio::process::Command>> {
    if !runner.path.is_file() {
        anyhow::bail!("Runner not found at {}", runner.path.display());
    }

    match runner.runner_type {
        RunnerType::Wine => {
            let mut command = tokio::process::Command::new(wine_bin_path(runner)?);
            command
                .arg("wineboot")
                .arg("--init")
                .env("WINEPREFIX", prefix)
                .env("WINEARCH", "win64");
            Ok(Some(command))
        }
        // Proton initializes `<STEAM_COMPAT_DATA_PATH>/pfx` as part of the
        // imminent real `proton run` command. Calling its embedded Wine first
        // bypasses Proton's prefix setup and is incompatible with newer GE builds.
        RunnerType::Proton => Ok(None),
    }
}

/// Initialize a Wine or Proton prefix using the runner's supported entrypoint.
///
/// # Errors
/// Returns an error if the prefix cannot be prepared or initialized.
pub async fn create_prefix(
    runner: &Runner,
    prefix: &Path,
    name: &'static str,
) -> anyhow::Result<()> {
    let prefix = prefix.to_path_buf();
    let runner_path = runner.path.clone();
    let runner = runner.clone();
    let command = background::mutation(name, move || {
        std::fs::create_dir_all(&prefix)?;
        prefix_init_command(&runner, &prefix)
    })
    .await?;
    if let Some(mut command) = command {
        let output = background::deadline(name, SETUP_TIMEOUT, async {
            Ok(command.kill_on_drop(true).output().await?)
        })
        .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let tail = stderr
                .lines()
                .filter(|line| !line.contains(":fixme:"))
                .rev()
                .take(15)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            tracing::error!("{name} failed ({}):\n{tail}", output.status);
            anyhow::bail!(
                "{name} exited with {}\nRunner: {}\n{tail}",
                output.status,
                runner_path.display()
            );
        }
    }
    Ok(())
}

/// Resolve the `wineserver` binary alongside the runner's wine64 binary, falling back to the
/// system `wineserver` if it isn't found there.
#[must_use]
pub fn wineserver_bin_path(runner: &Runner) -> PathBuf {
    let candidate = wine_bin_path(runner)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_default()
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
    let runner = runner.clone();
    let wineserver = background::blocking("Finding wineserver", IO_TIMEOUT, move || {
        Ok(wineserver_bin_path(&runner))
    })
    .await?;
    let deadline = Instant::now() + WINESERVER_STARTUP_GRACE;

    loop {
        let started = Instant::now();
        let mut command = tokio::process::Command::new(&wineserver);
        command.arg("--wait").env("WINEPREFIX", prefix);
        let mut child = background::spawn(command).await?;
        // Intentionally long-lived and async. Cancelling the monitor drops and
        // kills only this --wait helper, never the game or its wineserver.
        let status = child.wait().await?;
        if !status.success() {
            anyhow::bail!("wineserver --wait exited with {status}");
        }

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
    let runner = runner.clone();
    let binary = background::blocking("Finding wineserver", IO_TIMEOUT, move || {
        Ok(wineserver_bin_path(&runner))
    })
    .await?;
    let mut command = tokio::process::Command::new(binary);
    command.arg("--kill").env("WINEPREFIX", prefix);
    background::command_status(command, "Stopping prefix", IO_TIMEOUT).await?;
    tracing::info!("Killed all processes in {}", prefix.display());
    Ok(())
}

/// Wait until `WoW` (and everything else in its prefix) has exited.
///
/// # Errors
/// Returns an error if no runner can be resolved or `wineserver` fails.
pub async fn wait_for_wow_exit(config: &Config, client: &GameClient) -> anyhow::Result<()> {
    let config = config.clone();
    let client = client.clone();
    let (runner, config, client) =
        background::blocking("Preparing WoW monitor", DISCOVERY_TIMEOUT, move || {
            Ok((get_game_runner(&config.runner, &client)?, config, client))
        })
        .await?;
    let prefix = config.game_wine_prefix_root(&client, runner.runner_type);
    // `wow_prefix_path()` is the outer compat-data dir; for Proton the actual Wine prefix
    // `wineserver` operates on lives at `<compat_data_path>/pfx`, so we must resolve the real
    // root or `wineserver --wait` will find nothing there and return immediately.
    wait_for_prefix_idle(&runner, &prefix).await
}

/// Force-kill `WoW` and everything else running in its prefix.
///
/// # Errors
/// Returns an error if no runner can be resolved or `wineserver` fails.
pub async fn kill_wow(config: &Config, client: &GameClient) -> anyhow::Result<()> {
    let config = config.clone();
    let client = client.clone();
    let (runner, config, client) =
        background::blocking("Preparing WoW stop", IO_TIMEOUT, move || {
            Ok((get_game_runner(&config.runner, &client)?, config, client))
        })
        .await?;
    let prefix = config.game_wine_prefix_root(&client, runner.runner_type);
    kill_prefix(&runner, &prefix).await
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

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

    #[test]
    fn game_runner_uses_client_override() {
        let config = RunnerConfig::default();
        let mut client = GameClient::retail();
        client.runner = Some(RunnerOverride {
            binary: PathBuf::from("/opt/proton-client/proton"),
            runner_type: RunnerType::Proton,
        });

        let runner = get_game_runner(&config, &client).unwrap();

        assert_eq!(runner.path, PathBuf::from("/opt/proton-client/proton"));
        assert_eq!(runner.runner_type, RunnerType::Proton);
    }

    #[test]
    fn resolves_relative_client_executable() {
        let root = std::env::temp_dir().join(format!("arclight-test-{}", std::process::id()));
        let executable = root.join("_classic_/WowClassic.exe");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, []).unwrap();

        let resolved = resolve_game_executable(&root, &GameClient::classic()).unwrap();

        assert_eq!(resolved, executable);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_executable_outside_install_root() {
        let mut client = GameClient::retail();
        client.executable = PathBuf::from("../Wow.exe");

        let error = resolve_game_executable(Path::new("/games/wow"), &client).unwrap_err();

        assert!(error.to_string().contains("relative path"));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("arclight-{name}-{}-{unique}", std::process::id()))
    }

    #[test]
    fn resolves_wine_from_current_ge_proton_layout() {
        let root = temp_dir("ge-proton");
        let proton = root.join("proton");
        let wine = root.join("files/bin/wine");
        fs::create_dir_all(wine.parent().unwrap()).unwrap();
        fs::write(&proton, []).unwrap();
        fs::write(&wine, []).unwrap();
        let runner = Runner {
            name: "GE-Proton".into(),
            path: proton,
            runner_type: RunnerType::Proton,
        };

        assert_eq!(wine_bin_path(&runner).unwrap(), wine);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn proton_prefix_initialization_is_deferred_to_proton() {
        let root = temp_dir("proton-prefix");
        let proton = root.join("proton");
        fs::create_dir_all(&root).unwrap();
        fs::write(&proton, []).unwrap();
        let runner = Runner {
            name: "GE-Proton".into(),
            path: proton,
            runner_type: RunnerType::Proton,
        };

        assert!(
            prefix_init_command(&runner, &root.join("compatdata"))
                .unwrap()
                .is_none()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_proton_wine_reports_attempted_layouts() {
        let root = temp_dir("missing-wine");
        let proton = root.join("proton");
        fs::create_dir_all(&root).unwrap();
        fs::write(&proton, []).unwrap();
        let runner = Runner {
            name: "Broken Proton".into(),
            path: proton,
            runner_type: RunnerType::Proton,
        };

        let error = wine_bin_path(&runner).unwrap_err().to_string();

        assert!(error.contains("files/bin/wine"));
        assert!(error.contains("files/bin/wine64"));
        fs::remove_dir_all(root).unwrap();
    }
}
