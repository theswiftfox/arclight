use std::{
    fmt,
    path::{Path, PathBuf},
};

use super::{Runner, steam};
use crate::config::{AppPaths, RunnerType};

/// A detected runner for use in UI (implements Display for `pick_list`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedRunner {
    pub name: String,
    pub path: PathBuf,
    pub runner_type: RunnerType,
}

impl fmt::Display for DetectedRunner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_str = match self.runner_type {
            RunnerType::Wine => "wine",
            RunnerType::Proton => "proton",
        };
        write!(f, "{} ({})", self.name, type_str)
    }
}

impl From<&DetectedRunner> for Runner {
    fn from(d: &DetectedRunner) -> Self {
        Runner {
            name: d.name.clone(),
            path: d.path.clone(),
            runner_type: d.runner_type,
        }
    }
}

/// Detect available Wine/Proton runners on the system
#[must_use]
pub fn detect_runners() -> Vec<DetectedRunner> {
    let mut runners = Vec::new();

    // System Wine
    if let Ok(path) = which::which("wine") {
        runners.push(DetectedRunner {
            name: "System Wine".into(),
            path,
            runner_type: RunnerType::Wine,
        });
    }

    // Proton-GE / Wine-GE in common locations
    let home = dirs::home_dir().unwrap_or_default();

    // Steam compatibility tools
    let steam_compat_dirs = [
        home.join(".steam/steam/compatibilitytools.d"),
        home.join(".local/share/Steam/compatibilitytools.d"),
    ];

    for dir in &steam_compat_dirs {
        if dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    // Check for proton
                    let proton_bin = path.join("proton");
                    if proton_bin.exists() {
                        runners.push(DetectedRunner {
                            name: name.clone(),
                            path: proton_bin,
                            runner_type: RunnerType::Proton,
                        });
                        continue;
                    }
                    // Check for wine binary inside
                    let wine_bin = path.join("bin/wine");
                    if wine_bin.exists() {
                        runners.push(DetectedRunner {
                            name,
                            path: wine_bin,
                            runner_type: RunnerType::Wine,
                        });
                    }
                }
            }
        }
    }

    // Runners downloaded by Arclight
    collect_proton_runners(&mut runners, &AppPaths::runners_dir(), "");

    // Official Proton versions are installed into each Steam library's common directory.
    for library in steam::library_dirs() {
        collect_proton_runners(&mut runners, &library.join("steamapps/common"), "Steam ");
    }

    // Lutris runners
    let lutris_wine = home.join(".local/share/lutris/runners/wine");
    if lutris_wine.is_dir()
        && let Ok(entries) = std::fs::read_dir(&lutris_wine)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            let wine_bin = path.join("bin/wine");
            if wine_bin.exists() {
                runners.push(DetectedRunner {
                    name: format!(
                        "Lutris: {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    path: wine_bin,
                    runner_type: RunnerType::Wine,
                });
            }
        }
    }

    // Sort: Proton-GE first, then other Proton, then Wine
    runners.sort_by(|a, b| {
        let score = |r: &DetectedRunner| -> u8 {
            match r.runner_type {
                RunnerType::Proton if r.name.to_lowercase().contains("ge") => 0,
                RunnerType::Proton => 1,
                RunnerType::Wine => 2,
            }
        };
        score(a).cmp(&score(b)).then_with(|| a.name.cmp(&b.name))
    });
    runners.dedup_by(|a, b| same_path(&a.path, &b.path));

    runners
}

fn collect_proton_runners(runners: &mut Vec<DetectedRunner>, directory: &Path, prefix: &str) {
    if let Ok(entries) = std::fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path = entry.path().join("proton");
            if path.is_file() {
                runners.push(DetectedRunner {
                    name: format!("{prefix}{}", entry.file_name().to_string_lossy()),
                    path,
                    runner_type: RunnerType::Proton,
                });
            }
        }
    }
}

fn same_path(first: &Path, second: &Path) -> bool {
    first.canonicalize().unwrap_or_else(|_| first.to_path_buf())
        == second
            .canonicalize()
            .unwrap_or_else(|_| second.to_path_buf())
}

/// Pick the runner used automatically: the best Proton runner. Plain Wine
/// runners are never chosen automatically, only when selected explicitly.
#[must_use]
pub fn automatic_runner(runners: &[DetectedRunner]) -> Option<&DetectedRunner> {
    runners
        .iter()
        .find(|runner| runner.runner_type == RunnerType::Proton)
}

/// Find the best available runner (prefer Proton-GE, then other Proton).
///
/// # Errors
/// Returns an error if no Proton runner is found.
pub fn find_best_runner() -> anyhow::Result<Runner> {
    let runners = detect_runners();

    automatic_runner(&runners).map(Runner::from).ok_or_else(|| {
        anyhow::anyhow!(
            "No Proton runner found. Download Proton-GE from Settings or select a runner manually."
        )
    })
}
