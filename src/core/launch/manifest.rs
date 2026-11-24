//! Mojang per-version JSON (all eras, from rd-132211 to current).
use std::collections::HashMap;

use serde::Deserialize;

#[cfg(windows)]
pub const OS_NAME: &str = "windows";
#[cfg(target_os = "macos")]
pub const OS_NAME: &str = "osx";
#[cfg(all(not(windows), not(target_os = "macos")))]
pub const OS_NAME: &str = "linux";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub main_class: String,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    pub asset_index: AssetIndexRef,
    pub downloads: ClientDownloads,
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Deserialize, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Arg>,
    #[serde(default)]
    pub jvm: Vec<Arg>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Arg {
    Plain(String),
    Ruled { rules: Vec<Rule>, value: ArgValue },
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
}

#[derive(Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibDownloads>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
pub struct LibDownloads {
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Deserialize, Clone)]
pub struct Artifact {
    pub path: Option<String>,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Deserialize)]
pub struct ClientDownloads {
    pub client: Artifact,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
}

#[derive(Deserialize)]
pub struct AssetIndex {
    #[serde(default)]
    pub map_to_resources: bool,
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// Mojang rule semantics: last matching rule wins; feature-gated rules
/// (demo, custom resolution, quick play) are treated as not matching.
pub fn rules_allow(rules: &[Rule]) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for r in rules {
        let os_ok = r.os.as_ref().map_or(true, |os| {
            os.name.as_deref().map_or(true, |n| n == OS_NAME)
                && os.arch.as_deref().map_or(true, arch_matches)
        });
        if os_ok && r.features.is_none() {
            allowed = r.action == "allow";
        }
    }
    allowed
}

impl Arg {
    pub fn values(&self) -> Vec<&str> {
        match self {
            Arg::Plain(s) => vec![s.as_str()],
            Arg::Ruled { rules, value } if rules_allow(rules) => match value {
                ArgValue::One(s) => vec![s.as_str()],
                ArgValue::Many(v) => v.iter().map(String::as_str).collect(),
            },
            Arg::Ruled { .. } => vec![],
        }
    }
}

/// `group:artifact:version[:classifier]` → maven relative path.
pub fn maven_path(name: &str) -> String {
    let parts: Vec<&str> = name.split(':').collect();
    let (group, artifact, version) = (parts[0].replace('.', "/"), parts[1], parts.get(2).copied().unwrap_or(""));
    let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    format!("{group}/{artifact}/{version}/{artifact}-{version}{classifier}.jar")
}

fn arch_matches(a: &str) -> bool {
    match a {
        "x86" => cfg!(target_arch = "x86"),
        "arm64" | "aarch64" => cfg!(target_arch = "aarch64"),
        "x86_64" | "amd64" => cfg!(target_arch = "x86_64"),
        _ => true,
    }
}

/// Newer manifests gate natives by library *name* suffix rather than rules.
pub fn native_suffix_matches(name: &str) -> bool {
    let Some(cls) = name.rsplit(':').next().filter(|c| c.starts_with("natives-")) else { return true };
    match cls.trim_start_matches("natives-") {
        "windows" | "linux" | "macos" | "osx" => true,
        s if s.ends_with("arm64") || s.ends_with("aarch64") => cfg!(target_arch = "aarch64"),
        s if s.ends_with("x86") => cfg!(target_arch = "x86"),
        _ => true,
    }
}
