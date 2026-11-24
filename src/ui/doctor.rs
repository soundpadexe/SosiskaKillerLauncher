//! Crash Doctor dialog: explains why the game crashed and offers the fix.
use egui::{pos2, vec2, Align2, Color32, FontId, Id, Order, Rect, Sense, Stroke};

use super::anim::{spring, SOFT};
use super::theme::Palette;
use super::widgets::{ghost_button, lerp_color, play_button, ButtonState};
use crate::core::crash::{Cause, Diagnosis};
use crate::core::i18n::{t, Lang, K};

pub const OPEN_ID: &str = "doctor_open";

pub struct Doctor {
    pub instance: String,
    pub name: String,
    pub diag: Diagnosis,
    pub closing: bool,
    /// The culprit was switched off — offer "play again" instead.
    pub fixed: bool,
}

pub enum DoctorAction {
    None,
    Closed,
    DisableMod,
    MoreMemory,
    OpenLog,
    Retry,
}

fn explain(lang: Lang, d: &Diagnosis) -> (&'static str, String) {
    let m = d.mod_id.clone().or_else(|| d.mod_file.clone()).unwrap_or_else(|| "?".into());
    match &d.cause {
        Cause::BrokenMod => (t(lang, K::DocBrokenMod), t(lang, K::DocBrokenModDesc).replace("{m}", &m)),
        Cause::MissingDependency { needs } => (t(lang, K::DocMissing), t(lang, K::DocMissingDesc).replace("{m}", &m).replace("{n}", needs)),
        Cause::Incompatible => (t(lang, K::DocIncompatible), t(lang, K::DocIncompatibleDesc).replace("{m}", &m)),
        Cause::OutOfMemory => (t(lang, K::DocMemory), t(lang, K::DocMemoryDesc).to_owned()),
        Cause::WrongJava => (t(lang, K::DocJava), t(lang, K::DocJavaDesc).to_owned()),
        Cause::Graphics => (t(lang, K::DocGraphics), t(lang, K::DocGraphicsDesc).to_owned()),
        Cause::Unknown => (t(lang, K::DocUnknown), t(lang, K::DocUnknownDesc).to_owned()),
    }
}

pub fn show(ctx: &egui::Context, pal: &Palette, lang: Lang, doc: &mut Doctor) -> DoctorAction {
    let p = spring(ctx, Id::new(OPEN_ID), if doc.closing { 0.0 } else { 1.0 }, SOFT).clamp(0.0, 1.0);
    if doc.closing && p < 0.02 {
        return DoctorAction::Closed;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        doc.closing = true;
    }
    let mut act = DoctorAction::None;
    let screen = ctx.screen_rect();
    egui::Area::new(Id::new("doctor_area")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.set_min_size(screen.size());
        let bg = ui.interact(screen, Id::new("doctor_bg"), Sense::click());
        ui.painter().rect_filled(screen, 12.0, Color32::from_black_alpha((150.0 * p) as u8));
        if bg.clicked() {
            doc.closing = true;
        }
        let (title, desc) = explain(lang, &doc.diag);
        let w = 560.0f32.min(screen.width() - 40.0);
        let card = Rect::from_center_size(screen.center() + vec2(0.0, (1.0 - p) * 24.0), vec2(w, 360.0));
        let pt = ui.painter().clone();
        let mut pnt = pt.clone();
        pnt.set_opacity(p);
        let _ = ui.interact(card, Id::new("doctor_card"), Sense::click());
        pnt.add(egui::epaint::Shadow { offset: vec2(0.0, 16.0), blur: 60.0, spread: 0.0, color: Color32::from_black_alpha(170) }.as_shape(card, 26.0));
        pnt.rect_filled(card, 26.0, Color32::from_rgba_unmultiplied(18, 19, 25, 252));
        pnt.rect_stroke(card, 26.0, Stroke::new(1.0_f32, pal.panel_stroke));
        // Accent band: red for an unresolved crash, green once fixed.
        let tone = if doc.fixed { Color32::from_rgb(110, 200, 140) } else { pal.danger };
        let band = Rect::from_min_size(card.min, vec2(card.width(), 92.0));
        super::widgets::gradient_rounded(&pnt, band, 26.0, |q| {
            let k = ((q.x - band.left()) / band.width()).clamp(0.0, 1.0);
            tone.gamma_multiply(0.22 * (1.0 - k * 0.8))
        });
        let ic = pos2(card.left() + 52.0, card.top() + 46.0);
        pnt.circle_filled(ic, 24.0, tone.gamma_multiply(0.25));
        pnt.text(ic, Align2::CENTER_CENTER, if doc.fixed { "✔" } else { "✚" }, FontId::proportional(22.0), tone);
        pnt.text(pos2(card.left() + 90.0, card.top() + 34.0), Align2::LEFT_CENTER, t(lang, K::CrashDoctor), FontId::proportional(12.0), tone);
        pnt.text(pos2(card.left() + 90.0, card.top() + 58.0), Align2::LEFT_CENTER, if doc.fixed { t(lang, K::DocFixed) } else { title }, FontId::proportional(20.0), pal.text);

        let inner = Rect::from_min_max(pos2(card.left() + 30.0, band.bottom() + 16.0), pos2(card.right() - 30.0, card.bottom() - 24.0));
        let g = pnt.layout(format!("{}  ·  {desc}", doc.name), FontId::proportional(14.0), pal.text_dim, inner.width());
        let gh = g.size().y;
        pnt.galley(inner.min, g, pal.text_dim);
        let ev = Rect::from_min_size(pos2(inner.left(), inner.top() + gh + 14.0), vec2(inner.width(), 96.0));
        pnt.rect_filled(ev, 12.0, Color32::from_black_alpha(110));
        let eg = pnt.layout(doc.diag.evidence.clone(), FontId::monospace(11.5), lerp_color(pal.text_dim, pal.text, 0.3), ev.width() - 24.0);
        let clip = pnt.with_clip_rect(ev.shrink(8.0));
        clip.galley(ev.min + vec2(12.0, 10.0), eg, pal.text);

        // Buttons
        let row = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 44.0), inner.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::right_to_left(egui::Align::Center)));
        child.set_opacity(p);
        child.spacing_mut().item_spacing.x = 10.0;
        let primary: Option<(String, DoctorAction)> = if doc.fixed {
            Some((t(lang, K::PlayAgain).to_owned(), DoctorAction::Retry))
        } else {
            match (&doc.diag.cause, &doc.diag.mod_file) {
                (Cause::OutOfMemory, _) => Some((t(lang, K::DocMoreMemory).to_owned(), DoctorAction::MoreMemory)),
                (Cause::BrokenMod | Cause::Incompatible | Cause::MissingDependency { .. }, Some(f)) => {
                    Some((format!("{} {}", t(lang, K::DocDisable), doc.diag.mod_id.clone().unwrap_or_else(|| f.trim_end_matches(".jar").to_owned())), DoctorAction::DisableMod))
                }
                _ => None,
            }
        };
        if let Some((label, a)) = primary {
            let width = child.fonts(|f| f.layout_no_wrap(format!("▶  {label}"), FontId::proportional(18.0), Color32::WHITE).size().x).min(300.0) + 40.0;
            if play_button(&mut child, pal, vec2(width, 42.0), &label, ButtonState::Ready).clicked() {
                act = a;
            }
        }
        if ghost_button(&mut child, pal, t(lang, K::DocLog)).clicked() {
            act = DoctorAction::OpenLog;
        }
        if ghost_button(&mut child, pal, t(lang, K::Close)).clicked() {
            doc.closing = true;
        }
    });
    act
}
