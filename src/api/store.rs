//! Source-agnostic entry points for the content store.
use std::path::Path;

use crate::core::launch::download::{fetch_all, Dl};
use crate::core::launch::Progress;
use crate::models::content::{InstalledEntry, Project, Query, Source, Target};

pub async fn search(client: &reqwest::Client, source: Source, cf_key: &str, q: &Query) -> anyhow::Result<Vec<Project>> {
    match source {
        Source::CurseForge => super::curseforge::search(client, cf_key, q).await,
        _ => super::modrinth::search(client, q).await,
    }
}

pub async fn install(client: &reqwest::Client, cf_key: &str, p: &Project, t: &Target, installed: &[InstalledEntry]) -> anyhow::Result<Vec<InstalledEntry>> {
    match p.source {
        Source::CurseForge => super::curseforge::install(client, cf_key, p, t, installed).await,
        _ => super::modrinth::install(client, p, t, installed).await,
    }
}

pub(super) async fn download(client: &reqwest::Client, url: &str, path: &Path, sha1: Option<String>, size: u64) -> anyhow::Result<()> {
    let prog = Progress::new();
    // Always re-fetch: a same-named file may be a different build.
    let _ = std::fs::remove_file(path);
    fetch_all(client, vec![Dl { url: url.to_owned(), path: path.to_owned(), sha1, size }], &prog).await
}

/// Small async icon fetch → decoded RGBA thumbnail (64 px).
pub async fn fetch_icon(client: &reqwest::Client, url: &str) -> Option<egui::ColorImage> {
    let bytes = client.get(url).send().await.ok()?.error_for_status().ok()?.bytes().await.ok()?;
    tokio::task::spawn_blocking(move || {
        let img = image::load_from_memory(&bytes).ok()?.thumbnail(64, 64).to_rgba8();
        Some(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use crate::core::instances::Loader;
    use crate::models::content::{ContentKind, Query, Source, Target};

    /// Live: search Modrinth, install Mod Menu (requires Fabric API) into a temp instance.
    /// `cargo test -- --ignored modrinth_live --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn modrinth_live() {
        let client = crate::api::http_client();
        let q = Query { text: "mod menu".into(), kind: ContentKind::Mod, game_version: "1.21.4".into(), loader: Loader::Fabric, offset: 0 };
        let hits = super::search(&client, Source::Modrinth, "", &q).await.unwrap();
        println!("hits: {:?}", hits.iter().take(5).map(|h| &h.title).collect::<Vec<_>>());
        let mm = hits.iter().find(|h| h.title == "Mod Menu").expect("Mod Menu in results");
        let keep = std::env::var_os("SKL_STORE_DIR");
        let dir = keep.clone().map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join(format!("skl-store-live-{}", std::process::id())));
        let t = Target { game_version: "1.21.4".into(), loader: Loader::Fabric, dir: dir.clone() };
        let got = super::install(&client, "", mm, &t, &[]).await.unwrap();
        for e in &got {
            println!("installed {} -> {} (dep: {})", e.title, e.file, e.dependency);
            assert!(dir.join(&e.folder).join(&e.file).exists());
        }
        assert!(got.iter().any(|e| e.dependency && e.title.contains("Fabric API")), "Fabric API pulled in as dependency");
        if keep.is_none() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Image for store cards. Skins stay pixel-exact; capes are normalized to the
/// 64×32 grid (OptiFine 46×22 etc.) so previews map UVs correctly.
pub async fn fetch_image(client: &reqwest::Client, url: &str, kind: crate::ui::store::IconKind) -> Option<egui::ColorImage> {
    use crate::ui::store::IconKind;
    if kind == IconKind::Logo {
        return fetch_icon(client, url).await;
    }
    let bytes = client.get(url).send().await.ok()?.error_for_status().ok()?.bytes().await.ok()?;
    tokio::task::spawn_blocking(move || {
        let bytes = if kind == IconKind::Cape { crate::api::skins::normalize_cape(&bytes).ok()? } else { bytes.to_vec() };
        let mut img = image::load_from_memory(&bytes).ok()?.to_rgba8();
        if kind == IconKind::Skin && !looks_complete(&img) {
            return None; // partial uploads (only a head, an arm…) — hidden from the catalog
        }
        if img.width() > 128 {
            img = image::imageops::resize(&img, 128, 128 * img.height() / img.width(), image::imageops::FilterType::Nearest);
        }
        Some(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
    })
    .await
    .ok()
    .flatten()
}

/// A wearable skin has an opaque face and torso; gallery uploads that are just
/// one body part (or blank) fail this.
fn looks_complete(img: &image::RgbaImage) -> bool {
    if img.width() != 64 || !(img.height() == 64 || img.height() == 32) {
        return false;
    }
    let opaque = |x0: u32, y0: u32, w: u32, h: u32| {
        let n = (y0..y0 + h).flat_map(|y| (x0..x0 + w).map(move |x| (x, y))).filter(|&(x, y)| img.get_pixel(x, y)[3] > 200).count();
        n as f32 / (w * h) as f32
    };
    opaque(8, 8, 8, 8) > 0.8 && opaque(20, 20, 8, 12) > 0.8 && opaque(44, 20, 4, 12) > 0.5
}
