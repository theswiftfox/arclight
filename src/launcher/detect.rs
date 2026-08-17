use std::{fmt, path::PathBuf};

use super::Runner;
use crate::config::RunnerType;

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
        score(a).cmp(&score(b))
    });

    runners
}

/// Find the best available runner (prefer Proton-GE, then system Wine).
///
/// # Errors
/// Returns an error if no Wine or Proton runner is found on the system.
pub fn find_best_runner() -> anyhow::Result<Runner> {
    let runners = detect_runners();

    runners.first().map(Runner::from).ok_or_else(|| {
        anyhow::anyhow!("No Wine or Proton runner found. Please install Wine or Proton-GE.")
    })
}
