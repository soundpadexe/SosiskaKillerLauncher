//! Time-based tweens with real easing (egui's built-in animate_* is linear).
use egui::{Context, Id};

#[derive(Clone, Copy)]
struct Tw {
    from: f32,
    to: f32,
    start: f64,
    dur: f32,
}

fn motion_id() -> Id {
    Id::new("skl_motion_scale")
}

/// 0.0 disables all motion (reduce-motion setting).
pub fn set_motion(ctx: &Context, scale: f32) {
    ctx.data_mut(|d| d.insert_temp(motion_id(), scale));
}

pub fn motion(ctx: &Context) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(motion_id())).unwrap_or(1.0)
}

pub fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(4)
}

/// Smoothly approaches `target`; retargeting mid-flight continues from the current value.
pub fn tween(ctx: &Context, id: Id, target: f32, dur: f32) -> f32 {
    let dur = dur * motion(ctx);
    let now = ctx.input(|i| i.time);
    let Some(tw) = ctx.data(|d| d.get_temp::<Tw>(id)) else {
        ctx.data_mut(|d| d.insert_temp(id, Tw { from: target, to: target, start: now, dur }));
        return target;
    };
    let cur = eval(&tw, now);
    if tw.to != target {
        ctx.data_mut(|d| d.insert_temp(id, Tw { from: cur, to: target, start: now, dur }));
        ctx.request_repaint();
        return cur;
    }
    if now - tw.start < tw.dur as f64 {
        ctx.request_repaint();
    }
    cur
}

pub fn tween_bool(ctx: &Context, id: Id, on: bool, dur: f32) -> f32 {
    tween(ctx, id, if on { 1.0 } else { 0.0 }, dur)
}

fn eval(t: &Tw, now: f64) -> f32 {
    if t.dur <= 0.0 {
        return t.to;
    }
    let x = ((now - t.start) / t.dur as f64).clamp(0.0, 1.0) as f32;
    t.from + (t.to - t.from) * ease_out(x)
}

// ---------------------------------------------------------------- springs

#[derive(Clone, Copy)]
pub struct SpringCfg {
    /// Stiffness (ω² in 1/s²).
    pub k: f32,
    /// Damping ratio: <1 overshoots, 1 = critical.
    pub z: f32,
}

// All presets are critically damped or heavier: motion eases in and settles
// without overshoot or bounce, so nothing snaps or wobbles on screen.
/// Hover/press feedback (~0.35 s).
pub const SNAPPY: SpringCfg = SpringCfg { k: 150.0, z: 1.0 };
/// Movement of indicators, cards, handles (~0.5 s).
pub const BOUNCY: SpringCfg = SpringCfg { k: 90.0, z: 1.05 };
/// Large surfaces: sheets, modals, toasts (~0.65 s).
pub const SOFT: SpringCfg = SpringCfg { k: 60.0, z: 1.1 };

#[derive(Clone, Copy)]
struct Sp {
    x: f32,
    v: f32,
    t: f64,
    target: f32,
}

/// Damped spring integrated per frame (semi-implicit Euler, 240 Hz substeps).
/// Keeps velocity across retargets, so interrupted motion stays continuous.
pub fn spring(ctx: &Context, id: Id, target: f32, cfg: SpringCfg) -> f32 {
    let now = ctx.input(|i| i.time);
    if motion(ctx) == 0.0 {
        ctx.data_mut(|d| d.insert_temp(id, Sp { x: target, v: 0.0, t: now, target }));
        return target;
    }
    let mut s = ctx.data(|d| d.get_temp::<Sp>(id)).unwrap_or(Sp { x: target, v: 0.0, t: now, target });
    // Waking from rest: don't integrate the idle gap, start with one nominal frame.
    let at_rest = s.v == 0.0 && s.x == s.target;
    let dt = if at_rest { 1.0 / 120.0 } else { ((now - s.t) as f32).clamp(0.0, 1.0 / 15.0) } / motion(ctx);
    s.target = target;
    s.t = now;
    let steps = (dt * 240.0).ceil().max(1.0) as usize;
    let h = dt / steps as f32;
    let w = cfg.k.sqrt();
    for _ in 0..steps {
        let a = -cfg.k * (s.x - target) - 2.0 * cfg.z * w * s.v;
        s.v += a * h;
        s.x += s.v * h;
    }
    let scale = target.abs().max(1.0);
    if (s.x - target).abs() < 1e-3 * scale && s.v.abs() < 1e-2 * scale {
        s.x = target;
        s.v = 0.0;
    } else {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(id, s));
    s.x
}

pub fn spring_bool(ctx: &Context, id: Id, on: bool, cfg: SpringCfg) -> f32 {
    spring(ctx, id, if on { 1.0 } else { 0.0 }, cfg)
}

/// Instantly set a spring (e.g. to start an entrance from 0).
pub fn spring_reset(ctx: &Context, id: Id, value: f32) {
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|d| d.insert_temp(id, Sp { x: value, v: 0.0, t: now, target: f32::NAN }));
}

// ---------------------------------------------------------------- staggered entrance

fn view_id() -> Id {
    Id::new("skl_view_t0")
}

/// Marks "the view just appeared" — every `appear()` call replays from here.
pub fn restart_entrance(ctx: &Context) {
    let now = ctx.input(|i| i.time);
    ctx.data_mut(|d| d.insert_temp(view_id(), now));
}

/// 0→1 eased progress for an element that should enter `delay` seconds after the view.
pub fn appear(ctx: &Context, delay: f32, dur: f32) -> f32 {
    let m = motion(ctx);
    if m == 0.0 {
        return 1.0;
    }
    let t0 = ctx.data(|d| d.get_temp::<f64>(view_id())).unwrap_or(0.0);
    let now = ctx.input(|i| i.time);
    let x = (((now - t0) as f32 - delay * m) / (dur * m)).clamp(0.0, 1.0);
    if x < 1.0 {
        ctx.request_repaint();
    }
    ease_out(x)
}
