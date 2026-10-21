//! Download and install the latest Proton-GE release into Arclight's data directory.
use std::{path::PathBuf, time::Duration};

use anyhow::Context;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::{
    background::{self, IO_TIMEOUT},
    config::AppPaths,
};

const LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases/latest";
const API_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(30);
const EXTRACT_TIMEOUT: Duration = Duration::from_mins(5);

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("arclight/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// Download the latest Proton-GE release, verify its checksum, and extract it
/// into `AppPaths::runners_dir()`. Returns the path to the installed `proton` script.
///
/// # Errors
/// Returns an error if the release cannot be fetched, the checksum does not
/// match, or the archive cannot be extracted.
pub async fn install_latest() -> anyhow::Result<PathBuf> {
    let client = client()?;
    let release: Release = background::deadline("Proton-GE release lookup", API_TIMEOUT, async {
        Ok(client
            .get(LATEST_RELEASE_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    })
    .await?;

    let find_asset = |extension: &str| {
        let arch = std::env::consts::ARCH;
        [
            format!("{}-{arch}.{extension}", release.tag_name),
            format!("{}.{extension}", release.tag_name),
        ]
        .into_iter()
        .find_map(|name| release.assets.iter().find(|asset| asset.name == name))
    };
    let archive = find_asset("tar.gz").with_context(|| {
        format!(
            "release {} has no {} archive",
            release.tag_name,
            std::env::consts::ARCH
        )
    })?;
    let checksum = find_asset("sha512sum")
        .with_context(|| format!("release {} has no checksum file", release.tag_name))?;

    let runners_dir = AppPaths::runners_dir();
    let final_dir = runners_dir.join(&release.tag_name);
    let proton = final_dir.join("proton");
    if proton.is_file() {
        return Ok(proton);
    }

    let staging = runners_dir.join(format!(".download-{}", release.tag_name));
    let result = download_and_extract(&client, archive, checksum, &staging, &final_dir).await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    result?;

    if !proton.is_file() {
        anyhow::bail!("{} did not contain a proton script", release.tag_name);
    }
    Ok(proton)
}

async fn download_and_extract(
    client: &reqwest::Client,
    archive: &Asset,
    checksum: &Asset,
    staging: &std::path::Path,
    final_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let _ = tokio::fs::remove_dir_all(staging).await;
    tokio::fs::create_dir_all(staging).await?;

    let checksum_text = background::deadline("Proton-GE checksum download", API_TIMEOUT, async {
        Ok(client
            .get(&checksum.browser_download_url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    })
    .await?;
    // Rewrite so the check does not depend on the filename layout upstream.
    let expected = checksum_text
        .split_whitespace()
        .next()
        .context("checksum file was empty")?;
    tokio::fs::write(
        staging.join("archive.sha512sum"),
        format!("{expected}  archive.tar.gz\n"),
    )
    .await?;

    let archive_path = staging.join("archive.tar.gz");
    background::deadline("Proton-GE download", DOWNLOAD_TIMEOUT, async {
        let mut response = client
            .get(&archive.browser_download_url)
            .send()
            .await?
            .error_for_status()?;
        let mut file = tokio::fs::File::create(&archive_path).await?;
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(())
    })
    .await?;

    let mut verify = tokio::process::Command::new("sha512sum");
    verify
        .args(["--check", "--status", "archive.sha512sum"])
        .current_dir(staging);
    background::command_status(verify, "Proton-GE checksum verification", IO_TIMEOUT * 6)
        .await
        .context("downloaded Proton-GE archive failed checksum verification")?;

    let extract_dir = staging.join("extracted");
    tokio::fs::create_dir_all(&extract_dir).await?;
    let mut tar = tokio::process::Command::new("tar");
    tar.arg("-xzf")
        .arg(&archive_path)
        .arg("-C")
        .arg(&extract_dir);
    background::command_status(tar, "Proton-GE extraction", EXTRACT_TIMEOUT).await?;

    // The archive contains a single `<tag>/` directory.
    let mut entries = tokio::fs::read_dir(&extract_dir).await?;
    let extracted = entries
        .next_entry()
        .await?
        .context("Proton-GE archive was empty")?
        .path();
    if let Some(parent) = final_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::rename(&extracted, final_dir).await?;
    Ok(())
}
