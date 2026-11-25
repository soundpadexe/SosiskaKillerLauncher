//! Settings as a right-hand sheet: springs in over a dimmed backdrop,
//! sections cascade in with staggered offsets.
use egui::{pos2, vec2, Align2, Color32, FontId, Id, Order, Rect, RichText, Sense, Stroke, Ui, Vec2};

use super::anim::{spring, spring_bool, spring_reset, BOUNCY, SNAPPY, SOFT};
use super::icons;
use super::theme::Palette;
use super::wallpaper::{paint_cover, Wallpaper, BUILTIN_NAMES};
use super::widgets::{ghost_button, icon_button, lerp_color, segmented, slider, toggle};
use crate::core::auth::{is_valid_nickname, offline_uuid};
use crate::core::i18n::{t, Lang, K};
use crate::core::paths::Paths;
use crate::core::settings::{AfterLaunch, JvmPreset, Settings, ThemePreset, WallpaperSource};

const ACCENTS: [[u8; 3]; 8] = [
    [124, 140, 255],
    [94, 170, 255],
    [92, 190, 160],
    [150, 190, 110],
    [226, 180, 96],
    [232, 132, 96],
    [222, 112, 140],
    [190, 140, 230],
];
const SCALES: [f32; 5] = [0.9, 1.0, 1.1, 1.25, 1.5];
pub const SHEET_ID: &str = "settings_sheet";
const W: f32 = 760.0;
const NAV_W: f32 = 170.0;

#[derive(Default)]
pub struct Sheet {
    pub closing: bool,
    pub tab: usize,
}

/// Returns true once the close animation has finished.
pub fn show(ctx: &egui::Context, pal: &Palette, s: &mut Settings, sh: &mut Sheet, paths: &Paths, wp: &Wallpaper) -> bool {
    let p = spring(ctx, Id::new(SHEET_ID), if sh.closing { 0.0 } else { 1.0 }, SOFT);
    if sh.closing && p < 0.02 {
        return true;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        sh.closing = true;
    }
    let screen = ctx.screen_rect();
    egui::Area::new(Id::new("settings_area")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        ui.set_min_size(screen.size());
        let bg = ui.interact(screen, Id::new("settings_bg"), Sense::click());
        ui.painter().rect_filled(screen, 12.0, Color32::from_black_alpha((130.0 * p.clamp(0.0, 1.0)) as u8));
        if bg.clicked() {
            sh.closing = true;
        }
        let w = W.min(screen.width() - 40.0);
        let x = screen.right() - 10.0 - w * p;
        let sheet = Rect::from_min_size(pos2(x, screen.top() + 10.0), vec2(w, screen.height() - 20.0));
        let pnt = ui.painter();
        pnt.add(egui::epaint::Shadow { offset: vec2(-10.0, 0.0), blur: 50.0, spread: 0.0, color: Color32::from_black_alpha((150.0 * p.clamp(0.0, 1.0)) as u8) }.as_shape(sheet, 24.0));
        pnt.rect_filled(sheet, 24.0, Color32::from_rgba_unmultiplied(17, 18, 23, 250));
        pnt.rect_stroke(sheet, 24.0, Stroke::new(1.0_f32, pal.panel_stroke));
        let _ = ui.interact(sheet, Id::new("settings_sheet_block"), Sense::click());

        let inner = sheet.shrink2(vec2(30.0, 24.0));
        let lang = s.lang;
        pnt.text(inner.left_top() + vec2(0.0, 18.0), Align2::LEFT_CENTER, t(lang, K::Settings), FontId::proportional(26.0), pal.text);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(inner.right() - 40.0, inner.top()), Vec2::splat(40.0))), |ui| {
            if icon_button(ui, pal, 40.0, 0.0, |p, c, col| icons::close(p, c, 18.0, col)).clicked() {
                sh.closing = true;
            }
        });
        let body = Rect::from_min_max(inner.min + vec2(0.0, 58.0), inner.max);
        let tabs: [(&str, Section); 7] = [
            (t(lang, K::Account), account),
            (t(lang, K::Appearance), appearance),
            (t(lang, K::Interface), interface),
            (t(lang, K::Game), game),
            (t(lang, K::JavaTab), java_tab),
            (t(lang, K::Launcher), launcher),
            (t(lang, K::About), about),
        ];
        sh.tab = sh.tab.min(tabs.len() - 1);

        // Left rail: tab list with a sliding accent pill.
        let nav = Rect::from_min_size(body.min, vec2(NAV_W, body.height()));
        let row_h = 40.0;
        let ctx = ui.ctx().clone();
        let pill_y = spring(&ctx, Id::new("set_pill"), sh.tab as f32 * row_h, BOUNCY);
        let pill = Rect::from_min_size(pos2(nav.left(), nav.top() + pill_y), vec2(NAV_W, row_h - 4.0));
        ui.painter().rect_filled(pill, 12.0, pal.accent_alpha(0.18));
        ui.painter().rect_filled(Rect::from_min_size(pill.left_center() - vec2(0.0, 9.0), vec2(3.0, 18.0)), 2.0, pal.accent);
        for (i, (name, _)) in tabs.iter().enumerate() {
            let r = Rect::from_min_size(pos2(nav.left(), nav.top() + i as f32 * row_h), vec2(NAV_W, row_h - 4.0));
            let resp = ui.interact(r, Id::new(("set_tab", i)), Sense::click());
            let h = spring_bool(&ctx, resp.id.with("h"), resp.hovered() && i != sh.tab, SNAPPY);
            if h > 0.01 {
                ui.painter().rect_filled(r, 12.0, Color32::from_white_alpha((10.0 * h) as u8));
            }
            if resp.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() && i != sh.tab {
                sh.tab = i;
                spring_reset(&ctx, Id::new("set_page"), 0.0);
            }
            let col = if i == sh.tab { pal.text } else { lerp_color(pal.text_dim, pal.text, h) };
            ui.painter().text(r.left_center() + vec2(16.0 + 3.0 * h, 0.0), Align2::LEFT_CENTER, *name, FontId::proportional(14.5), col);
        }
        let sep = nav.right() + 14.0;
        ui.painter().vline(sep, body.y_range(), Stroke::new(1.0_f32, Color32::from_white_alpha(14)));

        // Page: fades/slides in on every tab switch, and with the sheet itself.
        let page_t = spring(&ctx, Id::new("set_page"), 1.0, SOFT).min(p.clamp(0.0, 1.0));
        let page = Rect::from_min_max(pos2(sep + 22.0, body.top()), body.max);
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(page), |ui| {
            egui::ScrollArea::vertical().id_salt(("set_scroll", sh.tab)).auto_shrink(false).show(ui, |ui| {
                ui.set_width(page.width() - 12.0);
                ui.set_opacity(page_t);
                ui.add_space((1.0 - page_t) * 18.0);
                let (title, f) = tabs[sh.tab];
                ui.label(RichText::new(title).size(20.0).color(pal.text).strong());
                ui.add_space(14.0);
                f(ui, pal, s, lang, paths, wp);
            });
        });
    });
    false
}

type Section = fn(&mut Ui, &Palette, &mut Settings, Lang, &Paths, &Wallpaper);

/// Framed single-line text field with focus glow.
fn input(ui: &mut Ui, pal: &Palette, id: &str, text: &mut String, w: f32, hint: &str, mono: bool) -> egui::Response {
    let fr = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover()).0;
    let ctx = ui.ctx().clone();
    let foc = ui.memory(|m| m.has_focus(Id::new(id)));
    let fo = spring_bool(&ctx, Id::new(id).with("f"), foc, SNAPPY);
    ui.painter().rect_filled(fr, 12.0, Color32::from_black_alpha((80.0 + 40.0 * fo) as u8));
    ui.painter().rect_stroke(fr, 12.0, Stroke::new(1.0_f32, lerp_color(Color32::from_white_alpha(20), pal.accent, fo)));
    let inner = fr.shrink2(vec2(14.0, 10.0));
    let font = if mono { FontId::monospace(13.0) } else { FontId::proportional(14.5) };
    super::widgets::place(ui, inner, egui::TextEdit::singleline(text).id(Id::new(id)).hint_text(RichText::new(hint).font(font.clone()).color(pal.text_dim.gamma_multiply(0.55))).frame(false).font(font).desired_width(inner.width()))
}

fn num_input(ui: &mut Ui, pal: &Palette, id: &str, v: &mut u32, hint: &str) {
    let mut s = if *v == 0 { String::new() } else { v.to_string() };
    if input(ui, pal, id, &mut s, 96.0, hint, true).changed() {
        let digits: String = s.chars().filter(char::is_ascii_digit).take(5).collect();
        *v = digits.parse().unwrap_or(0);
    }
}

fn dir_size(p: &std::path::Path) -> u64 {
    std::fs::read_dir(p).map_or(0, |rd| {
        rd.flatten()
            .map(|e| match e.metadata() {
                Ok(m) if m.is_dir() => dir_size(&e.path()),
                Ok(m) => m.len(),
                Err(_) => 0,
            })
            .sum()
    })
}

fn block(ui: &mut Ui, pal: &Palette, title: &str, desc: &str, control: impl FnOnce(&mut Ui)) {
    ui.label(RichText::new(title).size(14.5).color(pal.text));
    ui.label(RichText::new(desc).size(12.0).color(pal.text_dim));
    ui.add_space(6.0);
    ui.horizontal_wrapped(control);
    ui.add_space(14.0);
}

fn account(ui: &mut Ui, pal: &Palette, s: &mut Settings, lang: Lang, _: &Paths, _: &Wallpaper) {
    block(ui, pal, t(lang, K::Nickname), t(lang, K::NicknameDesc), |ui| {
        let ok = is_valid_nickname(&s.nickname);
        let fr = ui.allocate_exact_size(vec2(260.0, 40.0), Sense::hover()).0;
        let ctx = ui.ctx().clone();
        let foc = ui.memory(|m| m.has_focus(Id::new("nick_edit")));
        let fo = spring_bool(&ctx, Id::new("nick_f"), foc, SNAPPY);
        let bad = spring_bool(&ctx, Id::new("nick_bad"), !ok, SNAPPY);
        let border = lerp_color(lerp_color(Color32::from_white_alpha(20), pal.accent, fo), pal.danger, bad);
        ui.painter().rect_filled(fr, 12.0, Color32::from_black_alpha((80.0 + 40.0 * fo) as u8));
        ui.painter().rect_stroke(fr, 12.0, Stroke::new(1.0_f32, border));
        let inner = fr.shrink2(vec2(14.0, 10.0));
        super::widgets::place(ui, inner, egui::TextEdit::singleline(&mut s.nickname).id(Id::new("nick_edit")).frame(false).char_limit(16).font(FontId::proportional(15.0)).desired_width(inner.width()));
        ui.add_space(10.0);
        if ok {
            ui.label(RichText::new(offline_uuid(&s.nickname)).monospace().size(11.0).color(pal.text_dim));
        } else {
            ui.label(RichText::new(t(lang, K::NicknameInvalid)).size(12.0).color(pal.danger));
        }
    });
}

fn appearance(ui: &mut Ui, pal: &Palette, s: &mut Settings, lang: Lang, _: &Paths, wp: &Wallpaper) {
    block(ui, pal, t(lang, K::Theme), t(lang, K::ThemeDesc), |ui| {
        let names: Vec<&str> = ThemePreset::ALL.iter().map(|p| p.name()).collect();
        let mut i = ThemePreset::ALL.iter().position(|p| *p == s.preset).unwrap_or(0);
        if segmented(ui, pal, "theme", &names, &mut i, false) {
            s.preset = ThemePreset::ALL[i];
        }
    });
    block(ui, pal, t(lang, K::Accent), t(lang, K::AccentDesc), |ui| {
        for c in ACCENTS {
            if swatch(ui, Color32::from_rgb(c[0], c[1], c[2]), s.accent == c).clicked() {
                s.accent = c;
            }
        }
        ui.add_space(6.0);
        egui::color_picker::color_edit_button_srgb(ui, &mut s.accent);
    });
    block(ui, pal, t(lang, K::Wallpaper), t(lang, K::WallpaperDesc), |ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        for (i, name) in BUILTIN_NAMES.iter().enumerate() {
            let src = WallpaperSource::Builtin(i as u8);
            if wallpaper_card(ui, pal, wp.thumb(i), name, s.wallpaper == src).clicked() {
                s.wallpaper = src;
            }
        }
        let custom = matches!(s.wallpaper, WallpaperSource::Custom(_));
        if wallpaper_card(ui, pal, wp.custom_thumb(), t(lang, K::Custom), custom).clicked() {
            if let Some(path) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg"]).pick_file() {
                s.wallpaper = WallpaperSource::Custom(path);
            }
        }
    });
    let bl = format!("{:.0}", s.blur);
    block(ui, pal, t(lang, K::Blur), t(lang, K::BlurDesc), |ui| {
        slider(ui, pal, &mut s.blur, 0.0, 48.0, 1.0, &bl);
    });
    let ol = format!("{:.0}%", s.panel_opacity * 100.0);
    block(ui, pal, t(lang, K::PanelOpacity), t(lang, K::PanelOpacityDesc), |ui| {
        slider(ui, pal, &mut s.panel_opacity, 0.5, 1.6, 0.05, &ol);
    });
    let dl = format!("{:.0}%", s.dim * 100.0);
    block(ui, pal, t(lang, K::Dim), t(lang, K::DimDesc), |ui| {
        slider(ui, pal, &mut s.dim, 0.0, 0.9, 0.05, &dl);
    });
}

fn interface(ui: &mut Ui, pal: &Palette, s: &mut Settings, lang: Lang, _: &Paths, _: &Wallpaper) {
    block(ui, pal, t(lang, K::UiScale), t(lang, K::UiScaleDesc), |ui| {
        let labels: Vec<String> = SCALES.iter().map(|v| format!("{:.0}%", v * 100.0)).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mut i = SCALES.iter().position(|v| (v - s.ui_scale).abs() < 0.01).unwrap_or(1);
        if segmented(ui, pal, "scale", &refs, &mut i, true) {
            s.ui_scale = SCALES[i];
        }
    });
    block(ui, pal, t(lang, K::Language), t(lang, K::LanguageDesc), |ui| {
        let names: Vec<&str> = Lang::ALL.iter().map(|l| l.native_name()).collect();
        let mut i = Lang::ALL.iter().position(|l| *l == s.lang).unwrap_or(0);
        if segmented(ui, pal, "lang", &names, &mut i, false) {
            s.lang = Lang::ALL[i];
        }
    });
    if !s.reduce_motion {
        block(ui, pal, t(lang, K::AnimSpeed), t(lang, K::AnimSpeedDesc), |ui| {
            let names = [t(lang, K::SpeedFast), t(lang, K::SpeedNormal), t(lang, K::SpeedSmooth)];
            let mut i = s.anim_speed.min(2) as usize;
            if segmented(ui, pal, "anim_speed", &names, &mut i, false) {
                s.anim_speed = i as u8;
            }
        });
    }
    block(ui, pal, t(lang, K::Motion), t(lang, K::MotionDesc), |ui| {
        let mut on = !s.reduce_motion;
        if toggle(ui, pal, &mut on) {
            s.reduce_motion = !on;
        }
    });
}

fn game(ui: &mut Ui, pal: &Palette, s: &mut Settings, lang: Lang, _: &Paths, _: &Wallpaper) {
    let max = crate::core::sysinfo::max_memory_mb() as f32;
    let mut mem = (s.memory_mb as f32).min(max);
    let label = format!("{:.1} GB", mem / 1024.0);
    block(ui, pal, t(lang, K::Memory), t(lang, K::MemoryDesc), |ui| {
        if slider(ui, pal, &mut mem, 1024.0, max, 512.0, &label) {
            s.memory_mb = mem as u32;
        }
    });
    block(ui, pal, t(lang, K::Resolution), t(lang, K::ResolutionDesc), |ui| {
        const SIZES: [(u32, u32); 4] = [(0, 0), (1280, 720), (1600, 900), (1920, 1080)];
        let labels = [t(lang, K::Auto), "1280×720", "1600×900", "1920×1080"];
        let g = &mut s.game;
        let mut i = SIZES.iter().position(|&(w, h)| w == g.width && h == g.height).unwrap_or(usize::MAX);
        let mut pick = i.min(SIZES.len());
        if segmented(ui, pal, "res", &labels, &mut pick, false) {
            i = pick;
            (g.width, g.height) = SIZES[i];
        }
        ui.end_row();
        ui.add_space(4.0);
        num_input(ui, pal, "res_w", &mut g.width, "854");
        ui.label(RichText::new("×").size(16.0).color(pal.text_dim));
        num_input(ui, pal, "res_h", &mut g.height, "480");
    });
    block(ui, pal, t(lang, K::Fullscreen), t(lang, K::FullscreenDesc), |ui| {
        toggle(ui, pal, &mut s.game.fullscreen);
    });
    let mut keep = s.backup_keep as f32;
    let kl = format!("{}", s.backup_keep);
    block(ui, pal, t(lang, K::Backups), t(lang, K::BackupsDesc), |ui| {
        toggle(ui, pal, &mut s.backup_worlds);
        ui.add_space(12.0);
        if s.backup_worlds && slider(ui, pal, &mut keep, 1.0, 20.0, 1.0, &kl) {
            s.backup_keep = keep as u32;
        }
    });
    block(ui, pal, t(lang, K::QuickJoin), t(lang, K::QuickJoinDesc), |ui| {
        input(ui, pal, "quick_join", &mut s.game.server, 320.0, "play.example.org:25565", true);
        if !s.game.server.is_empty() && icon_button(ui, pal, 40.0, 0.0, |p, c, col| icons::close(p, c, 14.0, col)).clicked() {
            s.game.server.clear();
        }
    });
}

fn java_tab(ui: &mut Ui, pal: &Palette, s: &mut Settings, lang: Lang, _: &Paths, _: &Wallpaper) {
    let g = &mut s.game;
    block(ui, pal, t(lang, K::JvmPreset), t(lang, K::JvmPresetDesc), |ui| {
        const ALL: [JvmPreset; 3] = [JvmPreset::Default, JvmPreset::G1, JvmPreset::Zgc];
        let names = [t(lang, K::PresetDefault), t(lang, K::PresetG1), t(lang, K::PresetZgc)];
        let mut i = ALL.iter().position(|p| *p == g.jvm_preset).unwrap_or(0);
        if segmented(ui, pal, "jvm_preset", &names, &mut i, false) {
            g.jvm_preset = ALL[i];
        }
    });
    let auto = g.java_path.trim().is_empty();
    let bad = !auto && !std::path::Path::new(g.java_path.trim()).is_file();
    block(ui, pal, t(lang, K::JavaPath), t(lang, K::JavaPathDesc), |ui| {
        input(ui, pal, "java_path", &mut g.java_path, 300.0, "Auto · Mojang", true);
        if ghost_button(ui, pal, t(lang, K::Browse)).clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Java", &["exe"]).pick_file() {
                g.java_path = p.display().to_string();
            }
        }
        if !auto && ghost_button(ui, pal, t(lang, K::Reset)).clicked() {
            g.java_path.clear();
        }
    });
    if bad {
        ui.label(RichText::new("⚠ java.exe ?").size(12.0).color(pal.danger));
        ui.add_space(10.0);
    }
    block(ui, pal, t(lang, K::JvmArgs), t(lang, K::JvmArgsDesc), |ui| {
        input(ui, pal, "jvm_args", &mut g.jvm_args, 420.0, "-XX:+UseStringDeduplication", true);
    });
    block(ui, pal, t(lang, K::GameArgs), t(lang, K::GameArgsDesc), |ui| {
        input(ui, pal, "game_args", &mut g.game_args, 420.0, "--demo", true);
    });
}

fn launcher(ui: &mut Ui, pal: &Palette, s: &mut Settings, lang: Lang, paths: &Paths, _: &Wallpaper) {
    block(ui, pal, t(lang, K::AfterLaunch), t(lang, K::AfterLaunchDesc), |ui| {
        const ALL: [AfterLaunch; 2] = [AfterLaunch::Keep, AfterLaunch::Minimize];
        let names = [t(lang, K::KeepOpen), t(lang, K::MinimizeWin)];
        let mut i = ALL.iter().position(|p| *p == s.after_launch).unwrap_or(0);
        if segmented(ui, pal, "after_launch", &names, &mut i, false) {
            s.after_launch = ALL[i];
        }
    });
    if s.after_launch == AfterLaunch::Minimize {
        block(ui, pal, t(lang, K::RestoreOnExit), t(lang, K::RestoreOnExitDesc), |ui| {
            toggle(ui, pal, &mut s.restore_on_exit);
        });
    }
    // Cache size is walked at most once per second, not every frame.
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let key = Id::new("cache_size");
    let (at, size) = ctx.data(|d| d.get_temp::<(f64, u64)>(key)).unwrap_or((-10.0, 0));
    let size = if now - at > 1.0 {
        let v = dir_size(&paths.cache());
        ctx.data_mut(|d| d.insert_temp(key, (now, v)));
        v
    } else {
        size
    };
    let desc = format!("{} · {:.1} MB", t(lang, K::CacheDesc), size as f64 / 1_048_576.0);
    block(ui, pal, t(lang, K::Cache), &desc, |ui| {
        if ghost_button(ui, pal, t(lang, K::ClearCache)).clicked() {
            let _ = std::fs::remove_dir_all(paths.cache());
            let _ = std::fs::create_dir_all(paths.cache());
            ctx.data_mut(|d| d.insert_temp(key, (-10.0f64, 0u64)));
        }
        if ghost_button(ui, pal, t(lang, K::OpenFolder)).clicked() {
            crate::core::paths::open(paths.cache());
        }
    });
    block(ui, pal, t(lang, K::CfKey), t(lang, K::CfKeyDesc), |ui| {
        let fr = ui.allocate_exact_size(vec2(360.0, 40.0), Sense::hover()).0;
        ui.painter().rect_filled(fr, 12.0, Color32::from_black_alpha(90));
        ui.painter().rect_stroke(fr, 12.0, Stroke::new(1.0_f32, Color32::from_white_alpha(20)));
        let inner = fr.shrink2(vec2(14.0, 10.0));
        super::widgets::place(ui, inner, egui::TextEdit::singleline(&mut s.curseforge_key).password(true).frame(false).desired_width(inner.width()));
    });
}

fn about(ui: &mut Ui, pal: &Palette, _: &mut Settings, lang: Lang, paths: &Paths, _: &Wallpaper) {
    let mut desc = paths.root().display().to_string();
    if paths.portable {
        desc.push_str(&format!("  ({})", t(lang, K::Portable)));
    }
    block(ui, pal, t(lang, K::DataFolder), &desc, |ui| {
        if ghost_button(ui, pal, t(lang, K::OpenFolder)).clicked() {
            crate::core::paths::open(paths.root());
        }
    });
    ui.label(RichText::new(concat!("SosiskaKillerLauncher v", env!("CARGO_PKG_VERSION"), " · Pure Rust")).size(12.0).color(pal.text_dim));
}

fn swatch(ui: &mut Ui, color: Color32, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    let ctx = ui.ctx().clone();
    let s = spring_bool(&ctx, resp.id.with("s"), selected, BOUNCY);
    let h = spring_bool(&ctx, resp.id.with("h"), resp.hovered(), BOUNCY);
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let p = ui.painter();
    p.circle_filled(rect.center(), 9.0 + 1.5 * h - 1.5 * s, color);
    if s > 0.01 {
        p.circle_stroke(rect.center(), 12.5, Stroke::new(1.8_f32, Color32::WHITE.gamma_multiply(s.clamp(0.0, 1.0))));
    }
    resp
}

fn wallpaper_card(ui: &mut Ui, pal: &Palette, tex: Option<&egui::TextureHandle>, name: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(118.0, 96.0), Sense::click());
    let ctx = ui.ctx().clone();
    let h = spring_bool(&ctx, resp.id.with("h"), resp.hovered(), BOUNCY);
    let s = spring_bool(&ctx, resp.id.with("s"), selected, SNAPPY);
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let img = Rect::from_min_size(rect.min + vec2(0.0, 3.0 - 4.0 * h), vec2(118.0, 72.0));
    let p = ui.painter();
    p.add(egui::epaint::Shadow { offset: vec2(0.0, 4.0 + 5.0 * h), blur: 14.0, spread: 0.0, color: Color32::from_black_alpha(90) }.as_shape(img, 12.0));
    match tex {
        Some(tex) => paint_cover(p, tex, img, 12.0),
        None => {
            p.rect_filled(img, 12.0, Color32::from_white_alpha(14));
            icons::plus(p, img.center(), 22.0, pal.text_dim);
        }
    }
    p.rect_stroke(img, 12.0, Stroke::new(1.0_f32, Color32::from_white_alpha(22)));
    if s > 0.01 {
        p.rect_stroke(img.expand(3.0), 15.0, Stroke::new(2.0_f32, pal.accent.gamma_multiply(s.clamp(0.0, 1.0))));
    }
    p.text(pos2(img.left() + 2.0, rect.bottom() - 7.0), Align2::LEFT_CENTER, name, FontId::proportional(12.0), lerp_color(pal.text_dim, pal.text, s.max(h)));
    resp
}
