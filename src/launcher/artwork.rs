//! Official hero artwork from the public `WoW` game pages.
//!
//! Each game page carries a masthead image on Blizzard's content CDN. The URLs
//! contain content hashes, so the page is read to find the current one. The page
//! is checked about weekly and an image is only downloaded when its URL changed.
//! Everything is cached below `AppPaths::art_dir()`; failures keep what is cached.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::flavors::product_for_client;
use crate::{
    background::{self, IO_TIMEOUT},
    config::{AppPaths, GameClient},
};

const SITE: &str = "https://worldofwarcraft.blizzard.com/en-gb";
const IMAGE_HOST: &str = "https://blz-contentstack-images.akamaized.net/";
const REFRESH_AFTER: Duration = Duration::from_hours(168);
const PAGE_TIMEOUT: Duration = Duration::from_secs(20);
const IMAGE_TIMEOUT: Duration = Duration::from_mins(1);
const MAX_IMAGE_BYTES: u64 = 12 * 1024 * 1024;
const MIN_WIDTH: u32 = 1200;

/// Where the artwork of a product lives: a game page and part of the image file name.
struct Source {
    product: &'static str,
    page: &'static str,
    file_name_contains: &'static str,
}

const SOURCES: &[Source] = &[
    Source {
        product: "wow",
        page: "modern",
        file_name_contains: "Homepage_Masthead.",
    },
    Source {
        product: "wow_classic",
        page: "classic",
        file_name_contains: "classic-mop-bg.",
    },
    Source {
        product: "wow_classic_era",
        page: "classic",
        file_name_contains: "classic-bg.",
    },
    Source {
        product: "wowf",
        page: "forever",
        file_name_contains: "masthead-art.",
    },
];

/// PTR and beta flavors share the art of the product they test.
fn base_product(product: &str) -> &str {
    match product {
        "wowt" | "wow_beta" | "wowxptr" => "wow",
        // The Classic beta slot currently carries the 1.60 line that WoW: Forever uses.
        "wow_classic_beta" => "wowf",
        other => other
            .strip_suffix("_beta")
            .or_else(|| other.strip_suffix("_ptr"))
            .unwrap_or(other),
    }
}

fn source_for(product: &str) -> Option<&'static Source> {
    let base = base_product(product);
    SOURCES.iter().find(|source| source.product == base)
}

/// Image URLs on the content CDN found in a page, in order of appearance.
fn extract_image_urls(html: &str) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    for (start, _) in html.match_indices(IMAGE_HOST) {
        let rest = &html[start..];
        let end = rest
            .find(['"', '\'', ')', '\\', '<', '>', ' ', '\n', ',', '&'])
            .unwrap_or(rest.len());
        let url = &rest[..end];
        let path = url.split('?').next().unwrap_or(url).to_ascii_lowercase();
        let is_image = [".jpg", ".jpeg", ".png", ".webp"]
            .iter()
            .any(|extension| path.ends_with(extension));
        if is_image && !urls.iter().any(|known| known == url) {
            urls.push(url.to_owned());
        }
    }
    urls
}

fn pick_url<'a>(urls: &'a [String], source: &Source) -> Option<&'a str> {
    urls.iter()
        .map(String::as_str)
        .find(|url| file_name(url).contains(source.file_name_contains))
}

fn file_name(url: &str) -> &str {
    let path = url.split('?').next().unwrap_or(url);
    path.rsplit('/').next().unwrap_or(path)
}

fn extension(url: &str) -> &'static str {
    let name = file_name(url).to_ascii_lowercase();
    match std::path::Path::new(&name)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
    {
        Some("png") => "png",
        Some("webp") => "webp",
        _ => "jpg",
    }
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    url: String,
    file: String,
    /// Seconds since the epoch of the last successful page check
    checked: u64,
}

type Index = HashMap<String, Entry>;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn index_path(dir: &Path) -> PathBuf {
    dir.join("index.json")
}

fn load_index(dir: &Path) -> Index {
    std::fs::read_to_string(index_path(dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_index(dir: &Path, index: &Index) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let temporary = dir.join("index.json.tmp");
    std::fs::write(&temporary, serde_json::to_string_pretty(index)?)?;
    std::fs::rename(temporary, index_path(dir))?;
    Ok(())
}

fn is_stale(entry: Option<&Entry>, dir: &Path, now: u64) -> bool {
    entry.is_none_or(|entry| {
        !dir.join(&entry.file).is_file()
            || now.saturating_sub(entry.checked) >= REFRESH_AFTER.as_secs()
    })
}

/// Cached artwork path for each `(client id, product)`, without touching the network.
fn cached_paths(
    dir: &Path,
    index: &Index,
    clients: &[(String, String)],
) -> HashMap<String, PathBuf> {
    clients
        .iter()
        .filter_map(|(client_id, product)| {
            let entry = index.get(source_for(product)?.product)?;
            let path = dir.join(&entry.file);
            path.is_file().then(|| (client_id.clone(), path))
        })
        .collect()
}

/// `(client id, product)` for every configured client whose product is known.
fn client_products(root: &Path, clients: &[GameClient]) -> Vec<(String, String)> {
    clients
        .iter()
        .filter_map(|client| Some((client.id.clone(), product_for_client(root, client)?)))
        .collect()
}

/// Artwork that is already on disk.
///
/// # Errors
/// Returns an error if the background worker fails.
pub async fn cached(
    root: PathBuf,
    clients: Vec<GameClient>,
) -> anyhow::Result<HashMap<String, PathBuf>> {
    background::blocking("Reading cached artwork", IO_TIMEOUT, move || {
        let dir = AppPaths::art_dir();
        let products = client_products(&root, &clients);
        Ok(cached_paths(&dir, &load_index(&dir), &products))
    })
    .await
}

/// Check the game pages for new artwork (at most weekly per page) and return the
/// artwork path of every client that has one.
///
/// # Errors
/// Returns an error if the client list cannot be read. Network failures only
/// leave the previously cached artwork in place.
pub async fn refresh(
    root: PathBuf,
    clients: Vec<GameClient>,
) -> anyhow::Result<HashMap<String, PathBuf>> {
    let dir = AppPaths::art_dir();
    let (products, mut index) = {
        let dir = dir.clone();
        background::blocking("Reading cached artwork", IO_TIMEOUT, move || {
            Ok((client_products(&root, &clients), load_index(&dir)))
        })
        .await?
    };

    let now = now_secs();
    let mut stale: Vec<&'static Source> = Vec::new();
    for (_, product) in &products {
        if let Some(source) = source_for(product)
            && is_stale(index.get(source.product), &dir, now)
            && !stale.iter().any(|known| known.product == source.product)
        {
            stale.push(source);
        }
    }

    if !stale.is_empty() {
        let http = reqwest::Client::builder()
            .user_agent(concat!("arclight/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let mut pages: HashMap<&str, Vec<String>> = HashMap::new();
        for source in stale {
            if !pages.contains_key(source.page) {
                match fetch_page_urls(&http, source.page).await {
                    Ok(urls) => {
                        pages.insert(source.page, urls);
                    }
                    Err(error) => {
                        tracing::warn!("Artwork page '{}' unavailable: {error:#}", source.page);
                        continue;
                    }
                }
            }
            let urls = &pages[source.page];
            if let Err(error) = update_source(&http, &dir, &mut index, source, urls, now).await {
                tracing::warn!("Artwork for {} not updated: {error:#}", source.product);
            }
        }
    }

    Ok(cached_paths(&dir, &index, &products))
}

async fn fetch_page_urls(http: &reqwest::Client, page: &str) -> anyhow::Result<Vec<String>> {
    let url = format!("{SITE}/{page}");
    let html = background::deadline("Artwork page", PAGE_TIMEOUT, async {
        Ok(http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    })
    .await?;
    Ok(extract_image_urls(&html))
}

async fn update_source(
    http: &reqwest::Client,
    dir: &Path,
    index: &mut Index,
    source: &Source,
    urls: &[String],
    now: u64,
) -> anyhow::Result<()> {
    let url = pick_url(urls, source).context("no matching image on the page")?;
    if let Some(entry) = index.get_mut(source.product)
        && entry.url == url
        && dir.join(&entry.file).is_file()
    {
        entry.checked = now;
        return save_index(dir, index);
    }

    let file = format!(
        "{}-{:08x}.{}",
        source.product,
        fnv(url) & 0xFFFF_FFFF,
        extension(url)
    );
    download_image(http, url, dir, &file).await?;
    let previous = index.insert(
        source.product.to_owned(),
        Entry {
            url: url.to_owned(),
            file: file.clone(),
            checked: now,
        },
    );
    save_index(dir, index)?;
    if let Some(previous) = previous
        && previous.file != file
    {
        let _ = tokio::fs::remove_file(dir.join(previous.file)).await;
    }
    Ok(())
}

async fn download_image(
    http: &reqwest::Client,
    url: &str,
    dir: &Path,
    file: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(url.starts_with(IMAGE_HOST), "unexpected image host");
    let bytes = background::deadline("Artwork download", IMAGE_TIMEOUT, async {
        let response = http.get(url).send().await?.error_for_status()?;
        anyhow::ensure!(
            response
                .content_length()
                .is_none_or(|length| length <= MAX_IMAGE_BYTES),
            "image is too large"
        );
        let bytes = response.bytes().await?;
        anyhow::ensure!(bytes.len() as u64 <= MAX_IMAGE_BYTES, "image is too large");
        Ok(bytes)
    })
    .await?;

    tokio::fs::create_dir_all(dir).await?;
    let temporary = dir.join(format!("{file}.part"));
    tokio::fs::write(&temporary, &bytes).await?;
    let check = temporary.clone();
    let valid = background::blocking("Checking artwork", IO_TIMEOUT, move || {
        let (width, height) = image::ImageReader::open(&check)?
            .with_guessed_format()?
            .into_dimensions()?;
        Ok(width >= MIN_WIDTH && width > height)
    })
    .await;
    if !matches!(valid, Ok(true)) {
        let _ = tokio::fs::remove_file(&temporary).await;
        anyhow::bail!("downloaded image is not a wide landscape picture");
    }
    tokio::fs::rename(&temporary, dir.join(file)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = concat!(
        "<meta property=\"og:image\" content=\"",
        "https://blz-contentstack-images.akamaized.net/v3/assets/a/b/c/thumb.jpg\">",
        "<img src=\"https://blz-contentstack-images.akamaized.net/v3/assets/a/b/c/",
        "masthead-art-md.jpg\">",
        "{\"src\":\"https://blz-contentstack-images.akamaized.net/v3/assets/a/b/d/",
        "masthead-art.jpg?width=2600\"}",
        "<a href=\"https://example.com/masthead-art.jpg\">",
        "<img src=\"https://blz-contentstack-images.akamaized.net/v3/assets/a/b/c/",
        "thumb.jpg\">",
    );

    #[test]
    fn urls_inside_escaped_css_are_extracted() {
        let html = concat!(
            "style=\"--bg:url(&quot;https://blz-contentstack-images.akamaized.net/v3/",
            "assets/a/b/classic-bg.png&quot;);--sm:url(&quot;",
            "https://blz-contentstack-images.akamaized.net/v3/assets/a/b/classic-bg-sm.png",
            "&quot;)\"",
        );

        let urls = extract_image_urls(html);

        assert_eq!(urls.len(), 2);
        assert_eq!(
            pick_url(&urls, source_for("wow_classic_era").unwrap()),
            Some("https://blz-contentstack-images.akamaized.net/v3/assets/a/b/classic-bg.png")
        );
    }

    fn forever() -> &'static Source {
        source_for("wowf").unwrap()
    }

    #[test]
    fn image_urls_are_extracted_once_from_the_content_cdn_only() {
        let urls = extract_image_urls(PAGE);

        assert_eq!(urls.len(), 3);
        assert!(urls.iter().all(|url| url.starts_with(IMAGE_HOST)));
        assert!(urls[2].ends_with("masthead-art.jpg?width=2600"));
    }

    #[test]
    fn the_matching_image_is_picked_by_file_name() {
        let urls = extract_image_urls(PAGE);

        assert_eq!(
            file_name(pick_url(&urls, forever()).unwrap()),
            "masthead-art.jpg"
        );
        let missing = Source {
            product: "x",
            page: "x",
            file_name_contains: "nothing-like-this.",
        };
        assert_eq!(pick_url(&urls, &missing), None);
    }

    #[test]
    fn size_variants_do_not_match_the_full_size_pattern() {
        let urls = vec![
            format!("{IMAGE_HOST}v3/x/masthead-art-md.jpg"),
            format!("{IMAGE_HOST}v3/x/background-masthead-art-sm.png"),
            format!("{IMAGE_HOST}v3/x/classic-bg-sm.png"),
        ];

        assert_eq!(pick_url(&urls, forever()), None);
        assert_eq!(
            pick_url(&urls, source_for("wow_classic_era").unwrap()),
            None
        );
    }

    #[test]
    fn test_flavors_share_the_art_of_the_product_they_test() {
        assert_eq!(source_for("wowt").unwrap().product, "wow");
        assert_eq!(
            source_for("wow_classic_ptr").unwrap().product,
            "wow_classic"
        );
        assert_eq!(source_for("wow_classic_beta").unwrap().product, "wowf");
        assert_eq!(
            source_for("wow_classic_era").unwrap().product,
            "wow_classic_era"
        );
        assert!(source_for("hs_beta").is_none());
    }

    #[test]
    fn extensions_come_from_the_url_path() {
        assert_eq!(extension("https://h/a/x.PNG?w=1"), "png");
        assert_eq!(extension("https://h/a/x.webp"), "webp");
        assert_eq!(extension("https://h/a/x.jpeg"), "jpg");
    }

    #[test]
    fn staleness_follows_age_and_missing_files() {
        let dir = std::env::temp_dir().join(format!("arclight-art-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.jpg"), b"x").unwrap();
        let entry = |file: &str, checked: u64| Entry {
            url: "u".into(),
            file: file.into(),
            checked,
        };
        let week = REFRESH_AFTER.as_secs();

        assert!(is_stale(None, &dir, 1_000_000));
        assert!(!is_stale(
            Some(&entry("a.jpg", 1_000_000)),
            &dir,
            1_000_000 + week - 1
        ));
        assert!(is_stale(
            Some(&entry("a.jpg", 1_000_000)),
            &dir,
            1_000_000 + week
        ));
        assert!(is_stale(
            Some(&entry("gone.jpg", 1_000_000)),
            &dir,
            1_000_000
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cached_paths_only_include_clients_with_existing_files() {
        let dir = std::env::temp_dir().join(format!("arclight-art-cached-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wowf-1.jpg"), b"x").unwrap();
        let mut index = Index::new();
        index.insert(
            "wowf".into(),
            Entry {
                url: "u".into(),
                file: "wowf-1.jpg".into(),
                checked: 0,
            },
        );
        index.insert(
            "wow".into(),
            Entry {
                url: "u".into(),
                file: "missing.jpg".into(),
                checked: 0,
            },
        );
        let clients = vec![
            ("forever".to_owned(), "wowf".to_owned()),
            ("retail".to_owned(), "wow".to_owned()),
            ("other".to_owned(), "hs_beta".to_owned()),
        ];

        let paths = cached_paths(&dir, &index, &clients);

        assert_eq!(paths.len(), 1);
        assert_eq!(paths["forever"], dir.join("wowf-1.jpg"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
