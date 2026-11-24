use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

use crate::models::version::{Era, GameVersion, VersionList};

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const CACHE_NAME: &str = "version_manifest_v2.json";

#[derive(Deserialize)]
struct Manifest {
    latest: Latest,
    versions: Vec<RawVersion>,
}

#[derive(Deserialize)]
struct Latest {
    release: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawVersion {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
    release_time: String,
}

/// Fetches the full manifest (Pre-Classic → latest snapshot).
/// Falls back to the on-disk cache when offline.
pub async fn fetch_versions(client: &reqwest::Client, cache_dir: &Path) -> anyhow::Result<VersionList> {
    let cache = cache_dir.join(CACHE_NAME);

    let bytes = match fetch_remote(client).await {
        Ok(bytes) => {
            // Cache is best-effort; a write failure must not break the UI.
            let _ = tokio::fs::write(&cache, &bytes).await;
            bytes
        }
        Err(net_err) => {
            let bytes = tokio::fs::read(&cache)
                .await
                .map_err(|_| net_err.context("no cached manifest available"))?;
            bytes
        }
    };

    let manifest: Manifest = serde_json::from_slice(&bytes).context("malformed version manifest")?;
    let versions = manifest
        .versions
        .into_iter()
        .map(|v| {
            let date = v.release_time.get(..10).unwrap_or(&v.release_time).to_owned();
            GameVersion {
                era: Era::classify(&v.id, &v.kind),
                year: date.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0),
                id: v.id,
                url: v.url,
                date,
            }
        })
        .collect();

    Ok(VersionList {
        versions,
        latest_release: manifest.latest.release,

    })
}

async fn fetch_remote(client: &reqwest::Client) -> anyhow::Result<Vec<u8>> {
    let resp = client.get(MANIFEST_URL).send().await?.error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}

/// Fetches just the client jar (+ version JSON) for a version — used to read
/// real artwork for covers before the instance is first launched.
pub async fn ensure_client_jar(client: &reqwest::Client, version_url: &str, versions_dir: &Path, id: &str) -> anyhow::Result<std::path::PathBuf> {
    use crate::core::launch::{download, manifest::VersionJson, Progress};
    let dir = versions_dir.join(id);
    let jar = dir.join(format!("{id}.jar"));
    if jar.exists() {
        return Ok(jar);
    }
    std::fs::create_dir_all(&dir)?;
    let bytes = download::get_bytes(client, version_url).await?;
    std::fs::write(dir.join(format!("{id}.json")), &bytes)?;
    let vj: VersionJson = serde_json::from_slice(&bytes)?;
    let c = &vj.downloads.client;
    download::fetch_all(client, vec![download::Dl { url: c.url.clone(), path: jar.clone(), sha1: Some(c.sha1.clone()), size: c.size }], &Progress::new()).await?;
    Ok(jar)
}
