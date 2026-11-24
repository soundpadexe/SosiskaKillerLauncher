//! Content store: browse Modrinth / CurseForge, install into the selected
//! instance, manage what's installed.
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};

use egui::{pos2, vec2, Align2, Color32, ColorImage, FontId, Id, Rect, Sense, Stroke, TextureHandle, TextureOptions, Ui};

use super::anim::{appear, ease_out, motion, spring_bool, SNAPPY};
use super::icons;
use super::theme::Palette;
use super::widgets::{ghost_button, icon_button, lerp_color, segmented, toggle};
use crate::core::i18n::{t, Lang, K};
use crate::core::instances::{Instance, Loader};
use crate::models::content::{ContentKind, InstalledEntry, Project, Source};

// ---------------------------------------------------------------- state

#[derive(Default)]
pub struct StoreState {
    pub tab: usize,
    pub kind: usize,
    pub source: usize,
    pub query: String,
    dirty_at: Option<f64>,
    pub results: Vec<Project>,
    pub loading: bool,
    pub exhausted: bool,
    pub error: Option<String>,
    /// Bumped per search so late responses for old queries are dropped.
    pub req_id: u64,
    pub results_t0: f64,
}

impl StoreState {
    pub fn kind(&self) -> ContentKind {
        ContentKind::ALL[self.kind]
    }
    pub fn source(&self) -> Source {
        if self.source == 1 { Source::CurseForge } else { Source::Modrinth }
    }
}

pub enum InstallState {
    Installing,
    Failed,
}

pub enum StoreAction {
    None,
    Search { append: bool },
    Install(Project),
    Remove(InstalledEntry),
    Toggle(InstalledEntry, bool),
    OpenFolder(&'static str),
    OpenSettings,
    GuardScan,
    /// Relative file path inside the instance.
    Quarantine(String),
    DisableFile(String),
}

// ---------------------------------------------------------------- icons

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum IconKind {
    #[default]
    Logo,
    Skin,
    Cape,
}

pub struct Icons {
    pub kind: IconKind,
    map: HashMap<String, Option<TextureHandle>>,
    /// URLs that finished without a usable image (hidden by callers).
    failed: std::collections::HashSet<String>,
    wanted: Vec<String>,
    pub tx: Sender<(String, Option<ColorImage>)>,
    rx: Receiver<(String, Option<ColorImage>)>,
}

impl Default for Icons {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { kind: IconKind::Logo, map: HashMap::new(), failed: Default::default(), wanted: Vec::new(), tx, rx }
    }
}

impl Icons {
    pub fn of(kind: IconKind) -> Self {
        Self { kind, ..Self::default() }
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((url, img)) = self.rx.try_recv() {
            let opts = if self.kind == IconKind::Logo { TextureOptions::LINEAR } else { TextureOptions::NEAREST };
            if img.is_none() {
                self.failed.insert(url.clone());
            }
            let tex = img.map(|i| ctx.load_texture(format!("icon:{url}"), i, opts));
            self.map.insert(url, tex);
        }
    }
    pub fn get(&mut self, url: &str) -> Option<TextureHandle> {
        match self.map.get(url) {
            Some(t) => t.clone(),
            None => {
                self.map.insert(url.to_owned(), None);
                self.wanted.push(url.to_owned());
                None
            }
        }
    }
    pub fn failed(&self, url: &str) -> bool {
        self.failed.contains(url)
    }

    pub fn take_wanted(&mut self) -> Vec<String> {
        std::mem::take(&mut self.wanted)
    }
}

// ---------------------------------------------------------------- view

pub struct StoreView<'a> {
    pub instance: Option<&'a Instance>,
    pub installed: &'a [InstalledEntry],
    pub installing: &'a HashMap<String, InstallState>,
    pub has_cf_key: bool,
    pub guard: Option<&'a crate::core::guard::Report>,
    pub guard_busy: bool,
}

fn fmt_count(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.0}K", n as f64 / 1e3),
        n => n.to_string(),
    }
}

fn kind_label(lang: Lang, k: usize) -> &'static str {
    [t(lang, K::KindMods), t(lang, K::KindResourcePacks), t(lang, K::KindShaders)][k]
}

pub fn show(ui: &mut Ui, pal: &Palette, lang: Lang, st: &mut StoreState, v: StoreView, icons: &mut Icons) -> StoreAction {
    let mut action = StoreAction::None;
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let area = ui.max_rect().shrink2(vec2(34.0, 6.0));
    let p = ui.painter().clone();

    // Header
    let a = appear(&ctx, 0.0, 0.6);
    let mut hp = p.clone();
    hp.set_opacity(a);
    let hy = area.top() + (1.0 - a) * 6.0;
    hp.text(pos2(area.left(), hy + 18.0), Align2::LEFT_CENTER, t(lang, K::Mods), FontId::proportional(30.0), pal.text);
    let sub = match v.instance {
        Some(i) => format!("{} «{}» · {} · {}", t(lang, K::ForInstance), i.name, i.version, i.loader.name()),
        None => t(lang, K::NoInstanceSelected).to_owned(),
    };
    hp.text(pos2(area.left(), hy + 46.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.5), pal.text_dim);

    let Some(inst) = v.instance else { return action };

    // Toolbar: Browse/Installed · kind · source · search
    let bar = Rect::from_min_size(pos2(area.left(), area.top() + 72.0), vec2(area.width(), 40.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar), |ui| {
        ui.set_opacity(appear(&ctx, 0.05, 0.6));
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let tabs = [t(lang, K::Browse), t(lang, K::Installed)];
            segmented(ui, pal, "store_tab", &tabs, &mut st.tab, true);
            if st.tab == 0 {
                let kinds = [kind_label(lang, 0), kind_label(lang, 1), kind_label(lang, 2)];
                if segmented(ui, pal, "store_kind", &kinds, &mut st.kind, false) {
                    action = StoreAction::Search { append: false };
                }
                let sources = ["Modrinth", "CurseForge"];
                if segmented(ui, pal, "store_src", &sources, &mut st.source, false) {
                    action = StoreAction::Search { append: false };
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (fr, _) = ui.allocate_exact_size(vec2(240.0, 38.0), Sense::hover());
                    let foc = ui.memory(|m| m.has_focus(Id::new("store_q")));
                    let fo = spring_bool(&ctx, Id::new("store_q_f"), foc, SNAPPY);
                    ui.painter().rect_filled(fr, 12.0, Color32::from_black_alpha((70.0 + 40.0 * fo) as u8));
                    ui.painter().rect_stroke(fr, 12.0, Stroke::new(1.0_f32, lerp_color(Color32::from_white_alpha(18), pal.accent, fo)));
                    icons::search(ui.painter(), pos2(fr.left() + 20.0, fr.center().y), 18.0, pal.text_dim);
                    let inner = Rect::from_min_max(pos2(fr.left() + 36.0, fr.top() + 9.0), fr.max - vec2(10.0, 6.0));
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
                        if ui
                            .add(egui::TextEdit::singleline(&mut st.query).id(Id::new("store_q")).frame(false).hint_text(t(lang, K::SearchContent)).desired_width(inner.width()))
                            .changed()
                        {
                            st.dirty_at = Some(now);
                        }
                    });
                });
            } else {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ghost_button(ui, pal, t(lang, K::OpenFolder)).clicked() {
                        action = StoreAction::OpenFolder(ContentKind::Mod.folder());
                    }
                });
            }
        });
    });

    // Debounced search while typing
    if let Some(t0) = st.dirty_at {
        if now - t0 > 0.35 {
            st.dirty_at = None;
            action = StoreAction::Search { append: false };
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    let body = Rect::from_min_max(pos2(area.left() - 8.0, bar.bottom() + 14.0), pos2(area.right() + 8.0, ui.max_rect().bottom()));
    let mut top = body.top();

    // Contextual banner
    let banner = if st.tab == 0 && st.kind() == ContentKind::Mod && inst.loader == Loader::Vanilla {
        Some((t(lang, K::NeedsLoader), false))
    } else if st.tab == 0 && st.source() == Source::CurseForge && !v.has_cf_key {
        Some((t(lang, K::CfKeyMissing), true))
    } else {
        None
    };
    if let Some((msg, to_settings)) = banner {
        let r = Rect::from_min_size(pos2(body.left() + 8.0, top), vec2(body.width() - 16.0, 46.0));
        p.rect_filled(r, 12.0, pal.accent.gamma_multiply(0.12));
        p.rect_stroke(r, 12.0, Stroke::new(1.0_f32, pal.accent.gamma_multiply(0.35)));
        icons::alert(&p, pos2(r.left() + 22.0, r.center().y), 18.0, pal.accent);
        p.text(pos2(r.left() + 42.0, r.center().y), Align2::LEFT_CENTER, msg, FontId::proportional(13.0), pal.text);
        if to_settings && ui.interact(r, Id::new("cf_banner"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            action = StoreAction::OpenSettings;
        }
        top = r.bottom() + 12.0;
    }
    let list_rect = Rect::from_min_max(pos2(body.left(), top), body.max);

    if st.tab == 1 {
        let used = guard_card(ui, pal, lang, list_rect, v.guard, v.guard_busy, &mut action);
        let rest = Rect::from_min_max(pos2(list_rect.left(), list_rect.top() + used), list_rect.max);
        installed_list(ui, pal, lang, rest, v.installed, &mut action);
        return action;
    }

    // ---------------- results
    let installs_blocked = st.kind() == ContentKind::Mod && inst.loader == Loader::Vanilla;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(list_rect), |ui| {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let w = ui.available_width() - 16.0;
            if st.results.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(w, 120.0), Sense::hover());
                let msg = if st.loading { t(lang, K::Loading).to_owned() } else if let Some(e) = &st.error { e.clone() } else { t(lang, K::NoResults).to_owned() };
                if st.loading {
                    icons::ring(ui.painter(), r.center() - vec2(0.0, 18.0), 12.0, None, now, pal.accent, Color32::from_white_alpha(30));
                }
                ui.painter().text(r.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, msg, FontId::proportional(13.5), pal.text_dim);
                return;
            }
            for (i, proj) in st.results.iter().enumerate() {
                // Rows ease in after each search, staggered.
                let d = 0.04 * (i % crate::models::content::PAGE) as f64;
                let dur = 0.55 * motion(&ctx) as f64;
                let k = if dur <= 0.0 { 1.0 } else { ease_out((((now - st.results_t0 - d) / dur).clamp(0.0, 1.0)) as f32) };
                if k < 1.0 {
                    ctx.request_repaint();
                }
                let (row, resp) = ui.allocate_exact_size(vec2(w + 16.0, 86.0), Sense::hover());
                let row = row.shrink2(vec2(8.0, 4.0)).translate(vec2(0.0, (1.0 - k) * 8.0));
                let mut rp = ui.painter().clone();
                rp.set_opacity(k);
                let h = spring_bool(&ctx, Id::new(("row_h", proj.key())), resp.hovered(), SNAPPY);
                rp.rect_filled(row, 16.0, lerp_color(pal.panel, pal.panel.gamma_multiply(1.4), h));
                rp.rect_stroke(row, 16.0, Stroke::new(1.0_f32, Color32::from_white_alpha((14.0 + 14.0 * h) as u8)));

                let ic = Rect::from_min_size(row.min + vec2(14.0, 13.0), vec2(52.0, 52.0));
                match proj.icon_url.as_deref().and_then(|u| icons.get(u)) {
                    Some(tex) => {
                        let mut s = egui::epaint::RectShape::filled(ic, 12.0, Color32::WHITE);
                        s.fill_texture_id = tex.id();
                        s.uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                        rp.add(s);
                    }
                    None => {
                        let hue = (crate::ui::cover::seed_of(&proj.id) % 360) as f32 / 360.0;
                        rp.rect_filled(ic, 12.0, Color32::from(egui::ecolor::Hsva::new(hue, 0.3, 0.45, 1.0)));
                        let ch = proj.title.chars().next().unwrap_or('?').to_uppercase().to_string();
                        rp.text(ic.center(), Align2::CENTER_CENTER, ch, FontId::proportional(22.0), Color32::WHITE);
                    }
                }

                let tx = ic.right() + 16.0;
                let btn_w = 150.0;
                let text_w = (row.right() - btn_w - 30.0 - tx).max(80.0);
                let title = rp.layout_no_wrap(proj.title.clone(), FontId::proportional(15.5), pal.text);
                let tw = title.size().x.min(text_w);
                rp.galley(pos2(tx, row.top() + 14.0), title, pal.text);
                rp.text(pos2(tx + tw + 8.0, row.top() + 23.0), Align2::LEFT_CENTER, format!("· {}", proj.author), FontId::proportional(12.0), pal.text_dim);
                let mut job = egui::text::LayoutJob::simple(proj.description.clone(), FontId::proportional(12.5), pal.text_dim, text_w);
                job.wrap.max_rows = 1;
                job.wrap.overflow_character = Some('…');
                rp.galley(pos2(tx, row.top() + 38.0), ui.fonts(|f| f.layout_job(job)), pal.text_dim);
                rp.text(
                    pos2(tx, row.bottom() - 15.0),
                    Align2::LEFT_CENTER,
                    format!("{} {}  ·  {}", fmt_count(proj.downloads), t(lang, K::Downloads), proj.source.name()),
                    FontId::proportional(11.5),
                    pal.text_dim.gamma_multiply(0.8),
                );

                // Install button
                let br = Rect::from_center_size(pos2(row.right() - 16.0 - btn_w / 2.0, row.center().y), vec2(btn_w, 38.0));
                let is_installed = v.installed.iter().any(|e| e.source == proj.source && e.project_id == proj.id);
                let state = v.installing.get(&proj.key());
                let bresp = ui.interact(br, Id::new(("inst_btn", proj.key())), Sense::click());
                let bh = spring_bool(&ctx, bresp.id, bresp.hovered(), SNAPPY);
                let enabled = !installs_blocked && !matches!(state, Some(InstallState::Installing));
                if bresp.hovered() && enabled {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                match (state, is_installed) {
                    (Some(InstallState::Installing), _) => {
                        rp.rect_filled(br, 11.0, pal.accent.gamma_multiply(0.2));
                        icons::ring(&rp, pos2(br.left() + 22.0, br.center().y), 8.0, None, now, pal.accent, Color32::from_white_alpha(30));
                        rp.text(br.center() + vec2(10.0, 0.0), Align2::CENTER_CENTER, t(lang, K::Installing), FontId::proportional(13.0), pal.text);
                        ctx.request_repaint();
                    }
                    (_, true) => {
                        rp.rect_filled(br, 11.0, Color32::from_white_alpha((14.0 + 10.0 * bh) as u8));
                        icons::check(&rp, pos2(br.left() + 24.0, br.center().y), 16.0, pal.accent);
                        rp.text(br.center() + vec2(10.0, 0.0), Align2::CENTER_CENTER, t(lang, K::InstalledOk), FontId::proportional(13.0), pal.text);
                    }
                    (failed, false) => {
                        let fill = if enabled { lerp_color(pal.accent, Color32::WHITE, 0.1 * bh) } else { Color32::from_white_alpha(14) };
                        rp.rect_filled(br, 11.0, fill);
                        let label = if matches!(failed, Some(InstallState::Failed)) { t(lang, K::Retry) } else { t(lang, K::Install) };
                        rp.text(br.center(), Align2::CENTER_CENTER, label, FontId::proportional(13.5), if enabled { pal.on_accent } else { pal.text_dim });
                        if bresp.clicked() && enabled {
                            action = StoreAction::Install(proj.clone());
                        }
                    }
                }
            }
            // Infinite scroll sentinel
            if !st.exhausted {
                let (r, _) = ui.allocate_exact_size(vec2(w, 54.0), Sense::hover());
                if st.loading {
                    icons::ring(ui.painter(), r.center(), 10.0, None, now, pal.accent, Color32::from_white_alpha(30));
                    ctx.request_repaint();
                } else if ui.is_rect_visible(r) {
                    action = StoreAction::Search { append: true };
                }
            }
        });
    });
    action
}

fn installed_list(ui: &mut Ui, pal: &Palette, lang: Lang, rect: Rect, list: &[InstalledEntry], action: &mut StoreAction) {
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let w = ui.available_width() - 16.0;
            if list.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(w, 120.0), Sense::hover());
                ui.painter().text(r.center(), Align2::CENTER_CENTER, t(lang, K::NothingInstalled), FontId::proportional(14.0), pal.text_dim);
                return;
            }
            let mut last_folder = "";
            for (i, e) in list.iter().enumerate() {
                if e.folder != last_folder {
                    last_folder = &e.folder;
                    ui.add_space(if i == 0 { 2.0 } else { 14.0 });
                    let label = match e.folder.as_str() {
                        "mods" => t(lang, K::KindMods),
                        "resourcepacks" => t(lang, K::KindResourcePacks),
                        _ => t(lang, K::KindShaders),
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(label.to_uppercase()).size(11.0).color(pal.text_dim));
                    });
                    ui.add_space(4.0);
                }
                let a = appear(ui.ctx(), 0.05 + i as f32 * 0.03, 0.6);
                let (row, _) = ui.allocate_exact_size(vec2(w + 16.0, 60.0), Sense::hover());
                let row = row.shrink2(vec2(8.0, 3.0)).translate(vec2(0.0, (1.0 - a) * 6.0));
                let mut rp = ui.painter().clone();
                rp.set_opacity(a);
                rp.rect_filled(row, 14.0, pal.panel);
                rp.rect_stroke(row, 14.0, Stroke::new(1.0_f32, Color32::from_white_alpha(14)));
                let dim = if e.enabled { 1.0 } else { 0.45 };
                rp.text(pos2(row.left() + 18.0, row.top() + 21.0), Align2::LEFT_CENTER, &e.title, FontId::proportional(14.5), pal.text.gamma_multiply(dim));
                let tag = match (e.source, e.dependency) {
                    (crate::models::content::Source::Local, _) => t(lang, K::LocalFile).to_owned(),
                    (s, true) => format!("{} · {}", s.name(), t(lang, K::Dependency)),
                    (s, false) => s.name().to_owned(),
                };
                rp.text(pos2(row.left() + 18.0, row.top() + 40.0), Align2::LEFT_CENTER, format!("{}  ·  {}", e.file, tag), FontId::proportional(11.5), pal.text_dim.gamma_multiply(dim));

                let ctrl = Rect::from_min_max(pos2(row.right() - 120.0, row.top()), row.max - vec2(12.0, 0.0));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(ctrl), |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, pal, 36.0, 0.0, |p, c, col| icons::trash(p, c, 18.0, col)).on_hover_text(t(lang, K::Remove)).clicked() {
                            *action = StoreAction::Remove(e.clone());
                        }
                        ui.add_space(6.0);
                        let mut on = e.enabled;
                        if toggle(ui, pal, &mut on) {
                            *action = StoreAction::Toggle(e.clone(), on);
                        }
                    });
                });
            }
        });
    });
}

/// Safety Guard summary for the selected instance. Returns the height used.
fn guard_card(ui: &mut Ui, pal: &Palette, lang: Lang, area: Rect, report: Option<&crate::core::guard::Report>, busy: bool, action: &mut StoreAction) -> f32 {
    use crate::core::guard::Severity;
    let ctx = ui.ctx().clone();
    let findings = report.map(|r| r.findings.as_slice()).unwrap_or(&[]);
    let h = 62.0 + findings.len() as f32 * 54.0;
    let r = Rect::from_min_size(area.min + vec2(8.0, 0.0), vec2(area.width() - 16.0, h));
    let a = appear(&ctx, 0.05, 0.6);
    let mut p = ui.painter().clone();
    p.set_opacity(a);
    let worst = findings.first().map(|f| f.severity);
    let tone = match worst {
        Some(Severity::Malware) | Some(Severity::Conflict) => pal.danger,
        Some(Severity::Suspicious) => Color32::from_rgb(226, 180, 96),
        None => pal.accent,
    };
    p.rect_filled(r, 16.0, tone.gamma_multiply(0.1));
    p.rect_stroke(r, 16.0, Stroke::new(1.0_f32, tone.gamma_multiply(0.35)));
    let ic = pos2(r.left() + 26.0, r.top() + 31.0);
    if busy {
        icons::ring(&p, ic, 10.0, None, ctx.input(|i| i.time), tone, Color32::from_white_alpha(30));
        ctx.request_repaint();
    } else if worst.is_none() {
        icons::check(&p, ic, 18.0, tone);
    } else {
        icons::alert(&p, ic, 18.0, tone);
    }
    p.text(pos2(r.left() + 48.0, r.top() + 22.0), Align2::LEFT_CENTER, "Safety Guard", FontId::proportional(14.5), pal.text);
    let status = match (busy, report) {
        (true, _) => t(lang, K::Scanning).to_owned(),
        (false, Some(rep)) if rep.findings.is_empty() => format!("{} · {} {}", t(lang, K::GuardClean), rep.scanned, t(lang, K::GuardChecked)),
        (false, Some(rep)) => format!("{} · {} {}", rep.findings.len(), rep.scanned, t(lang, K::GuardChecked)),
        (false, None) => String::new(),
    };
    p.text(pos2(r.left() + 48.0, r.top() + 42.0), Align2::LEFT_CENTER, status, FontId::proportional(12.0), pal.text_dim);
    let btn = Rect::from_min_size(pos2(r.right() - 130.0, r.top() + 14.0), vec2(116.0, 34.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(btn), |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !busy && ghost_button(ui, pal, t(lang, K::GuardRescan)).clicked() {
                *action = StoreAction::GuardScan;
            }
        });
    });

    for (i, f) in findings.iter().enumerate() {
        let row = Rect::from_min_size(pos2(r.left() + 14.0, r.top() + 60.0 + i as f32 * 54.0), vec2(r.width() - 28.0, 48.0));
        p.rect_filled(row, 12.0, Color32::from_black_alpha(60));
        let (label, col) = match f.severity {
            Severity::Malware => (t(lang, K::SevMalware), pal.danger),
            Severity::Conflict => (t(lang, K::SevConflict), pal.danger.gamma_multiply(0.85)),
            Severity::Suspicious => (t(lang, K::SevSuspicious), Color32::from_rgb(226, 180, 96)),
        };
        let b = super::widgets::era_badge(&p, pos2(row.left() + 12.0, row.center().y), label, col);
        p.text(pos2(b.right() + 12.0, row.top() + 16.0), Align2::LEFT_CENTER, &f.file, FontId::proportional(13.0), pal.text);
        p.text(pos2(b.right() + 12.0, row.top() + 33.0), Align2::LEFT_CENTER, &f.what, FontId::proportional(11.5), pal.text_dim);
        let host_file = std::path::Path::new(&f.file).is_absolute();
        let br = Rect::from_min_size(pos2(row.right() - 140.0, row.top() + 8.0), vec2(132.0, 32.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(br), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (text, act) = match f.severity {
                    Severity::Conflict if !host_file => (t(lang, K::Disable), StoreAction::DisableFile(f.file.clone())),
                    _ => (t(lang, K::Quarantine), StoreAction::Quarantine(f.file.clone())),
                };
                if ghost_button(ui, pal, text).clicked() {
                    *action = act;
                }
            });
        });
    }
    h + 12.0
}
