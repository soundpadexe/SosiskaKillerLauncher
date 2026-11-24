//! Automatic world backups: before each launch, every world in `saves/` that
//! changed since its last backup is zipped to `backups/<world>/<stamp>.zip`.
//! Only the newest `keep` archives per world are kept.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

fn newest_mtime(dir: &Path) -> SystemTime {
    let mut best = SystemTime::UNIX_EPOCH;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else if let Ok(t) = m.modified() {
                best = best.max(t);
            }
        }
    }
    best
}

fn zip_dir(src: &Path, out: &Path) -> anyhow::Result<()> {
    let tmp = out.with_extension("zip.part");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![src.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(src)?.to_string_lossy().replace('\\', "/");
            if e.file_type()?.is_dir() {
                stack.push(p);
            } else if rel != "session.lock" {
                // A file the game holds open is skipped rather than failing the backup.
                if let Ok(bytes) = std::fs::read(&p) {
                    z.start_file(rel, opts)?;
                    z.write_all(&bytes)?;
                }
            }
        }
    }
    z.finish()?;
    std::fs::rename(tmp, out)?;
    Ok(())
}

pub fn backups_dir(instance: &Path) -> PathBuf {
    instance.join("backups")
}

/// Returns how many worlds were backed up.
pub fn backup_worlds(instance: &Path, keep: usize) -> anyhow::Result<usize> {
    let saves = instance.join("saves");
    let Ok(rd) = std::fs::read_dir(&saves) else { return Ok(0) };
    let mut n = 0;
    for w in rd.flatten().filter(|e| e.path().join("level.dat").exists()) {
        let name = w.file_name().to_string_lossy().into_owned();
        let dest = backups_dir(instance).join(&name);
        std::fs::create_dir_all(&dest)?;
        let mut existing: Vec<PathBuf> = std::fs::read_dir(&dest)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "zip")).collect();
        existing.sort();
        let last = existing.last().and_then(|p| p.metadata().ok()?.modified().ok()).unwrap_or(SystemTime::UNIX_EPOCH);
        if newest_mtime(&w.path()) <= last {
            continue; // unchanged since the last backup
        }
        let stamp = chrono_stamp();
        zip_dir(&w.path(), &dest.join(format!("{stamp}.zip")))?;
        n += 1;
        existing.push(dest.join(format!("{stamp}.zip")));
        while existing.len() > keep.max(1) {
            let _ = std::fs::remove_file(existing.remove(0));
        }
    }
    Ok(n)
}

/// `YYYY-MM-DD_HH-MM-SS` (UTC), sortable as text.
fn chrono_stamp() -> String {
    let s = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
    let (days, rem) = (s / 86_400, s % 86_400);
    // civil-from-days (Howard Hinnant)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn backs_up_only_changed_and_rotates() {
        let dir = std::env::temp_dir().join(format!("skl-bk-{}", std::process::id()));
        let w = dir.join("saves/World");
        std::fs::create_dir_all(w.join("region")).unwrap();
        std::fs::write(w.join("level.dat"), b"x").unwrap();
        std::fs::write(w.join("region/r.0.0.mca"), b"y").unwrap();
        assert_eq!(super::backup_worlds(&dir, 2).unwrap(), 1);
        assert_eq!(super::backup_worlds(&dir, 2).unwrap(), 0, "unchanged world is skipped");
        let n = std::fs::read_dir(dir.join("backups/World")).unwrap().count();
        assert_eq!(n, 1);
        assert_eq!(super::chrono_stamp().len(), 19);
        let _ = std::fs::remove_dir_all(dir);
    }
}
