//! Cover art pipeline: real game artwork from the user's own game files.
use crate::core::gameart;
use crate::core::launch::download::{fetch_all, get_bytes, Dl};
use crate::core::launch::manifest::{AssetIndex, VersionJson};
use crate::core::launch::Progress;
use crate::core::paths::Paths;

const PANORAMA_KEY: &str = "minecraft/textures/gui/title/background/panorama_0.png";

/// Version's title panorama (jar → asset index → single-file download), else a
/// block-texture landscape. `None` only when nothing can be obtained (offline, unknown).
pub async fn cover(client: &reqwest::Client, paths: &Paths, id: &str, version_url: Option<&str>) -> Option<image::RgbaImage> {
    let jar = paths.versions().join(id).join(format!("{id}.jar"));
    if !jar.exists() {
        super::mojang::ensure_client_jar(client, version_url?, &paths.versions(), id).await.ok()?;
    }
    let j = jar.clone();
    if let Some(p) = tokio::task::spawn_blocking(move || gameart::jar_panorama(&j)).await.ok().flatten() {
        return Some(p);
    }
    if let Some(p) = asset_panorama(client, paths, id).await {
        return Some(p);
    }
    let seed = crate::ui::cover::seed_of(id);
    tokio::task::spawn_blocking(move || gameart::extract(&jar, seed)).await.ok().flatten()
}

async fn asset_panorama(client: &reqwest::Client, paths: &Paths, id: &str) -> Option<image::RgbaImage> {
    let vj: VersionJson = serde_json::from_slice(&std::fs::read(paths.versions().join(id).join(format!("{id}.json"))).ok()?).ok()?;
    let ai = &vj.asset_index;
    let idx_path = paths.assets().join("indexes").join(format!("{}.json", ai.id));
    if !idx_path.exists() {
        std::fs::create_dir_all(idx_path.parent()?).ok()?;
        std::fs::write(&idx_path, get_bytes(client, &ai.url).await.ok()?).ok()?;
    }
    let index: AssetIndex = serde_json::from_slice(&std::fs::read(&idx_path).ok()?).ok()?;
    let obj = index.objects.get(PANORAMA_KEY)?;
    let path = paths.assets().join("objects").join(&obj.hash[..2]).join(&obj.hash);
    if !path.exists() {
        let url = format!("https://resources.download.minecraft.net/{}/{}", &obj.hash[..2], obj.hash);
        fetch_all(client, vec![Dl { url, path: path.clone(), sha1: Some(obj.hash.clone()), size: obj.size }], &Progress::new()).await.ok()?;
    }
    let bytes = std::fs::read(&path).ok()?;
    tokio::task::spawn_blocking(move || Some(gameart::shrink(image::load_from_memory(&bytes).ok()?.to_rgba8()))).await.ok().flatten()
}

#[cfg(test)]
mod tests {
    /// `SKL_ART_OUT=<dir> cargo test -- --ignored covers_live --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn covers_live() {
        let out = std::path::PathBuf::from(std::env::var("SKL_ART_OUT").unwrap());
        let paths = crate::core::paths::Paths::init().unwrap();
        let client = crate::api::http_client();
        let list = crate::api::mojang::fetch_versions(&client, &paths.cache()).await.unwrap();
        for id in ["1.21.4", "1.20.1", "1.12.2", "1.8.9", "b1.7.3"] {
            let url = list.versions.iter().find(|v| v.id == id).map(|v| v.url.clone());
            match super::cover(&client, &paths, id, url.as_deref()).await {
                Some(img) => {
                    println!("{id}: {}x{}", img.width(), img.height());
                    img.save(out.join(format!("cover_{id}.png"))).unwrap();
                }
                None => println!("{id}: none"),
            }
        }
    }
}
