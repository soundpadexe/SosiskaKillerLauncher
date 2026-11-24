//! "New instance" modal. The version is picked on a Time Machine timeline:
//! drag across 2009 → today, era bands under the track, a springy handle on top.
use egui::{pos2, vec2, Align2, Color32, FontId, Id, Order, Rect, RichText, Sense, Stroke, Ui};

use super::anim::{spring, spring_bool, BOUNCY, SNAPPY, SOFT};
use super::icons;
use super::theme::Palette;
use super::widgets::{era_badge, ghost_button, icon_button, lerp_color, play_button, segmented, ButtonState};
use crate::core::i18n::{era_blurb, t, Lang, K};
use crate::core::instances::Loader;
use crate::models::version::{Catalog, Era, GameVersion};

pub struct NewInstance {
    pub closing: bool,
    name: String,
    name_edited: bool,
    filter: usize,
    version: Option<String>,
    loader: usize,
}

pub enum ModalAction {
    None,
    Closed,
    Create { name: String, version: String, loader: Loader },
}

pub const OPEN_ID: &str = "newinst_open";

impl NewInstance {
    pub fn new(catalog: &Catalog) -> Self {
        let version = match catalog {
            Catalog::Ready(l) => Some(l.latest_release.clone()),
            _ => None,
        };
        Self { closing: false, name: version.clone().unwrap_or_default(), name_edited: false, filter: 0, version, loader: 0 }
    }
}

fn year_frac(date: &str) -> f32 {
    let p = |a: usize, b: usize| date.get(a..b).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    p(0, 4) + (p(5, 7) - 1.0) / 12.0 + (p(8, 10) - 1.0) / 365.0
}

pub fn show(ctx: &egui::Context, pal: &Palette, lang: Lang, st: &mut NewInstance, catalog: &Catalog) -> ModalAction {
    let mut action = ModalAction::None;
    let p = spring(ctx, Id::new(OPEN_ID), if st.closing { 0.0 } else { 1.0 }, SOFT);
    if st.closing && p < 0.03 {
        return ModalAction::Closed;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        st.closing = true;
    }
    let screen = ctx.screen_rect();

    egui::Area::new(Id::new("newinst_area")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.set_min_size(screen.size());
        let bg = ui.interact(screen, Id::new("newinst_bg"), Sense::click());
        ui.painter().rect_filled(screen, 12.0, Color32::from_black_alpha((160.0 * p.clamp(0.0, 1.0)) as u8));
        if bg.clicked() {
            st.closing = true;
        }

        let size = vec2((screen.width() - 80.0).min(860.0), (screen.height() - 60.0).min(620.0));
        let card = Rect::from_center_size(screen.center() + vec2(0.0, (1.0 - p) * 16.0), size * (0.985 + 0.015 * p));
        let pnt = ui.painter();
        pnt.add(egui::epaint::Shadow { offset: vec2(0.0, 24.0), blur: 60.0, spread: 0.0, color: Color32::from_black_alpha((140.0 * p.clamp(0.0, 1.0)) as u8) }.as_shape(card, 26.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(card), |ui| {
            ui.set_opacity(p.clamp(0.0, 1.0));
            let pp = ui.painter().clone();
            pp.rect_filled(card, 26.0, Color32::from_rgba_unmultiplied(18, 19, 24, 248));
            pp.rect_stroke(card, 26.0, Stroke::new(1.0_f32, pal.panel_stroke));
            // block clicks from reaching the backdrop
            let _ = ui.interact(card, Id::new("newinst_card"), Sense::click());
            body(ui, pal, lang, st, catalog, card.shrink2(vec2(34.0, 28.0)), &mut action);
        });
    });
    action
}

fn body(ui: &mut Ui, pal: &Palette, lang: Lang, st: &mut NewInstance, catalog: &Catalog, r: Rect, action: &mut ModalAction) {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();

    // Header
    p.text(r.left_top() + vec2(0.0, 16.0), Align2::LEFT_CENTER, t(lang, K::NewInstance), FontId::proportional(26.0), pal.text);
    let close = Rect::from_min_size(pos2(r.right() - 40.0, r.top()), vec2(40.0, 40.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(close), |ui| {
        if icon_button(ui, pal, 40.0, 0.0, |p, c, col| icons::close(p, c, 18.0, col)).clicked() {
            st.closing = true;
        }
    });

    let list: Vec<&GameVersion> = match catalog {
        Catalog::Ready(l) => {
            let mut v: Vec<&GameVersion> = l
                .versions
                .iter()
                .filter(|v| match st.filter {
                    0 => v.era == Era::Release,
                    1 => v.era.is_legacy(),
                    _ => v.era == Era::Snapshot,
                })
                .collect();
            v.reverse();
            v
        }
        _ => {
            p.text(r.center(), Align2::CENTER_CENTER, t(lang, K::Loading), FontId::proportional(16.0), pal.text_dim);
            return;
        }
    };
    let selected = st.version.as_deref().and_then(|id| catalog.find(id));

    // Name + filter row
    let row = Rect::from_min_size(r.left_top() + vec2(0.0, 58.0), vec2(r.width(), 40.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(row), |ui| {
        ui.horizontal(|ui| {
            let fr = ui.allocate_exact_size(vec2(300.0, 38.0), Sense::hover()).0;
            let foc = ui.memory(|m| m.has_focus(Id::new("ni_name")));
            let fo = spring_bool(&ctx, Id::new("ni_name_f"), foc, SNAPPY);
            ui.painter().rect_filled(fr, 12.0, Color32::from_black_alpha((80.0 + 40.0 * fo) as u8));
            ui.painter().rect_stroke(fr, 12.0, Stroke::new(1.0_f32, lerp_color(Color32::from_white_alpha(20), pal.accent, fo)));
            ui.painter().text(fr.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, t(lang, K::Name), FontId::proportional(11.0), pal.text_dim);
            let inner = Rect::from_min_max(fr.min + vec2(64.0, 10.0), fr.max - vec2(10.0, 6.0));
            if super::widgets::place(ui, inner, egui::TextEdit::singleline(&mut st.name).id(Id::new("ni_name")).frame(false).char_limit(40).desired_width(inner.width())).changed() {
                st.name_edited = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let labels = [t(lang, K::Releases), t(lang, K::Legacy), t(lang, K::Snapshots)];
                if segmented(ui, pal, "ni_filter", &labels, &mut st.filter, true) {
                    st.version = None;
                }
            });
        });
    });

    // Readout
    let ry = row.bottom() + 26.0;
    if let Some(v) = selected {
        let title = p.layout_no_wrap(v.id.clone(), FontId::proportional(40.0), pal.text);
        let tw = title.size().x;
        p.galley(pos2(r.left(), ry), title, pal.text);
        let b = era_badge(&p, pos2(r.left() + tw + 16.0, ry + 26.0), v.era.label(), v.era.color());
        p.text(pos2(b.right() + 10.0, ry + 26.0), Align2::LEFT_CENTER, format!("{} {}  ·  Java {}", t(lang, K::Released), v.date, v.java_hint()), FontId::proportional(12.5), pal.text_dim);
        let blurb = p.layout(era_blurb(lang, v.era).to_owned(), FontId::proportional(13.0), pal.text_dim, r.width());
        p.galley(pos2(r.left(), ry + 56.0), blurb, pal.text);
    } else {
        p.text(pos2(r.left(), ry + 20.0), Align2::LEFT_CENTER, t(lang, K::DragTimeline), FontId::proportional(18.0), pal.text_dim);
    }

    // ---------------- timeline
    let bar_y = ry + 140.0;
    let bar = Rect::from_min_max(pos2(r.left() + 8.0, bar_y - 2.0), pos2(r.right() - 8.0, bar_y + 2.0));
    if !list.is_empty() {
        let t0 = year_frac(&list[0].date).floor();
        let t1 = (year_frac(&list[list.len() - 1].date) + 1.0).ceil().max(t0 + 1.0);
        let x_of = |d: &str| bar.left() + (year_frac(d) - t0) / (t1 - t0) * bar.width();

        let hit = bar.expand2(vec2(8.0, 30.0));
        let resp = ui.interact(hit, Id::new("timeline"), Sense::click_and_drag());
        let nearest = |x: f32| list.iter().min_by(|a, b| (x_of(&a.date) - x).abs().total_cmp(&(x_of(&b.date) - x).abs())).copied();
        if resp.hovered() || resp.dragged() {
            ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if resp.is_pointer_button_down_on() {
            if let Some(v) = resp.interact_pointer_pos().and_then(|pp| nearest(pp.x)) {
                st.version = Some(v.id.clone());
            }
        }

        p.rect_filled(bar, 2.0, Color32::from_white_alpha(22));
        // Era bands
        let mut i = 0;
        while i < list.len() {
            let era = list[i].era;
            let mut j = i;
            while j + 1 < list.len() && list[j + 1].era == era {
                j += 1;
            }
            let (x0, x1) = (x_of(&list[i].date), x_of(&list[j].date).max(x_of(&list[i].date) + 3.0));
            p.rect_filled(Rect::from_min_max(pos2(x0, bar_y - 3.0), pos2(x1, bar_y + 3.0)), 3.0, era.color().gamma_multiply(0.75));
            i = j + 1;
        }
        // Year ticks
        let years = (t1 - t0) as usize;
        let every = ((years as f32 * 46.0 / bar.width()).ceil() as usize).max(1);
        for k in 0..=years {
            let x = bar.left() + k as f32 / (t1 - t0) * bar.width();
            p.line_segment([pos2(x, bar_y + 8.0), pos2(x, bar_y + 13.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(40)));
            if k % every == 0 {
                p.text(pos2(x, bar_y + 24.0), Align2::CENTER_CENTER, format!("{}", t0 as i32 + k as i32), FontId::proportional(10.5), pal.text_dim);
            }
        }
        // Version dots
        for v in &list {
            p.circle_filled(pos2(x_of(&v.date), bar_y - 10.0), 1.8, v.era.color().gamma_multiply(0.7));
        }
        // Hover ghost
        if let Some(hv) = resp.hover_pos().and_then(|pp| nearest(pp.x)) {
            if Some(hv.id.as_str()) != st.version.as_deref() {
                let x = x_of(&hv.date);
                p.line_segment([pos2(x, bar_y - 22.0), pos2(x, bar_y + 6.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(70)));
                p.text(pos2(x, bar_y - 30.0), Align2::CENTER_CENTER, &hv.id, FontId::proportional(11.0), pal.text_dim);
            }
        }
        // Springy handle
        if let Some(v) = selected.filter(|v| list.iter().any(|l| l.id == v.id)) {
            let target = x_of(&v.date);
            let x = spring(&ctx, Id::new("tl_handle"), target, BOUNCY);
            let grab = spring_bool(&ctx, Id::new("tl_grab"), resp.dragged() || resp.is_pointer_button_down_on(), SNAPPY);
            p.line_segment([pos2(x, bar_y - 34.0), pos2(x, bar_y + 6.0)], Stroke::new(2.0_f32, pal.accent));
            p.circle_filled(pos2(x, bar_y), 8.0 + 3.0 * grab, Color32::WHITE);
            p.circle_stroke(pos2(x, bar_y), 8.0 + 3.0 * grab, Stroke::new(2.5_f32, pal.accent));
            let g = p.layout_no_wrap(v.id.clone(), FontId::proportional(12.0), pal.on_accent);
            let bub = Rect::from_center_size(pos2(x, bar_y - 46.0 - 4.0 * grab), g.size() + vec2(18.0, 10.0));
            let bub = bub.translate(vec2((r.left() - bub.left()).max(0.0) + (r.right() - bub.right()).min(0.0), 0.0));
            p.rect_filled(bub, 8.0, pal.accent);
            p.galley(bub.center() - g.size() / 2.0, g, pal.on_accent);
        }
    }

    // ---------------- chips: same year as selection
    let chips_rect = Rect::from_min_max(pos2(r.left(), bar_y + 46.0), pos2(r.right(), bar_y + 46.0 + 84.0));
    let year = selected.map(|v| v.year);
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(chips_rect), |ui| {
        egui::ScrollArea::vertical().max_height(chips_rect.height()).auto_shrink(false).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for v in list.iter().rev().filter(|v| Some(v.year) == year) {
                    if chip(ui, pal, &v.id, st.version.as_deref() == Some(v.id.as_str())).clicked() {
                        st.version = Some(v.id.clone());
                    }
                }
            });
        });
    });

    // ---------------- loader + footer
    let foot = Rect::from_min_max(pos2(r.left(), r.bottom() - 52.0), r.max);
    let selected = st.version.as_deref().and_then(|id| catalog.find(id));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(foot), |ui| {
        ui.horizontal_centered(|ui| {
            ui.label(RichText::new(t(lang, K::Loader).to_uppercase()).size(11.0).color(pal.text_dim));
            ui.add_space(6.0);
            let names: Vec<&str> = Loader::ALL.iter().map(|l| l.name()).collect();
            segmented(ui, pal, "ni_loader", &names, &mut st.loader, false);
            let wanted = Loader::ALL[st.loader];
            if let Some(v) = selected {
                if !wanted.supports(&v.date, v.era.is_legacy()) {
                    ui.add_space(8.0);
                    ui.label(RichText::new(t(lang, K::LoaderUnsupported)).size(12.0).color(pal.danger));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok_loader = selected.is_some_and(|v| wanted.supports(&v.date, v.era.is_legacy()));
                let ready = ok_loader && !st.name.trim().is_empty();
                let state = if ready { ButtonState::Ready } else { ButtonState::Disabled };
                if play_button(ui, pal, vec2(180.0, 50.0), t(lang, K::Create), state).clicked() {
                    if let Some(v) = selected {
                        *action = ModalAction::Create { name: st.name.trim().to_owned(), version: v.id.clone(), loader: wanted };
                        st.closing = true;
                    }
                }
                ui.add_space(4.0);
                if ghost_button(ui, pal, t(lang, K::Cancel)).clicked() {
                    st.closing = true;
                }
            });
        });
    });

    if !st.name_edited {
        if let Some(v) = &st.version {
            let l = Loader::ALL[st.loader];
            st.name = if l == Loader::Vanilla { v.clone() } else { format!("{v} {}", l.name()) };
        }
    }
}

fn chip(ui: &mut Ui, pal: &Palette, text: &str, selected: bool) -> egui::Response {
    let font = FontId::proportional(12.5);
    let g = ui.painter().layout_no_wrap(text.to_owned(), font, pal.text);
    let (rect, resp) = ui.allocate_exact_size(g.size() + vec2(22.0, 12.0), Sense::click());
    let ctx = ui.ctx().clone();
    let h = spring_bool(&ctx, resp.id.with("h"), resp.hovered(), SNAPPY);
    let s = spring_bool(&ctx, resp.id.with("s"), selected, SNAPPY);
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let pnt = ui.painter();
    let fill = lerp_color(Color32::from_white_alpha((12.0 + 12.0 * h) as u8), pal.accent, s);
    pnt.rect_filled(rect, rect.height() / 2.0, fill);
    let col = lerp_color(pal.text, pal.on_accent, s);
    pnt.galley(rect.center() - g.size() / 2.0, g, col);
    resp
}
