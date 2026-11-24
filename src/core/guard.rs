//! Safety Guard: scans an instance's mods before launch.
//!
//! * Malware signatures inside every `.class` (and one level of jar-in-jar):
//!   Fractureiser (2023) stages + known C2 addresses, Discord token grabbers.
//! * Suspicious capabilities: browser credential paths, Discord webhooks.
//! * Conflicts that would crash the game: duplicate mod ids, mods for another loader.
//! * Host check for Fractureiser stage-2 persistence files.
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::core::instances::Loader;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Known malware — never launch.
    Malware,
    /// Would crash or silently not load — block until fixed.
    Conflict,
    /// Unusual capability for a client mod — warn.
    Suspicious,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub severity: Severity,
    /// Path relative to the instance dir (e.g. `mods/foo.jar`), or absolute for host files.
    pub file: String,
    pub what: String,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub scanned: usize,
}

impl Report {
    pub fn blocking(&self) -> bool {
        self.findings.iter().any(|f| f.severity != Severity::Suspicious)
    }
}

const SIGNATURES: &[(&str, &[u8], Severity)] = &[
    ("Fractureiser: stage-0 injector", b"dev/neko/nekoinjector", Severity::Malware),
    ("Fractureiser: client payload", b"dev/neko/nekoclient", Severity::Malware),
    ("Fractureiser: C2 address", b"85.217.144.130", Severity::Malware),
    ("Fractureiser: C2 address", b"107.189.3.101", Severity::Malware),
    ("Discord token grabber", b"Local Storage\\leveldb", Severity::Malware),
    ("Discord token grabber", b"Local Storage/leveldb", Severity::Malware),
    ("Reads saved browser passwords", b"Login Data", Severity::Suspicious),
    ("Reads browser encryption key", b"Local State", Severity::Suspicious),
    ("Sends data to a Discord webhook", b"discord.com/api/webhooks", Severity::Suspicious),
    ("Sends data to a Discord webhook", b"discordapp.com/api/webhooks", Severity::Suspicious),
];

/// Returns (signature hits, mod ids, is_forge_only) for one jar's bytes.
fn scan_jar(bytes: &[u8], depth: u8, hits: &mut Vec<(&'static str, Severity)>) -> (Vec<String>, bool, bool) {
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else { return (vec![], false, false) };
    let finders: Vec<_> = SIGNATURES.iter().map(|(n, s, sev)| (*n, memchr::memmem::Finder::new(s), *sev)).collect();
    let mut ids = Vec::new();
    let (mut has_fabric, mut has_forge) = (false, false);
    for i in 0..zip.len() {
        let Ok(mut e) = zip.by_index(i) else { continue };
        let name = e.name().to_owned();
        let interesting = name.ends_with(".class")
            || name == "fabric.mod.json"
            || name == "quilt.mod.json"
            || name.ends_with("mods.toml")
            || name == "mcmod.info"
            || (depth == 0 && name.starts_with("META-INF/jars/") && name.ends_with(".jar"));
        if !interesting || e.size() > 64 * 1024 * 1024 {
            continue;
        }
        let mut buf = Vec::with_capacity(e.size() as usize);
        if e.read_to_end(&mut buf).is_err() {
            continue;
        }
        match name.as_str() {
            "fabric.mod.json" | "quilt.mod.json" => {
                has_fabric = true;
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf) {
                    let id = v.get("id").or_else(|| v.get("quilt_loader").and_then(|q| q.get("id")));
                    if let Some(id) = id.and_then(|i| i.as_str()) {
                        ids.push(id.to_owned());
                    }
                }
            }
            n if n.ends_with("mods.toml") || n == "mcmod.info" => has_forge = true,
            n if n.ends_with(".jar") => {
                scan_jar(&buf, depth + 1, hits); // nested ids are bundled, not top-level mods
            }
            _ => {
                for (label, f, sev) in &finders {
                    if f.find(&buf).is_some() && !hits.iter().any(|(l, _)| l == label) {
                        hits.push((label, *sev));
                    }
                }
            }
        }
    }
    (ids, has_fabric, has_forge)
}

pub fn scan(instance_dir: &Path, loader: Loader) -> Report {
    let mut report = Report::default();
    let mods = instance_dir.join("mods");
    let mut seen_ids: HashMap<String, String> = HashMap::new();
    let mut jars: Vec<PathBuf> = std::fs::read_dir(&mods)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jar"))
        .collect();
    jars.sort();

    for jar in jars {
        let rel = format!("mods/{}", jar.file_name().unwrap_or_default().to_string_lossy());
        let Ok(bytes) = std::fs::read(&jar) else { continue };
        report.scanned += 1;
        let mut hits = Vec::new();
        let (ids, has_fabric, has_forge) = scan_jar(&bytes, 0, &mut hits);
        let forgeish = matches!(loader, Loader::Forge | Loader::NeoForge);
        for (what, severity) in hits {
            report.findings.push(Finding { severity, file: rel.clone(), what: what.to_owned() });
        }
        if loader == Loader::Vanilla {
            report.findings.push(Finding { severity: Severity::Conflict, file: rel.clone(), what: "Instance has no mod loader — this mod is ignored".into() });
        } else if !forgeish && has_forge && !has_fabric {
            report.findings.push(Finding { severity: Severity::Conflict, file: rel.clone(), what: format!("Made for Forge/NeoForge, won't load on {}", loader.name()) });
        } else if forgeish && has_fabric && !has_forge {
            report.findings.push(Finding { severity: Severity::Conflict, file: rel.clone(), what: format!("Made for Fabric/Quilt, won't load on {}", loader.name()) });
        }
        for id in ids {
            if let Some(other) = seen_ids.get(&id) {
                report.findings.push(Finding { severity: Severity::Conflict, file: rel.clone(), what: format!("Duplicate of «{id}» (also in {other})") });
            } else {
                seen_ids.insert(id, rel.clone());
            }
        }
    }

    // Fractureiser stage-2 persistence on the host
    let mut host = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let edge = PathBuf::from(local).join("Microsoft Edge");
        host.push(edge.join("libWebGL64.jar"));
        host.push(edge.join(".ref"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        host.push(PathBuf::from(home).join(".config/.data/lib.jar"));
    }
    for p in host.into_iter().filter(|p| p.exists()) {
        report.findings.push(Finding { severity: Severity::Malware, file: p.display().to_string(), what: "Fractureiser stage-2 file on this PC".into() });
    }
    report.findings.sort_by_key(|f| f.severity);
    report
}

/// Moves a file out of the instance into `<data>/quarantine/` (kept, never deleted).
pub fn quarantine(data_root: &Path, instance_dir: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let src = if Path::new(rel).is_absolute() { PathBuf::from(rel) } else { instance_dir.join(rel) };
    let q = data_root.join("quarantine");
    std::fs::create_dir_all(&q)?;
    let stamp = crate::core::instances::now();
    let dst = q.join(format!("{stamp}-{}.quarantined", src.file_name().unwrap_or_default().to_string_lossy()));
    std::fs::rename(&src, &dst).or_else(|_| std::fs::copy(&src, &dst).and_then(|_| std::fs::remove_file(&src)))?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut out);
        for (n, b) in entries {
            z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn detects_malware_duplicates_and_wrong_loader() {
        let dir = std::env::temp_dir().join(format!("skl-guard-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("mods")).unwrap();
        let clean = jar(&[("fabric.mod.json", br#"{"id":"cool"}"#), ("a/B.class", b"\xCA\xFE\xBA\xBEhello")]);
        std::fs::write(dir.join("mods/cool-1.jar"), &clean).unwrap();
        std::fs::write(dir.join("mods/cool-2.jar"), &clean).unwrap();
        // infection hidden in a nested jar
        let inner = jar(&[("x/U.class", b"....dev/neko/nekoinjector/Loader....")]);
        std::fs::write(dir.join("mods/evil.jar"), jar(&[("fabric.mod.json", br#"{"id":"evil"}"#), ("META-INF/jars/lib.jar", &inner)])).unwrap();
        std::fs::write(dir.join("mods/forgey.jar"), jar(&[("META-INF/mods.toml", b"modId=\"f\"")])).unwrap();

        let r = scan(&dir, Loader::Fabric);
        let has = |sev: Severity, file: &str, needle: &str| r.findings.iter().any(|f| f.severity == sev && f.file.ends_with(file) && f.what.contains(needle));
        assert!(has(Severity::Malware, "evil.jar", "Fractureiser"), "{r:#?}");
        assert!(has(Severity::Conflict, "cool-2.jar", "Duplicate"), "{r:#?}");
        assert!(has(Severity::Conflict, "forgey.jar", "Forge"), "{r:#?}");
        assert!(!r.findings.iter().any(|f| f.file.ends_with("cool-1.jar")));
        assert!(r.blocking());

        let q = quarantine(&dir, &dir, "mods/evil.jar").unwrap();
        assert!(q.exists() && !dir.join("mods/evil.jar").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod live {
    /// False-positive check on real mods: `SKL_GUARD_DIR=<instance> cargo test -- --ignored guard_dir --nocapture`
    #[test]
    #[ignore]
    fn guard_dir() {
        let dir = std::path::PathBuf::from(std::env::var("SKL_GUARD_DIR").unwrap());
        let t = std::time::Instant::now();
        let r = super::scan(&dir, crate::core::instances::Loader::Fabric);
        println!("scanned {} jars in {:?}: {:#?}", r.scanned, t.elapsed(), r.findings);
    }
}

#[cfg(test)]
mod loader_tests {
    #[test]
    fn fabric_mod_in_forge_instance_is_conflict() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("skl-guard-forge-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("mods")).unwrap();
        let mut out = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut out);
        z.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(br#"{"id":"fab"}"#).unwrap();
        z.finish().unwrap();
        std::fs::write(dir.join("mods/fab.jar"), out.into_inner()).unwrap();
        let r = super::scan(&dir, crate::core::instances::Loader::NeoForge);
        assert!(r.findings.iter().any(|f| f.what.contains("Fabric/Quilt")), "{r:#?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
