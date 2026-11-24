//! Vector icons drawn with the painter — crisp at any UI scale, zero font deps.
use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

fn st(c: Color32) -> Stroke {
    Stroke::new(1.7_f32, c)
}

pub fn grid(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let w = s * 0.32;
    let g = s * 0.09;
    for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let cc = c + vec2(dx * (w + g) / 2.0, dy * (w + g) / 2.0);
        p.rect_stroke(Rect::from_center_size(cc, Vec2::splat(w)), 2.5, st(col));
    }
}

pub fn plus(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let h = s * 0.3;
    p.line_segment([c - vec2(h, 0.0), c + vec2(h, 0.0)], st(col));
    p.line_segment([c - vec2(0.0, h), c + vec2(0.0, h)], st(col));
}

pub fn cube(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let r = s * 0.4;
    let pt = |deg: f32| c + vec2(deg.to_radians().cos(), deg.to_radians().sin()) * r;
    let hex: Vec<Pos2> = [30.0, 90.0, 150.0, 210.0, 270.0, 330.0].iter().map(|a| pt(*a)).collect();
    p.add(Shape::closed_line(hex, st(col)));
    for a in [90.0, 210.0, 330.0] {
        p.line_segment([c, pt(a)], st(col));
    }
}

pub fn gear(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let r = s * 0.27;
    for k in 0..8 {
        let a = (k as f32 * 45.0).to_radians();
        let d = vec2(a.cos(), a.sin());
        p.line_segment([c + d * (r - 1.0), c + d * (r + s * 0.12)], Stroke::new(3.2_f32, col));
    }
    p.circle_stroke(c, r, st(col));
    p.circle_stroke(c, r * 0.42, st(col));
}

pub fn folder(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let (w, h) = (s * 0.72, s * 0.54);
    let (l, t, r, b) = (c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0);
    let tab = s * 0.12;
    p.add(Shape::closed_line(
        vec![pos2(l, t), pos2(l + w * 0.38, t), pos2(l + w * 0.48, t + tab), pos2(r, t + tab), pos2(r, b), pos2(l, b)],
        st(col),
    ));
}

pub fn play(p: &Painter, c: Pos2, s: f32, col: Color32) {
    p.add(Shape::convex_polygon(
        vec![c + vec2(-0.24 * s, -0.3 * s), c + vec2(0.32 * s, 0.0), c + vec2(-0.24 * s, 0.3 * s)],
        col,
        Stroke::NONE,
    ));
}

pub fn trash(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let w = s * 0.5;
    let top = c.y - s * 0.22;
    p.line_segment([pos2(c.x - w * 0.62, top), pos2(c.x + w * 0.62, top)], st(col));
    p.line_segment([pos2(c.x - w * 0.18, top - s * 0.09), pos2(c.x + w * 0.18, top - s * 0.09)], st(col));
    p.add(Shape::closed_line(
        vec![pos2(c.x - w / 2.0, top), pos2(c.x + w / 2.0, top), pos2(c.x + w * 0.4, c.y + s * 0.3), pos2(c.x - w * 0.4, c.y + s * 0.3)],
        st(col),
    ));
}

pub fn search(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let cc = c - vec2(s * 0.07, s * 0.07);
    p.circle_stroke(cc, s * 0.24, st(col));
    p.line_segment([cc + vec2(s * 0.17, s * 0.17), c + vec2(s * 0.32, s * 0.32)], st(col));
}

pub fn clock(p: &Painter, c: Pos2, s: f32, col: Color32) {
    p.circle_stroke(c, s * 0.34, st(col));
    p.line_segment([c, c - vec2(0.0, s * 0.2)], st(col));
    p.line_segment([c, c + vec2(s * 0.15, s * 0.08)], st(col));
}

pub fn close(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let h = s * 0.25;
    p.line_segment([c - vec2(h, h), c + vec2(h, h)], st(col));
    p.line_segment([c + vec2(-h, h), c + vec2(h, -h)], st(col));
}

pub fn minimize(p: &Painter, c: Pos2, s: f32, col: Color32) {
    let h = s * 0.25;
    p.line_segment([c - vec2(h, 0.0), c + vec2(h, 0.0)], st(col));
}

pub fn maximize(p: &Painter, c: Pos2, s: f32, col: Color32, restored: bool) {
    let w = s * 0.44;
    if restored {
        p.rect_stroke(Rect::from_center_size(c + vec2(-1.5, 1.5), Vec2::splat(w * 0.8)), 1.5, st(col));
        p.add(Shape::line(vec![c + vec2(-w * 0.25, -w * 0.5), c + vec2(w * 0.5, -w * 0.5), c + vec2(w * 0.5, w * 0.25)], st(col)));
    } else {
        p.rect_stroke(Rect::from_center_size(c, Vec2::splat(w)), 1.5, st(col));
    }
}

pub fn check(p: &Painter, c: Pos2, s: f32, col: Color32) {
    p.add(Shape::line(vec![c + vec2(-0.26 * s, 0.0), c + vec2(-0.06 * s, 0.2 * s), c + vec2(0.28 * s, -0.2 * s)], Stroke::new(2.0_f32, col)));
}

pub fn alert(p: &Painter, c: Pos2, s: f32, col: Color32) {
    p.circle_stroke(c, s * 0.34, st(col));
    p.line_segment([c - vec2(0.0, s * 0.16), c + vec2(0.0, s * 0.04)], Stroke::new(2.0_f32, col));
    p.circle_filled(c + vec2(0.0, s * 0.15), 1.4, col);
}

/// Progress ring; `frac = None` spins indeterminately.
pub fn ring(p: &Painter, c: Pos2, r: f32, frac: Option<f32>, time: f64, col: Color32, track: Color32) {
    p.circle_stroke(c, r, Stroke::new(2.5_f32, track));
    let (a0, span) = match frac {
        Some(f) => (-std::f32::consts::FRAC_PI_2, f.clamp(0.0, 1.0) * std::f32::consts::TAU),
        None => ((time as f32 * 5.0) % std::f32::consts::TAU, 1.6),
    };
    let n = 48;
    let pts: Vec<Pos2> = (0..=n).map(|i| {
        let a = a0 + span * i as f32 / n as f32;
        c + vec2(a.cos(), a.sin()) * r
    }).collect();
    if span > 0.01 {
        p.add(Shape::line(pts, Stroke::new(2.5_f32, col)));
    }
}
