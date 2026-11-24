//! Per-instance record of installed content (`skl-content.json`) plus a scan
//! of the folders, so hand-dropped files show up too. Disabling a file renames
//! it to `*.disabled`, which every loader ignores.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::models::content::{ContentKind, InstalledEntry, Source};

const MANIFEST: &str = "skl-content.json";

fn load(dir: &Path) -> Vec<InstalledEntry> {
    std::fs::read(dir.join(MANIFEST)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, list: &[InstalledEntry]) -> anyhow::Result<()> {
    let tmp = dir.join(format!("{MANIFEST}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(list)?)?;
    std::fs::rename(tmp, dir.join(MANIFEST))?;
    Ok(())
}

pub fn file_path(dir: &Path, e: &InstalledEntry) -> PathBuf {
    let name = if e.enabled { e.file.clone() } else { format!("{}.disabled", e.file) };
    dir.join(&e.folder).join(name)
}

/// Manifest entries whose file still exists + untracked files found on disk.
pub fn list_all(dir: &Path) -> Vec<InstalledEntry> {
    let mut list: Vec<InstalledEntry> = load(dir).into_iter().filter(|e| file_path(dir, e).exists()).collect();
    let known: HashSet<(String, String)> = list.iter().map(|e| (e.folder.clone(), e.file.clone())).collect();
    for kind in ContentKind::ALL {
        let folder = kind.folder();
        let Ok(rd) = std::fs::read_dir(dir.join(folder)) else { continue };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let (file, enabled) = match name.strip_suffix(".disabled") {
                Some(base) => (base.to_owned(), false),
                None => (name.clone(), true),
            };
            let ext_ok = [".jar", ".zip"].iter().any(|x| file.to_lowercase().ends_with(x));
            if !ext_ok || known.contains(&(folder.to_owned(), file.clone())) {
                continue;
            }
            list.push(InstalledEntry {
                source: Source::Local,
                project_id: file.clone(),
                version_id: String::new(),
                title: file.trim_end_matches(".jar").trim_end_matches(".zip").to_owned(),
                file,
                folder: folder.to_owned(),
                dependency: false,
                enabled,
            });
        }
    }
    list.sort_by(|a, b| a.folder.cmp(&b.folder).then(a.dependency.cmp(&b.dependency)).then(a.title.to_lowercase().cmp(&b.title.to_lowercase())));
    list
}

/// Records freshly downloaded entries; replaces older files of the same project (updates).
pub fn merge(dir: &Path, new: Vec<InstalledEntry>) -> anyhow::Result<()> {
    let mut list = load(dir);
    for n in new {
        if let Some(pos) = list.iter().position(|e| e.source == n.source && e.project_id == n.project_id) {
            let old = list.remove(pos);
            if old.file != n.file || old.folder != n.folder {
                let _ = std::fs::remove_file(file_path(dir, &old));
            }
        }
        list.push(n);
    }
    save(dir, &list)
}

pub fn remove(dir: &Path, e: &InstalledEntry) -> anyhow::Result<()> {
    let p = file_path(dir, e);
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    let mut list = load(dir);
    list.retain(|x| !(x.folder == e.folder && x.file == e.file));
    save(dir, &list)
}

pub fn set_enabled(dir: &Path, e: &InstalledEntry, on: bool) -> anyhow::Result<()> {
    let from = file_path(dir, e);
    let to = file_path(dir, &InstalledEntry { enabled: on, ..e.clone() });
    if from != to {
        std::fs::rename(from, to)?;
    }
    let mut list = load(dir);
    if let Some(x) = list.iter_mut().find(|x| x.folder == e.folder && x.file == e.file) {
        x.enabled = on;
        save(dir, &list)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_toggle_remove_roundtrip() {
        let dir = std::env::temp_dir().join(format!("skl-content-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("mods")).unwrap();
        std::fs::write(dir.join("mods/a-1.jar"), b"x").unwrap();
        std::fs::write(dir.join("mods/hand.jar"), b"x").unwrap();
        let e = InstalledEntry {
            source: Source::Modrinth, project_id: "A".into(), version_id: "v1".into(), title: "A".into(),
            file: "a-1.jar".into(), folder: "mods".into(), dependency: false, enabled: true,
        };
        merge(&dir, vec![e.clone()]).unwrap();
        let all = list_all(&dir);
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|x| x.source == Source::Local && x.file == "hand.jar"));

        set_enabled(&dir, &e, false).unwrap();
        assert!(dir.join("mods/a-1.jar.disabled").exists());

        // update to v2 replaces the old file
        std::fs::write(dir.join("mods/a-2.jar"), b"y").unwrap();
        merge(&dir, vec![InstalledEntry { version_id: "v2".into(), file: "a-2.jar".into(), ..e.clone() }]).unwrap();
        assert!(!dir.join("mods/a-1.jar.disabled").exists());

        let cur = list_all(&dir).into_iter().find(|x| x.project_id == "A").unwrap();
        remove(&dir, &cur).unwrap();
        assert!(!dir.join("mods/a-2.jar").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
