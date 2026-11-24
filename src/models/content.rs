//! Shared types for the content store (mods, resource packs, shaders).
use serde::{Deserialize, Serialize};

use crate::core::instances::Loader;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    Modrinth,
    CurseForge,
    /// A file the user dropped into the folder by hand.
    Local,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Modrinth => "Modrinth",
            Source::CurseForge => "CurseForge",
            Source::Local => "Local",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentKind {
    Mod,
    ResourcePack,
    Shader,
}

impl ContentKind {
    pub const ALL: [ContentKind; 3] = [ContentKind::Mod, ContentKind::ResourcePack, ContentKind::Shader];

    pub fn folder(self) -> &'static str {
        match self {
            ContentKind::Mod => "mods",
            ContentKind::ResourcePack => "resourcepacks",
            ContentKind::Shader => "shaderpacks",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Project {
    pub source: Source,
    pub id: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub downloads: u64,
    pub icon_url: Option<String>,
}

impl Project {
    pub fn key(&self) -> String {
        format!("{:?}:{}", self.source, self.id)
    }
}

/// What a search/install is aimed at: one instance's game version + loader.
#[derive(Clone, Debug)]
pub struct Target {
    pub game_version: String,
    pub loader: Loader,
    pub dir: std::path::PathBuf,
}

#[derive(Clone, Debug)]
pub struct Query {
    pub text: String,
    pub kind: ContentKind,
    pub game_version: String,
    pub loader: Loader,
    pub offset: usize,
}

pub const PAGE: usize = 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstalledEntry {
    pub source: Source,
    pub project_id: String,
    pub version_id: String,
    pub title: String,
    /// File name without the `.disabled` suffix.
    pub file: String,
    pub folder: String,
    #[serde(default)]
    pub dependency: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}
