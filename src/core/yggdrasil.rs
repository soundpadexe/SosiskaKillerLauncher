//! Tiny local skin server that lives inside the launcher.
//!
//! * Modern clients (with Mojang authlib, 1.7.2+) are pointed here by
//!   authlib-injector and talk a Yggdrasil-compatible API. Texture properties
//!   are signed with our own RSA key, which the agent trusts via `/` metadata.
//! * Legacy clients (Classic → 1.6) use plain HTTP to dead Amazon/minecraft.net
//!   URLs; they get `-Dhttp.proxyHost` pointed here and we answer those paths.
use std::sync::{Arc, RwLock};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::{Pkcs1v15Sign, RsaPrivateKey};
use sha1::Digest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Default)]
pub struct Player {
    pub name: String,
    /// Offline UUID without dashes.
    pub uuid: String,
    pub skin: Option<Vec<u8>>,
    pub slim: bool,
    pub cape: Option<Vec<u8>>,
}

struct Shared {
    key: RsaPrivateKey,
    public_pem: String,
    player: RwLock<Player>,
    port: u16,
    /// Recent request paths (bounded) — diagnostics and tests.
    hits: std::sync::Mutex<Vec<String>>,
}

#[derive(Clone)]
pub struct SkinServer {
    shared: Arc<Shared>,
}

impl SkinServer {
    /// Loads (or creates once) the signing key and binds 127.0.0.1 on a free port.
    pub async fn start(key_path: std::path::PathBuf) -> anyhow::Result<Self> {
        let key = tokio::task::spawn_blocking(move || -> anyhow::Result<RsaPrivateKey> {
            if let Ok(pem) = std::fs::read_to_string(&key_path) {
                if let Ok(k) = RsaPrivateKey::from_pkcs8_pem(&pem) {
                    return Ok(k);
                }
            }
            let k = RsaPrivateKey::new(&mut rand::thread_rng(), 2048)?;
            if let Some(p) = key_path.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&key_path, k.to_pkcs8_pem(LineEnding::LF)?.as_bytes())?;
            Ok(k)
        })
        .await??;
        let public_pem = key.to_public_key().to_public_key_pem(LineEnding::LF)?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Shared { key, public_pem, player: RwLock::new(Player::default()), port, hits: Default::default() });
        let s2 = shared.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let s = s2.clone();
                tokio::spawn(async move {
                    let _ = handle(sock, &s).await;
                });
            }
        });
        Ok(Self { shared })
    }

    pub fn port(&self) -> u16 {
        self.shared.port
    }

    #[cfg(test)]
    pub fn hits(&self) -> Vec<String> {
        self.shared.hits.lock().unwrap().clone()
    }

    pub fn set_player(&self, p: Player) {
        *self.shared.player.write().unwrap() = p;
    }
}

struct Req {
    method: String,
    path: String,
    query: String,
    body: Vec<u8>,
}

async fn handle(mut sock: tokio::net::TcpStream, s: &Shared) -> anyhow::Result<()> {
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 32 * 1024 {
            return Ok(());
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("").to_owned();
    let target = first.next().unwrap_or("/").to_owned();
    let len: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0)
        .min(1 << 20);
    let mut body = buf[head_end..].to_vec();
    while body.len() < len {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    // Proxy requests carry an absolute URL: keep only the path.
    let target = match target.strip_prefix("http://") {
        Some(rest) => rest.find('/').map(|i| rest[i..].to_owned()).unwrap_or_else(|| "/".into()),
        None => target,
    };
    let (path, query) = target.split_once('?').map(|(a, b)| (a.to_owned(), b.to_owned())).unwrap_or((target.clone(), String::new()));
    {
        let mut h = s.hits.lock().unwrap();
        if h.len() >= 200 {
            h.remove(0);
        }
        h.push(format!("{method} {path}"));
    }
    let (status, ctype, out) = route(&Req { method, path, query, body }, s);
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        _ => "Not Found",
    };
    let header = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", out.len());
    sock.write_all(header.as_bytes()).await?;
    sock.write_all(&out).await?;
    sock.shutdown().await?;
    Ok(())
}

type Resp = (u16, &'static str, Vec<u8>);

fn json(v: serde_json::Value) -> Resp {
    (200, "application/json; charset=utf-8", v.to_string().into_bytes())
}
fn png(b: Vec<u8>) -> Resp {
    (200, "image/png", b)
}
const NONE: Resp = (204, "text/plain", Vec::new());
const NOT_FOUND: Resp = (404, "text/plain", Vec::new());

fn route(r: &Req, s: &Shared) -> Resp {
    let p = s.player.read().unwrap().clone();
    let is_me = |name: &str| !p.name.is_empty() && name.eq_ignore_ascii_case(&p.name);
    let path = r.path.as_str();

    // ---- legacy clients (via proxy)
    let legacy_name = |prefix: &str| path.strip_prefix(prefix).map(|n| n.trim_end_matches(".png").to_owned());
    if let Some(n) = legacy_name("/MinecraftSkins/").or_else(|| legacy_name("/skin/")) {
        return match (&p.skin, is_me(&n)) {
            (Some(b), true) => png(super::skins::legacy_crop(b)),
            _ => NOT_FOUND,
        };
    }
    let cloak_user = legacy_name("/MinecraftCloaks/")
        .or_else(|| (path == "/cloak/get.jsp").then(|| r.query.split('&').find_map(|kv| kv.strip_prefix("user=")).unwrap_or("").to_owned()));
    if let Some(n) = cloak_user {
        return match (&p.cape, is_me(&n)) {
            (Some(b), true) => png(b.clone()),
            _ => NOT_FOUND,
        };
    }

    // ---- Yggdrasil API (authlib-injector)
    match (r.method.as_str(), path) {
        ("GET", "/") => json(serde_json::json!({
            "meta": { "serverName": "SosiskaKillerLauncher", "implementationName": "SKL", "implementationVersion": env!("CARGO_PKG_VERSION"),
                      "feature.non_email_login": true },
            "skinDomains": ["127.0.0.1", "localhost"],
            "signaturePublickey": s.public_pem,
        })),
        ("POST", "/sessionserver/session/minecraft/join") => NONE,
        ("GET", "/sessionserver/session/minecraft/hasJoined") => {
            let user = r.query.split('&').find_map(|kv| kv.strip_prefix("username=")).unwrap_or("");
            if is_me(user) { json(profile(&p, s)) } else { NONE }
        }
        ("POST", "/api/profiles/minecraft") => {
            let names: Vec<String> = serde_json::from_slice(&r.body).unwrap_or_default();
            let hit: Vec<_> = names.iter().filter(|n| is_me(n)).map(|_| serde_json::json!({ "id": p.uuid, "name": p.name })).collect();
            json(serde_json::Value::Array(hit))
        }
        ("GET", "/minecraftservices/player/attributes") => json(serde_json::json!({
            "privileges": { "onlineChat": {"enabled": true}, "multiplayerServer": {"enabled": true},
                            "multiplayerRealms": {"enabled": false}, "telemetry": {"enabled": false} },
            "profanityFilterPreferences": { "profanityFilterOn": false },
        })),
        ("GET", "/minecraftservices/privacy/blocklist") => json(serde_json::json!({ "blockedProfiles": [] })),
        ("GET", _) => {
            if let Some(id) = path.strip_prefix("/sessionserver/session/minecraft/profile/") {
                return if id.replace('-', "").eq_ignore_ascii_case(&p.uuid) { json(profile(&p, s)) } else { NONE };
            }
            if let Some(n) = path.strip_prefix("/api/users/profiles/minecraft/") {
                return if is_me(n) { json(serde_json::json!({ "id": p.uuid, "name": p.name })) } else { NONE };
            }
            if let Some(h) = path.strip_prefix("/textures/skin/") {
                return match &p.skin {
                    Some(b) if super::skins::hash(b) == h.trim_end_matches(".png") => png(b.clone()),
                    _ => NOT_FOUND,
                };
            }
            if let Some(h) = path.strip_prefix("/textures/cape/") {
                return match &p.cape {
                    Some(b) if super::skins::hash(b) == h.trim_end_matches(".png") => png(b.clone()),
                    _ => NOT_FOUND,
                };
            }
            NOT_FOUND
        }
        _ => NOT_FOUND,
    }
}

/// Profile with a `textures` property signed SHA1withRSA, as authlib expects.
fn profile(p: &Player, s: &Shared) -> serde_json::Value {
    let base = format!("http://127.0.0.1:{}/textures", s.port);
    let mut textures = serde_json::Map::new();
    if let Some(b) = &p.skin {
        let mut skin = serde_json::json!({ "url": format!("{base}/skin/{}.png", super::skins::hash(b)) });
        if p.slim {
            skin["metadata"] = serde_json::json!({ "model": "slim" });
        }
        textures.insert("SKIN".into(), skin);
    }
    if let Some(b) = &p.cape {
        textures.insert("CAPE".into(), serde_json::json!({ "url": format!("{base}/cape/{}.png", super::skins::hash(b)) }));
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let value = B64.encode(
        serde_json::json!({ "timestamp": now, "profileId": p.uuid, "profileName": p.name, "textures": textures }).to_string(),
    );
    let digest = sha1::Sha1::digest(value.as_bytes());
    let signature = s.key.sign(Pkcs1v15Sign::new::<sha1::Sha1>(), &digest).map(|sig| B64.encode(sig)).unwrap_or_default();
    serde_json::json!({
        "id": p.uuid,
        "name": p.name,
        "properties": [{ "name": "textures", "value": value, "signature": signature }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn get(port: u16, raw: &str) -> String {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(raw.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    #[tokio::test]
    async fn serves_signed_profile_and_legacy_skin() {
        let key = std::env::temp_dir().join(format!("skl-ygg-test-{}.pem", std::process::id()));
        let srv = SkinServer::start(key.clone()).await.unwrap();
        let mut img = image::RgbaImage::from_pixel(64, 64, image::Rgba([1, 2, 3, 255]));
        img.put_pixel(0, 0, image::Rgba([9, 9, 9, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img).write_to(&mut png, image::ImageFormat::Png).unwrap();
        srv.set_player(Player { name: "Tester".into(), uuid: "f3d28cb072253cb1baeb2dadd2be89ae".into(), skin: Some(png.into_inner()), slim: true, cape: None });

        let meta = get(srv.port(), "GET / HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(meta.contains("BEGIN PUBLIC KEY"));

        let prof = get(srv.port(), "GET /sessionserver/session/minecraft/profile/f3d28cb072253cb1baeb2dadd2be89ae?unsigned=false HTTP/1.1\r\n\r\n").await;
        let body: serde_json::Value = serde_json::from_str(prof.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        let prop = &body["properties"][0];
        let decoded = String::from_utf8(B64.decode(prop["value"].as_str().unwrap()).unwrap()).unwrap();
        assert!(decoded.contains("\"model\":\"slim\""));
        // signature verifies against the advertised public key
        let pubkey = srv.shared.key.to_public_key();
        let sig = B64.decode(prop["signature"].as_str().unwrap()).unwrap();
        let digest = sha1::Sha1::digest(prop["value"].as_str().unwrap().as_bytes());
        pubkey.verify(Pkcs1v15Sign::new::<sha1::Sha1>(), &digest, &sig).unwrap();

        // legacy proxy request → 64×32 PNG
        let legacy = get(srv.port(), "GET http://s3.amazonaws.com/MinecraftSkins/Tester.png HTTP/1.1\r\nHost: s3.amazonaws.com\r\n\r\n").await;
        assert!(legacy.starts_with("HTTP/1.1 200"));
        let other = get(srv.port(), "GET http://s3.amazonaws.com/MinecraftSkins/Notch.png HTTP/1.1\r\n\r\n").await;
        assert!(other.starts_with("HTTP/1.1 404"));
        let _ = std::fs::remove_file(key);
    }

    /// Live: boot a real client with authlib-injector and confirm it pulls our skin.
    /// `SKL_TEST_VERSION=1.21.4 cargo test -- --ignored skins_live --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn skins_live() {
        use crate::core::launch::{install, Progress};
        let id = std::env::var("SKL_TEST_VERSION").unwrap_or_else(|_| "1.21.4".into());
        let paths = crate::core::paths::Paths::init().unwrap();
        let srv = SkinServer::start(std::env::temp_dir().join("skl-ygg-live.pem")).await.unwrap();
        let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([200, 60, 60, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let uuid = crate::core::auth::offline_uuid("Tester").replace('-', "");
        srv.set_player(Player { name: "Tester".into(), uuid: uuid.clone(), skin: Some(png.into_inner()), slim: false, cape: None });

        let client = crate::api::http_client();
        let list = crate::api::mojang::fetch_versions(&client, &paths.cache()).await.unwrap();
        let v = list.versions.iter().find(|v| v.id == id).unwrap();
        let req = install::LaunchRequest {
            id: v.id.clone(), url: v.url.clone(), legacy: v.era.is_legacy(), nickname: "Tester".into(),
            uuid: crate::core::auth::offline_uuid("Tester"), online: None, memory_mb: 2048,
            game_dir: paths.instances().join(format!("test-skin-{id}")), loader: crate::core::instances::Loader::Vanilla,
            skin_port: Some(srv.port()),
            opts: Default::default(), loader_version: None,
        };
        let mut child = install::run(&client, &paths, &req, &Progress::new()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(45)).await;
        let _ = child.kill();
        let hits = srv.hits();
        println!("server hits: {hits:#?}");
        let log = String::from_utf8_lossy(&std::fs::read(req.game_dir.join("logs/skl-launch.log")).unwrap()).into_owned();
        for l in log.lines().filter(|l| l.contains("authlib-injector")).take(6) {
            println!("{l}");
        }
        if std::env::var_os("SKL_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&req.game_dir);
        }
        // Skins load lazily (when a player model is drawn), so at the title screen we
        // verify the profile fetch and that authlib accepted our signed textures.
        assert!(hits.iter().any(|h| h.contains("/sessionserver/session/minecraft/profile/")), "client never asked for the profile");
        let lower = log.to_lowercase();
        assert!(!lower.contains("tampered") && !lower.contains("signature is missing") && !lower.contains("bad signature"), "authlib rejected textures");
    }
}
