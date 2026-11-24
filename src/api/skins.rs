//! Ready-made skins by nickname: Mojang (licensed players) and Ely.by.
use anyhow::{anyhow, Context};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkinSource {
    Mojang,
    ElyBy,
}

/// A player's current skin location (resolved, not downloaded).
#[derive(Clone)]
pub struct Found {
    pub name: String,
    pub skin_url: String,
}

#[derive(Deserialize)]
struct Tex {
    url: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "UPPERCASE")]
struct Textures {
    skin: Option<Tex>,
}

fn valid_name(n: &str) -> bool {
    (1..=16).contains(&n.len()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub async fn lookup(client: &reqwest::Client, source: SkinSource, name: &str) -> anyhow::Result<Found> {
    let name = name.trim();
    if !valid_name(name) {
        return Err(anyhow!("invalid nickname"));
    }
    let tex: Textures = match source {
        SkinSource::Mojang => {
            #[derive(Deserialize)]
            struct Id {
                id: String,
            }
            #[derive(Deserialize)]
            struct Prop {
                name: String,
                value: String,
            }
            #[derive(Deserialize)]
            struct Profile {
                properties: Vec<Prop>,
            }
            #[derive(Deserialize)]
            struct Payload {
                textures: Textures,
            }
            let r = client.get(format!("https://api.mojang.com/users/profiles/minecraft/{name}")).send().await?;
            if r.status() == reqwest::StatusCode::NOT_FOUND || r.status() == reqwest::StatusCode::NO_CONTENT {
                return Err(anyhow!("«{name}» not found on Mojang"));
            }
            let id: Id = r.error_for_status()?.json().await?;
            let prof: Profile = client
                .get(format!("https://sessionserver.mojang.com/session/minecraft/profile/{}", id.id))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let raw = prof.properties.into_iter().find(|p| p.name == "textures").context("no textures")?.value;
            serde_json::from_slice::<Payload>(&B64.decode(raw)?)?.textures
        }
        SkinSource::ElyBy => {
            let r = client.get(format!("https://skinsystem.ely.by/textures/{name}")).send().await?;
            if r.status() == reqwest::StatusCode::NO_CONTENT || r.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(anyhow!("«{name}» has no skin on Ely.by"));
            }
            r.error_for_status()?.json().await?
        }
    };
    let skin_tex = tex.skin.ok_or_else(|| anyhow!("«{name}» uses the default skin"))?;
    Ok(Found { name: name.to_owned(), skin_url: skin_tex.url.replacen("http://", "https://", 1) })
}

// ---------------------------------------------------------------- catalogs

#[derive(Clone, Debug)]
pub struct SkinItem {
    pub id: String,
    pub title: String,
    /// textures.minecraft.net hash — the skin is hosted by Mojang.
    pub hash: String,
}

impl SkinItem {
    pub fn url(&self) -> String {
        format!("https://textures.minecraft.net/texture/{}", self.hash)
    }
}

#[derive(Deserialize)]
struct MsList {
    skins: Vec<MsSkin>,
    pagination: Option<MsPage>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MsSkin {
    uuid: String,
    short_id: Option<String>,
    name: Option<String>,
    texture: String,
}
#[derive(Deserialize)]
struct MsPage {
    next: Option<MsAfter>,
}
#[derive(Deserialize)]
struct MsAfter {
    after: Option<String>,
}

/// MineSkin public gallery (every skin there is uploaded to Mojang's texture server).
/// When `query` is a player name, that player's real skin (Mojang, Ely.by) comes first.
pub async fn browse(client: &reqwest::Client, query: &str, after: Option<&str>) -> anyhow::Result<(Vec<SkinItem>, Option<String>)> {
    let q = query.trim();
    let mut params = vec![("size", "24".to_owned())];
    if !q.is_empty() {
        params.push(("filter", q.to_owned()));
    }
    if let Some(a) = after {
        params.push(("after", a.to_owned()));
    }
    let list: MsList = client.get("https://api.mineskin.org/v2/skins").query(&params).send().await?.error_for_status()?.json().await?;
    let mut items: Vec<SkinItem> = Vec::new();
    if after.is_none() && valid_name(q) {
        for (src, label) in [(SkinSource::Mojang, "Mojang"), (SkinSource::ElyBy, "Ely.by")] {
            if let Ok(f) = lookup(client, src, q).await {
                if let Some(hash) = f.skin_url.rsplit('/').next().filter(|_| f.skin_url.contains("textures.minecraft.net")) {
                    items.push(SkinItem { id: format!("{label}:{q}"), title: format!("{} · {label}", f.name), hash: hash.to_owned() });
                } else if src == SkinSource::ElyBy {
                    // Ely.by-hosted: keep the direct URL in `hash` slot via a marker
                    items.push(SkinItem { id: format!("{label}:{q}"), title: format!("{} · {label}", f.name), hash: format!("ely:{}", f.skin_url) });
                }
            }
        }
    }
    items.extend(list.skins.into_iter().map(|s| SkinItem {
        title: s.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("#{}", s.short_id.clone().unwrap_or_else(|| s.uuid[..8].to_owned()))),
        id: s.uuid,
        hash: s.texture,
    }));
    let next = list.pagination.and_then(|p| p.next).and_then(|n| n.after);
    Ok((items, next))
}

impl SkinItem {
    /// Direct image URL (Mojang texture server, or Ely.by storage).
    pub fn image_url(&self) -> String {
        match self.hash.strip_prefix("ely:") {
            Some(u) => u.to_owned(),
            None => self.url(),
        }
    }

    /// Command that shows this skin to everyone on SkinsRestorer servers.
    pub fn command(&self) -> String {
        format!("/skin url {}", self.image_url())
    }
}

#[derive(Clone, Debug)]
pub struct CapeItem {
    pub provider: String,
    pub owner: String,
    pub url: String,
}

/// All capes a player has across providers (Mojang, OptiFine, LabyMod, …) via capes.dev.
pub async fn capes_of(client: &reqwest::Client, name: &str) -> anyhow::Result<Vec<CapeItem>> {
    let name = name.trim();
    if !valid_name(name) {
        return Err(anyhow!("invalid nickname"));
    }
    let v: serde_json::Value = client.get(format!("https://api.capes.dev/load/{name}")).send().await?.error_for_status()?.json().await?;
    let mut out = Vec::new();
    if let Some(map) = v.as_object() {
        for (provider, c) in map {
            if c.get("exists").and_then(|e| e.as_bool()) != Some(true) {
                continue;
            }
            // `imageUrl` is the raw texture PNG (`capeUrl` is metadata JSON).
            if let Some(url) = c.get("imageUrl").and_then(|u| u.as_str()) {
                let owner = c.get("playerName").and_then(|p| p.as_str()).unwrap_or(name).to_owned();
                out.push(CapeItem { provider: provider.clone(), owner, url: url.to_owned() });
            }
        }
    }
    out.sort_by_key(|c| if c.provider == "minecraft" { 0 } else { 1 });
    Ok(out)
}

/// Brings any cape layout into the vanilla 64×32 grid (OptiFine uses 46×22),
/// keeping HD scale. Returns PNG bytes.
pub fn normalize_cape(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    let (w, h) = img.dimensions();
    let out = if w * 1 == h * 2 {
        img
    } else {
        // OptiFine-style: content in 46×22 units → place onto 64×32 units canvas.
        let unit = (w as f32 / 46.0).round().max(1.0) as u32;
        let mut canvas = image::RgbaImage::new(64 * unit, 32 * unit);
        image::imageops::overlay(&mut canvas, &img, 0, 0);
        canvas
    };
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(out).write_to(&mut buf, image::ImageFormat::Png)?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod catalog_tests {
    use super::*;

    /// `cargo test -- --ignored catalogs_live --nocapture`
    #[tokio::test]
    #[ignore]
    async fn catalogs_live() {
        let c = crate::api::http_client();
        let (page, next) = browse(&c, "", None).await.unwrap();
        println!("gallery: {} items, next={:?}, first={:?}", page.len(), next.is_some(), page.first().map(|i| (&i.title, i.image_url())));
        let (page2, _) = browse(&c, "", next.as_deref()).await.unwrap();
        assert!(page2.first().map(|i| &i.id) != page.first().map(|i| &i.id), "pagination advances");
        let (hits, _) = browse(&c, "jeb_", None).await.unwrap();
        println!("search jeb_: {:?}", hits.iter().take(4).map(|i| &i.title).collect::<Vec<_>>());
        assert!(hits.first().is_some_and(|i| i.title.contains("Mojang")));
        let capes = capes_of(&c, "jeb_").await.unwrap();
        println!("capes jeb_: {:?}", capes.iter().map(|c| &c.provider).collect::<Vec<_>>());
        let img = c.get(&capes[0].url).send().await.unwrap().bytes().await.unwrap();
        let norm = image::load_from_memory(&normalize_cape(&img).unwrap()).unwrap();
        assert_eq!(norm.width(), norm.height() * 2);
    }
}
