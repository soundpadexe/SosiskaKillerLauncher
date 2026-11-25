//! Full vanilla install + launch for any era. Idempotent: everything that is
//! already present (size match) is skipped, so a second launch starts instantly.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::Context;

use super::download::{fetch_all, get_bytes, Dl};
use super::manifest::{maven_path, native_suffix_matches, rules_allow, AssetIndex, VersionJson, OS_NAME};
use super::{authlib, java, loader, Progress, Stage};
use crate::core::instances::Loader;
use crate::core::paths::Paths;

const RESOURCES_URL: &str = "https://resources.download.minecraft.net";
#[cfg(windows)]
const CP_SEP: &str = ";";
#[cfg(not(windows))]
const CP_SEP: &str = ":";

pub struct LaunchRequest {
    pub id: String,
    pub url: String,
    pub legacy: bool,
    pub nickname: String,
    pub uuid: String,
    /// Licensed account: (Minecraft access token, xuid). None = offline.
    pub online: Option<(String, String)>,
    pub memory_mb: u32,
    /// Isolated instance folder used as `--gameDir`.
    pub game_dir: PathBuf,
    pub loader: Loader,
    pub loader_version: Option<String>,
    /// Port of the local skin server, when skins are enabled.
    pub skin_port: Option<u16>,
    /// User options from settings (resolution, server, JVM tuning).
    pub opts: crate::core::settings::GameOpts,
}

pub async fn run(client: &reqwest::Client, paths: &Paths, req: &LaunchRequest, prog: &Progress) -> anyhow::Result<Child> {
    // Safety Guard runs before anything is downloaded or executed.
    prog.set_stage(Stage::Scanning);
    let (dir, loader) = (req.game_dir.clone(), req.loader);
    let report = tokio::task::spawn_blocking(move || crate::core::guard::scan(&dir, loader)).await?;
    let blocking = report.blocking();
    let n = report.findings.len();
    prog.set_guard(report);
    if blocking {
        anyhow::bail!("Safety Guard: {n} problem(s) found");
    }
    prog.set_stage(Stage::Preparing);
    let vdir = paths.versions().join(&req.id);
    std::fs::create_dir_all(&req.game_dir)?;
    std::fs::create_dir_all(&vdir)?;
    let mut profile = match req.loader {
        Loader::Fabric | Loader::Quilt => Some(loader::resolve(client, req.loader, &req.id, req.loader_version.as_deref(), &vdir).await?),
        _ => None, // Forge/NeoForge are installed after vanilla files exist
    };

    // 1. Version JSON (cached for offline relaunch)
    let json_path = vdir.join(format!("{}.json", req.id));
    let bytes = match get_bytes(client, &req.url).await {
        Ok(b) => {
            std::fs::write(&json_path, &b)?;
            b
        }
        Err(e) => std::fs::read(&json_path).map_err(|_| e)?,
    };
    let vj: VersionJson = serde_json::from_slice(&bytes).context("version JSON")?;

    // 2. Java
    let component = vj.java_version.as_ref().map_or("jre-legacy", |j| j.component.as_str());
    let custom = PathBuf::from(req.opts.java_path.trim());
    let java = if !req.opts.java_path.trim().is_empty() && custom.is_file() {
        custom
    } else {
        java::ensure(client, &paths.java(), component, prog).await?
    };
    let java8 = component == "jre-legacy";
    let quick_play = bytes.windows(20).any(|w| w == b"quickPlayMultiplayer");

    // 3. Libraries, natives, client
    prog.set_stage(Stage::Libraries);
    let mut dls = Vec::new();
    let mut classpath: Vec<PathBuf> = Vec::new();
    // Maven name per classpath entry (None = client jar), for loader overrides.
    let mut cp_names: Vec<Option<String>> = Vec::new();
    let mut native_jars = Vec::new();
    // Loader libraries win over vanilla ones with the same group:artifact (e.g. newer ASM).
    let mut taken = std::collections::HashSet::new();
    if let Some(p) = &profile {
        for lib in &p.libraries {
            let base = lib.url.as_deref().unwrap_or(loader::default_maven(req.loader));
            let rel = maven_path(&lib.name);
            let path = paths.libraries().join(&rel);
            let url = format!("{}/{}", base.trim_end_matches('/'), rel);
            dls.push(Dl { url, path: path.clone(), sha1: lib.sha1.clone(), size: lib.size.unwrap_or(0) });
            classpath.push(path);
            cp_names.push(Some(lib.name.clone()));
            taken.insert(loader::lib_key(&lib.name));
        }
    }
    for lib in &vj.libraries {
        if !rules_allow(lib.rules.as_deref().unwrap_or(&[])) || !native_suffix_matches(&lib.name) {
            continue;
        }
        if lib.natives.is_none() && taken.contains(&loader::lib_key(&lib.name)) {
            continue;
        }
        let Some(d) = &lib.downloads else { continue };
        if let Some(art) = &d.artifact {
            let path = paths.libraries().join(art.path.clone().unwrap_or_else(|| maven_path(&lib.name)));
            if !classpath.contains(&path) {
                dls.push(Dl { url: art.url.clone(), path: path.clone(), sha1: Some(art.sha1.clone()), size: art.size });
                classpath.push(path);
                cp_names.push(Some(lib.name.clone()));
            }
        }
        let classifier = lib.natives.as_ref().and_then(|n| n.get(OS_NAME)).map(|k| k.replace("${arch}", "64"));
        if let Some(art) = classifier.and_then(|k| d.classifiers.as_ref()?.get(&k).cloned()) {
            let path = paths.libraries().join(art.path.clone().unwrap_or_else(|| format!("{}-natives", lib.name)));
            dls.push(Dl { url: art.url, path: path.clone(), sha1: Some(art.sha1), size: art.size });
            native_jars.push(path);
        }
    }
    let client_jar = vdir.join(format!("{}.jar", req.id));
    let c = &vj.downloads.client;
    dls.push(Dl { url: c.url.clone(), path: client_jar.clone(), sha1: Some(c.sha1.clone()), size: c.size });
    classpath.push(client_jar.clone());
    cp_names.push(None);
    fetch_all(client, dls, prog).await?;

    // 3b. Forge / NeoForge: run the official installer once, then overlay its version.
    if matches!(req.loader, Loader::Forge | Loader::NeoForge) {
        prog.set_stage(Stage::LoaderInstall);
        // Installer needs a console java (javaw has no stdout) of the same runtime.
        let console_java = java.with_file_name(if cfg!(windows) { "java.exe" } else { "java" });
        let p = loader::install_forge(client, req.loader, &req.id, req.loader_version.as_deref(), paths.root(), &paths.versions(), &console_java, &req.game_dir.join("logs")).await?;
        let mut extra = Vec::new();
        let mut forge_cp = Vec::new();
        let keys: std::collections::HashSet<String> = p.libraries.iter().map(|l| loader::lib_key(&l.name)).collect();
        for lib in &p.libraries {
            let path = paths.libraries().join(lib.rel_path());
            if !path.exists() {
                match lib.download_url(None) {
                    Some(url) => extra.push(Dl { url, path: path.clone(), sha1: lib.sha1(), size: lib.size() }),
                    None => anyhow::bail!("{} library missing after install: {}", req.loader.name(), lib.name),
                }
            }
            if !forge_cp.contains(&path) {
                forge_cp.push(path);
            }
        }
        fetch_all(client, extra, prog).await?;
        // The vanilla launcher runs inheriting versions from a copy of the parent jar
        // named after the child; Forge's ignoreList relies on that name.
        let id = p.version_name.clone().unwrap_or_else(|| req.id.clone());
        let child_jar = loader::forge_client_jar(&paths.versions(), &id);
        if !child_jar.exists() {
            std::fs::create_dir_all(child_jar.parent().unwrap())?;
            std::fs::copy(&client_jar, &child_jar)?;
        }
        for (path, name) in classpath.iter().zip(cp_names.iter()) {
            match name {
                Some(n) if keys.contains(&loader::lib_key(n)) => {}
                Some(_) => forge_cp.push(path.clone()),
                None => forge_cp.push(child_jar.clone()),
            }
        }
        classpath = forge_cp;
        profile = Some(p);
    }

    // 4. Assets
    prog.set_stage(Stage::Assets);
    let ai = &vj.asset_index;
    let idx_path = paths.assets().join("indexes").join(format!("{}.json", ai.id));
    fetch_all(client, vec![Dl { url: ai.url.clone(), path: idx_path.clone(), sha1: Some(ai.sha1.clone()), size: ai.size }], prog).await?;
    let index: AssetIndex = serde_json::from_slice(&std::fs::read(&idx_path)?).context("asset index")?;
    let objects = paths.assets().join("objects");
    let obj_path = |hash: &str| objects.join(&hash[..2]).join(hash);
    let mut seen = std::collections::HashSet::new();
    let dls = index
        .objects
        .values()
        .filter(|o| seen.insert(o.hash.clone()))
        .map(|o| Dl {
            url: format!("{RESOURCES_URL}/{}/{}", &o.hash[..2], o.hash),
            path: obj_path(&o.hash),
            sha1: Some(o.hash.clone()),
            size: o.size,
        })
        .collect();
    fetch_all(client, dls, prog).await?;

    let game_dir = req.game_dir.clone();
    std::fs::create_dir_all(game_dir.join("logs"))?;

    // Legacy layouts: Time Machine fix — old versions read sounds from
    // `resources/` (pre-1.6) or a virtual tree; materialize them so audio works.
    let game_assets = if index.map_to_resources || index.is_virtual {
        let target = if index.map_to_resources {
            game_dir.join("resources")
        } else {
            paths.assets().join("virtual").join(&ai.id)
        };
        let pairs: Vec<(PathBuf, PathBuf)> =
            index.objects.iter().map(|(name, o)| (obj_path(&o.hash), target.join(name))).collect();
        tokio::task::spawn_blocking(move || copy_missing(&pairs)).await??;
        if index.is_virtual { paths.assets().join("virtual").join(&ai.id) } else { paths.assets() }
    } else {
        paths.assets()
    };

    // 5. Natives
    prog.set_stage(Stage::Natives);
    let natives_dir = vdir.join("natives");
    let nd = natives_dir.clone();
    tokio::task::spawn_blocking(move || extract_natives(&native_jars, &nd)).await??;

    // 6. Command line
    prog.set_stage(Stage::Starting);
    let cp = classpath.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(CP_SEP);
    let s = |p: &Path| p.display().to_string();
    let vars: HashMap<&str, String> = HashMap::from([
        ("auth_player_name", req.nickname.clone()),
        ("version_name", profile.as_ref().and_then(|p| p.version_name.clone()).unwrap_or_else(|| req.id.clone())),
        ("game_directory", s(&game_dir)),
        ("assets_root", s(&paths.assets())),
        ("game_assets", s(&game_assets)),
        ("assets_index_name", ai.id.clone()),
        ("auth_uuid", req.uuid.replace('-', "")),
        ("auth_access_token", req.online.as_ref().map_or_else(|| "0".into(), |(t, _)| t.clone())),
        ("auth_session", req.online.as_ref().map_or_else(|| "0".into(), |(t, _)| format!("token:{t}:{}", req.uuid.replace('-', "")))),
        ("auth_xuid", req.online.as_ref().map_or_else(|| "0".into(), |(_, x)| x.clone())),
        ("clientid", "0".into()),
        ("user_type", if req.online.is_some() { "msa".into() } else { "legacy".into() }),
        ("user_properties", "{}".into()),
        ("version_type", vj.kind.clone()),
        ("natives_directory", s(&natives_dir)),
        ("library_directory", s(&paths.libraries())),
        ("classpath_separator", CP_SEP.into()),
        ("classpath", cp),
        ("launcher_name", "SKL".into()),
        ("launcher_version", env!("CARGO_PKG_VERSION").into()),
    ]);
    let subst = |a: &str| {
        let mut out = a.to_owned();
        for (k, v) in &vars {
            out = out.replace(&format!("${{{k}}}"), v);
        }
        out
    };

    let mut jvm = vec![
        format!("-Xmx{}M", req.memory_mb),
        format!("-Xms{}M", req.memory_mb.min(1024)),
        "-Dlog4j2.formatMsgNoLookups=true".into(),
    ];
    jvm.extend(req.opts.preset_flags(java8));
    if !java8 {
        // JDK 16+ on Windows opens an AF_UNIX loopback pipe for every NIO selector
        // (java.net.http, Netty…). Inside %TEMP% / AppData that fails on some PCs
        // (AV-protected or redirected folders) and crashes mods at startup, so the
        // socket dir goes to a short neutral path that verifiably works.
        if let Some(tmp) = unix_socket_dir() {
            jvm.push(format!("-Djdk.net.unixdomain.tmpdir={}", tmp.display()));
        }
    }
    if req.legacy {
        // Time Machine compatibility flags for 2009–2011 builds on modern PCs.
        jvm.push("-Djava.util.Arrays.useLegacyMergeSort=true".into());
        jvm.push("-Dsun.java2d.d3d=false".into());
    }
    // Offline skins: modern clients → authlib-injector agent; legacy → HTTP proxy.
    if let Some(port) = req.skin_port {
        let has_authlib = vj.libraries.iter().any(|l| l.name.starts_with("com.mojang:authlib:"));
        if has_authlib {
            match authlib::ensure(client, &paths.java()).await {
                Ok(agent) => jvm.push(format!("-javaagent:{}=http://127.0.0.1:{port}/", agent.display())),
                Err(e) => eprintln!("skins disabled: {e:#}"), // never block the launch over cosmetics
            }
        } else {
            jvm.push("-Dhttp.proxyHost=127.0.0.1".into());
            jvm.push(format!("-Dhttp.proxyPort={port}"));
        }
    }
    let args = vj.arguments.unwrap_or_default();
    if args.jvm.is_empty() {
        jvm.push("-Dminecraft.launcher.brand=SKL".into());
        jvm.extend(["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"].map(subst));
    } else {
        jvm.extend(args.jvm.iter().flat_map(|a| a.values()).map(subst));
    }
    let mut game: Vec<String> = match vj.minecraft_arguments {
        Some(legacy) if args.game.is_empty() => legacy.split_whitespace().map(subst).collect(),
        _ => args.game.iter().flat_map(|a| a.values()).map(subst).collect(),
    };
    let mut main_class = vj.main_class.clone();
    if let Some(p) = &profile {
        jvm.extend(p.jvm.iter().map(|a| subst(a)));
        match &p.legacy_game {
            // Old Forge ships the complete argument string (incl. --tweakClass).
            Some(full) => game = full.split_whitespace().map(subst).collect(),
            None => game.extend(p.game.iter().map(|a| subst(a))),
        }
        main_class = p.main_class.clone();
    }

    // User options go last so they win over version defaults.
    jvm.extend(req.opts.jvm_args.split_whitespace().map(String::from));
    let o = &req.opts;
    if o.width > 0 && o.height > 0 {
        game.extend(["--width".into(), o.width.to_string(), "--height".into(), o.height.to_string()]);
    }
    if o.fullscreen {
        game.push("--fullscreen".into());
    }
    if let Some((host, port)) = o.server_addr() {
        if quick_play {
            game.extend(["--quickPlayMultiplayer".into(), format!("{host}:{port}")]);
        } else {
            game.extend(["--server".into(), host, "--port".into(), port.to_string()]);
        }
    }
    game.extend(o.game_args.split_whitespace().map(String::from));

    let log_path = game_dir.join("logs").join("skl-launch.log");
    let mut log = std::fs::File::create(&log_path)?;
    use std::io::Write;
    writeln!(log, "{} {} {} {}\n", java.display(), jvm.join(" "), main_class, game.join(" "))?;

    let mut cmd = Command::new(&java);
    cmd.args(&jvm)
        .arg(&main_class)
        .args(&game)
        .current_dir(&game_dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.spawn().with_context(|| format!("failed to start {}", java.display()))
}

fn copy_missing(pairs: &[(PathBuf, PathBuf)]) -> anyhow::Result<()> {
    for (from, to) in pairs {
        if to.exists() {
            continue;
        }
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(from, to)?;
    }
    Ok(())
}

fn extract_natives(jars: &[PathBuf], dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    for jar in jars {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(jar)?)?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let Some(name) = entry.enclosed_name() else { continue };
            if entry.is_dir() || name.starts_with("META-INF") {
                continue;
            }
            let out = dir.join(name);
            if out.exists() {
                continue;
            }
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::io::copy(&mut entry, &mut std::fs::File::create(&out)?)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// End-to-end: real download + JVM start. `cargo test -- --ignored launch_smoke --nocapture`
    use crate::core::instances::Loader;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn launch_smoke() {
        let id = std::env::var("SKL_TEST_VERSION").unwrap_or_else(|_| "b1.7.3".into());
        let client = crate::api::http_client();
        let paths = crate::core::paths::Paths::init().unwrap();
        let list = crate::api::mojang::fetch_versions(&client, &paths.cache()).await.unwrap();
        let v = list.versions.iter().find(|v| v.id == id).unwrap();
        let req = super::LaunchRequest {
            id: v.id.clone(),
            url: v.url.clone(),
            legacy: v.era.is_legacy(),
            nickname: "Tester".into(),
            uuid: crate::core::auth::offline_uuid("Tester"), online: None,
            memory_mb: 2048,
            game_dir: paths.instances().join(format!("test-{id}")),
            skin_port: std::env::var("SKL_TEST_SKIN_PORT").ok().and_then(|p| p.parse().ok()),
            opts: Default::default(), loader_version: None,
            loader: match std::env::var("SKL_TEST_LOADER").as_deref() {
                Ok("fabric") => Loader::Fabric,
                Ok("quilt") => Loader::Quilt,
                Ok("forge") => Loader::Forge,
                Ok("neoforge") => Loader::NeoForge,
                _ => Loader::Vanilla,
            },
        };
        let prog = super::Progress::new();
        let mut child = super::run(&client, &paths, &req, &prog).await.unwrap();
        let wait = std::env::var("SKL_TEST_WAIT").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
        std::thread::sleep(std::time::Duration::from_secs(wait));
        let alive = child.try_wait().unwrap().is_none();
        let _ = child.kill();
        let log = String::from_utf8_lossy(&std::fs::read(req.game_dir.join("logs/skl-launch.log")).unwrap()).into_owned();
        println!("--- log tail ---\n{}", log.lines().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
        assert!(alive, "game process exited early");
    }
}

#[cfg(test)]
mod kill_tests {
    /// The supervisor must report `Exited` (not a crash) after a user-requested kill.
    #[test]
    fn kill_reports_exited() {
        use super::super::{Progress, Stage};
        #[cfg(windows)]
        let child = std::process::Command::new("ping").args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn().unwrap();
        #[cfg(not(windows))]
        let child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let p = Progress::new();
        p.supervise(child, || {});
        assert_eq!(p.stage(), Stage::Running);
        p.kill();
        for _ in 0..40 {
            if p.stage() != Stage::Running {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(p.stage(), Stage::Exited);
        assert!(p.was_killed());
    }
}

#[cfg(test)]
mod guard_gate {
    /// A launch must be refused before any download when the guard finds malware.
    #[tokio::test]
    async fn infected_instance_is_blocked() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("skl-gate-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("mods")).unwrap();
        let mut out = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut out);
        z.start_file("a/U.class", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"..85.217.144.130..").unwrap();
        z.finish().unwrap();
        std::fs::write(dir.join("mods/bad.jar"), out.into_inner()).unwrap();

        let paths = crate::core::paths::Paths::init().unwrap();
        let req = super::LaunchRequest {
            id: "1.21.4".into(), url: "http://invalid.invalid/never-fetched".into(), legacy: false,
            nickname: "Tester".into(), uuid: crate::core::auth::offline_uuid("Tester"), online: None, memory_mb: 1024,
            game_dir: dir.clone(), loader: crate::core::instances::Loader::Fabric, skin_port: None, opts: Default::default(), loader_version: None,
        };
        let prog = super::Progress::new();
        let err = super::run(&crate::api::http_client(), &paths, &req, &prog).await.unwrap_err();
        assert!(format!("{err}").contains("Safety Guard"), "{err:#}");
        assert!(prog.guard().unwrap().blocking());
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Short writable directory outside AppData for the JDK's AF_UNIX pipe sockets.
fn unix_socket_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let bases = [std::env::var_os("PUBLIC"), std::env::var_os("ProgramData")];
        for base in bases.into_iter().flatten() {
            let dir = PathBuf::from(base).join("skl-tmp");
            if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(dir.join(".probe"), b"").is_ok() {
                return Some(dir);
            }
        }
        None
    }
    #[cfg(not(windows))]
    None
}
