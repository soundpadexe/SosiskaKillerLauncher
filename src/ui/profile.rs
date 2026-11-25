//! Profile: nickname, skin & cape. Three tabs on the right — my skin, a
//! browsable skin catalog, and capes — with a live front/back model on the left
//! that previews whatever is selected before it is applied.
use egui::{pos2, vec2, Align2, Color32, FontId, Id, Mesh, Painter, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Ui};

use super::anim::{appear, ease_out, motion, spring_bool, BOUNCY, SNAPPY};
use super::store::Icons;
use super::theme::Palette;
use super::widgets::{ghost_button, lerp_color, play_button, segmented, ButtonState};
use crate::api::skins::{CapeItem, SkinItem};
use crate::core::auth::{is_valid_nickname, offline_uuid};
use crate::core::i18n::{t, Lang, K};
use crate::core::settings::{AccountMode, Settings};

/// Well-known profiles shown before the first cape search.
pub const POPULAR_CAPE_OWNERS: [&str; 6] = ["jeb_", "Dinnerbone", "Grumm", "Searge", "deadmau5", "Marc"];

#[derive(Default)]
pub struct Shop {
    pub tab: usize,
    pub query: String,
    dirty_at: Option<f64>,
    pub skins: Vec<SkinItem>,
    pub next: Option<String>,
    pub loading: bool,
    pub exhausted: bool,
    pub error: Option<String>,
    pub req: u64,
    pub results_t0: f64,
    pub selected: Option<String>,
    pub cape_query: String,
    pub capes: Vec<CapeItem>,
    pub capes_loading: bool,
    pub capes_error: Option<String>,
    pub capes_req: u64,
    pub capes_t0: f64,
    pub cape_selected: Option<usize>,
    capes_started: bool,
}

/// What the profile page needs to know about the Microsoft sign-in.
#[derive(Default)]
pub struct MsView {
    /// Signed-in account: (name, dashed uuid).
    pub account: Option<(String, String)>,
    /// Pending device-code login: (user code, page URL); `Some(None)` = still requesting a code.
    pub pending: Option<Option<(String, String)>>,
    pub uploading: bool,
}

pub enum ProfileAction {
    None,
    SetMode(AccountMode),
    MsLogin,
    MsCancel,
    MsLogout,
    OpenUrl(String),
    PickSkin,
    PickCape,
    ClearSkin,
    ClearCape,
    Copy(String),
    OpenEly,
    SkinSearch { append: bool },
    ApplySkin(SkinItem),
    CapeSearch(Vec<String>),
    ApplyCape(CapeItem),
}

// ---------------------------------------------------------------- figure

/// Texture-space rect in skin pixels → normalized UV (optionally mirrored).
fn uv(x: f32, y: f32, w: f32, h: f32, tex_h: f32, mirror: bool) -> Rect {
    let (x0, x1) = if mirror { (x + w, x) } else { (x, x + w) };
    Rect::from_min_max(pos2(x0 / 64.0, y / tex_h), pos2(x1 / 64.0, (y + h) / tex_h))
}

fn quad(p: &Painter, tex: &TextureHandle, dst: Rect, uv: Rect) {
    let mut m = Mesh::with_texture(tex.id());
    m.add_rect_with_uv(dst, uv, Color32::WHITE);
    p.add(m);
}

fn tex_h(tex: &TextureHandle) -> f32 {
    tex.size()[1] as f32 * 64.0 / tex.size()[0] as f32
}

/// Draws the whole figure. `origin` = top-left, `u` = pixels per skin pixel.
pub fn draw_figure(p: &Painter, origin: Pos2, u: f32, skin: Option<&TextureHandle>, slim: bool, back: bool, cape: Option<&TextureHandle>, fallback: Color32) {
    let aw = if slim { 3.0 } else { 4.0 };
    let r = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(origin + vec2(x * u, y * u), vec2(w * u, h * u));
    let left_arm_x = if slim { 1.0 } else { 0.0 };
    type Part = (Rect, (f32, f32), Option<(f32, f32)>, (f32, f32));
    let Some(tex) = skin else {
        for (x, y, w, h) in [(4.0, 0.0, 8.0, 8.0), (4.0, 8.0, 8.0, 12.0), (left_arm_x, 8.0, aw, 12.0), (12.0, 8.0, aw, 12.0), (4.0, 20.0, 4.0, 12.0), (8.0, 20.0, 4.0, 12.0)] {
            p.rect_filled(r(x, y, w, h).shrink(0.5), 2.0, fallback);
        }
        return;
    };
    let th = tex_h(tex);
    let legacy = th < 64.0;
    let parts: Vec<Part> = if !back {
        vec![
            (r(4.0, 0.0, 8.0, 8.0), (8.0, 8.0), Some((40.0, 8.0)), (8.0, 8.0)),
            (r(4.0, 8.0, 8.0, 12.0), (20.0, 20.0), Some((20.0, 36.0)), (20.0, 20.0)),
            (r(left_arm_x, 8.0, aw, 12.0), (44.0, 20.0), Some((44.0, 36.0)), (44.0, 20.0)),
            (r(12.0, 8.0, aw, 12.0), (36.0, 52.0), Some((52.0, 52.0)), (44.0, 20.0)),
            (r(4.0, 20.0, 4.0, 12.0), (4.0, 20.0), Some((4.0, 36.0)), (4.0, 20.0)),
            (r(8.0, 20.0, 4.0, 12.0), (20.0, 52.0), Some((4.0, 52.0)), (4.0, 20.0)),
        ]
    } else {
        let arm_back = 48.0 + aw;
        vec![
            (r(4.0, 0.0, 8.0, 8.0), (24.0, 8.0), Some((56.0, 8.0)), (24.0, 8.0)),
            (r(4.0, 8.0, 8.0, 12.0), (32.0, 20.0), Some((32.0, 36.0)), (32.0, 20.0)),
            (r(left_arm_x, 8.0, aw, 12.0), (40.0 + aw, 52.0), Some((56.0 + aw, 52.0)), (52.0, 20.0)),
            (r(12.0, 8.0, aw, 12.0), (arm_back, 20.0), Some((arm_back, 36.0)), (52.0, 20.0)),
            (r(4.0, 20.0, 4.0, 12.0), (28.0, 52.0), Some((12.0, 52.0)), (12.0, 20.0)),
            (r(8.0, 20.0, 4.0, 12.0), (12.0, 20.0), Some((12.0, 36.0)), (12.0, 20.0)),
        ]
    };
    for (i, (dst, src, over, legacy_src)) in parts.iter().enumerate() {
        let (w, h) = (dst.width() / u, dst.height() / u);
        let mirrored = legacy && matches!((back, i), (false, 3) | (false, 5) | (true, 2) | (true, 4));
        let s = if mirrored { *legacy_src } else { *src };
        quad(p, tex, *dst, uv(s.0, s.1, w, h, th, mirrored));
        if let (Some(o), true) = (over, !legacy || i == 0) {
            quad(p, tex, dst.expand(u * 0.5), uv(o.0, o.1, w, h, th, false));
        }
    }
    if back {
        if let Some(c) = cape {
            quad(p, c, r(3.0, 8.0, 10.0, 16.0), uv(1.0, 1.0, 10.0, 16.0, tex_h(c), false));
        }
    }
}

// ---------------------------------------------------------------- page

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    pal: &Palette,
    lang: Lang,
    s: &mut Settings,
    skin: Option<&TextureHandle>,
    cape: Option<&TextureHandle>,
    shop: &mut Shop,
    skin_icons: &mut Icons,
    cape_icons: &mut Icons,
    ms: &MsView,
) -> ProfileAction {
    let mut action = ProfileAction::None;
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let area = ui.max_rect().shrink2(vec2(34.0, 6.0));
    let p = ui.painter().clone();

    let a = appear(&ctx, 0.0, 0.6);
    let mut hp = p.clone();
    hp.set_opacity(a);
    hp.text(pos2(area.left(), area.top() + 18.0 + (1.0 - a) * 6.0), Align2::LEFT_CENTER, t(lang, K::Profile), FontId::proportional(30.0), pal.text);

    // What the model shows: a selected catalog skin / cape previews before applying.
    let sel_skin = (shop.tab == 1).then(|| shop.selected.as_ref().and_then(|id| shop.skins.iter().find(|i| &i.id == id))).flatten().cloned();
    let sel_cape = (shop.tab == 2).then(|| shop.cape_selected.and_then(|i| shop.capes.get(i))).flatten().cloned();
    let preview_skin = sel_skin.as_ref().and_then(|i| skin_icons.get(&i.image_url()));
    let preview_cape = sel_cape.as_ref().and_then(|c| cape_icons.get(&c.url));
    let previewing = preview_skin.is_some() || preview_cape.is_some();
    let show_skin = preview_skin.as_ref().or(skin);
    let show_cape = preview_cape.as_ref().or(cape);
    let show_slim = if preview_skin.is_some() { false } else { s.skin_slim };

    // ---- stage (left)
    let stage_w = (area.width() * 0.36).clamp(280.0, 420.0);
    let stage = Rect::from_min_max(pos2(area.left(), area.top() + 64.0), pos2(area.left() + stage_w, area.bottom() - 10.0));
    let a2 = appear(&ctx, 0.08, 0.7);
    let stage_r = stage.translate(vec2(0.0, (1.0 - a2) * 8.0));
    let mut sp = p.clone();
    sp.set_opacity(a2);
    sp.add(egui::epaint::Shadow { offset: vec2(0.0, 12.0), blur: 36.0, spread: 0.0, color: Color32::from_black_alpha(80) }.as_shape(stage_r, 24.0));
    sp.rect_filled(stage_r, 24.0, pal.panel);
    sp.rect_stroke(stage_r, 24.0, Stroke::new(1.0_f32, pal.panel_stroke));
    let view = stage_r.shrink2(vec2(10.0, 46.0));
    let resp = ui.interact(view, Id::new("skin3d_drag"), Sense::click_and_drag());
    super::skin3d::show(&sp, view, &resp, show_skin, show_slim, show_cape, Color32::from_white_alpha(40));
    let hint_a = spring_bool(&ctx, Id::new("skin3d_hint"), resp.hovered(), SNAPPY);
    sp.text(pos2(stage_r.center().x, stage_r.bottom() - 24.0), Align2::CENTER_CENTER, t(lang, K::Rotate3d), FontId::proportional(12.0), lerp_color(pal.text_dim.gamma_multiply(0.6), pal.text_dim, hint_a));
    let pv = spring_bool(&ctx, Id::new("preview_chip"), previewing, SNAPPY);
    if pv > 0.01 {
        let chip = Rect::from_center_size(pos2(stage_r.center().x, stage_r.top() + 26.0), vec2(170.0, 26.0));
        sp.rect_filled(chip, 13.0, pal.accent.gamma_multiply(0.18 * pv));
        sp.text(chip.center(), Align2::CENTER_CENTER, t(lang, K::Preview), FontId::proportional(12.0), pal.accent.gamma_multiply(pv));
    } else if skin.is_none() {
        sp.text(pos2(stage_r.center().x, stage_r.top() + 26.0), Align2::CENTER_CENTER, t(lang, K::NoSkin), FontId::proportional(12.5), pal.text_dim);
    }

    // ---- right column
    let col = Rect::from_min_max(pos2(stage.right() + 28.0, stage.top()), pos2(area.right(), area.bottom() - 10.0));
    let tabs_r = Rect::from_min_size(col.min, vec2(col.width(), 38.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(tabs_r), |ui| {
        ui.set_opacity(appear(&ctx, 0.1, 0.6));
        let labels = [t(lang, K::MySkin), t(lang, K::SkinCatalog), t(lang, K::Capes)];
        if segmented(ui, pal, "profile_tab", &labels, &mut shop.tab, true) {
            super::anim::restart_entrance(&ctx);
        }
    });
    let body = Rect::from_min_max(pos2(col.left(), tabs_r.bottom() + 16.0), col.max);
    match shop.tab {
        0 => my_skin(ui, pal, lang, s, ms, skin.is_some(), cape.is_some(), body, &mut action),
        1 => catalog(ui, pal, lang, shop, skin_icons, body, now, &mut action),
        _ => capes(ui, pal, lang, shop, cape_icons, body, now, &mut action),
    }
    action
}

fn section(ui: &mut Ui, pal: &Palette, title: &str) {
    ui.label(RichText::new(title.to_uppercase()).size(11.0).color(pal.accent).strong());
    ui.add_space(6.0);
}

fn input(ui: &mut Ui, pal: &Palette, id: &str, text: &mut String, hint: &str, width: f32, limit: usize) -> egui::Response {
    let ctx = ui.ctx().clone();
    let fr = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover()).0;
    let foc = ui.memory(|m| m.has_focus(Id::new(id)));
    let fo = spring_bool(&ctx, Id::new((id, "f")), foc, SNAPPY);
    ui.painter().rect_filled(fr, 12.0, Color32::from_black_alpha((80.0 + 40.0 * fo) as u8));
    ui.painter().rect_stroke(fr, 12.0, Stroke::new(1.0_f32, lerp_color(Color32::from_white_alpha(20), pal.accent, fo)));
    // `put` places the editor inside the frame without moving the layout cursor,
    // so the next widget starts after the *whole* field (no overlap).
    let inner = fr.shrink2(vec2(14.0, 11.0));
    super::widgets::place(ui, inner, egui::TextEdit::singleline(text).id(Id::new(id)).frame(false).char_limit(limit).hint_text(hint).desired_width(inner.width()))
}

#[allow(clippy::too_many_arguments)]
fn my_skin(ui: &mut Ui, pal: &Palette, lang: Lang, s: &mut Settings, ms: &MsView, has_skin: bool, has_cape: bool, body: Rect, action: &mut ProfileAction) {
    let ctx = ui.ctx().clone();
    let licensed = s.account_mode == AccountMode::Microsoft;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body), |ui| {
        ui.set_opacity(appear(&ctx, 0.12, 0.6));
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            section(ui, pal, t(lang, K::AccountType));
            let mut m = usize::from(licensed);
            if segmented(ui, pal, "account_mode", &[t(lang, K::Pirate), t(lang, K::Licensed)], &mut m, false) {
                *action = ProfileAction::SetMode(if m == 1 { AccountMode::Microsoft } else { AccountMode::Offline });
            }
            ui.add_space(16.0);
            if licensed {
                microsoft_block(ui, pal, lang, ms, action);
            } else {
                section(ui, pal, t(lang, K::Nickname));
                input(ui, pal, "profile_nick", &mut s.nickname, "", 300.0, 16);
                ui.add_space(6.0);
                if is_valid_nickname(&s.nickname) {
                    ui.label(RichText::new(offline_uuid(&s.nickname)).monospace().size(11.0).color(pal.text_dim));
                } else {
                    ui.label(RichText::new(t(lang, K::NicknameInvalid)).size(12.0).color(pal.danger));
                }
            }

            ui.add_space(22.0);
            section(ui, pal, t(lang, K::Skin));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if ghost_button(ui, pal, t(lang, K::PickSkin)).clicked() {
                    *action = ProfileAction::PickSkin;
                }
                if has_skin && ghost_button(ui, pal, t(lang, K::Remove)).clicked() {
                    *action = ProfileAction::ClearSkin;
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new(t(lang, K::SkinFormats)).size(11.5).color(pal.text_dim));
            if licensed {
                ui.add_space(4.0);
                let note = if ms.uploading { format!("{} …", t(lang, K::MsSkinNote)) } else { t(lang, K::MsSkinNote).to_owned() };
                ui.label(RichText::new(note).size(11.5).color(pal.accent));
            }
            ui.add_space(10.0);
            ui.label(RichText::new(t(lang, K::Model)).size(13.0).color(pal.text));
            ui.add_space(4.0);
            let mut m = usize::from(s.skin_slim);
            if segmented(ui, pal, "skin_model", &[t(lang, K::Classic), t(lang, K::Slim)], &mut m, false) {
                s.skin_slim = m == 1;
            }

            ui.add_space(22.0);
            section(ui, pal, t(lang, K::Cape));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if ghost_button(ui, pal, t(lang, K::PickCape)).clicked() {
                    *action = ProfileAction::PickCape;
                }
                if has_cape && ghost_button(ui, pal, t(lang, K::Remove)).clicked() {
                    *action = ProfileAction::ClearCape;
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new(t(lang, if licensed { K::MsCapeNote } else { K::CapeLocalNote })).size(11.5).color(pal.text_dim));
            if licensed {
                return; // a licensed skin is already visible to everyone
            }

            ui.add_space(22.0);
            section(ui, pal, t(lang, K::ForEveryone));
            match &s.skin_url {
                Some(url) if has_skin => command_box(ui, pal, lang, &format!("/skin url {url}"), action),
                _ => {
                    ui.label(RichText::new(t(lang, K::ElyHint)).size(12.0).color(pal.text_dim));
                    ui.add_space(6.0);
                    if ghost_button(ui, pal, t(lang, K::OpenEly)).clicked() {
                        *action = ProfileAction::OpenEly;
                    }
                }
            }
        });
    });
}

fn microsoft_block(ui: &mut Ui, pal: &Palette, lang: Lang, ms: &MsView, action: &mut ProfileAction) {
    section(ui, pal, "Microsoft");
    if let Some((name, uuid)) = &ms.account {
        ui.label(RichText::new(name).size(18.0).color(pal.text).strong());
        ui.label(RichText::new(uuid).monospace().size(11.0).color(pal.text_dim));
        ui.add_space(8.0);
        if ghost_button(ui, pal, t(lang, K::MsSignOut)).clicked() {
            *action = ProfileAction::MsLogout;
        }
        return;
    }
    match &ms.pending {
        None => {
            ui.label(RichText::new(t(lang, K::MsSignInDesc)).size(12.0).color(pal.text_dim));
            ui.add_space(8.0);
            if ghost_button(ui, pal, t(lang, K::MsSignIn)).clicked() {
                *action = ProfileAction::MsLogin;
            }
        }
        Some(code) => {
            if let Some((code, url)) = code {
                ui.label(RichText::new(t(lang, K::MsCode)).size(12.0).color(pal.text_dim));
                ui.label(RichText::new(code).monospace().size(30.0).color(pal.text).strong());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    if ghost_button(ui, pal, t(lang, K::MsOpenPage)).clicked() {
                        *action = ProfileAction::OpenUrl(url.clone());
                    }
                    if ghost_button(ui, pal, t(lang, K::Copy)).clicked() {
                        *action = ProfileAction::Copy(code.clone());
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new(url).monospace().size(11.0).color(pal.text_dim));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.add(egui::Spinner::new().size(14.0).color(pal.accent));
                ui.label(RichText::new(t(lang, K::MsWaiting)).size(12.0).color(pal.text_dim));
                if ghost_button(ui, pal, t(lang, K::Cancel)).clicked() {
                    *action = ProfileAction::MsCancel;
                }
            });
        }
    }
}

fn command_box(ui: &mut Ui, pal: &Palette, lang: Lang, cmd: &str, action: &mut ProfileAction) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let w = (ui.available_width() - 120.0).max(160.0);
        let g = ui.painter().layout(cmd.to_owned(), FontId::monospace(11.5), pal.text, w - 20.0);
        let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y + 18.0), Sense::hover());
        ui.painter().rect_filled(r, 10.0, Color32::from_black_alpha(90));
        ui.painter().galley(r.min + vec2(10.0, 9.0), g, pal.text);
        if ghost_button(ui, pal, t(lang, K::Copy)).clicked() {
            *action = ProfileAction::Copy(cmd.to_owned());
        }
    });
}

/// Card entrance progress since `t0`, staggered by index.
fn card_in(ctx: &egui::Context, now: f64, t0: f64, i: usize) -> f32 {
    let dur = 0.5 * motion(ctx) as f64;
    if dur <= 0.0 {
        return 1.0;
    }
    let k = (((now - t0 - 0.03 * (i % 24) as f64) / dur).clamp(0.0, 1.0)) as f32;
    if k < 1.0 {
        ctx.request_repaint();
    }
    ease_out(k)
}

#[allow(clippy::too_many_arguments)]
fn catalog(ui: &mut Ui, pal: &Palette, lang: Lang, shop: &mut Shop, icons: &mut Icons, body: Rect, now: f64, action: &mut ProfileAction) {
    let ctx = ui.ctx().clone();
    if shop.skins.is_empty() && !shop.loading && shop.error.is_none() && shop.req == 0 {
        *action = ProfileAction::SkinSearch { append: false };
    }
    let bar_h = if shop.selected.is_some() { 108.0 } else { 0.0 };
    let top = Rect::from_min_size(body.min, vec2(body.width(), 40.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(top), |ui| {
        if input(ui, pal, "skin_q", &mut shop.query, t(lang, K::SearchSkins), top.width().min(420.0), 32).changed() {
            shop.dirty_at = Some(now);
        }
    });
    if let Some(t0) = shop.dirty_at {
        if now - t0 > 0.4 {
            shop.dirty_at = None;
            *action = ProfileAction::SkinSearch { append: false };
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    let grid_r = Rect::from_min_max(pos2(body.left() - 6.0, top.bottom() + 12.0), pos2(body.right() + 6.0, body.bottom() - bar_h));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(grid_r), |ui| {
        egui::ScrollArea::vertical().id_salt("skin_grid").auto_shrink(false).show(ui, |ui| {
            let w = ui.available_width() - 12.0;
            let (cw, ch, gap) = (108.0, 150.0, 12.0);
            let cols = (((w + gap) / (cw + gap)).floor() as usize).max(1);
            if shop.skins.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(w, 140.0), Sense::hover());
                let msg = if shop.loading { t(lang, K::Loading).to_owned() } else { shop.error.clone().unwrap_or_else(|| t(lang, K::NoResults).to_owned()) };
                if shop.loading {
                    super::icons::ring(ui.painter(), r.center() - vec2(0.0, 18.0), 12.0, None, now, pal.accent, Color32::from_white_alpha(30));
                }
                ui.painter().text(r.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, msg, FontId::proportional(13.0), pal.text_dim);
                return;
            }
            let visible: Vec<&SkinItem> = shop.skins.iter().filter(|s| !icons.failed(&s.image_url())).collect();
            let rows = visible.len().div_ceil(cols);
            let (total, _) = ui.allocate_exact_size(vec2(w + 12.0, rows as f32 * (ch + gap)), Sense::hover());
            for (i, item) in visible.into_iter().enumerate() {
                let (r, c) = (i / cols, i % cols);
                let base = Rect::from_min_size(total.min + vec2(6.0 + c as f32 * (cw + gap), 6.0 + r as f32 * (ch + gap)), vec2(cw, ch));
                if !ui.is_rect_visible(base) {
                    continue;
                }
                let k = card_in(&ctx, now, shop.results_t0, i);
                let resp = ui.interact(base, Id::new(("skin_card", &item.id)), Sense::click());
                let h = spring_bool(&ctx, resp.id.with("h"), resp.hovered(), BOUNCY);
                let sel = shop.selected.as_deref() == Some(item.id.as_str());
                let sl = spring_bool(&ctx, resp.id.with("s"), sel, SNAPPY);
                if resp.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let rect = base.translate(vec2(0.0, (1.0 - k) * 10.0 - 3.0 * h));
                let mut cp = ui.painter().clone();
                cp.set_opacity(k);
                cp.rect_filled(rect, 16.0, lerp_color(pal.panel, pal.panel.gamma_multiply(1.5), h));
                cp.rect_stroke(rect, 16.0, Stroke::new(1.0_f32, Color32::from_white_alpha((14.0 + 18.0 * h) as u8)));
                if sl > 0.01 {
                    cp.rect_stroke(rect.expand(3.0), 19.0, Stroke::new(2.0_f32, pal.accent.gamma_multiply(sl.clamp(0.0, 1.0))));
                }
                let fu = 3.0;
                let origin = pos2(rect.center().x - 8.0 * fu, rect.top() + 12.0);
                let tex = icons.get(&item.image_url());
                draw_figure(&cp, origin, fu, tex.as_ref(), false, false, None, Color32::from_white_alpha(18));
                let mut job = egui::text::LayoutJob::simple(item.title.clone(), FontId::proportional(11.5), pal.text_dim, rect.width() - 14.0);
                job.wrap.max_rows = 1;
                job.wrap.overflow_character = Some('…');
                let g = ui.fonts(|f| f.layout_job(job));
                cp.galley(pos2(rect.center().x - g.size().x / 2.0, rect.bottom() - 22.0), g, pal.text_dim);
                if resp.clicked() {
                    shop.selected = Some(item.id.clone());
                }
            }
            if !shop.exhausted {
                let (r, _) = ui.allocate_exact_size(vec2(w, 50.0), Sense::hover());
                if shop.loading {
                    super::icons::ring(ui.painter(), r.center(), 10.0, None, now, pal.accent, Color32::from_white_alpha(30));
                } else if ui.is_rect_visible(r) {
                    *action = ProfileAction::SkinSearch { append: true };
                }
            }
        });
    });

    // Apply bar
    if let Some(item) = shop.selected.as_ref().and_then(|id| shop.skins.iter().find(|i| &i.id == id)).cloned() {
        let bar = Rect::from_min_max(pos2(body.left(), body.bottom() - bar_h + 12.0), body.max);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if play_button(ui, pal, vec2(170.0, 44.0), t(lang, K::Apply), ButtonState::Ready).clicked() {
                    *action = ProfileAction::ApplySkin(item.clone());
                }
                ui.label(RichText::new(&item.title).size(14.0).color(pal.text));
            });
            ui.add_space(8.0);
            command_box(ui, pal, lang, &item.command(), action);
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn capes(ui: &mut Ui, pal: &Palette, lang: Lang, shop: &mut Shop, icons: &mut Icons, body: Rect, now: f64, action: &mut ProfileAction) {
    let ctx = ui.ctx().clone();
    if !shop.capes_started {
        shop.capes_started = true;
        *action = ProfileAction::CapeSearch(POPULAR_CAPE_OWNERS.iter().map(|s| s.to_string()).collect());
    }
    let bar_h = if shop.cape_selected.is_some() { 64.0 } else { 0.0 };
    let top = Rect::from_min_size(body.min, vec2(body.width(), 40.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(top), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let r = input(ui, pal, "cape_q", &mut shop.cape_query, t(lang, K::SearchCapes), top.width().min(320.0), 16);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ghost_button(ui, pal, t(lang, K::Find)).clicked() || enter) && !shop.cape_query.trim().is_empty() {
                *action = ProfileAction::CapeSearch(vec![shop.cape_query.trim().to_owned()]);
            }
        });
    });
    let note_r = Rect::from_min_size(pos2(body.left(), top.bottom() + 8.0), vec2(body.width(), 20.0));
    ui.painter().text(note_r.left_center(), Align2::LEFT_CENTER, t(lang, K::CapeLocalNote), FontId::proportional(11.5), pal.text_dim);

    let grid_r = Rect::from_min_max(pos2(body.left() - 6.0, note_r.bottom() + 10.0), pos2(body.right() + 6.0, body.bottom() - bar_h));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(grid_r), |ui| {
        egui::ScrollArea::vertical().id_salt("cape_grid").auto_shrink(false).show(ui, |ui| {
            let w = ui.available_width() - 12.0;
            if shop.capes.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(w, 140.0), Sense::hover());
                let msg = if shop.capes_loading { t(lang, K::Loading).to_owned() } else { shop.capes_error.clone().unwrap_or_else(|| t(lang, K::NoResults).to_owned()) };
                if shop.capes_loading {
                    super::icons::ring(ui.painter(), r.center() - vec2(0.0, 18.0), 12.0, None, now, pal.accent, Color32::from_white_alpha(30));
                }
                ui.painter().text(r.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, msg, FontId::proportional(13.0), pal.text_dim);
                return;
            }
            let (cw, ch, gap) = (108.0, 150.0, 12.0);
            let cols = (((w + gap) / (cw + gap)).floor() as usize).max(1);
            let rows = shop.capes.len().div_ceil(cols);
            let (total, _) = ui.allocate_exact_size(vec2(w + 12.0, rows as f32 * (ch + gap)), Sense::hover());
            for (i, c) in shop.capes.iter().enumerate() {
                let base = Rect::from_min_size(total.min + vec2(6.0 + (i % cols) as f32 * (cw + gap), 6.0 + (i / cols) as f32 * (ch + gap)), vec2(cw, ch));
                let k = card_in(&ctx, now, shop.capes_t0, i);
                let resp = ui.interact(base, Id::new(("cape_card", i, &c.url)), Sense::click());
                let h = spring_bool(&ctx, resp.id.with("h"), resp.hovered(), BOUNCY);
                let sl = spring_bool(&ctx, resp.id.with("s"), shop.cape_selected == Some(i), SNAPPY);
                if resp.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let rect = base.translate(vec2(0.0, (1.0 - k) * 10.0 - 3.0 * h));
                let mut cp = ui.painter().clone();
                cp.set_opacity(k);
                cp.rect_filled(rect, 16.0, lerp_color(pal.panel, pal.panel.gamma_multiply(1.5), h));
                cp.rect_stroke(rect, 16.0, Stroke::new(1.0_f32, Color32::from_white_alpha((14.0 + 18.0 * h) as u8)));
                if sl > 0.01 {
                    cp.rect_stroke(rect.expand(3.0), 19.0, Stroke::new(2.0_f32, pal.accent.gamma_multiply(sl.clamp(0.0, 1.0))));
                }
                // Cape back face (10×16 texels) drawn large
                let face = Rect::from_center_size(pos2(rect.center().x, rect.top() + 58.0), vec2(50.0, 80.0));
                match icons.get(&c.url) {
                    Some(tex) => quad(&cp, &tex, face, uv(1.0, 1.0, 10.0, 16.0, tex_h(&tex), false)),
                    None => {
                        cp.rect_filled(face, 4.0, Color32::from_white_alpha(18));
                    }
                }
                let provider = match c.provider.as_str() {
                    "minecraft" => "Mojang",
                    "optifine" => "OptiFine",
                    "minecraftcapes" => "MinecraftCapes",
                    "labymod" => "LabyMod",
                    "tlauncher" => "TLauncher",
                    "skinmc" => "SkinMC",
                    other => other,
                };
                cp.text(pos2(rect.center().x, rect.bottom() - 30.0), Align2::CENTER_CENTER, provider, FontId::proportional(12.0), pal.text);
                cp.text(pos2(rect.center().x, rect.bottom() - 14.0), Align2::CENTER_CENTER, &c.owner, FontId::proportional(11.0), pal.text_dim);
                if resp.clicked() {
                    shop.cape_selected = Some(i);
                }
            }
        });
    });
    if let Some(c) = shop.cape_selected.and_then(|i| shop.capes.get(i)).cloned() {
        let bar = Rect::from_min_max(pos2(body.left(), body.bottom() - bar_h + 12.0), body.max);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if play_button(ui, pal, vec2(170.0, 44.0), t(lang, K::Apply), ButtonState::Ready).clicked() {
                    *action = ProfileAction::ApplyCape(c.clone());
                }
                ui.label(RichText::new(format!("{} · {}", c.owner, c.provider)).size(13.0).color(pal.text_dim));
            });
        });
    }
}
