//! Instance profiles: every instance has its own isolated game dir
//! (`instances/<id>/` → mods, saves, config, resourcepacks never mix).
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::paths::Paths;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Loader {
    #[default]
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl Loader {
    pub const ALL: [Loader; 5] = [Loader::Vanilla, Loader::Fabric, Loader::Quilt, Loader::Forge, Loader::NeoForge];

    pub fn name(self) -> &'static str {
        match self {
            Loader::Vanilla => "Vanilla",
            Loader::Fabric => "Fabric",
            Loader::Quilt => "Quilt",
            Loader::Forge => "Forge",
            Loader::NeoForge => "NeoForge",
        }
    }

    /// Earliest release date (manifest `releaseTime`) the loader supports.
    pub fn supports(self, date: &str, legacy: bool) -> bool {
        match self {
            Loader::Vanilla => true,
            Loader::Fabric => !legacy && date >= "2018-10-24",
            Loader::Quilt => !legacy && date >= "2019-07-19",
            // Installer v2 era (1.12.2 onward); per-version availability is checked at install.
            Loader::Forge => !legacy && date >= "2017-09-18",
            // NeoForge starts at 1.20.1.
            Loader::NeoForge => !legacy && date >= "2023-06-12",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub loader: Loader,
    pub created: u64,
    #[serde(default)]
    pub last_played: Option<u64>,
    #[serde(default)]
    pub playtime_secs: u64,
    /// Pinned loader build (from a modpack); None = latest stable.
    #[serde(default)]
    pub loader_version: Option<String>,
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn dir(paths: &Paths, id: &str) -> PathBuf {
    paths.instances().join(id)
}

pub fn load_all(paths: &Paths) -> Vec<Instance> {
    let mut out: Vec<Instance> = std::fs::read_dir(paths.instances())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read(e.path().join("instance.json")).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect();
    out.sort_by_key(|i: &Instance| std::cmp::Reverse(i.last_played.unwrap_or(0).max(i.created)));
    out
}

pub fn save(paths: &Paths, inst: &Instance) -> anyhow::Result<()> {
    let d = dir(paths, &inst.id);
    std::fs::create_dir_all(&d)?;
    std::fs::write(d.join("instance.json"), serde_json::to_vec_pretty(inst)?)?;
    Ok(())
}

pub fn create(paths: &Paths, name: &str, version: &str, loader: Loader) -> anyhow::Result<Instance> {
    let base = slugify(name);
    let mut id = base.clone();
    let mut n = 2;
    while dir(paths, &id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    let inst = Instance { id, name: name.trim().to_owned(), version: version.to_owned(), loader, created: now(), last_played: None, playtime_secs: 0, loader_version: None };
    for sub in ["mods", "saves", "config", "resourcepacks", "shaderpacks"] {
        std::fs::create_dir_all(dir(paths, &inst.id).join(sub))?;
    }
    save(paths, &inst)?;
    Ok(inst)
}

/// Permanently removes the instance folder (worlds included). UI asks for confirmation first.
pub fn delete(paths: &Paths, id: &str) -> anyhow::Result<()> {
    std::fs::remove_dir_all(dir(paths, id))?;
    Ok(())
}

fn slugify(s: &str) -> String {
    let slug: String = s
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() { "instance".into() } else { slug }
}

/// `YYYY-MM-DD` from unix seconds (no chrono dependency).
pub fn format_date(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn date() {
        assert_eq!(super::format_date(0), "1970-01-01");
        assert_eq!(super::format_date(1_700_000_000), "2023-11-14");
    }
    #[test]
    fn slug() {
        assert_eq!(super::slugify("  My Cool Pack!! 1.21 "), "my-cool-pack-1.21");
    }
}
