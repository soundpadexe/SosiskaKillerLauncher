//! Modrinth v2 — open API, no key needed.
use anyhow::{anyhow, bail, Context};
use serde::Deserialize;

use crate::core::instances::Loader;
use crate::models::content::{ContentKind, InstalledEntry, Project, Query, Source, Target, PAGE};

const BASE: &str = "https://api.modrinth.com/v2";

fn project_type(k: ContentKind) -> &'static str {
    match k {
        ContentKind::Mod => "mod",
        ContentKind::ResourcePack => "resourcepack",
        ContentKind::Shader => "shader",
    }
}

pub fn loader_slug(l: Loader) -> Option<&'static str> {
    match l {
        Loader::Vanilla => None,
        Loader::Fabric => Some("fabric"),
        Loader::Quilt => Some("quilt"),
        Loader::Forge => Some("forge"),
        Loader::NeoForge => Some("neoforge"),
    }
}

#[derive(Deserialize)]
struct SearchResp {
    hits: Vec<Hit>,
}
#[derive(Deserialize)]
struct Hit {
    project_id: String,
    title: String,
    description: String,
    author: String,
    downloads: u64,
    icon_url: Option<String>,
}

pub async fn search(client: &reqwest::Client, q: &Query) -> anyhow::Result<Vec<Project>> {
    let mut facets = vec![format!("[\"project_type:{}\"]", project_type(q.kind)), format!("[\"versions:{}\"]", q.game_version)];
    if q.kind == ContentKind::Mod {
        if let Some(l) = loader_slug(q.loader) {
            facets.push(format!("[\"categories:{l}\"]"));
        }
    }
    let params = [
        ("query", q.text.clone()),
        ("facets", format!("[{}]", facets.join(","))),
        ("limit", PAGE.to_string()),
        ("offset", q.offset.to_string()),
        ("index", if q.text.trim().is_empty() { "downloads" } else { "relevance" }.to_owned()),
    ];
    let resp: SearchResp = client.get(format!("{BASE}/search")).query(&params).send().await?.error_for_status()?.json().await?;
    Ok(resp
        .hits
        .into_iter()
        .map(|h| Project {
            source: Source::Modrinth,
            id: h.project_id,
            title: h.title,
            author: h.author,
            description: h.description,
            downloads: h.downloads,
            icon_url: h.icon_url.filter(|u| !u.is_empty()),
        })
        .collect())
}

#[derive(Deserialize)]
struct MVersion {
    id: String,
    project_id: String,
    files: Vec<MFile>,
    #[serde(default)]
    dependencies: Vec<MDep>,
}
#[derive(Deserialize)]
struct MFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    hashes: MHashes,
    size: u64,
}
#[derive(Deserialize)]
struct MHashes {
    sha1: Option<String>,
}
#[derive(Deserialize)]
struct MDep {
    project_id: Option<String>,
    version_id: Option<String>,
    dependency_type: String,
}
#[derive(Deserialize)]
struct MProject {
    title: String,
    project_type: String,
}

async fn best_version(client: &reqwest::Client, project: &str, kind: ContentKind, t: &Target) -> anyhow::Result<Option<MVersion>> {
    let mut params = vec![("game_versions", format!("[\"{}\"]", t.game_version))];
    if kind == ContentKind::Mod {
        if let Some(l) = loader_slug(t.loader) {
            params.push(("loaders", format!("[\"{l}\"]")));
        }
    }
    let list: Vec<MVersion> = client.get(format!("{BASE}/project/{project}/version")).query(&params).send().await?.error_for_status()?.json().await?;
    Ok(list.into_iter().next()) // newest first
}

fn kind_of(project_type: &str) -> ContentKind {
    match project_type {
        "resourcepack" => ContentKind::ResourcePack,
        "shader" => ContentKind::Shader,
        _ => ContentKind::Mod,
    }
}

/// Installs a project and, recursively, every *required* dependency that
/// isn't already present. Refuses if a declared incompatibility is installed.
pub async fn install(client: &reqwest::Client, root: &Project, t: &Target, installed: &[InstalledEntry]) -> anyhow::Result<Vec<InstalledEntry>> {
    let have: std::collections::HashSet<&str> =
        installed.iter().filter(|e| e.source == Source::Modrinth).map(|e| e.project_id.as_str()).collect();
    let mut queue: Vec<(String, Option<String>, bool)> = vec![(root.id.clone(), None, false)];
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    while let Some((pid, vid, is_dep)) = queue.pop() {
        if !seen.insert(pid.clone()) || (is_dep && have.contains(pid.as_str())) {
            continue;
        }
        let info: MProject = client.get(format!("{BASE}/project/{pid}")).send().await?.error_for_status()?.json().await?;
        let kind = kind_of(&info.project_type);
        let v = match vid {
            Some(vid) => client.get(format!("{BASE}/version/{vid}")).send().await?.error_for_status()?.json::<MVersion>().await?,
            None => best_version(client, &pid, kind, t)
                .await?
                .ok_or_else(|| anyhow!("«{}» has no build for {} {}", info.title, t.loader.name(), t.game_version))?,
        };
        for d in &v.dependencies {
            match (d.dependency_type.as_str(), &d.project_id, &d.version_id) {
                ("required", Some(p), vid) => queue.push((p.clone(), vid.clone(), true)),
                ("required", None, Some(vid)) => {
                    let dv: MVersion = client.get(format!("{BASE}/version/{vid}")).send().await?.error_for_status()?.json().await?;
                    queue.push((dv.project_id, Some(vid.clone()), true));
                }
                ("incompatible", Some(p), _) if have.contains(p.as_str()) => {
                    let other = installed.iter().find(|e| &e.project_id == p).map(|e| e.title.clone()).unwrap_or_default();
                    bail!("«{}» is incompatible with «{other}»", info.title);
                }
                _ => {}
            }
        }
        let f = v.files.iter().find(|f| f.primary).or(v.files.first()).context("version has no files")?;
        super::store::download(client, &f.url, &t.dir.join(kind.folder()).join(&f.filename), f.hashes.sha1.clone(), f.size).await?;
        out.push(InstalledEntry {
            source: Source::Modrinth,
            project_id: pid,
            version_id: v.id.clone(),
            title: info.title,
            file: f.filename.clone(),
            folder: kind.folder().to_owned(),
            dependency: is_dep,
            enabled: true,
        });
    }
    Ok(out)
}
