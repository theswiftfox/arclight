use std::path::PathBuf;

/// Return Steam's primary install directories and all configured library directories.
#[must_use]
pub fn library_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let roots = [home.join(".steam/steam"), home.join(".local/share/Steam")];
    let mut libraries = roots.to_vec();

    for root in roots {
        if let Ok(content) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            for path in vdf_paths(&content) {
                let path = PathBuf::from(path);
                if !libraries.contains(&path) {
                    libraries.push(path);
                }
            }
        }
    }

    libraries
}

fn vdf_paths(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let values = line
                .split('"')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>();
            (values.first()? == &"path")
                .then(|| values.get(1).map(|value| value.replace("\\\\", "\\")))
                .flatten()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_paths() {
        let vdf = "\"path\" \"/mnt/games/SteamLibrary\"";
        assert_eq!(vdf_paths(vdf), vec!["/mnt/games/SteamLibrary"]);
    }
}
