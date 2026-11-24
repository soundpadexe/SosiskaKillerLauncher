//! Hand-drawn controls. Every state change goes through `anim::tween`
//! so hover, selection and values glide instead of snapping.
use egui::{epaint::Shadow, pos2, vec2, Align2, Color32, FontId, Id, Rect, Response, Sense, Stroke, Ui, Vec2};

use super::anim::{tween, tween_bool};
use super::theme::Palette;

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn seg_widths(ui: &Ui, options: &[&str], font: &FontId) -> Vec<f32> {
    options
        .iter()
        .map(|o| ui.painter().layout_no_wrap((*o).to_owned(), font.clone(), Color32::WHITE).size().x + 30.0)
        .collect()
}

pub const SEG_H: f32 = 34.0;

/// Segmented control with a sliding pill. `accent` = filled accent pill, else raised neutral pill.
pub fn segmented(ui: &mut Ui, pal: &Palette, salt: &str, options: &[&str], selected: &mut usize, accent: bool) -> bool {
    let font = FontId::proportional(13.5);
    let widths = seg_widths(ui, options, &font);
    let size = vec2(widths.iter().sum::<f32>() + 8.0, SEG_H);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let id = ui.id().with(salt);
    segmented_in(ui, pal, id, rect, &widths, options, selected, accent, font)
}

#[allow(clippy::too_many_arguments)]
pub fn segmented_in(
    ui: &mut Ui,
    pal: &Palette,
    id: Id,
    rect: Rect,
    widths: &[f32],
    options: &[&str],
    selected: &mut usize,
    accent: bool,
    font: FontId,
) -> bool {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    p.rect_filled(rect, rect.height() / 2.0, Color32::from_black_alpha(70));
    p.rect_stroke(rect, rect.height() / 2.0, Stroke::new(1.0_f32, Color32::from_white_alpha(10)));

    let inner = rect.shrink(4.0);
    let mut x = inner.left();
    let rects: Vec<Rect> = widths
        .iter()
        .map(|w| {
            let r = Rect::from_min_size(pos2(x, inner.top()), vec2(*w, inner.height()));
            x += w;
            r
        })
        .collect();

    let mut changed = false;
    let mut hovered = vec![false; rects.len()];
    for (i, r) in rects.iter().enumerate() {
        let resp = ui.interact(*r, id.with(("seg", i)), Sense::click());
        hovered[i] = resp.hovered();
        if resp.clicked() && *selected != i {
            *selected = i;
            changed = true;
        }
        if resp.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    let target = rects[*selected];
    let px = tween(&ctx, id.with("px"), target.left(), 0.5);
    let pw = tween(&ctx, id.with("pw"), target.width(), 0.5);
    let pill = Rect::from_min_size(pos2(px, inner.top()), vec2(pw, inner.height()));
    let pr = pill.height() / 2.0;
    p.add(Shadow { offset: vec2(0.0, 2.0), blur: 10.0, spread: 0.0, color: Color32::from_black_alpha(70) }.as_shape(pill, pr));
    if accent {
        p.rect_filled(pill, pr, pal.accent);
    } else {
        p.rect_filled(pill, pr, Color32::from_white_alpha(30));
        p.rect_stroke(pill, pr, Stroke::new(1.0_f32, Color32::from_white_alpha(22)));
    }

    let active_text = if accent { pal.on_accent } else { pal.text };
    for (i, r) in rects.iter().enumerate() {
        let ts = tween_bool(&ctx, id.with(("t", i)), *selected == i, 0.3);
        let th = tween_bool(&ctx, id.with(("h", i)), hovered[i], 0.15);
        let base = lerp_color(pal.text_dim, pal.text, th * 0.6);
        p.text(r.center(), Align2::CENTER_CENTER, options[i], font.clone(), lerp_color(base, active_text, ts));
    }
    changed
}

/// Thin-track slider with animated knob. Returns true when the value changed.
pub fn slider(ui: &mut Ui, pal: &Palette, value: &mut f32, min: f32, max: f32, step: f32, label: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(260.0, 28.0), Sense::click_and_drag());
    let ctx = ui.ctx().clone();
    let track = Rect::from_min_max(pos2(rect.left() + 8.0, rect.center().y - 2.0), pos2(rect.right() - 64.0, rect.center().y + 2.0));
    let mut changed = false;
    if resp.is_pointer_button_down_on() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            let v = ((min + t * (max - min)) / step).round() * step;
            let v = v.clamp(min, max);
            if v != *value {
                *value = v;
                changed = true;
            }
        }
    }
    if resp.hovered() || resp.dragged() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let t = tween(&ctx, resp.id.with("v"), (*value - min) / (max - min), 0.16);
    let active = tween_bool(&ctx, resp.id.with("a"), resp.hovered() || resp.dragged(), 0.18);
    let p = ui.painter();
    p.rect_filled(track, 2.0, Color32::from_white_alpha(26));
    let fill = Rect::from_min_max(track.min, pos2(track.left() + track.width() * t, track.max.y));
    p.rect_filled(fill, 2.0, pal.accent);
    let knob = pos2(fill.right(), track.center().y);
    if active > 0.0 {
        p.circle_filled(knob, 8.0 + 6.0 * active, pal.accent.gamma_multiply(0.18 * active));
    }
    p.add(Shadow { offset: vec2(0.0, 1.0), blur: 6.0, spread: 0.0, color: Color32::from_black_alpha(90) }
        .as_shape(Rect::from_center_size(knob, Vec2::splat(14.0)), 7.0));
    p.circle_filled(knob, 7.0 + active, Color32::WHITE);
    p.text(pos2(rect.right(), rect.center().y), Align2::RIGHT_CENTER, label, FontId::monospace(12.5), pal.text_dim);
    changed
}

/// iOS-style switch.
pub fn toggle(ui: &mut Ui, pal: &Palette, on: &mut bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(44.0, 24.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let t = tween_bool(ui.ctx(), resp.id, *on, 0.26);
    let p = ui.painter();
    p.rect_filled(rect, 12.0, lerp_color(Color32::from_white_alpha(34), pal.accent, t));
    let c = pos2(rect.left() + 12.0 + 20.0 * t, rect.center().y);
    p.add(Shadow { offset: vec2(0.0, 1.0), blur: 4.0, spread: 0.0, color: Color32::from_black_alpha(80) }
        .as_shape(Rect::from_center_size(c, Vec2::splat(18.0)), 9.0));
    p.circle_filled(c, 9.0, Color32::WHITE);
    resp.clicked()
}

pub fn ghost_button(ui: &mut Ui, pal: &Palette, text: &str) -> Response {
    let font = FontId::proportional(13.5);
    let g = ui.painter().layout_no_wrap(text.to_owned(), font, pal.text);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 28.0, 32.0), Sense::click());
    let h = tween_bool(ui.ctx(), resp.id, resp.hovered(), 0.15);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let p = ui.painter();
    p.rect_filled(rect, 9.0, Color32::from_white_alpha((14.0 + 16.0 * h) as u8));
    p.rect_stroke(rect, 9.0, Stroke::new(1.0_f32, Color32::from_white_alpha((12.0 + 14.0 * h) as u8)));
    p.galley(rect.center() - g.size() / 2.0, g, pal.text);
    resp
}

pub enum ButtonState<'a> {
    Ready,
    Busy { label: &'a str, fraction: Option<f32> },
    /// Game is running: hover turns it red and swaps the label to `stop`.
    Running { label: &'a str, stop: &'a str },
    Disabled,
}

/// The big Play button: accent glow, hover lift, progress fill while installing.
pub fn play_button(ui: &mut Ui, pal: &Palette, size: Vec2, text: &str, state: ButtonState) -> Response {
    let ready = matches!(state, ButtonState::Ready | ButtonState::Running { .. });
    let (rect, resp) = ui.allocate_exact_size(size, if ready { Sense::click() } else { Sense::hover() });
    let ctx = ui.ctx().clone();
    let h = tween_bool(&ctx, resp.id.with("h"), ready && resp.hovered(), 0.35);
    let down = tween_bool(&ctx, resp.id.with("d"), ready && resp.is_pointer_button_down_on(), 0.2);
    if ready && resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let rect = rect.translate(vec2(0.0, -1.0 * h)).shrink(down * 0.8);
    let r = 14.0;
    let p = ui.painter();

    match state {
        ButtonState::Ready => {
            p.add(Shadow { offset: vec2(0.0, 8.0 + 4.0 * h), blur: 26.0 + 10.0 * h, spread: 0.0, color: pal.accent.gamma_multiply(0.35 + 0.2 * h) }
                .as_shape(rect, r));
            p.rect_filled(rect, r, lerp_color(pal.accent, Color32::WHITE, 0.1 * h));
            p.text(rect.center(), Align2::CENTER_CENTER, format!("▶  {text}"), FontId::proportional(18.0), pal.on_accent);
        }
        ButtonState::Busy { label, fraction } => {
            p.rect_filled(rect, r, pal.accent.gamma_multiply(0.22));
            p.rect_stroke(rect, r, Stroke::new(1.0_f32, pal.accent.gamma_multiply(0.5)));
            let inner = rect.shrink(4.0);
            match fraction {
                Some(f) => {
                    let f = tween(&ctx, resp.id.with("f"), f, 0.3);
                    let w = (inner.width() * f).max(inner.height());
                    p.rect_filled(Rect::from_min_size(inner.min, vec2(w, inner.height())), r - 4.0, pal.accent.gamma_multiply(0.85));
                }
                None => {
                    // Indeterminate shimmer
                    let tm = ctx.input(|i| i.time) as f32;
                    let x = ((tm * 0.7) % 1.4 - 0.2) * inner.width();
                    let band = Rect::from_min_size(pos2(inner.left() + x - 40.0, inner.top()), vec2(80.0, inner.height())).intersect(inner);
                    if band.width() > 0.0 {
                        p.rect_filled(band, r - 4.0, pal.accent.gamma_multiply(0.5));
                    }
                    ctx.request_repaint();
                }
            }
            let txt = match fraction {
                Some(f) => format!("{label}  {:.0}%", f * 100.0),
                None => label.to_owned(),
            };
            p.text(rect.center(), Align2::CENTER_CENTER, txt, FontId::proportional(14.5), pal.text);
        }
        ButtonState::Running { label, stop } => {
            let red = Color32::from_rgb(214, 64, 56);
            let base = pal.accent.gamma_multiply(0.28);
            p.add(Shadow { offset: vec2(0.0, 8.0), blur: 26.0, spread: 0.0, color: red.gamma_multiply(0.35 * h) }.as_shape(rect, r));
            p.rect_filled(rect, r, lerp_color(base, red, h));
            p.rect_stroke(rect, r, Stroke::new(1.0_f32, lerp_color(pal.accent.gamma_multiply(0.6), red, h)));
            // Pulsing "alive" dot fades out as the stop affordance fades in.
            let tm = ctx.input(|i| i.time) as f32;
            let pulse = (tm * 1.6).sin() * 0.5 + 0.5;
            let dot = pos2(rect.left() + 26.0, rect.center().y);
            p.circle_filled(dot, 4.5, pal.accent.gamma_multiply((0.55 + 0.45 * pulse) * (1.0 - h)));
            p.rect_filled(Rect::from_center_size(dot, Vec2::splat(11.0)), 2.0, Color32::WHITE.gamma_multiply(h));
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
            let font = FontId::proportional(16.0);
            p.text(rect.center() + vec2(8.0, 0.0), Align2::CENTER_CENTER, label, font.clone(), pal.text.gamma_multiply(1.0 - h));
            p.text(rect.center() + vec2(8.0, 0.0), Align2::CENTER_CENTER, stop, font, Color32::WHITE.gamma_multiply(h));
        }
        ButtonState::Disabled => {
            p.rect_filled(rect, r, Color32::from_white_alpha(14));
            p.text(rect.center(), Align2::CENTER_CENTER, format!("▶  {text}"), FontId::proportional(18.0), pal.text_dim);
        }
    }
    resp
}

pub fn era_badge(p: &egui::Painter, left_center: egui::Pos2, text: &str, color: Color32) -> Rect {
    let g = p.layout_no_wrap(text.to_owned(), FontId::proportional(11.5), color);
    let r = Rect::from_min_size(pos2(left_center.x, left_center.y - 11.0), vec2(g.size().x + 18.0, 22.0));
    p.rect_filled(r, 11.0, color.gamma_multiply(0.16));
    p.rect_stroke(r, 11.0, Stroke::new(1.0_f32, color.gamma_multiply(0.3)));
    p.galley(r.center() - g.size() / 2.0, g, color);
    r
}

/// Vertical gradient (used as a scrim so the title bar stays readable on any wallpaper).
pub fn vgradient(p: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    p.add(mesh);
}

/// Fill a rounded rect with a per-vertex color function (gradient that respects corners).
pub fn gradient_rounded(p: &egui::Painter, rect: Rect, r: f32, color_at: impl Fn(egui::Pos2) -> Color32) {
    let mut path = Vec::new();
    egui::epaint::tessellator::path::rounded_rectangle(&mut path, rect, egui::Rounding::same(r));
    let mut mesh = egui::Mesh::default();
    let c = rect.center();
    mesh.colored_vertex(c, color_at(c));
    for pt in &path {
        mesh.colored_vertex(*pt, color_at(*pt));
    }
    let n = path.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(mesh);
}

/// Square icon button (glass) — returns response; `danger` tints it red.
pub fn icon_button(ui: &mut Ui, pal: &Palette, size: f32, danger: f32, draw: impl FnOnce(&egui::Painter, egui::Pos2, Color32)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let ctx = ui.ctx().clone();
    let h = super::anim::spring_bool(&ctx, resp.id.with("h"), resp.hovered(), super::anim::SNAPPY);
    let pr = super::anim::spring_bool(&ctx, resp.id.with("p"), resp.is_pointer_button_down_on(), super::anim::SNAPPY);
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let r = rect.shrink(pr * 2.0);
    let p = ui.painter();
    let base = lerp_color(Color32::from_white_alpha((16.0 + 14.0 * h) as u8), pal.danger.gamma_multiply(0.85), danger);
    p.rect_filled(r, 14.0, base);
    p.rect_stroke(r, 14.0, Stroke::new(1.0_f32, Color32::from_white_alpha((16.0 + 16.0 * h) as u8)));
    draw(p, r.center(), lerp_color(pal.text, Color32::WHITE, danger));
    resp
}

/// Puts a widget inside an already-allocated frame `rect` without touching the
/// parent's layout cursor (a plain allocation would advance it to the *inner*
/// rect's edge and make the next control overlap the frame).
pub fn place(ui: &mut Ui, rect: Rect, w: impl egui::Widget) -> Response {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.add(w)
}
