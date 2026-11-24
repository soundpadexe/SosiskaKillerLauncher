//! Bottom-right notifications: spring in from the edge, restack smoothly, auto-dismiss.
use egui::{pos2, vec2, Align2, Color32, FontId, Id, LayerId, Order, Rect, Stroke};

use super::anim::{spring, spring_reset, BOUNCY, SOFT};
use super::icons;
use super::theme::Palette;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Success,
    Error,
}

struct Toast {
    id: u64,
    kind: Kind,
    title: String,
    body: String,
    born: f64,
    ttl: f64,
    dismissed: bool,
    /// Clicking the toast opens this file (e.g. the launch log).
    open: Option<std::path::PathBuf>,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
    next: u64,
}

pub struct Ongoing<'a> {
    pub title: &'a str,
    pub stage: &'a str,
    pub fraction: Option<f32>,
}

impl Toasts {
    pub fn push_with_path(&mut self, ctx: &egui::Context, kind: Kind, title: impl Into<String>, body: impl Into<String>, path: std::path::PathBuf) {
        self.push(ctx, kind, title, body);
        if let Some(t) = self.items.last_mut() {
            t.open = Some(path);
            t.ttl = 14.0;
        }
    }

    pub fn push(&mut self, ctx: &egui::Context, kind: Kind, title: impl Into<String>, body: impl Into<String>) {
        let id = self.next;
        self.next += 1;
        spring_reset(ctx, Id::new(("toast_in", id)), 0.0);
        let ttl = if kind == Kind::Error { 9.0 } else { 4.0 };
        self.items.push(Toast { id, kind, title: title.into(), body: body.into(), born: ctx.input(|i| i.time), ttl, dismissed: false, open: None });
    }

    pub fn show(&mut self, ctx: &egui::Context, pal: &Palette, ongoing: Option<Ongoing>) {
        let now = ctx.input(|i| i.time);
        let screen = ctx.screen_rect();
        let lp = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("toasts")));
        let w = 340.0;
        let mut y = screen.bottom() - 22.0;

        // Persistent install/launch card
        let og = spring(ctx, Id::new("toast_ongoing"), if ongoing.is_some() { 1.0 } else { 0.0 }, BOUNCY);
        if og > 0.01 {
            let lp = {
                let mut l = lp.clone();
                l.set_opacity(og.clamp(0.0, 1.0));
                l
            };
            let h = 74.0;
            let r = Rect::from_min_size(pos2(screen.right() - 22.0 - w + (1.0 - og) * 60.0, y - h), vec2(w, h));
            card(&lp, pal, r);
            if let Some(o) = &ongoing {
                icons::ring(&lp, pos2(r.left() + 32.0, r.center().y), 13.0, o.fraction, now, pal.accent, Color32::from_white_alpha(30));
                lp.text(pos2(r.left() + 60.0, r.top() + 24.0), Align2::LEFT_CENTER, o.title, FontId::proportional(14.0), pal.text);
                let pct = o.fraction.map(|f| format!("  {:.0}%", f * 100.0)).unwrap_or_default();
                lp.text(pos2(r.left() + 60.0, r.top() + 44.0), Align2::LEFT_CENTER, format!("{}{pct}", o.stage), FontId::proportional(12.0), pal.text_dim);
                let bar = Rect::from_min_max(pos2(r.left() + 60.0, r.bottom() - 14.0), pos2(r.right() - 18.0, r.bottom() - 11.0));
                lp.rect_filled(bar, 1.5, Color32::from_white_alpha(25));
                let f = spring(ctx, Id::new("toast_ongoing_f"), o.fraction.unwrap_or(0.0), SOFT).clamp(0.0, 1.0);
                lp.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * f, bar.height())), 1.5, pal.accent);
                if o.fraction.is_none() {
                    ctx.request_repaint();
                }
            }
            y -= (h + 10.0) * og.clamp(0.0, 1.0);
        }

        for t in self.items.iter_mut().rev() {
            if now - t.born > t.ttl {
                t.dismissed = true;
            }
            let p = spring(ctx, Id::new(("toast_in", t.id)), if t.dismissed { 0.0 } else { 1.0 }, BOUNCY);
            let body = lp.layout(t.body.clone(), FontId::proportional(12.0), pal.text_dim, w - 80.0);
            let h = 46.0 + body.size().y;
            let ty = spring(ctx, Id::new(("toast_y", t.id)), y - h, SOFT);
            let r = Rect::from_min_size(pos2(screen.right() - 22.0 - w + (1.0 - p) * 60.0, ty), vec2(w, h));
            let hovered = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p));
            if hovered {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                // Pause the countdown while the user reads it.
                t.born = t.born.max(now - t.ttl + 1.5);
            }
            if hovered && ctx.input(|i| i.pointer.primary_clicked()) {
                if let Some(path) = &t.open {
                    crate::core::paths::open(path);
                }
                t.dismissed = true;
            }
            if p > 0.01 {
                let lp = {
                    let mut l = lp.clone();
                    l.set_opacity(p.clamp(0.0, 1.0));
                    l
                };
                card(&lp, pal, r);
                let col = if t.kind == Kind::Error { pal.danger } else { pal.accent };
                let ic = pos2(r.left() + 30.0, r.top() + 24.0);
                lp.circle_filled(ic, 14.0, col.gamma_multiply(0.18));
                match t.kind {
                    Kind::Success => icons::check(&lp, ic, 18.0, col),
                    Kind::Error => icons::alert(&lp, ic, 18.0, col),
                }
                lp.text(pos2(r.left() + 56.0, r.top() + 24.0), Align2::LEFT_CENTER, &t.title, FontId::proportional(14.0), pal.text);
                lp.galley(pos2(r.left() + 56.0, r.top() + 38.0), body, pal.text_dim);
                // remaining-time hairline
                let left = (1.0 - ((now - t.born) / t.ttl) as f32).clamp(0.0, 1.0);
                lp.rect_filled(Rect::from_min_size(pos2(r.left() + 14.0, r.bottom() - 3.0), vec2((w - 28.0) * left, 2.0)), 1.0, col.gamma_multiply(0.5));
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            y -= (h + 10.0) * p.clamp(0.0, 1.0);
        }
        self.items.retain(|t| !(t.dismissed && spring(ctx, Id::new(("toast_in", t.id)), 0.0, BOUNCY) < 0.02));
    }
}

fn card(p: &egui::Painter, pal: &Palette, r: Rect) {
    p.add(egui::epaint::Shadow { offset: vec2(0.0, 10.0), blur: 30.0, spread: 0.0, color: Color32::from_black_alpha(110) }.as_shape(r, 16.0));
    p.rect_filled(r, 16.0, Color32::from_rgba_unmultiplied(20, 21, 27, 245));
    p.rect_stroke(r, 16.0, Stroke::new(1.0_f32, pal.panel_stroke));
}
