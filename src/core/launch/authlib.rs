//! authlib-injector (MIT, github.com/yushijinhun/authlib-injector): a Java agent
//! that redirects Mojang's authlib to a custom Yggdrasil API — our local skin server.
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Deserialize;
use sha2::Digest;

use super::download::get_bytes;

const META: &str = "https://authlib-injector.yushi.moe/artifact/latest.json";

#[derive(Deserialize)]
struct Meta {
    download_url: String,
    checksums: Checksums,
}
#[derive(Deserialize)]
struct Checksums {
    sha256: String,
}

fn sha256_hex(b: &[u8]) -> String {
    sha2::Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// Returns the agent jar, updating it when a newer verified build exists.
/// Offline: falls back to the copy already on disk.
pub async fn ensure(client: &reqwest::Client, dir: &Path) -> anyhow::Result<PathBuf> {
    let jar = dir.join("authlib-injector.jar");
    let meta = async { anyhow::Ok(serde_json::from_slice::<Meta>(&get_bytes(client, META).await?)?) }.await;
    let meta = match meta {
        Ok(m) => m,
        Err(e) if jar.exists() => {
            let _ = e;
            return Ok(jar);
        }
        Err(e) => return Err(e.context("authlib-injector metadata")),
    };
    if std::fs::read(&jar).map(|b| sha256_hex(&b) == meta.checksums.sha256).unwrap_or(false) {
        return Ok(jar);
    }
    let bytes = get_bytes(client, &meta.download_url).await.context("authlib-injector download")?;
    if sha256_hex(&bytes) != meta.checksums.sha256 {
        bail!("authlib-injector checksum mismatch");
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(&jar, bytes)?;
    Ok(jar)
}
