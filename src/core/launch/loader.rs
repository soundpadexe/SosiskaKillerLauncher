//! Mod loaders as overlays on top of vanilla (extra libraries, main class, args).
//! * Fabric / Quilt: meta servers return a ready profile JSON.
//! * Forge / NeoForge: the official installer is run headless (`--installClient`)
//!   against our data dir (standard `versions/` + `libraries/` layout); its
//!   processors patch the game, then we read the version JSON it wrote.
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

use super::download::get_bytes;
use crate::core::instances::Loader;

pub struct Profile {
    pub main_class: String,
    pub libraries: Vec<ProfileLib>,
    pub jvm: Vec<String>,
    pub game: Vec<String>,
    /// Old Forge (1.12.2) replaces the whole vanilla argument string.
    pub legacy_game: Option<String>,
    /// Version id of the loader profile (`1.20.1-forge-47.4.28`, `neoforge-21.1.267`).
    pub version_name: Option<String>,
}

#[derive(Deserialize)]
pub struct ProfileLib {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub downloads: Option<LibDownloads>,
}

#[derive(Deserialize)]
pub struct LibDownloads {
    pub artifact: Option<LibArtifact>,
}

#[derive(Deserialize)]
pub struct LibArtifact {
    pub path: Option<String>,
    #[serde(default)]
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

impl ProfileLib {
    /// Relative path inside `libraries/`.
    pub fn rel_path(&self) -> String {
        self.downloads.as_ref().and_then(|d| d.artifact.as_ref()).and_then(|a| a.path.clone()).unwrap_or_else(|| super::manifest::maven_path(&self.name))
    }

    /// Where to fetch it from, if anywhere (installer-generated jars have no URL).
    pub fn download_url(&self, fallback_maven: Option<&str>) -> Option<String> {
        if let Some(a) = self.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            return (!a.url.is_empty()).then(|| a.url.clone());
        }
        let base = self.url.as_deref().or(fallback_maven)?;
        Some(format!("{}/{}", base.trim_end_matches('/'), self.rel_path()))
    }

    pub fn sha1(&self) -> Option<String> {
        self.downloads.as_ref().and_then(|d| d.artifact.as_ref()).and_then(|a| a.sha1.clone()).or_else(|| self.sha1.clone())
    }

    pub fn size(&self) -> u64 {
        self.downloads.as_ref().and_then(|d| d.artifact.as_ref()).and_then(|a| a.size).or(self.size).unwrap_or(0)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProfile {
    #[serde(default)]
    id: Option<String>,
    main_class: String,
    #[serde(default)]
    arguments: Option<serde_json::Value>,
    #[serde(default)]
    minecraft_arguments: Option<String>,
    libraries: Vec<ProfileLib>,
}

#[derive(Deserialize)]
struct LoaderEntry {
    loader: LoaderVer,
}
#[derive(Deserialize)]
struct LoaderVer {
    version: String,
    #[serde(default)]
    stable: Option<bool>,
}

fn meta(loader: Loader) -> (&'static str, &'static str) {
    match loader {
        Loader::Fabric => ("https://meta.fabricmc.net/v2", "https://maven.fabricmc.net/"),
        Loader::Quilt => ("https://meta.quiltmc.org/v3", "https://maven.quiltmc.org/repository/release/"),
        _ => unreachable!("only Fabric/Quilt are served as profiles"),
    }
}

pub fn default_maven(loader: Loader) -> &'static str {
    meta(loader).1
}

pub async fn resolve(client: &reqwest::Client, loader: Loader, game: &str, pin: Option<&str>, cache_dir: &Path) -> anyhow::Result<Profile> {
    let (base, _) = meta(loader);
    let cache = cache_dir.join(format!("{}{}-profile.json", loader.name().to_lowercase(), pin.map(|p| format!("-{p}")).unwrap_or_default()));

    let fetched = async {
        if let Some(v) = pin {
            return get_bytes(client, &format!("{base}/versions/loader/{game}/{v}/profile/json")).await;
        }
        let list: Vec<LoaderEntry> = serde_json::from_slice(&get_bytes(client, &format!("{base}/versions/loader/{game}")).await?)?;
        let pick = list
            .iter()
            .find(|e| e.loader.stable.unwrap_or(true) && !e.loader.version.contains('-'))
            .or(list.first())
            .with_context(|| format!("{} has no loader for Minecraft {game}", loader.name()))?;
        get_bytes(client, &format!("{base}/versions/loader/{game}/{}/profile/json", pick.loader.version)).await
    }
    .await;

    let bytes = match fetched {
        Ok(b) => {
            let _ = std::fs::write(&cache, &b);
            b
        }
        Err(e) => std::fs::read(&cache).map_err(|_| e)?,
    };
    let mut p = parse_profile(&bytes)?;
    p.version_name = None; // Fabric/Quilt keep the vanilla version name
    Ok(p)
}

fn parse_profile(bytes: &[u8]) -> anyhow::Result<Profile> {
    let raw: RawProfile = serde_json::from_slice(bytes).context("loader profile")?;
    // Plain-string args only: object args are feature-gated (demo, resolution, quick play).
    let strings = |key: &str| -> Vec<String> {
        raw.arguments
            .as_ref()
            .and_then(|a| a.get(key))
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    };
    Ok(Profile {
        jvm: strings("jvm"),
        game: strings("game"),
        legacy_game: raw.minecraft_arguments.clone(),
        version_name: raw.id.clone(),
        main_class: raw.main_class,
        libraries: raw.libraries,
    })
}

/// `group:artifact[:classifier]` — identity of a library ignoring its version.
pub fn lib_key(name: &str) -> String {
    let p: Vec<&str> = name.split(':').collect();
    match p.as_slice() {
        [g, a, _, c, ..] => format!("{g}:{a}:{c}"),
        [g, a, ..] => format!("{g}:{a}"),
        _ => name.to_owned(),
    }
}

// ---------------------------------------------------------------- Forge / NeoForge

const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";
const NEO_MAVEN: &str = "https://maven.neoforged.net/releases/net/neoforged";
const NEO_API: &str = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged";

#[derive(Deserialize)]
struct Promos {
    promos: std::collections::HashMap<String, String>,
}
#[derive(Deserialize)]
struct NeoVersions {
    versions: Vec<String>,
}

/// Numeric-aware compare of dotted versions ("21.1.77" < "21.1.267").
fn ver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |s: &str| s.split(['.', '-']).map(|x| x.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parts(a).cmp(&parts(b))
}

/// NeoForge numbering: MC 1.A.B → "A.B.x"; MC 1.A → "A.0.x"; new-style MC "26.1" → "26.1.x".
pub fn neoforge_prefix(game: &str) -> String {
    let p: Vec<&str> = game.split('.').collect();
    match p.as_slice() {
        ["1", a] => format!("{a}.0."),
        ["1", a, b] => format!("{a}.{b}."),
        _ => format!("{game}."),
    }
}

fn pick_latest(list: &[String], keep: impl Fn(&str) -> bool) -> Option<String> {
    let cands: Vec<&String> = list.iter().filter(|v| keep(v)).collect();
    let stable: Vec<&&String> = cands.iter().filter(|v| !v.contains("beta") && !v.contains("alpha")).collect();
    let pool: Vec<&String> = if stable.is_empty() { cands } else { stable.into_iter().copied().collect() };
    pool.into_iter().max_by(|a, b| ver_cmp(a, b)).cloned()
}

/// (installer URL, file name) of the best loader build for this game version.
async fn installer_for(client: &reqwest::Client, loader: Loader, game: &str, pin: Option<&str>) -> anyhow::Result<(String, String)> {
    if let Some(v) = pin {
        return Ok(pinned_installer(loader, game, v));
    }
    match loader {
        Loader::Forge => {
            let promos: Promos = serde_json::from_slice(&get_bytes(client, "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json").await?)?;
            let v = promos
                .promos
                .get(&format!("{game}-recommended"))
                .or_else(|| promos.promos.get(&format!("{game}-latest")))
                .with_context(|| format!("Forge has no build for Minecraft {game}"))?;
            let file = format!("forge-{game}-{v}-installer.jar");
            Ok((format!("{FORGE_MAVEN}/{game}-{v}/{file}"), file))
        }
        Loader::NeoForge if game == "1.20.1" => {
            // NeoForge's first release shipped as `net.neoforged:forge:1.20.1-47.1.x`.
            let list: NeoVersions = serde_json::from_slice(&get_bytes(client, &format!("{NEO_API}/forge")).await?)?;
            let v = pick_latest(&list.versions, |v| v.starts_with("1.20.1-")).context("NeoForge has no build for 1.20.1")?;
            let file = format!("forge-{v}-installer.jar");
            Ok((format!("{NEO_MAVEN}/forge/{v}/{file}"), file))
        }
        Loader::NeoForge => {
            let list: NeoVersions = serde_json::from_slice(&get_bytes(client, &format!("{NEO_API}/neoforge")).await?)?;
            let prefix = neoforge_prefix(game);
            let v = pick_latest(&list.versions, |v| v.starts_with(&prefix)).with_context(|| format!("NeoForge has no build for Minecraft {game}"))?;
            let file = format!("neoforge-{v}-installer.jar");
            Ok((format!("{NEO_MAVEN}/neoforge/{v}/{file}"), file))
        }
        _ => unreachable!(),
    }
}

/// Version id the installer will create (from its `install_profile.json`).
fn installer_version_id(jar: &Path) -> anyhow::Result<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar)?)?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("install_profile.json")?, &mut s)?;
    let v: serde_json::Value = serde_json::from_str(&s)?;
    v.get("version")
        .and_then(|x| x.as_str())
        .or_else(|| v.pointer("/install/target").and_then(|x| x.as_str()))
        .map(str::to_owned)
        .context("installer has no version id")
}

/// Installs Forge/NeoForge once (cached by version id) and returns the overlay profile.
/// `java` must be a console `java` (not `javaw`) so the installer log is captured.
pub async fn install_forge(client: &reqwest::Client, loader: Loader, game: &str, pin: Option<&str>, root: &Path, versions: &Path, java: &Path, log_dir: &Path) -> anyhow::Result<Profile> {
    let cache = root.join("cache").join("installers");
    std::fs::create_dir_all(&cache)?;
    let marker = cache.join(format!("{}-{game}{}.txt", loader.name().to_lowercase(), pin.map(|p| format!("-{p}")).unwrap_or_default()));

    // Offline / repeat launch: reuse what was installed before.
    let installer = match installer_for(client, loader, game, pin).await {
        Ok((url, file)) => {
            let path = cache.join(&file);
            if !path.exists() {
                let bytes = get_bytes(client, &url).await.with_context(|| format!("download {file}"))?;
                std::fs::write(&path, bytes)?;
            }
            std::fs::write(&marker, &file)?;
            path
        }
        Err(e) => match std::fs::read_to_string(&marker) {
            Ok(file) => cache.join(file.trim()),
            Err(_) => return Err(e),
        },
    };

    let id = installer_version_id(&installer)?;
    let json = versions.join(&id).join(format!("{id}.json"));
    // The installer writes the JSON *before* its processors finish, so only a
    // marker written after a clean exit means "installed". Interrupted installs re-run.
    let done = versions.join(&id).join(".skl-installed");
    if !done.exists() {
        // The installer insists on a vanilla-launcher style profile file.
        let profiles = root.join("launcher_profiles.json");
        if !profiles.exists() {
            std::fs::write(&profiles, br#"{"profiles":{},"selectedProfile":"","launcherVersion":{"name":"SKL","format":21}}"#)?;
        }
        std::fs::create_dir_all(log_dir)?;
        let log_path = log_dir.join(format!("{}-installer.log", loader.name().to_lowercase()));
        let log = std::fs::File::create(&log_path)?;
        let (java, installer, root2) = (java.to_owned(), installer.clone(), root.to_owned());
        let status = tokio::task::spawn_blocking(move || {
            let mut cmd = std::process::Command::new(java);
            cmd.arg("-jar").arg(&installer).arg("--installClient").arg(&root2).current_dir(&root2).stdout(log.try_clone()?).stderr(log);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            cmd.status()
        })
        .await??;
        if !status.success() || !json.exists() {
            anyhow::bail!("{} installer failed (see {})", loader.name(), log_path.display());
        }
        std::fs::write(&done, b"ok")?;
    }
    let mut p = parse_profile(&std::fs::read(&json)?)?;
    p.version_name = Some(id);
    Ok(p)
}

pub fn forge_client_jar(versions: &Path, id: &str) -> PathBuf {
    versions.join(id).join(format!("{id}.jar"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn neoforge_numbering() {
        assert_eq!(super::neoforge_prefix("1.21.1"), "21.1.");
        assert_eq!(super::neoforge_prefix("1.21"), "21.0.");
        assert_eq!(super::neoforge_prefix("1.20.4"), "20.4.");
        assert_eq!(super::neoforge_prefix("26.1"), "26.1.");
        let list: Vec<String> = ["21.1.9", "21.1.77", "21.1.267", "21.1.268-beta", "21.2.1"].map(String::from).to_vec();
        assert_eq!(super::pick_latest(&list, |v| v.starts_with("21.1.")).as_deref(), Some("21.1.267"));
    }
}

/// Installer of an exact build (modpacks pin their loader version).
/// Accepts `47.2.0`, `forge-47.2.0`, `1.20.1-47.2.0` and NeoForge `21.1.77`.
pub fn pinned_installer(loader: Loader, game: &str, pin: &str) -> (String, String) {
    let v = pin.trim_start_matches("forge-").trim_start_matches("neoforge-");
    let v = v.strip_prefix(&format!("{game}-")).unwrap_or(v);
    match loader {
        Loader::NeoForge if game == "1.20.1" => {
            let file = format!("forge-1.20.1-{v}-installer.jar");
            (format!("{NEO_MAVEN}/forge/1.20.1-{v}/{file}"), file)
        }
        Loader::NeoForge => {
            let file = format!("neoforge-{v}-installer.jar");
            (format!("{NEO_MAVEN}/neoforge/{v}/{file}"), file)
        }
        _ => {
            let file = format!("forge-{game}-{v}-installer.jar");
            (format!("{FORGE_MAVEN}/{game}-{v}/{file}"), file)
        }
    }
}
