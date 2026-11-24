//! Window chrome: left navigation rail, drag strip, window controls, edge resize.
use egui::{pos2, vec2, Align2, Color32, FontId, Id, LayerId, Order, Rect, Sense, Stroke, TextureHandle, Ui, Vec2, ViewportCommand};

use super::anim::{spring, spring_bool, BOUNCY, SNAPPY, SOFT};
use super::icons;
use super::theme::Palette;
use super::widgets::lerp_color;
use crate::core::i18n::{t, Lang, K};

pub const RAIL_W: f32 = 76.0;
pub const TOP_H: f32 = 44.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RailAction {
    None,
    Library,
    New,
    Settings,
    Folder,
    Mods,
    Profile,
}

#[derive(Clone, Copy)]
enum Ic {
    Grid,
    Plus,
    Cube,
    Folder,
    Gear,
}

fn draw_icon(p: &egui::Painter, ic: Ic, c: egui::Pos2, col: Color32) {
    let s = 22.0;
    match ic {
        Ic::Grid => icons::grid(p, c, s, col),
        Ic::Plus => icons::plus(p, c, s, col),
        Ic::Cube => icons::cube(p, c, s, col),
        Ic::Folder => icons::folder(p, c, s, col),
        Ic::Gear => icons::gear(p, c, s, col),
    }
}

/// `active`: 0 library, 1 new, 2 mods, 4 settings (index into the item list).
pub fn rail(ui: &mut Ui, pal: &Palette, rect: Rect, logo: &TextureHandle, lang: Lang, active: usize, nickname: &str, face: Option<&TextureHandle>) -> RailAction {
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    p.rect_filled(rect, egui::Rounding { nw: super::theme::RADIUS, sw: super::theme::RADIUS, ..Default::default() }, pal.panel);
    p.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0_f32, pal.panel_stroke));

    // Logo
    let lc = pos2(rect.center().x, rect.top() + 34.0);
    p.circle_filled(lc, 20.0, pal.accent.gamma_multiply(0.08));
    p.image(logo.id(), Rect::from_center_size(lc, Vec2::splat(30.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);

    let items: [(Ic, &str, RailAction, bool); 5] = [
        (Ic::Grid, t(lang, K::Library), RailAction::Library, true),
        (Ic::Plus, t(lang, K::NewInstance), RailAction::New, true),
        (Ic::Cube, t(lang, K::Mods), RailAction::Mods, true),
        (Ic::Folder, t(lang, K::OpenFolder), RailAction::Folder, true),
        (Ic::Gear, t(lang, K::Settings), RailAction::Settings, true),
    ];
    let slot = |i: usize| -> Rect {
        if i == 5 {
            return Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 44.0), Vec2::splat(50.0));
        }
        let y = if i < 3 { rect.top() + 82.0 + i as f32 * 58.0 } else { rect.bottom() - 84.0 - (4 - i) as f32 * 58.0 - 58.0 };
        Rect::from_center_size(pos2(rect.center().x, y + 26.0), Vec2::splat(50.0))
    };

    // Sliding active indicator (spring, slight overshoot)
    let target = slot(active);
    let iy = spring(&ctx, Id::new("rail_ind"), target.center().y, BOUNCY);
    let ind = Rect::from_center_size(pos2(target.center().x, iy), Vec2::splat(50.0));
    p.rect_filled(ind, 15.0, pal.accent.gamma_multiply(0.18));
    p.rect_filled(Rect::from_center_size(pos2(rect.left() + 3.0, iy), vec2(4.0, 22.0)), 2.0, pal.accent);

    let mut action = RailAction::None;
    for (i, (ic, label, act, enabled)) in items.iter().enumerate() {
        let r = slot(i);
        let resp = ui.interact(r, Id::new(("rail", i)), Sense::click());
        let h = spring_bool(&ctx, resp.id.with("h"), resp.hovered(), SNAPPY);
        let press = spring_bool(&ctx, resp.id.with("p"), resp.is_pointer_button_down_on(), SNAPPY);
        if resp.hovered() && *enabled {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if h > 0.01 && i != active {
            p.rect_filled(r.shrink(2.0 * (1.0 - h)), 15.0, Color32::from_white_alpha((14.0 * h) as u8));
        }
        let col = if i == active {
            pal.accent
        } else if *enabled {
            lerp_color(pal.text_dim, pal.text, h)
        } else {
            pal.text_dim.gamma_multiply(0.45)
        };
        let c = r.center() + vec2(0.0, press * 0.6);
        draw_icon(&p, *ic, c, col);
        if resp.clicked() && *enabled {
            action = *act;
        }

        // Label bubble that springs out of the rail
        let tip = spring_bool(&ctx, resp.id.with("tip"), resp.hovered(), SOFT);
        if tip > 0.02 {
            let lp = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new(("rail_tip", i))));
            let text = if *enabled { label.to_string() } else { format!("{label} · {}", t(lang, K::Soon)) };
            let g = lp.layout_no_wrap(text, FontId::proportional(13.0), pal.text);
            let br = Rect::from_min_size(pos2(rect.right() + 8.0 + 4.0 * tip, r.center().y - 15.0), vec2(g.size().x + 22.0, 30.0));
            lp.rect_filled(br, 10.0, Color32::from_rgba_unmultiplied(18, 19, 24, (235.0 * tip) as u8));
            lp.rect_stroke(br, 10.0, Stroke::new(1.0_f32, pal.panel_stroke.gamma_multiply(tip)));
            lp.galley(br.center() - g.size() / 2.0, g, pal.text.gamma_multiply(tip));
        }
    }

    // Avatar → account settings
    let av = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 44.0), Vec2::splat(40.0));
    let resp = ui.interact(av, Id::new("rail_avatar"), Sense::click());
    let h = spring_bool(&ctx, resp.id, resp.hovered(), BOUNCY);
    let uuid = crate::core::auth::offline_uuid(nickname);
    let hue = u8::from_str_radix(&uuid[..2], 16).unwrap_or(0) as f32 / 255.0;
    let col: Color32 = egui::ecolor::Hsva::new(hue, 0.35, 0.62, 1.0).into();
    let avr = av.expand(2.0 * h);
    match face {
        Some(tex) => {
            // Skin face: head front + hat layer, pixel-crisp.
            let th = tex.size()[1] as f32 * 64.0 / tex.size()[0] as f32;
            let f = avr.shrink(4.0);
            p.rect_filled(avr, 12.0 + 2.0 * h, Color32::from_black_alpha(60));
            for (x, y, e) in [(8.0, 8.0, 0.0), (40.0, 8.0, 1.5)] {
                let mut m = egui::Mesh::with_texture(tex.id());
                m.add_rect_with_uv(f.expand(e), Rect::from_min_max(pos2(x / 64.0, y / th), pos2((x + 8.0) / 64.0, (y + 8.0) / th)), Color32::WHITE);
                p.add(m);
            }
        }
        None => {
    p.rect_filled(avr, 12.0 + 2.0 * h, col);
    let initial = nickname.chars().next().unwrap_or('?').to_ascii_uppercase().to_string();
    p.text(avr.center(), Align2::CENTER_CENTER, initial, FontId::proportional(18.0), Color32::WHITE);
        }
    }
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        action = RailAction::Profile;
    }
    action
}

/// Invisible drag strip over the content top + min/max/close.
pub fn top_bar(ui: &mut Ui, pal: &Palette, rect: Rect) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let bw = 46.0;
    let drag_rect = Rect::from_min_max(rect.min, pos2(rect.right() - bw * 3.0, rect.bottom()));
    let drag = ui.interact(drag_rect, Id::new("drag_strip"), Sense::click_and_drag());
    if drag.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    for i in 0..3 {
        let r = Rect::from_min_size(pos2(rect.right() - bw * (i as f32 + 1.0), rect.top()), vec2(bw, rect.height()));
        let resp = ui.interact(r, Id::new(("winbtn", i)), Sense::click());
        let h = spring_bool(&ctx, resp.id, resp.hovered(), SNAPPY);
        let p = ui.painter();
        if h > 0.01 {
            let (bg, rounding) = if i == 0 {
                (Color32::from_rgb(196, 43, 28), if maximized { egui::Rounding::ZERO } else { egui::Rounding { ne: super::theme::RADIUS, ..Default::default() } })
            } else {
                (Color32::from_white_alpha(16), egui::Rounding::ZERO)
            };
            p.rect_filled(r, rounding, bg.gamma_multiply(h.clamp(0.0, 1.0)));
        }
        let col = lerp_color(pal.text.gamma_multiply(0.75), Color32::WHITE, h);
        match i {
            0 => icons::close(p, r.center(), 20.0, col),
            1 => icons::maximize(p, r.center(), 20.0, col, maximized),
            _ => icons::minimize(p, r.center(), 20.0, col),
        }
        if resp.clicked() {
            match i {
                0 => ctx.send_viewport_cmd(ViewportCommand::Close),
                1 => ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized)),
                _ => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
            }
        }
    }
}

/// Borderless windows lose native edge resizing — reimplement it.
pub fn handle_resize(ctx: &egui::Context) {
    use egui::viewport::ResizeDirection as D;
    use egui::CursorIcon as C;
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let r = ctx.screen_rect();
    let m = 6.0;
    let (l, rt, tp, b) = (pos.x < r.left() + m, pos.x > r.right() - m, pos.y < r.top() + m, pos.y > r.bottom() - m);
    let hit = match (l, rt, tp, b) {
        (true, _, true, _) => Some((D::NorthWest, C::ResizeNwSe)),
        (_, true, _, true) => Some((D::SouthEast, C::ResizeNwSe)),
        (_, true, true, _) => Some((D::NorthEast, C::ResizeNeSw)),
        (true, _, _, true) => Some((D::SouthWest, C::ResizeNeSw)),
        (true, ..) => Some((D::West, C::ResizeHorizontal)),
        (_, true, ..) => Some((D::East, C::ResizeHorizontal)),
        (_, _, true, _) => Some((D::North, C::ResizeVertical)),
        (.., true) => Some((D::South, C::ResizeVertical)),
        _ => None,
    };
    if let Some((dir, icon)) = hit {
        ctx.set_cursor_icon(icon);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}
