//! Read-only version check: what is installed (`.build.info`) against the latest
//! build Blizzard's public version service reports. Nothing is downloaded or changed.
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;

use super::flavors::{parse_bpsv, product_for_client, valid_product};
use crate::{
    background::{self, IO_TIMEOUT},
    config::{GameClient, Region},
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Latest versions are reused for this long before the service is asked again.
pub const CACHE_TTL: Duration = Duration::from_mins(30);

/// `(branch, product)` as used in `https://<branch>.version.battle.net/<product>/versions`.
pub type LatestKey = (String, String);

/// One product row of `.build.info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub product: String,
    pub branch: String,
    pub version: String,
}

/// The newest build the version service lists for a product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub version: String,
    pub build_id: u64,
}

/// What the UI shows for one client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientVersion {
    pub installed: String,
    /// `None` when the latest version is unknown (check disabled or failed)
    pub latest: Option<String>,
    pub outdated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub clients: HashMap<String, ClientVersion>,
    /// Latest versions fetched by this run, for the caller's cache
    pub fetched: HashMap<LatestKey, Latest>,
}

/// Build number: the last component of a version such as `1.15.9.70576`.
fn build_of(version: &str) -> Option<u64> {
    version.rsplit('.').next()?.parse().ok()
}

fn valid_branch(branch: &str) -> bool {
    branch.len() == 2 && branch.bytes().all(|byte| byte.is_ascii_lowercase())
}

#[must_use]
pub fn installed_from_build_info(text: &str) -> Vec<Installed> {
    parse_bpsv(text)
        .into_iter()
        .filter(|row| row.get("Active").is_none_or(|active| active != "0"))
        .filter_map(|row| {
            let installed = Installed {
                product: row.get("Product")?.clone(),
                branch: row.get("Branch")?.to_ascii_lowercase(),
                version: row.get("Version")?.clone(),
            };
            (valid_product(&installed.product)
                && valid_branch(&installed.branch)
                && !installed.version.is_empty())
            .then_some(installed)
        })
        .collect()
}

/// Installed products from `<root>/.build.info`; empty when the file does not exist.
///
/// # Errors
/// Returns an error if the file exists but cannot be read.
pub fn read_build_info(root: &Path) -> anyhow::Result<Vec<Installed>> {
    match std::fs::read_to_string(root.join(".build.info")) {
        Ok(text) => Ok(installed_from_build_info(&text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).context("reading .build.info"),
    }
}

/// The row for `product`, preferring the configured region's branch. Some products
/// are single-region (the Classic beta is `us` only), so any active row is accepted.
#[must_use]
pub fn pick_installed<'a>(
    all: &'a [Installed],
    product: &str,
    region: Region,
) -> Option<&'a Installed> {
    let mut rows = all.iter().filter(|row| row.product == product);
    let first = rows.next()?;
    std::iter::once(first)
        .chain(rows)
        .find(|row| row.branch == region.code())
        .or(Some(first))
}

/// # Errors
/// Returns an error if the response has no row for `branch` or the row is incomplete.
pub fn parse_latest(body: &str, branch: &str) -> anyhow::Result<Latest> {
    let row = parse_bpsv(body)
        .into_iter()
        .find(|row| row.get("Region").is_some_and(|region| region == branch))
        .with_context(|| format!("version service has no '{branch}' entry"))?;
    let version = row
        .get("VersionsName")
        .filter(|version| !version.is_empty())
        .context("version service entry has no version name")?
        .clone();
    let build_id = row
        .get("BuildId")
        .and_then(|id| id.parse().ok())
        .or_else(|| build_of(&version))
        .context("version service entry has no build id")?;
    Ok(Latest { version, build_id })
}

async fn fetch_latest(
    client: &reqwest::Client,
    branch: &str,
    product: &str,
) -> anyhow::Result<Latest> {
    anyhow::ensure!(
        valid_branch(branch) && valid_product(product),
        "invalid version lookup"
    );
    let url = format!("https://{branch}.version.battle.net/{product}/versions");
    let body = background::deadline("Version lookup", REQUEST_TIMEOUT, async {
        Ok(client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    })
    .await?;
    parse_latest(&body, branch)
}

fn compare(installed: &str, latest: Option<&Latest>) -> ClientVersion {
    let outdated = latest
        .zip(build_of(installed))
        .is_some_and(|(latest, installed)| latest.build_id > installed);
    ClientVersion {
        installed: installed.to_owned(),
        latest: latest.map(|latest| latest.version.clone()),
        outdated,
    }
}

struct Resolved {
    client_id: String,
    installed: Installed,
}

fn resolve_installed(
    root: &Path,
    clients: &[GameClient],
    region: Region,
) -> anyhow::Result<Vec<Resolved>> {
    let installed = read_build_info(root)?;
    Ok(clients
        .iter()
        .filter_map(|client| {
            let product = product_for_client(root, client)?;
            let row = pick_installed(&installed, &product, region)?;
            Some(Resolved {
                client_id: client.id.clone(),
                installed: row.clone(),
            })
        })
        .collect())
}

/// Look up installed and latest versions for every configured client.
///
/// `fresh` holds latest versions that are still recent enough to reuse. With
/// `network` off, only installed versions are read.
///
/// # Errors
/// Returns an error if `.build.info` cannot be read. Failed version lookups do
/// not fail the check; those clients just have no latest version.
pub async fn check<S: std::hash::BuildHasher>(
    root: PathBuf,
    clients: Vec<GameClient>,
    region: Region,
    network: bool,
    fresh: HashMap<LatestKey, Latest, S>,
) -> anyhow::Result<Report> {
    let resolved = background::blocking("Reading installed versions", IO_TIMEOUT, move || {
        resolve_installed(&root, &clients, region)
    })
    .await?;

    let mut latest = fresh;
    let mut fetched = HashMap::new();
    if network {
        let missing: Vec<LatestKey> = resolved
            .iter()
            .map(|row| (row.installed.branch.clone(), row.installed.product.clone()))
            .filter(|key| !latest.contains_key(key))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if !missing.is_empty() {
            let http = reqwest::Client::builder()
                .user_agent(concat!("arclight/", env!("CARGO_PKG_VERSION")))
                .build()?;
            let lookups = missing.iter().map(|(branch, product)| {
                let http = &http;
                async move { fetch_latest(http, branch, product).await }
            });
            for (key, result) in missing
                .iter()
                .zip(iced::futures::future::join_all(lookups).await)
            {
                match result {
                    Ok(version) => {
                        latest.insert(key.clone(), version.clone());
                        fetched.insert(key.clone(), version);
                    }
                    Err(error) => {
                        tracing::warn!("Latest version of {} unavailable: {error:#}", key.1);
                    }
                }
            }
        }
    }

    let clients = resolved
        .into_iter()
        .map(|row| {
            let key = (row.installed.branch.clone(), row.installed.product.clone());
            let latest = network.then(|| latest.get(&key)).flatten();
            (row.client_id, compare(&row.installed.version, latest))
        })
        .collect();
    Ok(Report { clients, fetched })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILD_INFO: &str = "Branch!STRING:0|Active!DEC:1|Build Key!HEX:16|CDN Key!HEX:16|Install Key!HEX:16|IM Size!DEC:4|CDN Path!STRING:0|CDN Hosts!STRING:0|CDN Servers!STRING:0|Tags!STRING:0|Armadillo!STRING:0|Last Activated!STRING:0|Version!STRING:0|KeyRing!HEX:16|Product!STRING:0\n\
eu|1|5604ce35|c74a0a5a|||tpr/wow|level3.blizzard.com|http://level3.blizzard.com/|Windows x86_64 EU? acct-DEU?|||1.15.9.70576||wow_classic_era\n\
us|1|ad1802f4|c74a0a5a|||tpr/wow|level3.blizzard.com|http://level3.blizzard.com/|Windows x86_64 US?|||1.60.1.70532||wow_classic_beta\n\
eu|1|7feb61f3|c74a0a5a|||tpr/wow|level3.blizzard.com|http://level3.blizzard.com/|Windows x86_64 EU?|||1.60.1.70707||wowf\n\
us|0|00000000|c74a0a5a|||tpr/wow|level3.blizzard.com|http://level3.blizzard.com/|Windows x86_64 US?|||11.0.0.1||wow\n";

    const VERSIONS: &str = "Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|KeyRing!HEX:\
                            16|BuildId!DEC:4|VersionsName!String:0|ProductConfig!HEX:16\n## seqn \
                            = 4111171\nus|5604ce35|c74a0a5a||70576|1.15.9.70576|c9934edf\\
                            neu|5604ce35|c74a0a5a||70580|1.15.9.70580|c9934edf\n";

    fn latest(version: &str) -> Latest {
        Latest {
            version: version.into(),
            build_id: build_of(version).unwrap(),
        }
    }

    #[test]
    fn build_info_lists_active_products() {
        let installed = installed_from_build_info(BUILD_INFO);

        let products: Vec<_> = installed.iter().map(|row| row.product.as_str()).collect();
        assert_eq!(products, ["wow_classic_era", "wow_classic_beta", "wowf"]);
        assert_eq!(installed[2].version, "1.60.1.70707");
        assert_eq!(installed[2].branch, "eu");
    }

    #[test]
    fn single_region_products_match_despite_the_configured_region() {
        let installed = installed_from_build_info(BUILD_INFO);

        let beta = pick_installed(&installed, "wow_classic_beta", Region::EU).unwrap();
        assert_eq!(beta.branch, "us");
        assert!(pick_installed(&installed, "wow", Region::EU).is_none());
    }

    #[test]
    fn configured_region_wins_when_a_product_has_several_rows() {
        let text = "Branch!STRING:0|Active!DEC:1|Version!STRING:0|Product!STRING:0\nus|1|1.0.0.\
                    100|wow\neu|1|1.0.0.200|wow\n";
        let installed = installed_from_build_info(text);

        assert_eq!(
            pick_installed(&installed, "wow", Region::EU)
                .unwrap()
                .version,
            "1.0.0.200"
        );
        assert_eq!(
            pick_installed(&installed, "wow", Region::KR)
                .unwrap()
                .version,
            "1.0.0.100"
        );
    }

    #[test]
    fn rows_with_unsafe_values_are_ignored() {
        let text = "Branch!STRING:0|Active!DEC:1|Version!STRING:0|Product!STRING:0\neu|1|1.0.0.1|.\
                    ./evil\neu.evil.com|1|1.0.0.1|wow\n";

        assert!(installed_from_build_info(text).is_empty());
    }

    #[test]
    fn latest_is_read_for_the_requested_branch() {
        assert_eq!(
            parse_latest(VERSIONS, "eu").unwrap(),
            latest("1.15.9.70580")
        );
        assert_eq!(parse_latest(VERSIONS, "us").unwrap().build_id, 70576);
        assert!(parse_latest(VERSIONS, "kr").is_err());
        assert!(parse_latest("not a table", "eu").is_err());
    }

    #[test]
    fn newer_latest_build_is_outdated() {
        let status = compare("1.15.9.70576", Some(&latest("1.15.9.70580")));

        assert!(status.outdated);
        assert_eq!(status.latest.as_deref(), Some("1.15.9.70580"));
    }

    #[test]
    fn equal_or_newer_installed_builds_are_not_outdated() {
        assert!(!compare("1.15.9.70580", Some(&latest("1.15.9.70580"))).outdated);
        assert!(!compare("1.60.1.70707", Some(&latest("1.60.1.70532"))).outdated);
    }

    #[test]
    fn unknown_latest_is_never_outdated() {
        let status = compare("1.15.9.70576", None);

        assert!(!status.outdated);
        assert_eq!(status.latest, None);
        assert!(!compare("garbage", Some(&latest("1.15.9.70580"))).outdated);
    }
}
