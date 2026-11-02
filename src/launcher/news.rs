//! Headlines from the public `WoW` news page, for the overlay on the main screen.
//!
//! The page is server-rendered, so the articles are read straight from its HTML.
//! Thumbnails are cached below `AppPaths::news_dir()` and only kept while they are
//! still part of the latest news.
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Utc};

use super::artwork::fnv;
use crate::{
    background,
    config::{AppPaths, NewsLocale},
};

pub const SITE: &str = "https://worldofwarcraft.blizzard.com";
const PAGE_TIMEOUT: Duration = Duration::from_secs(20);
const IMAGE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_ITEMS: usize = 20;

#[derive(Debug, Clone)]
pub struct NewsItem {
    pub title: String,
    pub description: String,
    pub url: String,
    pub published: Option<DateTime<Utc>>,
    pub thumbnail: Option<PathBuf>,
}

impl NewsItem {
    #[must_use]
    pub fn age_label(&self, now: DateTime<Utc>) -> String {
        let Some(published) = self.published else {
            return String::new();
        };
        let age = now.signed_duration_since(published);
        let plural = |count: i64, unit: &str| {
            format!("{count} {unit}{} ago", if count == 1 { "" } else { "s" })
        };
        if age.num_minutes() < 60 {
            "just now".to_owned()
        } else if age.num_hours() < 24 {
            plural(age.num_hours(), "hour")
        } else if age.num_days() < 30 {
            plural(age.num_days(), "day")
        } else {
            published.format("%d %b %Y").to_string()
        }
    }
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let length = text[from..].find(end)?;
    Some(&text[from..from + length])
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let decoded = rest.find(';').filter(|end| *end <= 10).and_then(|end| {
            let character = match &rest[1..end] {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => ' ',
                entity => {
                    let digits = entity.strip_prefix('#')?;
                    let code = match digits.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => digits.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((character, end))
        });
        if let Some((character, end)) = decoded {
            out.push(character);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

fn clean_text(html: &str) -> String {
    let mut plain = String::with_capacity(html.len());
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => plain.push(character),
            _ => {}
        }
    }
    decode_entities(&plain)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[must_use]
pub fn patch_notes_url(locale: NewsLocale) -> String {
    format!("{SITE}/{}/content-update-notes", locale.code())
}

fn article_url(href: &str, locale: NewsLocale) -> Option<String> {
    let href = href.trim();
    let absolute = if href.starts_with('/') {
        format!("{SITE}/{}{href}", locale.code())
    } else {
        href.to_owned()
    };
    let url = reqwest::Url::parse(&absolute).ok()?;
    (url.scheme() == "https" && url.host_str() == Some("worldofwarcraft.blizzard.com"))
        .then(|| url.to_string())
}

fn thumbnail_url(source: &str) -> Option<String> {
    let absolute = if source.starts_with("//") {
        format!("https:{source}")
    } else {
        source.to_owned()
    };
    let url = reqwest::Url::parse(&absolute).ok()?;
    let host = url.host_str()?;
    (url.scheme() == "https"
        && (host.ends_with(".akamaihd.net") || host.ends_with(".akamaized.net")))
    .then(|| url.to_string())
}

/// An article together with the URL of its thumbnail.
struct Parsed {
    item: NewsItem,
    thumbnail_url: Option<String>,
}

fn parse(html: &str, locale: NewsLocale) -> Vec<Parsed> {
    html.split("<article class=\"NewsBlog\">")
        .skip(1)
        .filter_map(|chunk| {
            let article = chunk.split("</article>").next()?;
            let title = clean_text(between(article, "class=\"NewsBlog-title\">", "</div>")?);
            let url = article_url(between(article, "NewsBlog-link\" href=\"", "\"")?, locale)?;
            if title.is_empty() {
                return None;
            }
            let description = between(article, "class=\"NewsBlog-desc", "</p>")
                .and_then(|text| text.split_once('>'))
                .map(|(_, text)| clean_text(text))
                .unwrap_or_default();
            let published = between(article, "iso8601&quot;:&quot;", "&quot;")
                .and_then(|iso| DateTime::parse_from_rfc3339(iso).ok())
                .map(|date| date.with_timezone(&Utc));
            let thumbnail_url = between(article, "data-src=\"", "\"").and_then(thumbnail_url);
            Some(Parsed {
                item: NewsItem {
                    title,
                    description,
                    url,
                    published,
                    thumbnail: None,
                },
                thumbnail_url,
            })
        })
        .take(MAX_ITEMS)
        .collect()
}

const EXTENSIONS: [&str; 3] = ["png", "jpg", "webp"];

/// The CDN does not match file extensions to content, so the real format decides the name.
fn sniffed_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

fn cached_thumbnail(dir: &Path, url: &str) -> Option<PathBuf> {
    EXTENSIONS
        .iter()
        .map(|extension| dir.join(format!("{:016x}.{extension}", fnv(url))))
        .find(|path| path.is_file())
}

async fn download_thumbnail(http: reqwest::Client, url: String, dir: PathBuf) -> Option<PathBuf> {
    if let Some(path) = cached_thumbnail(&dir, &url) {
        return Some(path);
    }
    let bytes = background::deadline("News thumbnail", IMAGE_TIMEOUT, async {
        let bytes = http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        anyhow::ensure!(bytes.len() <= MAX_IMAGE_BYTES, "image is too large");
        Ok(bytes)
    })
    .await
    .map_err(|error| tracing::debug!("News thumbnail {url} unavailable: {error:#}"))
    .ok()?;
    let Some(extension) = sniffed_extension(&bytes) else {
        tracing::debug!("News thumbnail {url} is not a supported image");
        return None;
    };

    tokio::fs::create_dir_all(&dir).await.ok()?;
    let path = dir.join(format!("{:016x}.{extension}", fnv(&url)));
    let temporary = path.with_extension("part");
    tokio::fs::write(&temporary, &bytes).await.ok()?;
    if tokio::fs::rename(&temporary, &path).await.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
        return None;
    }
    Some(path)
}

async fn remove_unused(dir: &Path, keep: &HashSet<PathBuf>) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if !keep.contains(&entry.path()) {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

/// Read the latest news and, when `thumbnails` is set, make sure their images are cached.
///
/// # Errors
/// Returns an error if the news page cannot be loaded or contains no articles.
pub async fn fetch(locale: NewsLocale, thumbnails: bool) -> anyhow::Result<Vec<NewsItem>> {
    let http = reqwest::Client::builder()
        .user_agent(concat!("arclight/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let html = background::deadline("News page", PAGE_TIMEOUT, async {
        Ok(http
            .get(format!("{SITE}/{}/news", locale.code()))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    })
    .await?;
    let parsed = parse(&html, locale);
    anyhow::ensure!(!parsed.is_empty(), "no articles found on the news page");

    let dir = AppPaths::news_dir();
    let mut downloads = tokio::task::JoinSet::new();
    if thumbnails {
        for (index, url) in parsed
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| Some((index, entry.thumbnail_url.clone()?)))
        {
            let (http, dir) = (http.clone(), dir.clone());
            downloads.spawn(async move { (index, download_thumbnail(http, url, dir).await) });
        }
    }
    let mut items: Vec<NewsItem> = parsed.into_iter().map(|entry| entry.item).collect();
    while let Some(Ok((index, path))) = downloads.join_next().await {
        items[index].thumbnail = path;
    }
    let keep: HashSet<PathBuf> = items
        .iter()
        .filter_map(|item| item.thumbnail.clone())
        .collect();
    if thumbnails {
        remove_unused(&dir, &keep).await;
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = concat!(
        "<div><article class=\"NewsBlog\"><div class=\"NewsBlog-tile\">",
        "<img class=\"NewsBlog-image\" src=\"data:image/gif;base64,AAAA\" ",
        "data-src=\"//bnetcmsus-a.akamaihd.net/cms/a/B.png\"/></div>",
        "<div class=\"NewsBlog-title\">Ruf &amp; Ehre &quot;Forever&quot;</div>",
        "<p class=\"NewsBlog-desc color-beige-medium\">Ein <b>neuer</b> Tag&#x27;s Start</p>",
        "<div class=\"NewsBlog-date\" ",
        "data-props=\"{&quot;iso8601&quot;:&quot;2026-10-30T17:01:00.000Z&quot;,",
        "&quot;relative&quot;:true}\">3 days ago</div>",
        "<a class=\"Link NewsBlog-link\" href=\"/news/1/süße-leckerbissen\"></a></article>",
        "<article class=\"NewsBlog\"><div class=\"NewsBlog-title\">No link</div></article></div>",
    );

    #[test]
    fn articles_are_parsed() {
        let parsed = parse(PAGE, NewsLocale::DeDe);

        assert_eq!(parsed.len(), 1);
        let entry = &parsed[0];
        assert_eq!(entry.item.title, "Ruf & Ehre \"Forever\"");
        assert_eq!(entry.item.description, "Ein neuer Tag's Start");
        assert_eq!(
            entry.item.url,
            "https://worldofwarcraft.blizzard.com/de-de/news/1/s%C3%BC%C3%9Fe-leckerbissen"
        );
        assert_eq!(
            entry.thumbnail_url.as_deref(),
            Some("https://bnetcmsus-a.akamaihd.net/cms/a/B.png")
        );
        assert_eq!(
            entry.item.published.unwrap().to_rfc3339(),
            "2026-10-30T17:01:00+00:00"
        );
    }

    #[test]
    fn foreign_hosts_are_rejected() {
        assert!(article_url("https://evil.example/news/1", NewsLocale::DeDe).is_none());
        assert!(thumbnail_url("https://evil.example/a.png").is_none());
        assert!(thumbnail_url("http://x.akamaihd.net/a.png").is_none());
    }

    #[test]
    fn image_format_is_detected_from_content() {
        assert_eq!(sniffed_extension(b"\x89PNG\r\n\x1a\n"), Some("png"));
        assert_eq!(sniffed_extension(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(sniffed_extension(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniffed_extension(b"GIF89a"), None);
    }

    #[test]
    fn links_follow_the_locale() {
        let parsed = parse(PAGE, NewsLocale::FrFr);
        assert!(parsed[0].item.url.contains("/fr-fr/news/1/"));
        assert!(patch_notes_url(NewsLocale::KoKr).ends_with("/ko-kr/content-update-notes"));
    }

    #[test]
    fn system_language_picks_a_locale() {
        assert_eq!(
            NewsLocale::from_system_locale("de_DE.UTF-8"),
            NewsLocale::DeDe
        );
        assert_eq!(
            NewsLocale::from_system_locale("pt_PT.UTF-8"),
            NewsLocale::PtBr
        );
        assert_eq!(
            NewsLocale::from_system_locale("en_US.UTF-8"),
            NewsLocale::EnUs
        );
        assert_eq!(NewsLocale::from_system_locale("C"), NewsLocale::EnGb);
    }
}
