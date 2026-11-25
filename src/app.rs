use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;

use egui::{vec2, ColorImage, Id, Rect, Rounding, Stroke, TextureHandle, TextureOptions, UiBuilder};

use crate::api;
use crate::core::auth::{is_valid_nickname, offline_uuid};
use crate::core::i18n::{t, K};
use crate::core::instances::{self, Instance};
use crate::core::launch::{install, Progress, Stage};
use crate::core::paths::Paths;
use crate::core::msauth::{self, Account};
use crate::core::settings::{AccountMode, Settings, ThemePreset};
use crate::models::version::{Catalog, VersionList};
use crate::ui::anim::{restart_entrance, set_motion, spring_reset};
use crate::ui::cover::Covers;
use crate::ui::home::{self, HomeAction, HomeState, LaunchView};
use crate::ui::newinst::{self, ModalAction, NewInstance};
use crate::ui::settings::{self as sheet, Sheet};
use crate::ui::shell::{self, RailAction, RAIL_W, TOP_H};
use crate::ui::profile::{self, ProfileAction};
use crate::ui::store::{self, Icons, InstallState, StoreAction, StoreState, StoreView};
use crate::models::content::{InstalledEntry, Project, Query, Target, PAGE};
use std::collections::HashMap;
use crate::ui::theme::{self, Palette, RADIUS};
use crate::ui::toast::{Kind, Ongoing, Toasts};
use crate::ui::wallpaper::Wallpaper;
use crate::ui::widgets::vgradient;

struct Import {
    name: String,
    progress: Arc<Progress>,
    rx: Receiver<Result<Instance, String>>,
}

struct Launch {
    instance: String,
    progress: Arc<Progress>,
    last_stage: Stage,
    /// Unix time the game process came up, for playtime accounting.
    started: Option<u64>,
    log: std::path::PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Library,
    Mods,
    Profile,
}

enum ShopMsg {
    Page { req: u64, append: bool, res: anyhow::Result<(Vec<crate::api::skins::SkinItem>, Option<String>)> },
    Capes { req: u64, res: anyhow::Result<Vec<crate::api::skins::CapeItem>> },
    Applied { cape: bool, res: anyhow::Result<(Vec<u8>, Option<String>, String)> },
}

enum MsMsg {
    Code { gen: u64, code: String, url: String },
    /// Signed in: account + its current skin (PNG, slim) if it has a custom one.
    Done { gen: u64, res: anyhow::Result<(Account, Option<(Vec<u8>, bool)>)> },
    Uploaded(anyhow::Result<()>),
}

type SearchMsg = (u64, bool, anyhow::Result<Vec<Project>>);
type InstallMsg = (String, String, String, anyhow::Result<Vec<InstalledEntry>>);

pub struct SklApp {
    view: View,
    store: StoreState,
    icons: Icons,
    installed: Vec<InstalledEntry>,
    installed_for: Option<String>,
    installing: HashMap<String, InstallState>,
    search_tx: mpsc::Sender<SearchMsg>,
    search_rx: Receiver<SearchMsg>,
    install_tx: mpsc::Sender<InstallMsg>,
    install_rx: Receiver<InstallMsg>,
    paths: Paths,
    settings: Settings,
    saved: Settings,
    runtime: tokio::runtime::Runtime,
    http: reqwest::Client,

    logo: TextureHandle,
    wallpaper: Wallpaper,
    covers: Covers,
    applied_theme: Option<(ThemePreset, [u8; 3], u32)>,
    applied_scale: f32,

    catalog: Catalog,
    catalog_rx: Option<Receiver<anyhow::Result<VersionList>>>,
    instances: Vec<Instance>,
    home: HomeState,
    modal: Option<NewInstance>,
    sheet: Option<Sheet>,
    toasts: Toasts,
    launch: Option<Launch>,
    import: Option<Import>,
    doctor: Option<crate::ui::doctor::Doctor>,
    skin_server: Option<crate::core::yggdrasil::SkinServer>,
    skin_rx: Option<Receiver<anyhow::Result<crate::core::yggdrasil::SkinServer>>>,
    /// Bumped whenever skin/cape files change; drives texture + server refresh.
    skin_rev: u64,
    skin_synced: Option<(String, bool, u64)>,
    skin_tex: Option<TextureHandle>,
    cape_tex: Option<TextureHandle>,
    skin_loaded_rev: Option<u64>,
    /// Set when a launch was blocked: open Mods → Installed next frame.
    guard_jump: bool,
    shop: profile::Shop,
    skin_icons: Icons,
    cape_icons: Icons,
    shop_tx: mpsc::Sender<ShopMsg>,
    shop_rx: Receiver<ShopMsg>,
    guard: Option<(String, crate::core::guard::Report)>,
    guard_busy: bool,
    guard_tx: mpsc::Sender<(String, crate::core::guard::Report)>,
    guard_rx: Receiver<(String, crate::core::guard::Report)>,
    ms_account: Option<Account>,
    /// Bumped to cancel a pending device-code login.
    ms_gen: Arc<std::sync::atomic::AtomicU64>,
    ms_pending: Option<Option<(String, String)>>,
    ms_uploading: bool,
    /// (skin_rev, slim) last examined for upload.
    ms_checkpoint: Option<(u64, bool)>,
    /// (skin hash, slim) that the Mojang profile is known to have.
    ms_sent: Option<(Option<String>, bool)>,
    ms_tx: mpsc::Sender<MsMsg>,
    ms_rx: Receiver<MsMsg>,
}

impl SklApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let paths = Paths::init().expect("cannot create SKL data directory");
        let settings = Settings::load(&paths.settings_file());
        crate::core::skins::use_microsoft(settings.account_mode == AccountMode::Microsoft);
        let ms_account = Account::load(&paths.account_file());
        theme::install_text_styles(&cc.egui_ctx);
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().thread_name("skl-io").build().expect("tokio runtime");
        let logo = cc.egui_ctx.load_texture("skl_logo", ColorImage::from_rgba_unmultiplied([96, 96], &crate::icon::render(96)), TextureOptions::LINEAR);
        let instances = instances::load_all(&paths);
        restart_entrance(&cc.egui_ctx);

        let (search_tx, search_rx) = mpsc::channel();
        let (install_tx, install_rx) = mpsc::channel();
        let (guard_tx, guard_rx) = mpsc::channel();
        let (shop_tx, shop_rx) = mpsc::channel();
        let (ms_tx, ms_rx) = mpsc::channel();
        let mut app = Self {
            view: View::Library,
            store: StoreState::default(),
            icons: Icons::default(),
            installed: Vec::new(),
            installed_for: None,
            installing: HashMap::new(),
            search_tx,
            search_rx,
            install_tx,
            install_rx,
            saved: settings.clone(),
            settings,
            runtime,
            http: api::http_client(),
            logo,
            wallpaper: Wallpaper::default(),
            covers: Covers::default(),
            applied_theme: None,
            applied_scale: 0.0,
            catalog: Catalog::Loading,
            catalog_rx: None,
            instances,
            home: HomeState::default(),
            modal: None,
            sheet: None,
            toasts: Toasts::default(),
            launch: None,
            import: None,
            doctor: None,
            skin_server: None,
            skin_rx: None,
            skin_rev: 0,
            skin_synced: None,
            skin_tex: None,
            cape_tex: None,
            skin_loaded_rev: None,
            guard_jump: false,
            shop: profile::Shop::default(),
            skin_icons: Icons::of(crate::ui::store::IconKind::Skin),
            cape_icons: Icons::of(crate::ui::store::IconKind::Cape),
            shop_tx,
            shop_rx,
            guard: None,
            guard_busy: false,
            guard_tx,
            guard_rx,
            ms_account,
            ms_gen: Default::default(),
            ms_pending: None,
            ms_uploading: false,
            ms_checkpoint: None,
            ms_sent: None,
            ms_tx,
            ms_rx,
            paths,
        };
        app.request_versions(&cc.egui_ctx);
        app.start_skin_server(&cc.egui_ctx);
        // Dev aid: open a given screen directly (`SKL_VIEW=library|mods|profile0|profile1|profile2`).
        match std::env::var("SKL_VIEW").as_deref() {
            Ok("mods") => app.view = View::Mods,
            Ok(v) if v.starts_with("profile") => {
                app.view = View::Profile;
                app.shop.tab = v.trim_start_matches("profile").parse().unwrap_or(0);
            }
            Ok("doctor") => {
                let text = "java.lang.RuntimeException: Could not execute entrypoint stage 'client' due to errors, provided by 'debugify' at 'dev.isxander.debugify.client.DebugifyClient::onInitializeClient'!";
                let diag = crate::core::crash::diagnose(text, &[("debugify".into(), "debugify-1.21.1+1.0.jar".into())]).unwrap();
                app.doctor = Some(crate::ui::doctor::Doctor { instance: String::new(), name: "Fabulously Optimized".into(), diag, closing: false, fixed: false });
            }
            Ok(v) if v.starts_with("settings") => {
                app.sheet = Some(Sheet { tab: v.trim_start_matches("settings").parse().unwrap_or(0), ..Default::default() });
            }
            _ => {}
        }
        app
    }

    fn request_versions(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let (client, cache, ctx) = (self.http.clone(), self.paths.cache(), ctx.clone());
        self.runtime.spawn(async move {
            let _ = tx.send(api::mojang::fetch_versions(&client, &cache).await);
            ctx.request_repaint();
        });
        self.catalog = Catalog::Loading;
        self.catalog_rx = Some(rx);
    }

    fn poll_background(&mut self, ctx: &egui::Context) {
        if let Some(Ok(res)) = self.catalog_rx.as_ref().map(|rx| rx.try_recv()) {
            self.catalog_rx = None;
            match res {
                Ok(list) => self.catalog = Catalog::Ready(list),
                Err(e) => {
                    self.toasts.push(ctx, Kind::Error, t(self.settings.lang, K::LoadFailed), format!("{e:#}"));
                    self.catalog = Catalog::Failed(format!("{e:#}"));
                }
            }
        }
        if self.settings.selected_instance.as_ref().map_or(true, |id| !self.instances.iter().any(|i| &i.id == id)) {
            self.settings.selected_instance = self.instances.first().map(|i| i.id.clone());
        }

        // Launch stage transitions → toasts
        if let Some(l) = &mut self.launch {
            let stage = l.progress.stage();
            if stage != l.last_stage {
                let lang = self.settings.lang;
                let name = self.instances.iter().find(|i| i.id == l.instance).map(|i| i.name.clone()).unwrap_or_default();
                let was_running = l.last_stage == Stage::Running;
                match &stage {
                    Stage::Running => {
                        l.started = Some(instances::now());
                        if self.settings.after_launch == crate::core::settings::AfterLaunch::Minimize {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        self.toasts.push(ctx, Kind::Success, t(lang, K::GameStarted), name);
                        if let Some(r) = l.progress.guard().filter(|r| !r.findings.is_empty()) {
                            let files: Vec<_> = r.findings.iter().map(|f| f.file.clone()).collect();
                            self.toasts.push(ctx, Kind::Error, t(lang, K::GuardWarn), files.join(", "));
                            self.guard = Some((l.instance.clone(), r));
                        }
                    }
                    Stage::Exited => self.toasts.push(ctx, Kind::Success, t(lang, K::GameStopped), name),
                    Stage::Failed(_) if !was_running && l.progress.guard().is_some_and(|r| r.blocking()) => {
                        // Blocked before anything ran: show the findings where they can be fixed.
                        let r = l.progress.guard().unwrap();
                        let body = r.findings.iter().take(3).map(|f| format!("{} — {}", f.file, f.what)).collect::<Vec<_>>().join("
");
                        self.toasts.push(ctx, Kind::Error, t(lang, K::GuardBlocked), body);
                        self.guard = Some((l.instance.clone(), r));
                        self.guard_jump = true;
                    }
                    Stage::Failed(e) => {
                        // A crash after the game was up differs from a failed install.
                        if was_running {
                            if let Some(diag) = crate::core::crash::analyze(&instances::dir(&self.paths, &l.instance)) {
                                self.doctor = Some(crate::ui::doctor::Doctor { instance: l.instance.clone(), name: name.clone(), diag, closing: false, fixed: false });
                                crate::ui::anim::spring_reset(ctx, Id::new(crate::ui::doctor::OPEN_ID), 0.0);
                            }
                        }
                        let title = if was_running { t(lang, K::Crashed) } else { t(lang, K::Failed) };
                        let body = format!("{}\n{}", e.lines().next().unwrap_or(""), t(lang, K::OpenLog));
                        self.toasts.push_with_path(ctx, Kind::Error, title, body, l.log.clone());
                    }
                    _ => {}
                }
                if was_running && stage != Stage::Running && self.settings.restore_on_exit && self.settings.after_launch == crate::core::settings::AfterLaunch::Minimize {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                // Playtime: count only real game time, persist on exit.
                if was_running && stage != Stage::Running {
                    if let (Some(start), Some(inst)) = (l.started.take(), self.instances.iter_mut().find(|i| i.id == l.instance)) {
                        inst.playtime_secs += instances::now().saturating_sub(start);
                        let _ = instances::save(&self.paths, inst);
                    }
                }
                l.last_stage = stage;
            }
        }
    }

    fn start_launch(&mut self, ctx: &egui::Context, id: &str) {
        let lang = self.settings.lang;
        if self.launch.as_ref().is_some_and(|l| l.progress.is_busy()) {
            return;
        }
        let licensed = self.settings.account_mode == AccountMode::Microsoft;
        if licensed && self.ms_account.is_none() {
            self.toasts.push(ctx, Kind::Error, t(lang, K::Failed), t(lang, K::MsNeedLogin));
            self.view = View::Profile;
            self.shop.tab = 0;
            return;
        }
        if !licensed && !is_valid_nickname(&self.settings.nickname) {
            self.toasts.push(ctx, Kind::Error, t(lang, K::Failed), t(lang, K::NicknameInvalid));
            self.sheet = Some(Sheet::default());
            spring_reset(ctx, Id::new(sheet::SHEET_ID), 0.0);
            return;
        }
        let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) else { return };
        inst.last_played = Some(instances::now());
        let _ = instances::save(&self.paths, inst);
        let v = self.catalog.find(&inst.version);
        let ms = self.ms_account.as_ref().filter(|_| licensed);
        let mut req = install::LaunchRequest {
            id: inst.version.clone(),
            url: v.map(|v| v.url.clone()).unwrap_or_default(),
            legacy: v.map_or_else(|| crate::models::version::Era::guess(&inst.version).is_legacy(), |v| v.era.is_legacy()),
            uuid: ms.map_or_else(|| offline_uuid(&self.settings.nickname), |a| a.dashed_uuid()),
            nickname: ms.map_or_else(|| self.settings.nickname.clone(), |a| a.name.clone()),
            online: None,
            memory_mb: self.settings.memory_mb.min(crate::core::sysinfo::max_memory_mb()),
            game_dir: instances::dir(&self.paths, &inst.id),
            loader: inst.loader,
            // Licensed accounts get real Mojang skins; the local skin server is for offline only.
            skin_port: if licensed { None } else { self.skin_server.as_ref().map(|s| s.port()) },
            opts: self.settings.game.clone(),
            loader_version: inst.loader_version.clone(),
        };
        let prog = Progress::new();
        self.launch = Some(Launch {
            instance: inst.id.clone(),
            progress: prog.clone(),
            last_stage: Stage::Preparing,
            started: None,
            log: instances::dir(&self.paths, &inst.id).join("logs").join("skl-launch.log"),
        });
        let backup = self.settings.backup_worlds.then_some((req.game_dir.clone(), self.settings.backup_keep as usize));
        let (client, paths, ctx) = (self.http.clone(), self.paths.clone(), ctx.clone());
        self.runtime.spawn(async move {
            if let Some((dir, keep)) = backup {
                // Runs while Java/assets are checked; worlds are not touched until the game is up.
                tokio::task::spawn_blocking(move || {
                    if let Err(e) = crate::core::backup::backup_worlds(&dir, keep) {
                        eprintln!("backup failed: {e:#}");
                    }
                });
            }
            if licensed {
                match msauth::fresh_from_disk(&client, &paths.account_file()).await {
                    Ok(a) => {
                        req.uuid = a.dashed_uuid();
                        req.nickname = a.name.clone();
                        req.online = Some((a.mc_token, a.xuid));
                    }
                    Err(e) => {
                        prog.set_stage(Stage::Failed(format!("Microsoft: {e:#}")));
                        ctx.request_repaint();
                        return;
                    }
                }
            }
            match install::run(&client, &paths, &req, &prog).await {
                Ok(child) => {
                    let ctx2 = ctx.clone();
                    prog.supervise(child, move || ctx2.request_repaint());
                }
                Err(e) => prog.set_stage(Stage::Failed(format!("{e:#}"))),
            }
            ctx.request_repaint();
        });
    }

    fn current_instance(&self) -> Option<&Instance> {
        let id = self.settings.selected_instance.as_deref()?;
        self.instances.iter().find(|i| i.id == id)
    }

    fn store_target(&self) -> Option<Target> {
        let i = self.current_instance()?;
        Some(Target { game_version: i.version.clone(), loader: i.loader, dir: instances::dir(&self.paths, &i.id) })
    }

    fn refresh_installed(&mut self, force: bool) {
        let sel = self.settings.selected_instance.clone();
        if force || self.installed_for != sel {
            if self.installed_for != sel {
                self.guard = None;
            }
            self.installed = self.store_target().map(|t| crate::core::content::list_all(&t.dir)).unwrap_or_default();
            if self.installed_for != sel {
                self.installing.clear();
            }
            self.installed_for = sel;
        }
    }

    fn do_search(&mut self, ctx: &egui::Context, append: bool) {
        let Some(t) = self.store_target() else { return };
        if append && (self.store.loading || self.store.exhausted) {
            return;
        }
        let source = self.store.source();
        if source == crate::models::content::Source::CurseForge && self.settings.curseforge_key.trim().is_empty() {
            self.store.results.clear();
            self.store.loading = false;
            self.store.exhausted = true;
            return;
        }
        self.store.req_id += 1;
        if !append {
            self.store.results.clear();
            self.store.exhausted = false;
            self.store.error = None;
        }
        self.store.loading = true;
        let q = Query { text: self.store.query.trim().to_owned(), kind: self.store.kind(), game_version: t.game_version, loader: t.loader, offset: self.store.results.len() };
        let (client, key, tx, id, ctx) = (self.http.clone(), self.settings.curseforge_key.trim().to_owned(), self.search_tx.clone(), self.store.req_id, ctx.clone());
        self.runtime.spawn(async move {
            let _ = tx.send((id, append, crate::api::store::search(&client, source, &key, &q).await));
            ctx.request_repaint();
        });
    }

    fn do_install(&mut self, ctx: &egui::Context, p: Project) {
        let Some(t) = self.store_target() else { return };
        let Some(inst_id) = self.settings.selected_instance.clone() else { return };
        let key = p.key();
        self.installing.insert(key.clone(), InstallState::Installing);
        let (client, cf, tx, ctx, have) = (self.http.clone(), self.settings.curseforge_key.trim().to_owned(), self.install_tx.clone(), ctx.clone(), self.installed.clone());
        self.runtime.spawn(async move {
            let res = crate::api::store::install(&client, &cf, &p, &t, &have).await;
            if let Ok(list) = &res {
                let _ = crate::core::content::merge(&t.dir, list.clone());
            }
            let _ = tx.send((key, inst_id, p.title.clone(), res));
            ctx.request_repaint();
        });
    }

    fn do_guard_scan(&mut self, ctx: &egui::Context) {
        let Some(i) = self.current_instance() else { return };
        let (id, dir, loader, tx, ctx) = (i.id.clone(), instances::dir(&self.paths, &i.id), i.loader, self.guard_tx.clone(), ctx.clone());
        self.guard_busy = true;
        self.runtime.spawn(async move {
            if let Ok(r) = tokio::task::spawn_blocking(move || crate::core::guard::scan(&dir, loader)).await {
                let _ = tx.send((id, r));
            }
            ctx.request_repaint();
        });
    }

    fn poll_extras(&mut self, ctx: &egui::Context) {
        while let Ok((id, r)) = self.guard_rx.try_recv() {
            self.guard_busy = false;
            self.guard = Some((id, r));
        }
        let lang = self.settings.lang;
        let now = ctx.input(|i| i.time);
        while let Ok(msg) = self.shop_rx.try_recv() {
            match msg {
                ShopMsg::Page { req, append, res } if req == self.shop.req => {
                    self.shop.loading = false;
                    match res {
                        Ok((items, next)) => {
                            if !append {
                                self.shop.results_t0 = now;
                            }
                            self.shop.exhausted = next.is_none() || items.is_empty();
                            self.shop.next = next;
                            self.shop.skins.extend(items);
                        }
                        Err(e) => {
                            self.shop.exhausted = true;
                            self.shop.error = Some(format!("{e:#}"));
                        }
                    }
                }
                ShopMsg::Capes { req, res } if req == self.shop.capes_req => {
                    self.shop.capes_loading = false;
                    self.shop.capes_t0 = now;
                    match res {
                        Ok(list) => self.shop.capes = list,
                        Err(e) => self.shop.capes_error = Some(format!("{e:#}")),
                    }
                }
                ShopMsg::Applied { cape, res } => {
                    let res = res.and_then(|(bytes, url, title)| {
                        if cape {
                            crate::core::skins::import_cape_bytes(&self.paths, bytes)?;
                        } else {
                            self.settings.skin_slim = crate::core::skins::import_skin_bytes(&self.paths, bytes)?;
                            self.settings.skin_url = url;
                        }
                        Ok(title)
                    });
                    match res {
                        Ok(title) => {
                            self.skin_rev += 1;
                            self.toasts.push(ctx, Kind::Success, t(lang, if cape { K::CapeSaved } else { K::SkinSaved }), title);
                        }
                        Err(e) => self.toasts.push(ctx, Kind::Error, t(lang, K::SkinError), format!("{e:#}")),
                    }
                }
                _ => {} // stale page / capes response
            }
        }
    }

    fn start_skin_server(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let key = self.paths.root().join("skins").join("ygg-key.pem");
        let ctx = ctx.clone();
        self.runtime.spawn(async move {
            let _ = tx.send(crate::core::yggdrasil::SkinServer::start(key).await);
            ctx.request_repaint();
        });
        self.skin_rx = Some(rx);
    }

    /// Keeps the preview textures and the local skin server in sync with files + settings.
    fn sync_skins(&mut self, ctx: &egui::Context) {
        if let Some(Ok(res)) = self.skin_rx.as_ref().map(|rx| rx.try_recv()) {
            self.skin_rx = None;
            match res {
                Ok(s) => self.skin_server = Some(s),
                Err(e) => eprintln!("skin server unavailable: {e:#}"),
            }
        }
        if self.skin_loaded_rev != Some(self.skin_rev) {
            self.skin_loaded_rev = Some(self.skin_rev);
            let load = |which: &str| {
                let b = crate::core::skins::read(&self.paths, which)?;
                let img = image::load_from_memory(&b).ok()?.to_rgba8();
                let ci = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                Some(ctx.load_texture(format!("{which}-{}", self.skin_rev), ci, TextureOptions::NEAREST))
            };
            self.skin_tex = load("skin");
            self.cape_tex = load("cape");
        }
        self.sync_ms_skin(ctx);
        let want = (self.settings.nickname.clone(), self.settings.skin_slim, self.skin_rev);
        if let Some(srv) = &self.skin_server {
            if self.skin_synced.as_ref() != Some(&want) {
                srv.set_player(crate::core::yggdrasil::Player {
                    name: self.settings.nickname.clone(),
                    uuid: offline_uuid(&self.settings.nickname).replace('-', ""),
                    skin: crate::core::skins::read(&self.paths, "skin"),
                    slim: self.settings.skin_slim,
                    cape: crate::core::skins::read(&self.paths, "cape"),
                });
                self.skin_synced = Some(want);
            }
        }
    }

    fn set_account_mode(&mut self, m: AccountMode) {
        let s = &mut self.settings;
        if s.account_mode == m {
            return;
        }
        std::mem::swap(&mut s.skin_slim, &mut s.skin_slim_alt);
        std::mem::swap(&mut s.skin_url, &mut s.skin_url_alt);
        s.account_mode = m;
        crate::core::skins::use_microsoft(m == AccountMode::Microsoft);
        self.skin_rev += 1;
        self.ms_sent = None;
        self.ms_checkpoint = None;
    }

    fn ms_login(&mut self, ctx: &egui::Context) {
        let gen = self.ms_gen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        self.ms_pending = Some(None);
        let (client, tx, live, ctx) = (self.http.clone(), self.ms_tx.clone(), self.ms_gen.clone(), ctx.clone());
        self.runtime.spawn(async move {
            let res = async {
                let dc = msauth::device_code(&client).await?;
                let _ = tx.send(MsMsg::Code { gen, code: dc.user_code.clone(), url: dc.verification_uri.clone() });
                ctx.request_repaint();
                let cancelled = || live.load(std::sync::atomic::Ordering::SeqCst) != gen;
                let (access, refresh) = msauth::wait_for_user(&client, &dc, cancelled).await?;
                let (acc, profile) = msauth::finish_login(&client, &access, refresh).await?;
                // Show the account's own skin right away (absent = default Steve/Alex).
                let skin = match profile.active_skin() {
                    Some(sk) => match client.get(sk.url.replacen("http://", "https://", 1)).send().await {
                        Ok(r) => r.bytes().await.ok().map(|b| (b.to_vec(), sk.variant.eq_ignore_ascii_case("slim"))),
                        Err(_) => None,
                    },
                    None => None,
                };
                anyhow::Ok((acc, skin))
            }
            .await;
            let _ = tx.send(MsMsg::Done { gen, res });
            ctx.request_repaint();
        });
    }

    fn poll_ms(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang;
        let live = self.ms_gen.load(std::sync::atomic::Ordering::SeqCst);
        while let Ok(msg) = self.ms_rx.try_recv() {
            match msg {
                MsMsg::Code { gen, code, url } if gen == live => self.ms_pending = Some(Some((code, url))),
                MsMsg::Done { gen, res } if gen == live => {
                    self.ms_pending = None;
                    match res {
                        Ok((acc, skin)) => {
                            if let Err(e) = acc.save(&self.paths.account_file()) {
                                self.toasts.push(ctx, Kind::Error, t(lang, K::MsLoginFailed), format!("{e:#}"));
                                continue;
                            }
                            if self.settings.account_mode == AccountMode::Microsoft {
                                match skin {
                                    Some((bytes, slim)) => {
                                        if crate::core::skins::import_skin_bytes(&self.paths, bytes).is_ok() {
                                            self.settings.skin_slim = slim;
                                        }
                                    }
                                    None => crate::core::skins::clear(&self.paths, "skin"),
                                }
                                self.settings.skin_url = None;
                                self.skin_rev += 1;
                            }
                            self.toasts.push(ctx, Kind::Success, t(lang, K::MsSignedIn), acc.name.clone());
                            self.ms_account = Some(acc);
                            self.ms_sent = None;
                            self.ms_checkpoint = None;
                        }
                        Err(e) if format!("{e}") == "cancelled" => {}
                        Err(e) => self.toasts.push(ctx, Kind::Error, t(lang, K::MsLoginFailed), format!("{e:#}")),
                    }
                }
                MsMsg::Uploaded(res) => {
                    self.ms_uploading = false;
                    match res {
                        Ok(()) => self.toasts.push(ctx, Kind::Success, t(lang, K::MsSkinUploaded), String::new()),
                        Err(e) => self.toasts.push(ctx, Kind::Error, t(lang, K::MsSkinFailed), format!("{e:#}")),
                    }
                }
                _ => {} // stale login
            }
        }
    }

    /// Licensed mode: push skin / model changes to the real Mojang profile.
    fn sync_ms_skin(&mut self, ctx: &egui::Context) {
        if self.settings.account_mode != AccountMode::Microsoft || self.ms_account.is_none() || self.ms_uploading {
            return;
        }
        let want = (self.skin_rev, self.settings.skin_slim);
        if self.ms_checkpoint == Some(want) || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        self.ms_checkpoint = Some(want);
        let bytes = crate::core::skins::read(&self.paths, "skin");
        let cur = (bytes.as_deref().map(crate::core::skins::hash), self.settings.skin_slim);
        let Some(prev) = self.ms_sent.replace(cur.clone()) else { return }; // first look = baseline
        if prev == cur {
            return;
        }
        self.ms_uploading = true;
        let (client, paths, tx, ctx, slim) = (self.http.clone(), self.paths.clone(), self.ms_tx.clone(), ctx.clone(), self.settings.skin_slim);
        self.runtime.spawn(async move {
            let res = async {
                let acc = msauth::fresh_from_disk(&client, &paths.account_file()).await?;
                msauth::set_skin(&client, &acc.mc_token, bytes, slim).await
            }
            .await;
            let _ = tx.send(MsMsg::Uploaded(res));
            ctx.request_repaint();
        });
    }

    fn profile_action(&mut self, ctx: &egui::Context, a: ProfileAction) {
        let lang = self.settings.lang;
        match &a {
            ProfileAction::SkinSearch { append } => {
                let append = *append;
                if append && (self.shop.loading || self.shop.exhausted || self.shop.next.is_none()) {
                    return;
                }
                self.shop.req += 1;
                if !append {
                    self.shop.skins.clear();
                    self.shop.next = None;
                    self.shop.exhausted = false;
                    self.shop.error = None;
                    self.shop.selected = None;
                }
                self.shop.loading = true;
                let (client, q, after, tx, req, ctx) = (self.http.clone(), self.shop.query.clone(), self.shop.next.clone().filter(|_| append), self.shop_tx.clone(), self.shop.req, ctx.clone());
                self.runtime.spawn(async move {
                    let res = crate::api::skins::browse(&client, &q, after.as_deref()).await;
                    let _ = tx.send(ShopMsg::Page { req, append, res });
                    ctx.request_repaint();
                });
                return;
            }
            ProfileAction::CapeSearch(names) => {
                self.shop.capes_req += 1;
                self.shop.capes_loading = true;
                self.shop.capes.clear();
                self.shop.capes_error = None;
                self.shop.cape_selected = None;
                let (client, names, tx, req, ctx) = (self.http.clone(), names.clone(), self.shop_tx.clone(), self.shop.capes_req, ctx.clone());
                self.runtime.spawn(async move {
                    let mut all = Vec::new();
                    let mut last_err = None;
                    for n in &names {
                        match crate::api::skins::capes_of(&client, n).await {
                            Ok(mut v) => all.append(&mut v),
                            Err(e) => last_err = Some(e),
                        }
                    }
                    let res = match (all.is_empty(), last_err) {
                        (true, Some(e)) if names.len() == 1 => Err(e),
                        _ => Ok(all),
                    };
                    let _ = tx.send(ShopMsg::Capes { req, res });
                    ctx.request_repaint();
                });
                return;
            }
            ProfileAction::ApplySkin(item) => {
                let (client, url, title, tx, ctx) = (self.http.clone(), item.image_url(), item.title.clone(), self.shop_tx.clone(), ctx.clone());
                self.runtime.spawn(async move {
                    let res = async { anyhow::Ok(client.get(&url).send().await?.error_for_status()?.bytes().await?.to_vec()) }.await;
                    let _ = tx.send(ShopMsg::Applied { cape: false, res: res.map(|b| (b, Some(url), title)) });
                    ctx.request_repaint();
                });
                return;
            }
            ProfileAction::ApplyCape(c) => {
                let (client, url, title, tx, ctx) = (self.http.clone(), c.url.clone(), format!("{} · {}", c.owner, c.provider), self.shop_tx.clone(), ctx.clone());
                self.runtime.spawn(async move {
                    let res = async {
                        let b = client.get(&url).send().await?.error_for_status()?.bytes().await?;
                        crate::api::skins::normalize_cape(&b)
                    }
                    .await;
                    let _ = tx.send(ShopMsg::Applied { cape: true, res: res.map(|b| (b, None, title)) });
                    ctx.request_repaint();
                });
                return;
            }
            ProfileAction::Copy(cmd) => {
                ctx.output_mut(|o| o.copied_text = cmd.clone());
                self.toasts.push(ctx, Kind::Success, t(lang, K::Copied), cmd.clone());
                return;
            }
            ProfileAction::OpenEly => {
                crate::core::paths::open("https://ely.by");
                return;
            }
            ProfileAction::OpenUrl(u) => {
                crate::core::paths::open(u);
                return;
            }
            ProfileAction::SetMode(m) => {
                self.set_account_mode(*m);
                return;
            }
            ProfileAction::MsLogin => {
                self.ms_login(ctx);
                return;
            }
            ProfileAction::MsCancel => {
                self.ms_gen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.ms_pending = None;
                return;
            }
            ProfileAction::MsLogout => {
                let _ = std::fs::remove_file(self.paths.account_file());
                self.ms_account = None;
                self.ms_sent = None;
                self.ms_checkpoint = None;
                return;
            }
            _ => {}
        }
        let pick = || rfd::FileDialog::new().add_filter("PNG", &["png"]).pick_file();
        let res = match a {
            ProfileAction::None
            | ProfileAction::SkinSearch { .. }
            | ProfileAction::CapeSearch(_)
            | ProfileAction::ApplySkin(_)
            | ProfileAction::ApplyCape(_)
            | ProfileAction::Copy(_)
            | ProfileAction::OpenEly
            | ProfileAction::OpenUrl(_)
            | ProfileAction::SetMode(_)
            | ProfileAction::MsLogin
            | ProfileAction::MsCancel
            | ProfileAction::MsLogout => return,
            ProfileAction::PickSkin => match pick() {
                Some(f) => crate::core::skins::import_skin(&self.paths, &f).map(|slim| {
                    self.settings.skin_slim = slim;
                    self.settings.skin_url = None; // a local file has no public URL
                    Some(K::SkinSaved)
                }),
                None => return,
            },
            ProfileAction::PickCape => match pick() {
                Some(f) => crate::core::skins::import_cape(&self.paths, &f).map(|_| Some(K::CapeSaved)),
                None => return,
            },
            ProfileAction::ClearSkin => {
                crate::core::skins::clear(&self.paths, "skin");
                self.settings.skin_url = None;
                Ok(None)
            }
            ProfileAction::ClearCape => {
                crate::core::skins::clear(&self.paths, "cape");
                Ok(None)
            }
        };
        match res {
            Ok(msg) => {
                self.skin_rev += 1;
                if let Some(k) = msg {
                    self.toasts.push(ctx, Kind::Success, t(lang, k), String::new());
                }
            }
            Err(e) => self.toasts.push(ctx, Kind::Error, t(lang, K::SkinError), format!("{e:#}")),
        }
    }

    fn poll_covers(&mut self, ctx: &egui::Context) {
        self.covers.poll(ctx);
        for v in self.covers.take_wanted() {
            let url = self.catalog.find(&v).map(|g| g.url.clone());
            let jar_ready = self.paths.versions().join(&v).join(format!("{v}.jar")).exists();
            if url.is_none() && !jar_ready && matches!(self.catalog, Catalog::Loading) {
                self.covers.retry(&v); // ask again once the manifest is in
                continue;
            }
            let (client, paths, tx, ctx) = (self.http.clone(), self.paths.clone(), self.covers.tx.clone(), ctx.clone());
            self.runtime.spawn(async move {
                let img = crate::api::art::cover(&client, &paths, &v, url.as_deref())
                    .await
                    .map(|i| ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw()));
                let _ = tx.send((v, img));
                ctx.request_repaint();
            });
        }
    }

    fn poll_store(&mut self, ctx: &egui::Context) {
        for icons in [&mut self.icons, &mut self.skin_icons, &mut self.cape_icons] {
            icons.poll(ctx);
            let kind = icons.kind;
            for url in icons.take_wanted() {
                let (client, tx, ctx) = (self.http.clone(), icons.tx.clone(), ctx.clone());
                self.runtime.spawn(async move {
                    let img = crate::api::store::fetch_image(&client, &url, kind).await;
                    let _ = tx.send((url, img));
                    ctx.request_repaint();
                });
            }
        }
        while let Ok((id, append, res)) = self.search_rx.try_recv() {
            if id != self.store.req_id {
                continue; // stale response for an older query
            }
            self.store.loading = false;
            match res {
                Ok(list) => {
                    self.store.exhausted = list.len() < PAGE;
                    if !append {
                        self.store.results_t0 = ctx.input(|i| i.time);
                    }
                    self.store.results.extend(list);
                }
                Err(e) => {
                    self.store.exhausted = true;
                    self.store.error = Some(format!("{e:#}"));
                }
            }
        }
        let lang = self.settings.lang;
        while let Ok((key, inst_id, title, res)) = self.install_rx.try_recv() {
            match res {
                Ok(list) => {
                    self.installing.remove(&key);
                    let deps = list.len().saturating_sub(1);
                    let body = if deps > 0 { format!("{title} · +{deps} {}", t(lang, K::WithDeps)) } else { title };
                    self.toasts.push(ctx, Kind::Success, t(lang, K::InstalledOk), body);
                }
                Err(e) => {
                    self.installing.insert(key, InstallState::Failed);
                    self.toasts.push(ctx, Kind::Error, t(lang, K::InstallFailed), format!("{title}: {e:#}"));
                }
            }
            if self.settings.selected_instance.as_deref() == Some(inst_id.as_str()) {
                self.refresh_installed(true);
                self.do_guard_scan(ctx);
            }
        }
    }

    fn open_modal(&mut self, ctx: &egui::Context) {
        if self.modal.is_none() {
            spring_reset(ctx, Id::new(newinst::OPEN_ID), 0.0);
            self.modal = Some(NewInstance::new(&self.catalog));
        }
    }

    fn open_sheet(&mut self, ctx: &egui::Context) {
        if self.sheet.is_none() {
            spring_reset(ctx, Id::new(sheet::SHEET_ID), 0.0);
            self.sheet = Some(Sheet::default());
        }
    }

    fn persist_settings(&mut self, ctx: &egui::Context) {
        if self.settings != self.saved && !ctx.input(|i| i.pointer.any_down()) && self.settings.save(&self.paths.settings_file()).is_ok() {
            self.saved = self.settings.clone();
        }
    }
}

impl SklApp {
    fn pick_modpack(&mut self, ctx: &egui::Context) {
        if let Some(p) = rfd::FileDialog::new().add_filter("Modpack", &["mrpack", "zip"]).pick_file() {
            self.start_import(ctx, p);
        }
    }

    fn start_import(&mut self, ctx: &egui::Context, path: std::path::PathBuf) {
        let lang = self.settings.lang;
        if self.import.is_some() {
            return;
        }
        // Read the manifest up front so mistakes surface instantly, not after downloads.
        let plan = match crate::core::modpack::inspect(&path) {
            Ok(p) => p,
            Err(e) => {
                self.toasts.push(ctx, Kind::Error, t(lang, K::ImportFailed), format!("{e:#}"));
                return;
            }
        };
        if plan.format == crate::core::modpack::Format::CurseForge && self.settings.curseforge_key.trim().is_empty() {
            self.toasts.push(ctx, Kind::Error, t(lang, K::ImportFailed), t(lang, K::CfKeyMissing));
            return;
        }
        let progress = Progress::new();
        let (tx, rx) = mpsc::channel();
        let name = format!("{} · {} {}", plan.name, plan.loader.name(), plan.game);
        self.import = Some(Import { name, progress: progress.clone(), rx });
        let (client, paths, key, ctx2) = (self.http.clone(), self.paths.clone(), self.settings.curseforge_key.clone(), ctx.clone());
        self.runtime.spawn(async move {
            let r = crate::core::modpack::import(&client, &paths, &key, &path, &progress).await.map_err(|e| format!("{e:#}"));
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
        ctx.request_repaint();
    }

    fn poll_import(&mut self, ctx: &egui::Context) {
        let Some(imp) = &self.import else { return };
        let Ok(res) = imp.rx.try_recv() else {
            if imp.progress.fraction().is_some() {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            return;
        };
        let lang = self.settings.lang;
        self.import = None;
        match res {
            Ok(inst) => {
                self.toasts.push(ctx, Kind::Success, t(lang, K::PackImported), inst.name.clone());
                self.settings.selected_instance = Some(inst.id.clone());
                self.instances.insert(0, inst);
                self.view = View::Library;
                restart_entrance(ctx);
            }
            Err(e) => self.toasts.push(ctx, Kind::Error, t(lang, K::ImportFailed), e),
        }
    }

    /// Drag a .mrpack / .zip anywhere onto the window to import it.
    fn handle_drops(&mut self, ctx: &egui::Context, pal: &crate::ui::theme::Palette) {
        let is_pack = |p: &std::path::Path| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mrpack") || e.eq_ignore_ascii_case("zip"));
        let hovering = ctx.input(|i| i.raw.hovered_files.iter().any(|f| f.path.as_deref().is_some_and(is_pack)));
        let a = crate::ui::anim::spring_bool(ctx, Id::new("drop_overlay"), hovering, crate::ui::anim::SNAPPY);
        if a > 0.01 {
            let screen = ctx.screen_rect();
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::new("drop_layer")));
            p.rect_filled(screen.shrink(1.0), 12.0, egui::Color32::from_black_alpha((150.0 * a) as u8));
            let box_r = Rect::from_center_size(screen.center(), vec2(420.0, 180.0) * (0.94 + 0.06 * a));
            p.rect_filled(box_r, 24.0, pal.accent.gamma_multiply(0.14 * a));
            p.rect_stroke(box_r, 24.0, Stroke::new(2.0_f32, pal.accent.gamma_multiply(a)));
            let lang = self.settings.lang;
            p.text(box_r.center() - vec2(0.0, 14.0), egui::Align2::CENTER_CENTER, t(lang, K::DropPack), egui::FontId::proportional(22.0), pal.text.gamma_multiply(a));
            p.text(box_r.center() + vec2(0.0, 22.0), egui::Align2::CENTER_CENTER, ".mrpack · CurseForge .zip", egui::FontId::proportional(13.0), pal.text_dim.gamma_multiply(a));
        }
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).filter(|p| is_pack(p)).collect());
        if let Some(p) = dropped.into_iter().next() {
            self.start_import(ctx, p);
        }
    }
}

impl eframe::App for SklApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background(ctx);
        self.poll_store(ctx);
        self.poll_covers(ctx);
        self.sync_skins(ctx);
        self.poll_extras(ctx);
        self.poll_ms(ctx);
        if std::mem::take(&mut self.guard_jump) {
            self.view = View::Mods;
            self.store.tab = 1;
            restart_entrance(ctx);
        }
        self.refresh_installed(false);
        let pal = Palette::from_settings(&self.settings);
        let theme_key = (self.settings.preset, self.settings.accent, (self.settings.panel_opacity * 100.0) as u32);
        if self.applied_theme != Some(theme_key) {
            theme::apply(ctx, &pal);
            self.applied_theme = Some(theme_key);
        }
        if (self.applied_scale - self.settings.ui_scale).abs() > 0.001 && !ctx.input(|i| i.pointer.any_down()) {
            ctx.set_zoom_factor(self.settings.ui_scale);
            self.applied_scale = self.settings.ui_scale;
        }
        set_motion(ctx, self.settings.motion_scale());
        self.wallpaper.update(ctx, &self.settings.wallpaper, self.settings.blur);
        self.wallpaper.ensure_thumbs(ctx);
        shell::handle_resize(ctx);
        if self.launch.as_ref().is_some_and(|l| l.progress.is_busy() && l.progress.stage() != Stage::Running) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let rounding = if maximized { Rounding::ZERO } else { Rounding::same(RADIUS) };
        let lang = self.settings.lang;
        let mut rail_action = RailAction::None;
        let mut home_action = HomeAction::None;
        let mut store_action = StoreAction::None;
        let mut profile_action = ProfileAction::None;

        let shown_name = match (&self.ms_account, self.settings.account_mode) {
            (Some(a), AccountMode::Microsoft) => a.name.clone(),
            _ => self.settings.nickname.clone(),
        };
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let full = ui.max_rect();
            self.wallpaper.paint(ui.painter(), full, rounding, self.settings.dim);

            let rail = Rect::from_min_size(full.min, vec2(RAIL_W, full.height()));
            let content = Rect::from_min_max(egui::pos2(rail.right(), full.top()), full.max);
            vgradient(ui.painter(), Rect::from_min_size(content.min, vec2(content.width() - 1.0, 90.0)), egui::Color32::from_black_alpha(110), egui::Color32::TRANSPARENT);

            let active = if self.sheet.is_some() { 4 } else if self.modal.is_some() { 1 } else if self.view == View::Mods { 2 } else if self.view == View::Profile { 5 } else { 0 };
            rail_action = shell::rail(ui, &pal, rail, &self.logo, lang, active, &shown_name, self.skin_tex.as_ref());
            shell::top_bar(ui, &pal, Rect::from_min_size(content.min, vec2(content.width(), TOP_H)));

            let page = Rect::from_min_max(content.min + vec2(0.0, TOP_H), content.max - vec2(0.0, 6.0));
            ui.allocate_new_ui(UiBuilder::new().max_rect(page), |ui| {
                if self.view == View::Profile {
                    let ms = profile::MsView {
                        account: self.ms_account.as_ref().map(|a| (a.name.clone(), a.dashed_uuid())),
                        pending: self.ms_pending.clone(),
                        uploading: self.ms_uploading,
                    };
                    profile_action = profile::show(ui, &pal, lang, &mut self.settings, self.skin_tex.as_ref(), self.cape_tex.as_ref(), &mut self.shop, &mut self.skin_icons, &mut self.cape_icons, &ms);
                } else if self.view == View::Mods {
                    let view = StoreView {
                        instance: self.settings.selected_instance.as_deref().and_then(|id| self.instances.iter().find(|i| i.id == id)),
                        installed: &self.installed,
                        installing: &self.installing,
                        has_cf_key: !self.settings.curseforge_key.trim().is_empty(),
                        guard: self.guard.as_ref().filter(|(id, _)| Some(id) == self.settings.selected_instance.as_ref()).map(|(_, r)| r),
                        guard_busy: self.guard_busy,
                    };
                    store_action = store::show(ui, &pal, lang, &mut self.store, view, &mut self.icons);
                } else {
                let launch = self.launch.as_ref().map(|l| LaunchView { instance: &l.instance, progress: &l.progress });
                home_action = home::show(ui, &pal, lang, &mut self.home, &self.instances, self.settings.selected_instance.as_deref(), &mut self.covers, &self.catalog, launch, &shown_name);
                }
            });
            ui.painter().rect_stroke(full.shrink(0.5), rounding, Stroke::new(1.0_f32, pal.panel_stroke));
        });

        match rail_action {
            RailAction::New => self.open_modal(ctx),
            RailAction::Settings => self.open_sheet(ctx),
            RailAction::Folder => {
                crate::core::paths::open(self.paths.root());
            }
            RailAction::Profile => {
                if self.view != View::Profile {
                    self.view = View::Profile;
                    restart_entrance(ctx);
                }
            }
            RailAction::Mods => {
                if self.view != View::Mods {
                    self.view = View::Mods;
                    restart_entrance(ctx);
                    self.do_guard_scan(ctx);
                    if self.store.results.is_empty() {
                        self.do_search(ctx, false);
                    }
                }
            }
            RailAction::Library => {
                self.view = View::Library;
                if let Some(m) = &mut self.modal {
                    m.closing = true;
                }
                if let Some(s) = &mut self.sheet {
                    s.closing = true;
                }
                restart_entrance(ctx);
            }
            RailAction::None => {}
        }
        self.profile_action(ctx, profile_action);
        match store_action {
            StoreAction::Search { append } => self.do_search(ctx, append),
            StoreAction::Install(p) => self.do_install(ctx, p),
            StoreAction::Remove(e) => {
                if let Some(t) = self.store_target() {
                    let _ = crate::core::content::remove(&t.dir, &e);
                    self.refresh_installed(true);
                }
            }
            StoreAction::Toggle(e, on) => {
                if let Some(t) = self.store_target() {
                    let _ = crate::core::content::set_enabled(&t.dir, &e, on);
                    self.refresh_installed(true);
                }
            }
            StoreAction::OpenFolder(folder) => {
                if let Some(t) = self.store_target() {
                    let d = t.dir.join(folder);
                    let _ = std::fs::create_dir_all(&d);
                    crate::core::paths::open(d);
                }
            }
            StoreAction::OpenSettings => self.open_sheet(ctx),
            StoreAction::GuardScan => self.do_guard_scan(ctx),
            StoreAction::Quarantine(rel) => {
                if let Some(tg) = self.store_target() {
                    match crate::core::guard::quarantine(self.paths.root(), &tg.dir, &rel) {
                        Ok(_) => self.toasts.push(ctx, Kind::Success, t(lang, K::QuarantineDone), rel),
                        Err(e) => self.toasts.push(ctx, Kind::Error, t(lang, K::Failed), format!("{e:#}")),
                    }
                    self.refresh_installed(true);
                    self.do_guard_scan(ctx);
                }
            }
            StoreAction::DisableFile(rel) => {
                if let Some(tg) = self.store_target() {
                    let (folder, file) = rel.split_once('/').unwrap_or(("mods", rel.as_str()));
                    match self.installed.iter().find(|e| e.folder == folder && e.file == file).cloned() {
                        Some(e) => {
                            let _ = crate::core::content::set_enabled(&tg.dir, &e, false);
                        }
                        None => {
                            let _ = std::fs::rename(tg.dir.join(&rel), tg.dir.join(format!("{rel}.disabled")));
                        }
                    }
                    self.refresh_installed(true);
                    self.do_guard_scan(ctx);
                }
            }
            StoreAction::None => {}
        }
        match home_action {
            HomeAction::Select(id) => self.settings.selected_instance = Some(id),
            HomeAction::Launch(id) => {
                self.settings.selected_instance = Some(id.clone());
                self.start_launch(ctx, &id);
            }
            HomeAction::OpenFolder(id) => {
                crate::core::paths::open(instances::dir(&self.paths, &id));
            }
            HomeAction::Delete(id) => {
                if self.launch.as_ref().is_some_and(|l| l.instance == id && l.progress.is_busy()) {
                    // never delete a running instance
                } else if instances::delete(&self.paths, &id).is_ok() {
                    self.instances.retain(|i| i.id != id);
                }
            }
            HomeAction::New => self.open_modal(ctx),
            HomeAction::Import => self.pick_modpack(ctx),
            HomeAction::Retry => self.request_versions(ctx),
            HomeAction::Stop(id) => {
                if let Some(l) = self.launch.as_ref().filter(|l| l.instance == id) {
                    l.progress.kill();
                }
            }
            HomeAction::None => {}
        }

        if let Some(m) = &mut self.modal {
            match newinst::show(ctx, &pal, lang, m, &self.catalog) {
                ModalAction::Closed => self.modal = None,
                ModalAction::Create { name, version, loader } => match instances::create(&self.paths, &name, &version, loader) {
                    Ok(inst) => {
                        self.settings.selected_instance = Some(inst.id.clone());
                        self.instances.insert(0, inst);
                        restart_entrance(ctx);
                    }
                    Err(e) => self.toasts.push(ctx, Kind::Error, t(lang, K::Failed), format!("{e:#}")),
                },
                ModalAction::None => {}
            }
        }
        if let Some(d) = &mut self.doctor {
            use crate::ui::doctor::{show, DoctorAction};
            match show(ctx, &pal, lang, d) {
                DoctorAction::Closed => self.doctor = None,
                DoctorAction::OpenLog => {
                    if let Some(p) = d.diag.log.clone() {
                        crate::core::paths::open_text(p);
                    }
                }
                DoctorAction::MoreMemory => {
                    self.settings.memory_mb = (self.settings.memory_mb + 2048).min(crate::core::sysinfo::max_memory_mb());
                    d.fixed = true;
                }
                DoctorAction::DisableMod => {
                    if let Some(f) = d.diag.mod_file.clone() {
                        let dir = instances::dir(&self.paths, &d.instance);
                        let e = crate::core::content::list_all(&dir).into_iter().find(|e| e.folder == "mods" && e.file == f);
                        let r = match e {
                            Some(e) => crate::core::content::set_enabled(&dir, &e, false),
                            None => std::fs::rename(dir.join("mods").join(&f), dir.join("mods").join(format!("{f}.disabled"))).map_err(Into::into),
                        };
                        match r {
                            Ok(()) => {
                                d.fixed = true;
                                self.toasts.push(ctx, Kind::Success, t(lang, K::DocDisabled), f);
                                self.refresh_installed(true);
                            }
                            Err(e) => self.toasts.push(ctx, Kind::Error, t(lang, K::Failed), format!("{e:#}")),
                        }
                    }
                }
                DoctorAction::Retry => {
                    let id = d.instance.clone();
                    self.doctor = None;
                    self.start_launch(ctx, &id);
                }
                DoctorAction::None => {}
            }
        }
        if let Some(s) = &mut self.sheet {
            if sheet::show(ctx, &pal, &mut self.settings, s, &self.paths, &self.wallpaper) {
                self.sheet = None;
            }
        }

        self.poll_import(ctx);
        self.handle_drops(ctx, &pal);
        let ongoing_name = self.launch.as_ref().and_then(|l| self.instances.iter().find(|i| i.id == l.instance)).map(|i| i.name.clone());
        let ongoing = self.launch.as_ref().and_then(|l| {
            let st = l.progress.stage();
            let label = home::stage_label(lang, &st)?;
            (st != Stage::Running).then(|| (label, l.progress.fraction()))
        });
        let import_og = self.import.as_ref().map(|i| (i.name.clone(), i.progress.fraction()));
        let og = match (&import_og, ongoing) {
            (Some((name, f)), _) => Some(Ongoing { title: name, stage: t(lang, K::ImportingPack), fraction: *f }),
            (None, Some((stage, fraction))) => Some(Ongoing { title: ongoing_name.as_deref().unwrap_or(""), stage, fraction }),
            _ => None,
        };
        self.toasts.show(ctx, &pal, og);

        if std::env::var_os("SKL_FPS").is_some() {
            let dt = ctx.input(|i| i.unstable_dt);
            ctx.debug_painter().text(ctx.screen_rect().left_bottom() + vec2(90.0, -14.0), egui::Align2::LEFT_CENTER, format!("{:.1} ms", dt * 1000.0), egui::FontId::monospace(12.0), egui::Color32::YELLOW);
        }
        self.persist_settings(ctx);
    }
}
