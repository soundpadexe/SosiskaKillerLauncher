//! Modpack import: Modrinth `.mrpack` and CurseForge `.zip` (manifest.json).
//! Creates a new isolated instance with the pack's exact Minecraft + loader
//! versions, extracts overrides and downloads every file (hash-verified).
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Deserialize;

use super::instances::{self, Instance, Loader};
use super::launch::download::{fetch_all, Dl};
use super::launch::{Progress, Stage};
use super::paths::Paths;

#[derive(Debug, PartialEq)]
pub enum Format {
    Modrinth,
    CurseForge,
}

/// What a pack declares, before anything is written.
#[derive(Debug)]
pub struct Plan {
    pub format: Format,
    pub name: String,
    pub game: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    /// Modrinth: ready downloads. CurseForge: resolved later via the API.
    mr_files: Vec<MrFile>,
    cf_files: Vec<CfRef>,
    overrides: Vec<String>,
}

// ---- Modrinth index

#[derive(Deserialize)]
struct MrIndex {
    name: String,
    files: Vec<MrFile>,
    dependencies: std::collections::HashMap<String, String>,
}

#[derive(Deserialize, Debug)]
struct MrFile {
    path: String,
    hashes: std::collections::HashMap<String, String>,
    downloads: Vec<String>,
    #[serde(default, rename = "fileSize")]
    file_size: u64,
    #[serde(default)]
    env: Option<std::collections::HashMap<String, String>>,
}

// ---- CurseForge manifest

#[derive(Deserialize)]
struct CfManifest {
    #[serde(default)]
    name: String,
    minecraft: CfMc,
    files: Vec<CfRef>,
    #[serde(default = "default_overrides")]
    overrides: String,
}

fn default_overrides() -> String {
    "overrides".into()
}

#[derive(Deserialize)]
struct CfMc {
    version: String,
    #[serde(default, rename = "modLoaders")]
    mod_loaders: Vec<CfLoader>,
}

#[derive(Deserialize)]
struct CfLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize, Debug, Clone)]
struct CfRef {
    #[serde(rename = "projectID")]
    project: u64,
    #[serde(rename = "fileID")]
    file: u64,
    #[serde(default = "yes")]
    required: bool,
}

fn yes() -> bool {
    true
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut v = Vec::new();
    f.read_to_end(&mut v).ok()?;
    Some(v)
}

/// Reads the pack's manifest without touching the disk.
pub fn inspect(pack: &Path) -> anyhow::Result<Plan> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pack)?).context("not a zip archive")?;
    if let Some(raw) = read_entry(&mut zip, "modrinth.index.json") {
        let idx: MrIndex = serde_json::from_slice(&raw).context("modrinth.index.json")?;
        let dep = |k: &str| idx.dependencies.get(k).cloned();
        let game = dep("minecraft").context("pack has no Minecraft version")?;
        let (loader, lv) = if let Some(v) = dep("fabric-loader") {
            (Loader::Fabric, Some(v))
        } else if let Some(v) = dep("quilt-loader") {
            (Loader::Quilt, Some(v))
        } else if let Some(v) = dep("neoforge") {
            (Loader::NeoForge, Some(v))
        } else if let Some(v) = dep("forge") {
            (Loader::Forge, Some(v))
        } else {
            (Loader::Vanilla, None)
        };
        let files = idx.files.into_iter().filter(|f| f.env.as_ref().and_then(|e| e.get("client")).map_or(true, |c| c != "unsupported")).collect();
        return Ok(Plan {
            format: Format::Modrinth,
            name: idx.name,
            game,
            loader,
            loader_version: lv,
            mr_files: files,
            cf_files: vec![],
            overrides: vec!["overrides".into(), "client-overrides".into()],
        });
    }
    if let Some(raw) = read_entry(&mut zip, "manifest.json") {
        let m: CfManifest = serde_json::from_slice(&raw).context("manifest.json")?;
        let primary = m.minecraft.mod_loaders.iter().find(|l| l.primary).or(m.minecraft.mod_loaders.first());
        let (loader, lv) = match primary.map(|l| l.id.as_str()) {
            Some(id) if id.starts_with("neoforge-") => (Loader::NeoForge, Some(id["neoforge-".len()..].to_owned())),
            Some(id) if id.starts_with("forge-") => (Loader::Forge, Some(id["forge-".len()..].to_owned())),
            Some(id) if id.starts_with("fabric-") => (Loader::Fabric, Some(id["fabric-".len()..].to_owned())),
            Some(id) if id.starts_with("quilt-") => (Loader::Quilt, Some(id["quilt-".len()..].to_owned())),
            _ => (Loader::Vanilla, None),
        };
        let name = if m.name.trim().is_empty() { pack.file_stem().unwrap_or_default().to_string_lossy().into_owned() } else { m.name };
        return Ok(Plan {
            format: Format::CurseForge,
            name,
            game: m.minecraft.version,
            loader,
            loader_version: lv,
            mr_files: vec![],
            cf_files: m.files.into_iter().filter(|f| f.required).collect(),
            overrides: vec![m.overrides],
        });
    }
    bail!("not a modpack: no modrinth.index.json or manifest.json")
}

/// Relative path from an archive/manifest, rejecting anything that could escape the instance.
fn safe_rel(p: &str) -> Option<PathBuf> {
    let path = Path::new(p);
    if path.is_absolute() || p.contains(':') {
        return None;
    }
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::Normal(s) => out.push(s),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn extract_overrides(pack: &Path, prefixes: &[String], dest: &Path) -> anyhow::Result<usize> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pack)?)?;
    let mut n = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let name = f.name().replace('\\', "/");
        let Some(rest) = prefixes.iter().find_map(|p| name.strip_prefix(&format!("{p}/"))) else { continue };
        let Some(rel) = safe_rel(rest) else { continue };
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut w = std::fs::File::create(&out)?;
        std::io::copy(&mut f, &mut w)?;
        n += 1;
    }
    Ok(n)
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}

#[derive(Deserialize)]
struct CfFileInfo {
    id: u64,
    #[serde(rename = "modId")]
    mod_id: u64,
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(default, rename = "fileLength")]
    length: u64,
    #[serde(default)]
    hashes: Vec<CfHash>,
}

#[derive(Deserialize)]
struct CfHash {
    value: String,
    algo: u8,
}

#[derive(Deserialize)]
struct CfModInfo {
    id: u64,
    #[serde(rename = "classId", default)]
    class_id: Option<u64>,
}

const CF: &str = "https://api.curseforge.com/v1";

async fn cf_downloads(client: &reqwest::Client, key: &str, refs: &[CfRef], dest: &Path) -> anyhow::Result<Vec<Dl>> {
    if refs.is_empty() {
        return Ok(vec![]);
    }
    if key.trim().is_empty() {
        bail!("CurseForge API key required");
    }
    let file_ids: Vec<u64> = refs.iter().map(|r| r.file).collect();
    let mod_ids: Vec<u64> = refs.iter().map(|r| r.project).collect();
    let files: Data<Vec<CfFileInfo>> = client.post(format!("{CF}/mods/files")).header("x-api-key", key).json(&serde_json::json!({ "fileIds": file_ids })).send().await?.error_for_status()?.json().await?;
    let mods: Data<Vec<CfModInfo>> = client.post(format!("{CF}/mods")).header("x-api-key", key).json(&serde_json::json!({ "modIds": mod_ids })).send().await?.error_for_status()?.json().await?;
    let class: std::collections::HashMap<u64, u64> = mods.data.iter().filter_map(|m| Some((m.id, m.class_id?))).collect();
    let mut out = Vec::new();
    for f in files.data {
        let folder = match class.get(&f.mod_id) {
            Some(12) => "resourcepacks",
            Some(6552) => "shaderpacks",
            Some(6945) => "datapacks",
            _ => "mods",
        };
        let Some(name) = safe_rel(&f.file_name) else { continue };
        // Authors may disable third-party downloads; the CDN path is still public.
        let url = f.download_url.unwrap_or_else(|| format!("https://edge.forgecdn.net/files/{}/{}/{}", f.id / 1000, f.id % 1000, f.file_name));
        let sha1 = f.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone());
        out.push(Dl { url: url.replace(' ', "%20"), path: dest.join(folder).join(name), sha1, size: f.length });
    }
    Ok(out)
}

/// Full import. Creates the instance first so a failed download can simply be retried by re-importing.
pub async fn import(client: &reqwest::Client, paths: &Paths, cf_key: &str, pack: &Path, prog: &Progress) -> anyhow::Result<Instance> {
    prog.set_stage(Stage::Preparing);
    let pack2 = pack.to_owned();
    let plan = tokio::task::spawn_blocking(move || inspect(&pack2)).await??;
    let mut inst = instances::create(paths, &plan.name, &plan.game, plan.loader)?;
    inst.loader_version = plan.loader_version.clone();
    instances::save(paths, &inst)?;
    let dest = instances::dir(paths, &inst.id);

    let result = async {
        let (pack2, prefixes, dest2) = (pack.to_owned(), plan.overrides.clone(), dest.clone());
        tokio::task::spawn_blocking(move || extract_overrides(&pack2, &prefixes, &dest2)).await??;

        let mut dls: Vec<Dl> = Vec::new();
        for f in &plan.mr_files {
            let Some(rel) = safe_rel(&f.path) else { continue };
            let Some(url) = f.downloads.first() else { continue };
            dls.push(Dl { url: url.clone(), path: dest.join(rel), sha1: f.hashes.get("sha1").cloned(), size: f.file_size });
        }
        dls.extend(cf_downloads(client, cf_key, &plan.cf_files, &dest).await?);
        prog.set_stage(Stage::Libraries);
        fetch_all(client, dls, prog).await
    }
    .await;
    match result {
        Ok(()) => {
            prog.set_stage(Stage::Exited);
            Ok(inst)
        }
        Err(e) => {
            // Keep nothing half-made: a broken instance would only confuse.
            let _ = instances::delete(paths, &inst.id);
            prog.set_stage(Stage::Failed(format!("{e:#}")));
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in entries {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn inspect_both_formats() {
        let dir = std::env::temp_dir().join("skl-modpack-test");
        std::fs::create_dir_all(&dir).unwrap();
        let mr = dir.join("a.mrpack");
        make_zip(&mr, &[(
            "modrinth.index.json",
            br#"{"formatVersion":1,"game":"minecraft","name":"Cool Pack","versionId":"1",
                "files":[{"path":"mods/a.jar","hashes":{"sha1":"x"},"downloads":["https://x/a.jar"],"fileSize":3},
                         {"path":"mods/server.jar","hashes":{},"downloads":["https://x"],"env":{"client":"unsupported","server":"required"}}],
                "dependencies":{"minecraft":"1.20.1","fabric-loader":"0.15.11"}}"#,
        ), ("overrides/config/a.txt", b"hi"), ("overrides/../evil.txt", b"no")]);
        let p = inspect(&mr).unwrap();
        assert_eq!((p.name.as_str(), p.game.as_str(), p.loader, p.loader_version.as_deref()), ("Cool Pack", "1.20.1", Loader::Fabric, Some("0.15.11")));
        assert_eq!(p.mr_files.len(), 1, "server-only file skipped");

        let out = dir.join("out");
        let _ = std::fs::remove_dir_all(&out);
        assert_eq!(extract_overrides(&mr, &p.overrides, &out).unwrap(), 1, "path traversal rejected");
        assert!(out.join("config/a.txt").exists());

        let cf = dir.join("b.zip");
        make_zip(&cf, &[(
            "manifest.json",
            br#"{"minecraft":{"version":"1.20.1","modLoaders":[{"id":"forge-47.2.0","primary":true}]},
                "manifestType":"minecraftModpack","name":"CF Pack","files":[{"projectID":1,"fileID":2,"required":true}],"overrides":"overrides"}"#,
        )]);
        let p = inspect(&cf).unwrap();
        assert_eq!((p.format, p.loader, p.loader_version.as_deref(), p.cf_files.len()), (Format::CurseForge, Loader::Forge, Some("47.2.0"), 1));
        assert!(safe_rel("../x").is_none() && safe_rel("C:/x").is_none() && safe_rel("mods/ok.jar").is_some());
    }
}

#[cfg(test)]
mod live {
    /// Real pack from Modrinth → import → launch with the pinned loader.
    /// `cargo test --release -- --ignored modpack_live --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn modpack_live() {
        let client = crate::api::http_client();
        let paths = crate::core::paths::Paths::init().unwrap();
        let slug = std::env::var("SKL_TEST_PACK").unwrap_or_else(|_| "fabulously-optimized".into());
        let game = std::env::var("SKL_TEST_VERSION").unwrap_or_else(|_| "1.21.1".into());
        let versions: serde_json::Value = client
            .get(format!("https://api.modrinth.com/v2/project/{slug}/version"))
            .query(&[("game_versions", format!("[\"{game}\"]"))])
            .send().await.unwrap().json().await.unwrap();
        let file = versions[0]["files"].as_array().unwrap().iter().find(|f| f["filename"].as_str().unwrap().ends_with(".mrpack")).unwrap();
        let bytes = client.get(file["url"].as_str().unwrap()).send().await.unwrap().bytes().await.unwrap();
        let pack = std::env::temp_dir().join("skl-live.mrpack");
        std::fs::write(&pack, &bytes).unwrap();

        let prog = crate::core::launch::Progress::new();
        let t0 = std::time::Instant::now();
        let inst = super::import(&client, &paths, "", &pack, &prog).await.unwrap();
        let dir = crate::core::instances::dir(&paths, &inst.id);
        let mods = std::fs::read_dir(dir.join("mods")).map(|r| r.count()).unwrap_or(0);
        println!("imported {} ({:?} {:?}) in {:?}: {mods} mods", inst.name, inst.loader, inst.loader_version, t0.elapsed());
        assert!(mods > 0);

        let list = crate::api::mojang::fetch_versions(&client, &paths.cache()).await.unwrap();
        let v = list.versions.iter().find(|v| v.id == inst.version).unwrap();
        let req = crate::core::launch::install::LaunchRequest {
            id: v.id.clone(), url: v.url.clone(), legacy: false, nickname: "Tester".into(),
            uuid: crate::core::auth::offline_uuid("Tester"), memory_mb: 3072, game_dir: dir.clone(),
            loader: inst.loader, loader_version: inst.loader_version.clone(), skin_port: None, opts: Default::default(),
        };
        let lp = crate::core::launch::Progress::new();
        let mut child = crate::core::launch::install::run(&client, &paths, &req, &lp).await.unwrap();
        std::thread::sleep(std::time::Duration::from_secs(60));
        let alive = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let log = String::from_utf8_lossy(&std::fs::read(dir.join("logs/skl-launch.log")).unwrap()).into_owned();
        let loaded = log.lines().filter(|l| l.contains("Loading") && l.contains("mods")).take(2).collect::<Vec<_>>().join("\n");
        println!("{loaded}\n--- tail ---\n{}", log.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
        if alive {
            let _ = crate::core::instances::delete(&paths, &inst.id);
        }
        assert!(alive, "game exited early");
    }
}
