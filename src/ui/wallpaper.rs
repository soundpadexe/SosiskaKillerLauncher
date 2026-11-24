use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use egui::{epaint::RectShape, pos2, Color32, ColorImage, Rect, Rounding, TextureHandle, TextureOptions};
use image::RgbaImage;

use crate::core::settings::WallpaperSource;

pub const BUILTIN_NAMES: [&str; 3] = ["Graphite", "Dusk", "Moss"];
const MAX_SIDE: u32 = 1920;

struct JobResult {
    source: Arc<RgbaImage>,
    blurred: ColorImage,
    thumb: Option<ColorImage>,
}

struct Job {
    key: WallpaperSource,
    sigma: f32,
    rx: Receiver<JobResult>,
}

/// Wallpaper with off-thread decode + Gaussian blur. The blurred result is
/// uploaded once as a GPU texture; while a new blur is computing the old
/// texture keeps rendering, so the slider never stalls the UI.
pub struct Wallpaper {
    key: Option<WallpaperSource>,
    sigma: f32,
    source: Option<Arc<RgbaImage>>,
    texture: Option<TextureHandle>,
    job: Option<Job>,
    thumbs: Vec<TextureHandle>,
    custom_thumb: Option<TextureHandle>,
}

impl Default for Wallpaper {
    fn default() -> Self {
        Self { key: None, sigma: -1.0, source: None, texture: None, job: None, thumbs: Vec::new(), custom_thumb: None }
    }
}

impl Wallpaper {
    pub fn update(&mut self, ctx: &egui::Context, want: &WallpaperSource, want_sigma: f32) {
        if let Some(job) = self.job.take() {
            match job.rx.try_recv() {
                Ok(res) => {
                    match &mut self.texture {
                        Some(tex) => tex.set(res.blurred, TextureOptions::LINEAR),
                        None => self.texture = Some(ctx.load_texture("wallpaper", res.blurred, TextureOptions::LINEAR)),
                    }
                    if let Some(th) = res.thumb {
                        self.custom_thumb = Some(ctx.load_texture("wallpaper_custom_thumb", th, TextureOptions::LINEAR));
                    }
                    self.source = Some(res.source);
                    self.key = Some(job.key);
                    self.sigma = job.sigma;
                }
                Err(TryRecvError::Empty) => self.job = Some(job),
                Err(TryRecvError::Disconnected) => {}
            }
        }

        let stale = self.key.as_ref() != Some(want) || (self.sigma - want_sigma).abs() > 0.05;
        if self.job.is_none() && stale {
            let reuse = (self.key.as_ref() == Some(want)).then(|| self.source.clone()).flatten();
            let (tx, rx) = mpsc::channel();
            let key = want.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let fresh = reuse.is_none();
                let source = reuse.unwrap_or_else(|| Arc::new(load_source(&key)));
                let thumb = (fresh && matches!(key, WallpaperSource::Custom(_))).then(|| {
                    let t = image::imageops::thumbnail(&*source, 240, (240 * source.height() / source.width().max(1)).max(1));
                    ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw())
                });
                let blurred = blur(&source, want_sigma);
                let _ = tx.send(JobResult { source, blurred, thumb });
                ctx.request_repaint();
            });
            self.job = Some(Job { key: want.clone(), sigma: want_sigma, rx });
        }
    }

    /// Paints the wallpaper with "cover" fit into `rect`.
    pub fn paint(&self, painter: &egui::Painter, rect: Rect, rounding: Rounding, dim: f32) {
        match &self.texture {
            Some(tex) => {
                let [tw, th] = tex.size();
                let ta = tw as f32 / th as f32;
                let ra = rect.width() / rect.height();
                let uv = if ra > ta {
                    let h = ta / ra;
                    Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
                } else {
                    let w = ra / ta;
                    Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
                };
                let mut shape = RectShape::filled(rect, rounding, Color32::WHITE);
                shape.fill_texture_id = tex.id();
                shape.uv = uv;
                painter.add(shape);
            }
            None => {
                painter.rect_filled(rect, rounding, Color32::from_rgb(14, 15, 18));
            }
        }
        if dim > 0.0 {
            painter.rect_filled(rect, rounding, Color32::from_black_alpha((dim.clamp(0.0, 1.0) * 255.0) as u8));
        }
    }
}

fn load_source(src: &WallpaperSource) -> RgbaImage {
    match src {
        WallpaperSource::Builtin(i) => builtin(*i),
        WallpaperSource::Custom(path) => image::open(path)
            .map(|img| {
                let img = if img.width().max(img.height()) > MAX_SIDE {
                    img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
                } else {
                    img
                };
                img.to_rgba8()
            })
            .unwrap_or_else(|_| builtin(0)),
    }
}

fn blur(src: &RgbaImage, sigma: f32) -> ColorImage {
    let size = [src.width() as usize, src.height() as usize];
    if sigma < 0.5 {
        return ColorImage::from_rgba_unmultiplied(size, src.as_raw());
    }
    let out = image::imageops::fast_blur(src, sigma);
    ColorImage::from_rgba_unmultiplied(size, out.as_raw())
}

/// Procedural built-in wallpapers: deep base + two soft, desaturated light pools.
/// Generated instead of embedded to keep the .exe tiny; dithered to avoid banding.
pub fn builtin(index: u8) -> RgbaImage {
    builtin_sized(index, 1600, 1000)
}

pub fn builtin_sized(index: u8, w: u32, h: u32) -> RgbaImage {
    let (base, a, b): ([f32; 3], [f32; 3], [f32; 3]) = match index % 3 {
        0 => ([13.0, 14.0, 17.0], [58.0, 66.0, 86.0], [44.0, 38.0, 54.0]),
        1 => ([16.0, 13.0, 19.0], [92.0, 62.0, 78.0], [42.0, 52.0, 82.0]),
        _ => ([14.0, 16.0, 14.0], [58.0, 74.0, 60.0], [70.0, 66.0, 52.0]),
    };
    RgbaImage::from_fn(w, h, |x, y| {
        let fx = x as f32 / w as f32;
        let fy = y as f32 / h as f32;
        let glow = |cx: f32, cy: f32, r: f32| {
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2) * 0.6).sqrt();
            (1.0 - d / r).clamp(0.0, 1.0).powf(2.2)
        };
        let ga = glow(0.18, 0.2, 0.75);
        let gb = glow(0.88, 0.85, 0.8);
        // Cheap hash dither (±1 LSB) to kill 8-bit banding in smooth gradients.
        let n = ((x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 1000) as f32 / 1000.0 - 0.5;
        let c = |i: usize| (base[i] + a[i] * ga + b[i] * gb + n).clamp(0.0, 255.0) as u8;
        image::Rgba([c(0), c(1), c(2), 255])
    })
}

impl Wallpaper {
    pub fn ensure_thumbs(&mut self, ctx: &egui::Context) {
        if self.thumbs.is_empty() {
            self.thumbs = (0..BUILTIN_NAMES.len() as u8)
                .map(|i| {
                    let img = builtin_sized(i, 240, 150);
                    let ci = ColorImage::from_rgba_unmultiplied([240, 150], img.as_raw());
                    ctx.load_texture(format!("wp_thumb_{i}"), ci, TextureOptions::LINEAR)
                })
                .collect();
        }
    }

    pub fn thumb(&self, i: usize) -> Option<&TextureHandle> {
        self.thumbs.get(i)
    }

    pub fn custom_thumb(&self) -> Option<&TextureHandle> {
        self.custom_thumb.as_ref()
    }
}

/// Paint a texture with "cover" fit and rounded corners.
pub fn paint_cover(painter: &egui::Painter, tex: &TextureHandle, rect: Rect, rounding: impl Into<Rounding>) {
    let [tw, th] = tex.size();
    let ta = tw as f32 / th as f32;
    let ra = rect.width() / rect.height();
    let uv = if ra > ta {
        let h = ta / ra;
        Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
    } else {
        let w = ra / ta;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    };
    let mut shape = RectShape::filled(rect, rounding, Color32::WHITE);
    shape.fill_texture_id = tex.id();
    shape.uv = uv;
    painter.add(shape);
}

/// Cover fit with extra zoom (≥1, for hover push-in) and an alpha tint (for crossfades).
pub fn paint_cover_ex(painter: &egui::Painter, tex: &TextureHandle, rect: Rect, rounding: impl Into<Rounding>, zoom: f32, alpha: f32) {
    let [tw, th] = tex.size();
    let ta = tw as f32 / th as f32;
    let ra = rect.width() / rect.height();
    let (mut w, mut h) = if ra > ta { (1.0, ta / ra) } else { (ra / ta, 1.0) };
    w /= zoom.max(1.0);
    h /= zoom.max(1.0);
    let uv = Rect::from_center_size(pos2(0.5, 0.5), egui::vec2(w, h));
    let mut shape = RectShape::filled(rect, rounding, Color32::WHITE.gamma_multiply(alpha.clamp(0.0, 1.0)));
    shape.fill_texture_id = tex.id();
    shape.uv = uv;
    painter.add(shape);
}

/// Like `paint_cover_ex`, but the visible window wanders slowly over the image
/// (Lissajous path inside the zoom margin) — a calm Ken Burns effect.
pub fn paint_cover_drift(painter: &egui::Painter, tex: &TextureHandle, rect: Rect, rounding: impl Into<Rounding>, zoom: f32, alpha: f32, t: f32) {
    let [tw, th] = tex.size();
    let ta = tw as f32 / th as f32;
    let ra = rect.width() / rect.height();
    let (w0, h0) = if ra > ta { (1.0, ta / ra) } else { (ra / ta, 1.0) };
    let (w, h) = (w0 / zoom.max(1.0), h0 / zoom.max(1.0));
    // Wander only within the zoom margin, so the framing stays the same.
    let (mx, my) = ((w0 - w) / 2.0, (h0 - h) / 2.0);
    let c = pos2(0.5 + mx * 0.9 * (t * 0.045).sin(), 0.5 + my * 0.9 * (t * 0.031 + 1.3).sin());
    let mut shape = RectShape::filled(rect, rounding, Color32::WHITE.gamma_multiply(alpha.clamp(0.0, 1.0)));
    shape.fill_texture_id = tex.id();
    shape.uv = Rect::from_center_size(c, egui::vec2(w, h));
    painter.add(shape);
}
