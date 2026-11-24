//! Crash Doctor: reads the newest crash report / latest.log / launcher log of an
//! instance and turns the stack-trace soup into one human cause, the mod file
//! to blame (if any) and a fix the UI can offer as a button.
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum Cause {
    /// A specific mod threw during startup / a mixin failed.
    BrokenMod,
    /// The loader refused to start: a mod needs another mod (or another version of it).
    MissingDependency { needs: String },
    /// Two mods that cannot coexist.
    Incompatible,
    OutOfMemory,
    /// A mod was built for a newer Java / Minecraft.
    WrongJava,
    /// Native crash in the graphics driver.
    Graphics,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Diagnosis {
    pub cause: Cause,
    /// Mod id named by the logs (e.g. `debugify`).
    pub mod_id: Option<String>,
    /// Jar in `mods/` that provides that id — what "Disable" acts on.
    pub mod_file: Option<String>,
    /// The single most telling log line, shown verbatim.
    pub evidence: String,
    /// File the evidence came from (for "Open log").
    pub log: Option<PathBuf>,
}

fn newest(dir: &Path, ext: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(ext))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

fn read(p: &Path) -> String {
    // Logs can be huge; the cause is near the top of a crash report and near the end of a log.
    let bytes = std::fs::read(p).unwrap_or_default();
    let s = String::from_utf8_lossy(&bytes);
    if s.len() > 400_000 {
        let mut cut = s.len() - 300_000;
        while !s.is_char_boundary(cut) {
            cut += 1;
        }
        format!("{}\n{}", &s[..100_000.min(s.len())], &s[cut..])
    } else {
        s.into_owned()
    }
}

/// `text` between the first `start` and the next `end` after it.
fn between<'a>(text: &'a str, start: &str, end: char) -> Option<&'a str> {
    let i = text.find(start)? + start.len();
    let rest = &text[i..];
    Some(&rest[..rest.find(end)?])
}

fn line_with<'a>(text: &'a str, needle: &str) -> Option<&'a str> {
    text.lines().find(|l| l.contains(needle)).map(str::trim)
}

/// Mod id → jar file name, read from fabric.mod.json / quilt.mod.json / mods.toml.
pub fn mod_files(mods: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(mods) else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".jar") {
            continue;
        }
        let Ok(f) = std::fs::File::open(e.path()) else { continue };
        let Ok(mut zip) = zip::ZipArchive::new(f) else { continue };
        let mut text = String::new();
        for meta in ["fabric.mod.json", "quilt.mod.json", "META-INF/mods.toml", "META-INF/neoforge.mods.toml"] {
            if let Ok(mut m) = zip.by_name(meta) {
                let _ = m.read_to_string(&mut text);
                let ids: Vec<String> = if meta.ends_with(".json") {
                    serde_json::from_str::<serde_json::Value>(&text)
                        .ok()
                        .and_then(|v| v.get("id").or_else(|| v.pointer("/quilt_loader/id")).and_then(|x| x.as_str()).map(|s| vec![s.to_owned()]))
                        .unwrap_or_default()
                } else {
                    text.lines()
                        .filter_map(|l| l.trim().strip_prefix("modId"))
                        .filter_map(|r| r.split('"').nth(1).map(str::to_owned))
                        .collect()
                };
                for id in ids {
                    out.push((id, name.clone()));
                }
                break;
            }
        }
    }
    out
}

fn file_for(mods: &[(String, String)], id: &str) -> Option<String> {
    mods.iter().find(|(m, _)| m.eq_ignore_ascii_case(id)).map(|(_, f)| f.clone())
}

pub fn analyze(instance: &Path) -> Option<Diagnosis> {
    let report = newest(&instance.join("crash-reports"), ".txt");
    let latest = instance.join("logs").join("latest.log");
    let launch = instance.join("logs").join("skl-launch.log");
    let mods = mod_files(&instance.join("mods"));

    // Only trust a crash report written during this session (newer than the launch log).
    let fresh = |p: &Path| {
        let t = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        matches!((t(p), t(&launch)), (Some(a), Some(b)) if a >= b)
    };
    let mut sources: Vec<PathBuf> = Vec::new();
    if let Some(r) = report.filter(|r| fresh(r)) {
        sources.push(r);
    }
    for p in [latest, launch.clone()] {
        if p.exists() {
            sources.push(p);
        }
    }
    let mut best: Option<Diagnosis> = None;
    for src in sources {
        let text = read(&src);
        if let Some(mut d) = diagnose(&text, &mods) {
            d.log = Some(src);
            let specific = d.cause != Cause::Unknown;
            if specific {
                return Some(d);
            }
            best.get_or_insert(d);
        }
    }
    best
}

/// Pure text analysis (testable without files).
pub fn diagnose(text: &str, mods: &[(String, String)]) -> Option<Diagnosis> {
    let d = |cause, mod_id: Option<String>, evidence: &str| {
        let mod_file = mod_id.as_deref().and_then(|id| file_for(mods, id));
        Some(Diagnosis { cause, mod_id, mod_file, evidence: evidence.chars().take(220).collect(), log: None })
    };
    if let Some(l) = line_with(text, "java.lang.OutOfMemoryError") {
        return d(Cause::OutOfMemory, None, l);
    }
    // Fabric dependency resolution: "- Mod 'Sodium' (sodium) 0.5 requires version 1.20.4 of 'Minecraft' (minecraft)..."
    if text.contains("Incompatible mods found") || text.contains("Mod resolution failed") || text.contains("Some of your mods are incompatible") {
        if let Some(l) = text.lines().map(str::trim).find(|l| l.starts_with("- Mod '") && (l.contains("requires") || l.contains("is incompatible") || l.contains("which is missing"))) {
            let id = between(l, "(", ')').map(str::to_owned);
            let needs = l.rsplit_once(" of ").map(|(_, r)| r.split(',').next().unwrap_or(r).trim().trim_end_matches('.').to_owned()).unwrap_or_default();
            let cause = if l.contains("is incompatible") { Cause::Incompatible } else { Cause::MissingDependency { needs } };
            return d(cause, id, l);
        }
    }
    // Forge: "Missing or unsupported mandatory dependencies: Mod ID: 'x', Requested by: 'y'"
    if let Some(l) = line_with(text, "Missing or unsupported mandatory dependencies") {
        let rest = &text[text.find(l).unwrap_or(0)..];
        let needs = between(rest, "Mod ID: '", '\'').unwrap_or("?").to_owned();
        let by = between(rest, "Requested by: '", '\'').map(str::to_owned);
        return d(Cause::MissingDependency { needs }, by, l);
    }
    if let Some(l) = line_with(text, "UnsupportedClassVersionError").or_else(|| line_with(text, "compiled by a more recent version of the Java")) {
        return d(Cause::WrongJava, None, l);
    }
    if let Some(l) = line_with(text, "provided by '") {
        return d(Cause::BrokenMod, between(l, "provided by '", '\'').map(str::to_owned), l);
    }
    // Mixin failures name the owning mod.
    for key in ["from mod ", "Mixin apply for mod "] {
        if let Some(l) = text.lines().find(|l| l.contains(key) && (l.contains("Mixin") || l.contains("mixin"))) {
            let id: String = l[l.find(key).unwrap() + key.len()..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
            if !id.is_empty() {
                return d(Cause::BrokenMod, Some(id), l.trim());
            }
        }
    }
    // Forge crash reports: "Mod File: /…/mods/foo.jar" under the suspected mod.
    if let Some(l) = line_with(text, "Mod File: ") {
        let file = l.rsplit(['/', '\\']).next().unwrap_or("").to_owned();
        if file.ends_with(".jar") && mods.iter().any(|(_, f)| *f == file) {
            let id = mods.iter().find(|(_, f)| *f == file).map(|(i, _)| i.clone());
            return Some(Diagnosis { cause: Cause::BrokenMod, mod_id: id, mod_file: Some(file), evidence: l.chars().take(220).collect(), log: None });
        }
    }
    if let Some(l) = line_with(text, "EXCEPTION_ACCESS_VIOLATION").or_else(|| line_with(text, "atio6axx")).or_else(|| line_with(text, "nvoglv")).or_else(|| line_with(text, "ig9icd")) {
        return d(Cause::Graphics, None, l);
    }
    let l = text.lines().map(str::trim).find(|l| l.starts_with("Caused by:") || l.starts_with("Exception in thread") || l.starts_with("java.lang.")).or_else(|| line_with(text, "Description:"))?;
    d(Cause::Unknown, None, l)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mods() -> Vec<(String, String)> {
        vec![("debugify".into(), "Debugify-1.21.jar".into()), ("sodium".into(), "sodium-0.6.jar".into())]
    }
    #[test]
    fn finds_culprits() {
        let r = diagnose("java.lang.RuntimeException: Could not execute entrypoint stage 'client' due to errors, provided by 'debugify' at 'x'!", &mods()).unwrap();
        assert_eq!((r.cause, r.mod_file.as_deref()), (Cause::BrokenMod, Some("Debugify-1.21.jar")));

        let fab = "Incompatible mods found!\n net.fabricmc...\n\t - Mod 'Sodium' (sodium) 0.6 requires version 1.21.4 of 'Minecraft' (minecraft), but only the wrong version is present: 1.21.1!";
        let r = diagnose(fab, &mods()).unwrap();
        assert!(matches!(r.cause, Cause::MissingDependency { .. }));
        assert_eq!(r.mod_file.as_deref(), Some("sodium-0.6.jar"));

        let r = diagnose("Exception: java.lang.OutOfMemoryError: Java heap space", &[]).unwrap();
        assert_eq!(r.cause, Cause::OutOfMemory);

        let r = diagnose("[main/ERROR]: Mixin apply for mod sodium failed sodium.mixins.json:Foo", &mods()).unwrap();
        assert_eq!(r.mod_id.as_deref(), Some("sodium"));

        let forge = "Missing or unsupported mandatory dependencies:\n\tMod ID: 'architectury', Requested by: 'rei', Expected range: '[13,)'";
        let r = diagnose(forge, &[]).unwrap();
        assert_eq!(r.cause, Cause::MissingDependency { needs: "architectury".into() });
        assert_eq!(r.mod_id.as_deref(), Some("rei"));
    }
}
