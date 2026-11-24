//! CurseForge v1 — requires a personal API key (x-api-key).
//! Respects authors who disabled third-party distribution (no download URL → refuse).
use anyhow::{anyhow, bail, Context};
use serde::Deserialize;

use crate::core::instances::Loader;
use crate::models::content::{ContentKind, InstalledEntry, Project, Query, Source, Target, PAGE};

const BASE: &str = "https://api.curseforge.com/v1";
const MINECRAFT: &str = "432";

fn class_id(k: ContentKind) -> &'static str {
    match k {
        ContentKind::Mod => "6",
        ContentKind::ResourcePack => "12",
        ContentKind::Shader => "6552",
    }
}

fn loader_type(l: Loader) -> Option<&'static str> {
    match l {
        Loader::Vanilla => None,
        Loader::Fabric => Some("4"),
        Loader::Quilt => Some("5"),
        Loader::Forge => Some("1"),
        Loader::NeoForge => Some("6"),
    }
}

#[derive(Deserialize)]
struct Data<T> {
    data: T,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMod {
    id: u64,
    name: String,
    summary: String,
    download_count: f64,
    logo: Option<Logo>,
    #[serde(default)]
    authors: Vec<Author>,
    class_id: Option<u64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Logo {
    thumbnail_url: String,
}
#[derive(Deserialize)]
struct Author {
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFile {
    id: u64,
    file_name: String,
    download_url: Option<String>,
    #[serde(default)]
    hashes: Vec<Hash>,
    file_length: u64,
    #[serde(default)]
    dependencies: Vec<Dep>,
}
#[derive(Deserialize)]
struct Hash {
    value: String,
    algo: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dep {
    mod_id: u64,
    relation_type: u32,
}

pub async fn search(client: &reqwest::Client, key: &str, q: &Query) -> anyhow::Result<Vec<Project>> {
    let mut params = vec![
        ("gameId", MINECRAFT.to_owned()),
        ("classId", class_id(q.kind).to_owned()),
        ("searchFilter", q.text.clone()),
        ("gameVersion", q.game_version.clone()),
        ("sortField", "2".to_owned()), // popularity
        ("sortOrder", "desc".to_owned()),
        ("pageSize", PAGE.to_string()),
        ("index", q.offset.to_string()),
    ];
    if q.kind == ContentKind::Mod {
        if let Some(l) = loader_type(q.loader) {
            params.push(("modLoaderType", l.to_owned()));
        }
    }
    let resp: Data<Vec<CfMod>> = client.get(format!("{BASE}/mods/search")).header("x-api-key", key).query(&params).send().await?.error_for_status()?.json().await?;
    Ok(resp
        .data
        .into_iter()
        .map(|m| Project {
            source: Source::CurseForge,
            id: m.id.to_string(),
            title: m.name,
            author: m.authors.first().map(|a| a.name.clone()).unwrap_or_default(),
            description: m.summary,
            downloads: m.download_count as u64,
            icon_url: m.logo.map(|l| l.thumbnail_url).filter(|u| !u.is_empty()),
        })
        .collect())
}

fn kind_of(class: Option<u64>) -> ContentKind {
    match class {
        Some(12) => ContentKind::ResourcePack,
        Some(6552) => ContentKind::Shader,
        _ => ContentKind::Mod,
    }
}

pub async fn install(client: &reqwest::Client, key: &str, root: &Project, t: &Target, installed: &[InstalledEntry]) -> anyhow::Result<Vec<InstalledEntry>> {
    let have: std::collections::HashSet<&str> =
        installed.iter().filter(|e| e.source == Source::CurseForge).map(|e| e.project_id.as_str()).collect();
    let mut queue = vec![(root.id.clone(), false)];
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    while let Some((id, is_dep)) = queue.pop() {
        if !seen.insert(id.clone()) || (is_dep && have.contains(id.as_str())) {
            continue;
        }
        let m: Data<CfMod> = client.get(format!("{BASE}/mods/{id}")).header("x-api-key", key).send().await?.error_for_status()?.json().await?;
        let kind = kind_of(m.data.class_id);
        let mut params = vec![("gameVersion", t.game_version.clone())];
        if kind == ContentKind::Mod {
            if let Some(l) = loader_type(t.loader) {
                params.push(("modLoaderType", l.to_owned()));
            }
        }
        let files: Data<Vec<CfFile>> = client.get(format!("{BASE}/mods/{id}/files")).header("x-api-key", key).query(&params).send().await?.error_for_status()?.json().await?;
        let f = files
            .data
            .into_iter()
            .max_by_key(|f| f.id) // newest upload
            .ok_or_else(|| anyhow!("«{}» has no build for {} {}", m.data.name, t.loader.name(), t.game_version))?;
        for d in &f.dependencies {
            match d.relation_type {
                3 => queue.push((d.mod_id.to_string(), true)),
                5 if have.contains(d.mod_id.to_string().as_str()) => bail!("«{}» is incompatible with an installed mod", m.data.name),
                _ => {}
            }
        }
        let url = f.download_url.clone().with_context(|| format!("the author of «{}» disabled downloads from third-party launchers", m.data.name))?;
        let sha1 = f.hashes.iter().find(|h| h.algo == 1).map(|h| h.value.clone());
        super::store::download(client, &url, &t.dir.join(kind.folder()).join(&f.file_name), sha1, f.file_length).await?;
        out.push(InstalledEntry {
            source: Source::CurseForge,
            project_id: id,
            version_id: f.id.to_string(),
            title: m.data.name,
            file: f.file_name,
            folder: kind.folder().to_owned(),
            dependency: is_dep,
            enabled: true,
        });
    }
    Ok(out)
}
