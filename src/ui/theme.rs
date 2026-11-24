use egui::{Color32, FontFamily, FontId, Rounding, Stroke, TextStyle};

use crate::core::settings::{Settings, ThemePreset};

pub const RADIUS: f32 = 12.0;
pub const WIDGET_RADIUS: f32 = 8.0;

/// Palette derived from the active preset + accent.
#[derive(Clone, Copy)]
pub struct Palette {
    pub panel: Color32,
    pub panel_stroke: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
}

impl Palette {
    pub fn from_settings(s: &Settings) -> Self {
        let [r, g, b] = s.accent;
        let accent = Color32::from_rgb(r, g, b);
        let lum = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
        let on_accent = if lum > 0.6 { Color32::from_rgb(12, 12, 14) } else { Color32::WHITE };

        let (panel, panel_stroke) = match s.preset {
            ThemePreset::FrostedGlass => (
                ([24, 26, 32], 150.0),
                Color32::from_white_alpha(26),
            ),
            ThemePreset::DarkOled => (
                ([0, 0, 0], 215.0),
                Color32::from_white_alpha(14),
            ),
            ThemePreset::ClassicStone => (
                ([46, 46, 44], 225.0),
                Color32::from_white_alpha(20),
            ),
        };

        let ([r, g, b], a) = panel;
        let panel = Color32::from_rgba_unmultiplied(r, g, b, (a * s.panel_opacity).clamp(40.0, 255.0) as u8);
        Self {
            panel,
            panel_stroke,
            text: Color32::from_rgb(236, 237, 240),
            text_dim: Color32::from_rgb(150, 154, 164),
            accent,
            on_accent,
            danger: Color32::from_rgb(232, 112, 100),
        }
    }

    pub fn accent_alpha(&self, a: f32) -> Color32 {
        self.accent.gamma_multiply(a)
    }
}

pub fn install_text_styles(ctx: &egui::Context) {
    ctx.style_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(26.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.slider_width = 260.0;
        style.animation_time = 0.14;
    });
}

pub fn apply(ctx: &egui::Context, pal: &Palette) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = pal.panel;
    v.window_rounding = Rounding::same(RADIUS);
    v.extreme_bg_color = Color32::from_black_alpha(110);
    v.faint_bg_color = Color32::from_white_alpha(6);
    v.override_text_color = Some(pal.text);
    v.hyperlink_color = pal.accent;
    v.selection.bg_fill = pal.accent_alpha(0.55);
    v.selection.stroke = Stroke::new(1.0_f32, pal.accent);
    v.slider_trailing_fill = true;

    let r = Rounding::same(WIDGET_RADIUS);
    for (w, fill) in [
        (&mut v.widgets.noninteractive, Color32::TRANSPARENT),
        (&mut v.widgets.inactive, Color32::from_white_alpha(12)),
        (&mut v.widgets.hovered, Color32::from_white_alpha(22)),
        (&mut v.widgets.active, Color32::from_white_alpha(30)),
        (&mut v.widgets.open, Color32::from_white_alpha(22)),
    ] {
        w.rounding = r;
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, pal.text_dim);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, pal.panel_stroke);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_white_alpha(30));
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, pal.accent);

    ctx.set_visuals(v);
}
