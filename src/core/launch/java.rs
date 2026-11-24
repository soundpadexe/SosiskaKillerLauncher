//! Mojang's own Java runtimes (jre-legacy = 8, java-runtime-alpha = 16,
//! gamma = 17, delta = 21), installed into the launcher's isolated folder.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

use super::download::{fetch_all, get_bytes, Dl};
use super::{Progress, Stage};

const ALL_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[cfg(all(windows, target_arch = "x86_64"))]
const PLATFORM: &str = "windows-x64";
#[cfg(all(windows, target_arch = "aarch64"))]
const PLATFORM: &str = "windows-arm64";
#[cfg(target_os = "linux")]
const PLATFORM: &str = "linux";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const PLATFORM: &str = "mac-os-arm64";
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const PLATFORM: &str = "mac-os";

#[cfg(windows)]
const JAVA_BIN: &str = "bin/javaw.exe";
#[cfg(not(windows))]
const JAVA_BIN: &str = "bin/java";

#[derive(Deserialize)]
struct RuntimeEntry {
    manifest: Ref,
    version: VersionName,
}
#[derive(Deserialize)]
struct Ref {
    url: String,
}
#[derive(Deserialize)]
struct VersionName {
    name: String,
}
#[derive(Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeFile>,
}
#[derive(Deserialize)]
struct RuntimeFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    downloads: Option<RuntimeDownloads>,
}
#[derive(Deserialize)]
struct RuntimeDownloads {
    raw: Raw,
}
#[derive(Deserialize)]
struct Raw {
    url: String,
    sha1: String,
    size: u64,
}

pub async fn ensure(client: &reqwest::Client, root: &Path, component: &str, prog: &Progress) -> anyhow::Result<PathBuf> {
    let dir = root.join(component);
    let java = dir.join(JAVA_BIN);
    let marker = dir.join(".skl-ok");
    if java.exists() && marker.exists() {
        return Ok(java);
    }

    prog.set_stage(Stage::Java);
    let all: HashMap<String, HashMap<String, Vec<RuntimeEntry>>> =
        serde_json::from_slice(&get_bytes(client, ALL_URL).await?).context("java runtime index")?;
    let entry = all
        .get(PLATFORM)
        .and_then(|m| m.get(component))
        .and_then(|v| v.first())
        .with_context(|| format!("no Java runtime '{component}' for {PLATFORM}"))?;
    let manifest: RuntimeManifest = serde_json::from_slice(&get_bytes(client, &entry.manifest.url).await?)?;

    let mut items = Vec::new();
    for (path, f) in manifest.files {
        let target = dir.join(&path);
        match (f.kind.as_str(), f.downloads) {
            ("directory", _) => std::fs::create_dir_all(&target)?,
            ("file", Some(d)) => items.push(Dl { url: d.raw.url, path: target, sha1: Some(d.raw.sha1), size: d.raw.size }),
            _ => {}
        }
    }
    fetch_all(client, items, prog).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::write(&marker, &entry.version.name)?;
    Ok(java)
}
