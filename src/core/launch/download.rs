use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;

use anyhow::{bail, Context};
use futures_util::{stream, StreamExt, TryStreamExt};
use tokio::io::AsyncWriteExt;

use super::Progress;

const PARALLEL: usize = 24;

pub struct Dl {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: u64,
}

pub async fn get_bytes(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = client.get(url).send().await?.error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}

/// Downloads everything that is missing (size check), in parallel, verifying SHA-1.
pub async fn fetch_all(client: &reqwest::Client, items: Vec<Dl>, prog: &Progress) -> anyhow::Result<()> {
    let pending: Vec<Dl> = items.into_iter().filter(|d| !is_present(d)).collect();
    prog.total.store(pending.iter().map(|d| d.size).sum(), Relaxed);
    prog.done.store(0, Relaxed);
    stream::iter(pending)
        .map(|d| fetch_one(client, d, prog))
        .buffer_unordered(PARALLEL)
        .try_collect::<Vec<()>>()
        .await?;
    Ok(())
}

fn is_present(d: &Dl) -> bool {
    std::fs::metadata(&d.path).map(|m| d.size == 0 || m.len() == d.size).unwrap_or(false)
}

async fn fetch_one(client: &reqwest::Client, d: Dl, prog: &Progress) -> anyhow::Result<()> {
    let mut last = None;
    for attempt in 0..3u64 {
        match try_fetch(client, &d, prog).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(400 * (attempt + 1))).await;
            }
        }
    }
    Err(last.unwrap()).with_context(|| format!("download failed: {}", d.url))
}

async fn try_fetch(client: &reqwest::Client, d: &Dl, prog: &Progress) -> anyhow::Result<()> {
    if let Some(parent) = d.path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = PathBuf::from(format!("{}.part", d.path.display()));
    let mut got = 0u64;
    let result = async {
        let mut resp = client.get(&d.url).send().await?.error_for_status()?;
        let mut file = tokio::fs::File::create(&tmp).await?;
        let mut hasher = sha1_smol::Sha1::new();
        while let Some(chunk) = resp.chunk().await? {
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            got += chunk.len() as u64;
            prog.done.fetch_add(chunk.len() as u64, Relaxed);
        }
        file.flush().await?;
        drop(file);
        if let Some(expected) = d.sha1.as_deref().filter(|s| !s.is_empty()) {
            if hasher.digest().to_string() != expected {
                bail!("checksum mismatch");
            }
        }
        tokio::fs::rename(&tmp, &d.path).await?;
        anyhow::Ok(())
    }
    .await;
    if result.is_err() {
        prog.done.fetch_sub(got.min(prog.done.load(Relaxed)), Relaxed);
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    result
}
