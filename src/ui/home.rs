//! Library: hero banner of the selected instance + animated card grid.
use egui::{pos2, vec2, Align2, Color32, FontId, Id, Rect, Sense, Stroke, Ui};

use super::anim::{appear, ease_out, motion, spring, spring_bool, BOUNCY, SNAPPY, SOFT};
use super::cover::Covers;
use super::icons;
use super::theme::Palette;
use super::wallpaper::paint_cover_ex;
use super::widgets::{era_badge, gradient_rounded, icon_button, lerp_color, play_button, ButtonState};
use crate::core::i18n::{format_playtime, t, Lang, K};
use crate::core::instances::{format_date, Instance};
use crate::core::launch::{Progress, Stage};
use crate::models::version::{Catalog, Era};

#[derive(Default)]
pub struct HomeState {
    shown: Option<String>,
    previous: Option<String>,
    switch_t: f64,
    armed: Option<(String, f64)>,
    pub query: String,
}

pub enum HomeAction {
    None,
    Select(String),
    Launch(String),
    OpenFolder(String),
    Delete(String),
    New,
    Import,
    Retry,
    Stop(String),
}

pub struct LaunchView<'a> {
    pub instance: &'a str,
    pub progress: &'a Progress,
}

pub fn era_of(catalog: &Catalog, version: &str) -> Era {
    catalog.find(version).map(|v| v.era).unwrap_or_else(|| Era::guess(version))
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    pal: &Palette,
    lang: Lang,
    st: &mut HomeState,
    instances: &[Instance],
    selected: Option<&str>,
    covers: &mut Covers,
    catalog: &Catalog,
    launch: Option<LaunchView>,
    nickname: &str,
) -> HomeAction {
    let mut action = HomeAction::None;
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let area = ui.max_rect().shrink2(vec2(34.0, 6.0));
    let p = ui.painter().clone();

    // ---------------- header
    let a_head = appear(&ctx, 0.0, 0.6);
    let hy = area.top() + (1.0 - a_head) * 6.0;
    let mut hp = p.clone();
    hp.set_opacity(a_head);
    hp.text(pos2(area.left(), hy + 18.0), Align2::LEFT_CENTER, t(lang, K::Library), FontId::proportional(30.0), pal.text);
    hp.text(
        pos2(area.left(), hy + 46.0),
        Align2::LEFT_CENTER,
        {
            let total: u64 = instances.iter().map(|i| i.playtime_secs).sum();
            let greet = t(lang, match local_hour() { 5..=11 => K::GreetMorning, 12..=17 => K::GreetDay, 18..=22 => K::GreetEvening, _ => K::GreetNight });
            let mut s = format!("{greet}, {nickname}  ·  {} {}", instances.len(), t(lang, K::InstancesCount));
            if total >= 60 {
                s.push_str(&format!("  ·  {} {}", crate::core::i18n::format_playtime(total), t(lang, K::PlayedTotal)));
            }
            s
        },
        FontId::proportional(12.5),
        pal.text_dim,
    );
    let search_rect = Rect::from_min_size(pos2(area.right() - 260.0, hy + 6.0), vec2(260.0, 38.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(search_rect), |ui| {
        ui.set_opacity(a_head);
        let f = ui.painter();
        let focused = ui.memory(|m| m.has_focus(Id::new("lib_search")));
        let fo = spring_bool(&ctx, Id::new("lib_search_f"), focused, SNAPPY);
        f.rect_filled(search_rect, 12.0, Color32::from_black_alpha((70.0 + 40.0 * fo) as u8));
        f.rect_stroke(search_rect, 12.0, Stroke::new(1.0_f32, lerp_color(Color32::from_white_alpha(18), pal.accent, fo)));
        icons::search(f, pos2(search_rect.left() + 20.0, search_rect.center().y), 18.0, pal.text_dim);
        let inner = Rect::from_min_max(pos2(search_rect.left() + 36.0, search_rect.top() + 9.0), search_rect.max - vec2(10.0, 6.0));
        super::widgets::place(ui, inner, egui::TextEdit::singleline(&mut st.query).id(Id::new("lib_search")).frame(false).hint_text(t(lang, K::Search)).desired_width(inner.width()));
    });

    let filtered: Vec<&Instance> = {
        let q = st.query.trim().to_lowercase();
        instances.iter().filter(|i| q.is_empty() || i.name.to_lowercase().contains(&q) || i.version.contains(&q)).collect()
    };

    // ---------------- hero
    let hero = Rect::from_min_size(pos2(area.left(), area.top() + 76.0), vec2(area.width(), (area.height() * 0.44).clamp(220.0, 320.0)));
    let sel = selected.and_then(|id| instances.iter().find(|i| i.id == id));
    if st.shown.as_deref() != sel.map(|i| i.id.as_str()) {
        st.previous = st.shown.take();
        st.shown = sel.map(|i| i.id.clone());
        st.switch_t = now;
    }
    let dur = 0.7 * motion(&ctx) as f64;
    let xf = if dur <= 0.0 { 1.0 } else { ((now - st.switch_t) / dur).clamp(0.0, 1.0) as f32 };
    if xf < 1.0 {
        ctx.request_repaint();
    }
    let xf = ease_out(xf);
    let a_hero = appear(&ctx, 0.05, 0.7);
    let hero = hero.translate(vec2(0.0, (1.0 - a_hero) * 8.0));
    let mut hpnt = p.clone();
    hpnt.set_opacity(a_hero);

    match sel {
        Some(inst) => {
            let era = era_of(catalog, &inst.version);
            let hover = ui.rect_contains_pointer(hero);
            let zoom = spring_bool(&ctx, Id::new("hero_zoom"), hover, SOFT);
            hpnt.add(egui::epaint::Shadow { offset: vec2(0.0, 14.0), blur: 40.0, spread: 0.0, color: Color32::from_black_alpha(90) }.as_shape(hero, 26.0));
            if let Some(prev) = st.previous.as_ref().and_then(|id| instances.iter().find(|i| &i.id == id)) {
                if xf < 1.0 {
                    let tex = covers.get_wide(&ctx, &prev.id, &prev.version, era_of(catalog, &prev.version));
                    paint_cover_ex(&hpnt, &tex, hero, 26.0, 1.0 + 0.03 * zoom, 1.0);
                }
            }
            let tex = covers.get_wide(&ctx, &inst.id, &inst.version, era);
            super::wallpaper::paint_cover_drift(&hpnt, &tex, hero, 26.0, 1.08 + 0.02 * zoom + 0.02 * (1.0 - xf), xf, drift_t(&ctx));
            let (l, w) = (hero.left(), hero.width());
            gradient_rounded(&hpnt, hero, 26.0, |pt| {
                let k = 1.0 - ((pt.x - l) / (w * 0.72)).clamp(0.0, 1.0);
                Color32::from_black_alpha((215.0 * k.powf(1.3)) as u8)
            });
            hpnt.rect_stroke(hero, 26.0, Stroke::new(1.0_f32, Color32::from_white_alpha(22)));

            // text slides in with the crossfade
            let tx = hero.left() + 38.0 + (1.0 - xf) * 10.0;
            let mut tp = hpnt.clone();
            tp.set_opacity(a_hero * xf.max(0.15));
            let mut y = hero.top() + 40.0;
            let badge = era_badge(&tp, pos2(tx, y), era.label(), era.color());
            let lb = era_badge(&tp, pos2(badge.right() + 8.0, y), inst.loader.name(), pal.accent);
            if era.is_legacy() {
                icons::clock(&tp, pos2(lb.right() + 20.0, y), 16.0, pal.accent);
                tp.text(pos2(lb.right() + 32.0, y), Align2::LEFT_CENTER, t(lang, K::TimeMachine), FontId::proportional(12.0), pal.accent);
            }
            y += 22.0;
            let name = tp.layout(inst.name.clone(), FontId::proportional(46.0), pal.text, w * 0.6);
            let nh = name.size().y;
            tp.galley(pos2(tx, y), name, pal.text);
            y += nh + 4.0;
            let last = inst.last_played.map(format_date).unwrap_or_else(|| t(lang, K::Never).to_owned());
            tp.text(pos2(tx, y + 10.0), Align2::LEFT_CENTER, format!("Minecraft {}  ·  {} {}  ·  {} {}", inst.version, t(lang, K::LastPlayed), last, t(lang, K::PlayTime), format_playtime(inst.playtime_secs)), FontId::proportional(13.5), Color32::from_white_alpha(190));
            if era.is_legacy() {
                let note = tp.layout(t(lang, K::TimeMachineNote).to_owned(), FontId::proportional(12.0), Color32::from_white_alpha(150), w * 0.5);
                tp.galley(pos2(tx, y + 26.0), note, pal.text);
            }

            // buttons
            let by = hero.bottom() - 36.0 - 54.0;
            let busy = launch.as_ref().filter(|l| l.instance == inst.id).map(|l| l.progress);
            let other_busy = launch.as_ref().is_some_and(|l| l.instance != inst.id && l.progress.is_busy());
            let running = busy.is_some_and(|p| p.stage() == Stage::Running);
            let state = match busy.map(|p| (p.stage(), p.fraction())) {
                Some((Stage::Running, _)) => ButtonState::Running { label: t(lang, K::Running), stop: t(lang, K::Stop) },
                Some((s, f)) if stage_label(lang, &s).is_some() => ButtonState::Busy { label: stage_label(lang, &s).unwrap(), fraction: f },
                _ if other_busy => ButtonState::Disabled,
                _ => ButtonState::Ready,
            };
            let btn_rect = Rect::from_min_size(pos2(hero.left() + 38.0, by), vec2(220.0, 54.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(btn_rect.union(Rect::from_min_size(btn_rect.right_top() + vec2(14.0, 0.0), vec2(140.0, 54.0)))), |ui| {
                ui.set_opacity(a_hero);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if play_button(ui, pal, btn_rect.size(), t(lang, K::Play), state).clicked() {
                        action = if running { HomeAction::Stop(inst.id.clone()) } else { HomeAction::Launch(inst.id.clone()) };
                    }
                    if icon_button(ui, pal, 54.0, 0.0, |p, c, col| icons::folder(p, c, 22.0, col)).on_hover_text(t(lang, K::OpenFolder)).clicked() {
                        action = HomeAction::OpenFolder(inst.id.clone());
                    }
                    let armed = st.armed.as_ref().is_some_and(|(id, t0)| id == &inst.id && now - t0 < 3.0);
                    let danger = spring_bool(&ctx, Id::new("del_armed"), armed, SNAPPY);
                    let del = icon_button(ui, pal, 54.0, danger, |p, c, col| icons::trash(p, c, 22.0, col));
                    let del = if armed { del.on_hover_text(t(lang, K::ConfirmDelete)) } else { del.on_hover_text(t(lang, K::Delete)) };
                    if del.clicked() {
                        if armed {
                            st.armed = None;
                            action = HomeAction::Delete(inst.id.clone());
                        } else {
                            st.armed = Some((inst.id.clone(), now));
                        }
                    }
                    if armed {
                        ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    }
                });
            });
            if let Some(Stage::Failed(e)) = busy.map(|p| p.stage()) {
                let g = tp.layout(format!("{}: {}", t(lang, K::Failed), e.lines().next().unwrap_or("")), FontId::proportional(12.0), pal.danger, w * 0.55);
                tp.galley(pos2(hero.left() + 38.0, by - 10.0 - g.size().y), g, pal.danger);
            }
        }
        None => {
            hpnt.rect_filled(hero, 26.0, pal.panel);
            hpnt.rect_stroke(hero, 26.0, Stroke::new(1.0_f32, pal.panel_stroke));
            let (msg, sub, btn, act) = match catalog {
                Catalog::Loading => (t(lang, K::Loading), String::new(), None, HomeAction::None),
                Catalog::Failed(e) => (t(lang, K::LoadFailed), e.lines().next().unwrap_or("").to_owned(), Some(t(lang, K::Retry)), HomeAction::Retry),
                Catalog::Ready(_) => (t(lang, K::EmptyTitle), t(lang, K::EmptyDesc).to_owned(), Some(t(lang, K::NewInstance)), HomeAction::New),
            };
            hpnt.text(hero.center() - vec2(0.0, 30.0), Align2::CENTER_CENTER, msg, FontId::proportional(26.0), pal.text);
            hpnt.text(hero.center() + vec2(0.0, 4.0), Align2::CENTER_CENTER, sub, FontId::proportional(13.5), pal.text_dim);
            if let Some(label) = btn {
                let br = Rect::from_center_size(hero.center() + vec2(0.0, 56.0), vec2(200.0, 48.0));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(br), |ui| {
                    if play_button(ui, pal, br.size(), label, ButtonState::Ready).clicked() {
                        action = act;
                    }
                });
            }
        }
    }

    // ---------------- grid
    let a_lbl = appear(&ctx, 0.12, 0.45);
    let gy = hero.bottom() + 26.0;
    let mut lp = p.clone();
    lp.set_opacity(a_lbl);
    lp.text(pos2(area.left(), gy + 8.0), Align2::LEFT_CENTER, t(lang, K::Instances).to_uppercase(), FontId::proportional(11.5), pal.text_dim);
    let grid_rect = Rect::from_min_max(pos2(area.left() - 8.0, gy + 24.0), pos2(area.right() + 8.0, ui.max_rect().bottom()));

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(grid_rect), |ui| {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let gap = 18.0;
            let w = ui.available_width() - 16.0;
            let cols = (((w + gap) / (230.0 + gap)).floor() as usize).max(1);
            let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
            let ch = cw * 0.58 + 66.0;
            let n = filtered.len() + 1;
            let rows = n.div_ceil(cols);
            let (total, _) = ui.allocate_exact_size(vec2(w + 16.0, rows as f32 * (ch + gap) + 12.0), Sense::hover());
            let origin = total.min + vec2(8.0, 8.0);

            for i in 0..n {
                let (r, c) = (i / cols, i % cols);
                let base = Rect::from_min_size(origin + vec2(c as f32 * (cw + gap), r as f32 * (ch + gap)), vec2(cw, ch));
                let a = appear(&ctx, 0.15 + i as f32 * 0.06, 0.7);
                let base = base.translate(vec2(0.0, (1.0 - a) * 10.0));
                let mut cp = ui.painter().clone();
                cp.set_opacity(a);

                if i == filtered.len() {
                    let resp = ui.interact(base, Id::new("card_new"), Sense::click());
                    let h = spring_bool(&ctx, resp.id, resp.hovered(), BOUNCY);
                    if resp.hovered() {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let rr = base.shrink(4.0 * (1.0 - h) - 2.0 * h + 2.0);
                    cp.rect_filled(rr, 18.0, Color32::from_white_alpha((6.0 + 10.0 * h) as u8));
                    dashed_rect(&cp, rr, lerp_color(Color32::from_white_alpha(50), pal.accent, h));
                    let cc = rr.center() - vec2(0.0, 12.0);
                    cp.circle_filled(cc, 22.0 + 3.0 * h, pal.accent.gamma_multiply(0.15 + 0.2 * h));
                    icons::plus(&cp, cc, 22.0, lerp_color(pal.text, pal.accent, h));
                    cp.text(cc + vec2(0.0, 40.0), Align2::CENTER_CENTER, t(lang, K::NewInstance), FontId::proportional(13.5), lerp_color(pal.text_dim, pal.text, h));
                    // Secondary action: import a modpack (registered after the card so it wins clicks).
                    let pill = Rect::from_center_size(pos2(rr.center().x, rr.bottom() - 26.0), vec2(170.0, 30.0));
                    let pr = ui.interact(pill, Id::new("card_import"), Sense::click());
                    let ph = spring_bool(&ctx, pr.id, pr.hovered(), SNAPPY);
                    cp.rect_filled(pill, 15.0, lerp_color(Color32::from_white_alpha(14), pal.accent.gamma_multiply(0.3), ph));
                    cp.text(pill.center(), Align2::CENTER_CENTER, format!("⤓  {}", t(lang, K::ImportPack)), FontId::proportional(12.5), lerp_color(pal.text_dim, pal.text, ph.max(h * 0.5)));
                    if pr.clicked() {
                        action = HomeAction::Import;
                    } else if resp.clicked() && !pr.hovered() {
                        action = HomeAction::New;
                    }
                    continue;
                }

                let inst = filtered[i];
                let card_running = launch.as_ref().is_some_and(|l| l.instance == inst.id && l.progress.stage() == Stage::Running);
                let resp = ui.interact(base, Id::new(("card", &inst.id)), Sense::click());
                let lift = spring_bool(&ctx, resp.id.with("lift"), resp.hovered(), BOUNCY);
                let is_sel = selected == Some(inst.id.as_str());
                let s = spring_bool(&ctx, resp.id.with("sel"), is_sel, SNAPPY);
                if resp.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let rect = base.translate(vec2(0.0, -3.0 * lift));
                cp.add(egui::epaint::Shadow {
                    offset: vec2(0.0, 6.0 + 8.0 * lift),
                    blur: 16.0 + 18.0 * lift,
                    spread: 0.0,
                    color: Color32::from_black_alpha((60.0 + 50.0 * lift) as u8),
                }
                .as_shape(rect, 18.0));
                cp.rect_filled(rect, 18.0, pal.panel);
                let cover_rect = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width() * 0.58));
                let era = era_of(catalog, &inst.version);
                let tex = covers.get(&ctx, &inst.id, &inst.version, era);
                paint_cover_ex(&cp, &tex, cover_rect, egui::Rounding { nw: 18.0, ne: 18.0, ..Default::default() }, 1.0 + 0.03 * lift, 1.0);
                cp.rect_stroke(rect, 18.0, Stroke::new(1.0_f32, Color32::from_white_alpha((20.0 + 20.0 * lift) as u8)));
                if s > 0.01 {
                    cp.rect_stroke(rect.expand(3.5), 21.0, Stroke::new(2.0_f32, pal.accent.gamma_multiply(s.clamp(0.0, 1.0))));
                }

                // running pulse
                if launch.as_ref().is_some_and(|l| l.instance == inst.id && l.progress.is_busy()) {
                    let pulse = ((now * 1.6).sin() as f32 * 0.5 + 0.5) * 0.6 + 0.4;
                    cp.circle_filled(cover_rect.left_top() + vec2(18.0, 18.0), 5.0, pal.accent.gamma_multiply(pulse));
                    cp.circle_stroke(cover_rect.left_top() + vec2(18.0, 18.0), 5.0 + 5.0 * pulse, Stroke::new(1.0_f32, pal.accent.gamma_multiply(1.0 - pulse)));
                    ctx.request_repaint_after(std::time::Duration::from_millis(30));
                }

                cp.text(pos2(rect.left() + 16.0, cover_rect.bottom() + 22.0), Align2::LEFT_CENTER, &inst.name, FontId::proportional(15.0), pal.text);
                cp.text(
                    pos2(rect.left() + 16.0, cover_rect.bottom() + 44.0),
                    Align2::LEFT_CENTER,
                    format!("{} · {}", inst.version, inst.loader.name()),
                    FontId::proportional(12.0),
                    pal.text_dim,
                );
                cp.circle_filled(pos2(rect.right() - 20.0, cover_rect.bottom() + 33.0), 4.0, era.color());

                // play bubble pops on hover
                let pc = cover_rect.center();
                let pr = Rect::from_center_size(pc, vec2(52.0, 52.0));
                let presp = ui.interact(pr, Id::new(("card_play", &inst.id)), Sense::click());
                let pop = spring(&ctx, presp.id, if resp.hovered() || presp.hovered() { 1.0 } else { 0.0 }, BOUNCY);
                if pop > 0.02 {
                    let ph = spring_bool(&ctx, presp.id.with("h"), presp.hovered(), SNAPPY);
                    cp.add(egui::epaint::Shadow { offset: vec2(0.0, 6.0), blur: 20.0, spread: 0.0, color: Color32::from_black_alpha(120) }
                        .as_shape(Rect::from_center_size(pc, egui::Vec2::splat(46.0 * pop)), 23.0 * pop));
                    let fill = if card_running { lerp_color(pal.accent, Color32::from_rgb(214, 64, 56), ph) } else { pal.accent };
                    cp.circle_filled(pc, (22.0 + 2.0 * ph) * pop.max(0.0), fill);
                    if card_running {
                        cp.rect_filled(Rect::from_center_size(pc, egui::Vec2::splat(13.0 * pop.clamp(0.0, 1.0))), 2.0, Color32::WHITE);
                    } else {
                        icons::play(&cp, pc + vec2(2.0, 0.0), 22.0 * pop.clamp(0.0, 1.0), pal.on_accent);
                    }
                }
                if presp.clicked() {
                    action = if card_running { HomeAction::Stop(inst.id.clone()) } else { HomeAction::Launch(inst.id.clone()) };
                } else if resp.double_clicked() {
                    action = HomeAction::Launch(inst.id.clone());
                } else if resp.clicked() {
                    action = HomeAction::Select(inst.id.clone());
                }
            }
        });
    });

    action
}

fn dashed_rect(p: &egui::Painter, r: Rect, col: Color32) {
    let st = Stroke::new(1.5_f32, col);
    let pts = [r.left_top() + vec2(18.0, 0.0), r.right_top() - vec2(18.0, 0.0)];
    p.extend(egui::Shape::dashed_line(&pts, st, 6.0, 5.0));
    let pts = [r.left_bottom() + vec2(18.0, 0.0), r.right_bottom() - vec2(18.0, 0.0)];
    p.extend(egui::Shape::dashed_line(&pts, st, 6.0, 5.0));
    let pts = [r.left_top() + vec2(0.0, 18.0), r.left_bottom() - vec2(0.0, 18.0)];
    p.extend(egui::Shape::dashed_line(&pts, st, 6.0, 5.0));
    let pts = [r.right_top() + vec2(0.0, 18.0), r.right_bottom() - vec2(0.0, 18.0)];
    p.extend(egui::Shape::dashed_line(&pts, st, 6.0, 5.0));
    for (c, a0) in [(r.left_top() + vec2(18.0, 18.0), 180.0f32), (r.right_top() + vec2(-18.0, 18.0), 270.0), (r.right_bottom() - vec2(18.0, 18.0), 0.0), (r.left_bottom() + vec2(18.0, -18.0), 90.0)] {
        let arc: Vec<egui::Pos2> = (0..=8).map(|i| {
            let a = (a0 + 90.0 * i as f32 / 8.0).to_radians();
            c + vec2(a.cos(), a.sin()) * 18.0
        }).collect();
        p.add(egui::Shape::line(arc, st));
    }
}

pub fn stage_label(lang: Lang, st: &Stage) -> Option<&'static str> {
    Some(match st {
        Stage::Scanning => t(lang, K::Scanning),
        Stage::Preparing => t(lang, K::StagePreparing),
        Stage::Java => t(lang, K::StageJava),
        Stage::Libraries => t(lang, K::StageLibraries),
        Stage::Assets => t(lang, K::StageAssets),
        Stage::Natives => t(lang, K::StageNatives),
        Stage::LoaderInstall => t(lang, K::StageLoader),
        Stage::Starting => t(lang, K::StageStarting),
        Stage::Running => t(lang, K::Running),
        Stage::Exited | Stage::Failed(_) => return None,
    })
}

/// Slow "living" pan of the hero cover (a few px per second); frozen when motion is off.
fn drift_t(ctx: &egui::Context) -> f32 {
    if super::anim::motion(ctx) == 0.0 {
        return 0.0;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(33));
    ctx.input(|i| i.time) as f32
}

/// Local wall-clock hour (for the greeting).
fn local_hour() -> u16 {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTime {
            year: u16,
            month: u16,
            dow: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            ms: u16,
        }
        extern "system" {
            fn GetLocalTime(st: *mut SystemTime);
        }
        let mut st = SystemTime { year: 0, month: 0, dow: 0, day: 0, hour: 12, minute: 0, second: 0, ms: 0 };
        // SAFETY: GetLocalTime only writes the provided, correctly laid-out struct.
        unsafe { GetLocalTime(&mut st) };
        st.hour
    }
    #[cfg(not(windows))]
    {
        12
    }
}
