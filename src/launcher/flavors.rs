//! Find installed `WoW` flavors (`_retail_`, `_classic_era_`, ...) below the install root.
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

use crate::config::GameClient;

/// Split a BPSV table (the format of `.build.info`, `.flavor.info` and the version
/// service) into rows keyed by column name. Columns are looked up by name because
/// Blizzard reorders them between releases.
#[must_use]
pub fn parse_bpsv(text: &str) -> Vec<HashMap<String, String>> {
    let mut columns: Option<Vec<String>> = None;
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split('|');
        match &columns {
            None => {
                columns = Some(
                    fields
                        .map(|field| field.split('!').next().unwrap_or(field).to_owned())
                        .collect(),
                );
            }
            Some(columns) => rows.push(
                columns
                    .iter()
                    .cloned()
                    .zip(fields.map(str::to_owned))
                    .collect(),
            ),
        }
    }
    rows
}

/// Product codes end up in URLs, so only accept what Blizzard actually uses.
#[must_use]
pub fn valid_product(product: &str) -> bool {
    !product.is_empty()
        && product.len() <= 32
        && product
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The flavor folder (`_retail_`) a client's executable lives in.
#[must_use]
pub fn flavor_dir(executable: &Path) -> Option<&str> {
    let mut components = executable.components();
    let Some(Component::Normal(dir)) = components.next() else {
        return None;
    };
    components.next()?;
    dir.to_str().filter(|dir| is_flavor_dir(dir))
}

fn is_flavor_dir(name: &str) -> bool {
    name.len() > 2 && name.starts_with('_') && name.ends_with('_')
}

/// Product code of a flavor folder: from its `.flavor.info`, else a known folder name.
#[must_use]
pub fn product_for_dir(root: &Path, dir: &str) -> Option<String> {
    if !is_flavor_dir(dir) {
        return None;
    }
    std::fs::read_to_string(root.join(dir).join(".flavor.info"))
        .ok()
        .and_then(|text| {
            parse_bpsv(&text)
                .into_iter()
                .find_map(|row| row.get("Product Flavor").cloned())
        })
        .filter(|product| valid_product(product))
        .or_else(|| product_from_dir_name(dir).map(str::to_owned))
}

fn product_from_dir_name(dir: &str) -> Option<&'static str> {
    Some(match dir {
        "_retail_" => "wow",
        "_classic_" => "wow_classic",
        "_classic_era_" => "wow_classic_era",
        "_ptr_" => "wowt",
        "_classic_ptr_" => "wow_classic_ptr",
        "_beta_" => "wow_beta",
        "_classic_beta_" => "wow_classic_beta",
        "_forever_" => "wowf",
        _ => return None,
    })
}

/// Product code for a configured client, if it can be determined.
#[must_use]
pub fn product_for_client(root: &Path, client: &GameClient) -> Option<String> {
    product_for_dir(root, flavor_dir(&client.executable)?)
}

/// A flavor folder found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedFlavor {
    /// Folder name below the install root, e.g. `_forever_`
    pub dir: String,
    /// Executable relative to the install root
    pub executable: PathBuf,
    /// Suggested client name
    pub name: String,
}

impl DetectedFlavor {
    /// Build a client for this flavor with an id and name that do not clash with `existing`.
    #[must_use]
    pub fn to_client(&self, existing: &[GameClient]) -> GameClient {
        let base = self.dir.trim_matches('_').replace('_', "-");
        let id = unique(&base, "-", |candidate| {
            existing.iter().all(|client| client.id != candidate)
        });
        let name = unique(&self.name, " ", |candidate| {
            existing
                .iter()
                .all(|client| !client.name.trim().eq_ignore_ascii_case(candidate))
        });
        GameClient {
            id,
            name,
            executable: self.executable.clone(),
            ..GameClient::default()
        }
    }
}

fn unique(base: &str, separator: &str, is_free: impl Fn(&str) -> bool) -> String {
    if is_free(base) {
        return base.to_owned();
    }
    (2..=u32::MAX)
        .map(|suffix| format!("{base}{separator}{suffix}"))
        .find(|candidate| is_free(candidate))
        .expect("unbounded range")
}

/// Detected flavors that no configured client covers yet.
#[must_use]
pub fn unconfigured<'a>(
    detected: &'a [DetectedFlavor],
    clients: &[GameClient],
) -> Vec<&'a DetectedFlavor> {
    detected
        .iter()
        .filter(|flavor| {
            clients
                .iter()
                .all(|client| flavor_dir(&client.executable) != Some(flavor.dir.as_str()))
        })
        .collect()
}

/// Scan the install root for flavor folders that contain a game executable.
#[must_use]
pub fn detect_flavors(root: &Path) -> Vec<DetectedFlavor> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut flavors: Vec<DetectedFlavor> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let dir = entry.file_name().into_string().ok()?;
            if !is_flavor_dir(&dir) {
                return None;
            }
            let executable = find_executable(&entry.path())?;
            let name = display_name(product_for_dir(root, &dir).as_deref(), &dir);
            Some(DetectedFlavor {
                executable: PathBuf::from(&dir).join(executable),
                dir,
                name,
            })
        })
        .collect();
    flavors.sort_by(|a, b| a.dir.cmp(&b.dir));
    flavors
}

fn find_executable(dir: &Path) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            lower.starts_with("wow")
                && Path::new(&lower)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                && !lower.contains("error")
                && !lower.contains("crash")
        })
        .collect();
    names.sort();
    names.into_iter().next()
}

fn display_name(product: Option<&str>, dir: &str) -> String {
    match product {
        Some("wow") => "Retail".into(),
        Some("wow_classic") => "Classic".into(),
        Some("wow_classic_era") => "Classic Era".into(),
        Some("wowt") => "Retail PTR".into(),
        Some("wow_beta") => "Retail Beta".into(),
        Some("wow_classic_beta") => "Classic Beta".into(),
        Some("wow_classic_ptr") => "Classic PTR".into(),
        Some("wowf") => "Forever".into(),
        _ => dir
            .trim_matches('_')
            .split('_')
            .map(|word| {
                let mut chars = word.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "arclight-flavors-{name}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn bpsv_uses_column_names_and_skips_comments() {
        let rows = parse_bpsv(
            "Region!STRING:0|BuildId!DEC:4|VersionsName!String:0\n## seqn = \
             1\nus|70688|12.1.5.70688\neu|70688|12.1.5.70688\n",
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["Region"], "eu");
        assert_eq!(rows[0]["VersionsName"], "12.1.5.70688");
    }

    #[test]
    fn bpsv_keeps_empty_fields_in_place() {
        let rows = parse_bpsv("A!STRING:0|B!HEX:16|C!DEC:4\nx||3\n");

        assert_eq!(rows[0]["B"], "");
        assert_eq!(rows[0]["C"], "3");
    }

    #[test]
    fn products_are_restricted_to_url_safe_codes() {
        assert!(valid_product("wow_classic_era"));
        assert!(valid_product("wowf"));
        assert!(!valid_product(""));
        assert!(!valid_product("wow/../x"));
        assert!(!valid_product("WOW"));
    }

    #[test]
    fn flavor_dir_is_the_first_folder_of_the_executable() {
        assert_eq!(flavor_dir(Path::new("_retail_/Wow.exe")), Some("_retail_"));
        assert_eq!(flavor_dir(Path::new("Wow.exe")), None);
        assert_eq!(flavor_dir(Path::new("bin/Wow.exe")), None);
    }

    #[test]
    fn product_prefers_flavor_info_over_folder_name() {
        let root = temp_dir("product");
        touch(
            &root.join("_classic_beta_/.flavor.info"),
            "Product Flavor!STRING:0\nwow_classic_beta\n",
        );
        touch(&root.join("_retail_/Wow.exe"), "");
        touch(
            &root.join("_odd_/.flavor.info"),
            "Product Flavor!STRING:0\nwow_custom\n",
        );

        assert_eq!(
            product_for_dir(&root, "_classic_beta_").as_deref(),
            Some("wow_classic_beta")
        );
        assert_eq!(product_for_dir(&root, "_retail_").as_deref(), Some("wow"));
        assert_eq!(
            product_for_dir(&root, "_odd_").as_deref(),
            Some("wow_custom")
        );
        assert_eq!(product_for_dir(&root, "_unknown_"), None);
        assert_eq!(product_for_dir(&root, "Data"), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn detects_flavors_with_executables_and_skips_the_rest() {
        let root = temp_dir("detect");
        touch(&root.join("_forever_/Wow.exe"), "");
        touch(&root.join("_forever_/BlizzardError.exe"), "");
        touch(
            &root.join("_forever_/.flavor.info"),
            "Product Flavor!STRING:0\nwowf\n",
        );
        touch(&root.join("_classic_beta_/WowB.exe"), "");
        touch(&root.join("_classic_beta_/BlizzardError.exe"), "");
        touch(&root.join("_retail_/.flavor.info"), "");
        touch(&root.join("Data/data.001"), "");

        let flavors = detect_flavors(&root);

        assert_eq!(
            flavors,
            [
                DetectedFlavor {
                    dir: "_classic_beta_".into(),
                    executable: "_classic_beta_/WowB.exe".into(),
                    name: "Classic Beta".into(),
                },
                DetectedFlavor {
                    dir: "_forever_".into(),
                    executable: "_forever_/Wow.exe".into(),
                    name: "Forever".into(),
                },
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unconfigured_ignores_flavors_that_already_have_a_client() {
        let detected = [
            DetectedFlavor {
                dir: "_retail_".into(),
                executable: "_retail_/Wow.exe".into(),
                name: "Retail".into(),
            },
            DetectedFlavor {
                dir: "_forever_".into(),
                executable: "_forever_/Wow.exe".into(),
                name: "Forever".into(),
            },
        ];

        let missing = unconfigured(&detected, &[GameClient::retail(), GameClient::classic()]);

        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].dir, "_forever_");
    }

    #[test]
    fn new_clients_get_unique_ids_and_names() {
        let flavor = DetectedFlavor {
            dir: "_classic_era_".into(),
            executable: "_classic_era_/WowClassic.exe".into(),
            name: "Classic Era".into(),
        };

        let first = flavor.to_client(&[GameClient::retail()]);
        assert_eq!(first.id, "classic-era");
        assert_eq!(first.name, "Classic Era");
        assert_eq!(first.executable, Path::new("_classic_era_/WowClassic.exe"));

        let second = flavor.to_client(&[GameClient::retail(), first]);
        assert_eq!(second.id, "classic-era-2");
        assert_eq!(second.name, "Classic Era 2");
    }
}
