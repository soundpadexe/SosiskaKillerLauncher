//! Procedural cover art: a blocky voxel landscape per instance.
//! Seeded by instance id, palette by era — every instance gets a unique,
//! recognisable thumbnail without shipping a single image.
use std::collections::HashMap;

use egui::{ColorImage, TextureHandle, TextureOptions};

use crate::models::version::Era;

type Rgb = [f32; 3];

struct Pal {
    sky_top: Rgb,
    sky_bot: Rgb,
    far: Rgb,
    mid: Rgb,
    grass: Rgb,
    dirt: Rgb,
    sun: Rgb,
    leaves: Rgb,
}

fn palette(era: Era) -> Pal {
    match era {
        Era::Release => Pal {
            sky_top: [74.0, 118.0, 168.0], sky_bot: [186.0, 206.0, 214.0], far: [118.0, 146.0, 168.0], mid: [78.0, 112.0, 106.0],
            grass: [104.0, 150.0, 84.0], dirt: [112.0, 82.0, 58.0], sun: [255.0, 244.0, 214.0], leaves: [62.0, 110.0, 62.0],
        },
        Era::Snapshot => Pal {
            sky_top: [52.0, 46.0, 86.0], sky_bot: [222.0, 150.0, 116.0], far: [128.0, 96.0, 118.0], mid: [86.0, 70.0, 96.0],
            grass: [132.0, 120.0, 78.0], dirt: [92.0, 64.0, 58.0], sun: [255.0, 214.0, 160.0], leaves: [92.0, 84.0, 60.0],
        },
        Era::Beta | Era::Alpha => Pal {
            sky_top: [112.0, 160.0, 206.0], sky_bot: [236.0, 206.0, 160.0], far: [150.0, 160.0, 150.0], mid: [110.0, 128.0, 96.0],
            grass: [128.0, 160.0, 72.0], dirt: [128.0, 92.0, 60.0], sun: [255.0, 236.0, 190.0], leaves: [80.0, 120.0, 52.0],
        },
        Era::Infdev | Era::Indev => Pal {
            sky_top: [14.0, 18.0, 40.0], sky_bot: [56.0, 58.0, 98.0], far: [44.0, 50.0, 84.0], mid: [32.0, 38.0, 62.0],
            grass: [52.0, 82.0, 66.0], dirt: [48.0, 40.0, 46.0], sun: [226.0, 230.0, 240.0], leaves: [34.0, 58.0, 48.0],
        },
        Era::Classic | Era::PreClassic => Pal {
            sky_top: [120.0, 126.0, 134.0], sky_bot: [194.0, 194.0, 190.0], far: [150.0, 152.0, 152.0], mid: [118.0, 120.0, 122.0],
            grass: [112.0, 132.0, 96.0], dirt: [104.0, 104.0, 104.0], sun: [236.0, 236.0, 230.0], leaves: [92.0, 112.0, 82.0],
        },
    }
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn hash(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn rnd(seed: u64, i: u64) -> f32 {
    (hash(seed ^ hash(i)) >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth 1-D value noise in [0,1].
fn noise(seed: u64, x: f32) -> f32 {
    let i = x.floor();
    let f = x - i;
    let s = f * f * (3.0 - 2.0 * f);
    let a = rnd(seed, i as i64 as u64);
    let b = rnd(seed, (i as i64 + 1) as u64);
    a + (b - a) * s
}

pub fn render(seed: u64, era: Era, w: usize, h: usize) -> ColorImage {
    let p = palette(era);
    let b = (h / 13).max(4); // block size in px
    let cols = w / b + 2;
    let rows = h / b + 1;
    let heights = |layer: u64, base: f32, amp: f32, freq: f32| -> Vec<usize> {
        (0..cols).map(|c| (base + amp * noise(seed.wrapping_add(layer * 7919), c as f32 * freq)) as usize).collect()
    };
    let far = heights(1, rows as f32 * 0.42, rows as f32 * 0.22, 0.18);
    let mid = heights(2, rows as f32 * 0.30, rows as f32 * 0.2, 0.25);
    let near = heights(3, rows as f32 * 0.16, rows as f32 * 0.16, 0.33);
    let sun_c = (((0.18 + rnd(seed, 99) * 0.6) * w as f32) as usize / b, rows / 4);
    let trees: Vec<usize> = (0..cols).filter(|c| rnd(seed, 1000 + *c as u64) < 0.09 && *c > 1 && *c + 2 < cols).collect();

    let mut px = vec![egui::Color32::BLACK; w * h];
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (x / b, y / b);
            let from_bottom = rows.saturating_sub(cy + 1);
            let t = y as f32 / h as f32;
            let mut c = mix(p.sky_top, p.sky_bot, t.powf(0.8));

            // Square sun / moon
            if cx.abs_diff(sun_c.0) <= 1 && cy.abs_diff(sun_c.1) <= 1 {
                c = mix(c, p.sun, 0.9);
            }
            // Blocky clouds
            let cloud_row = rows / 6;
            if (cy == cloud_row || cy == cloud_row + 1) && noise(seed ^ 0xC10D, cx as f32 * 0.35) > 0.62 + 0.08 * (cy - cloud_row) as f32 {
                c = mix(c, [250.0, 250.0, 250.0], 0.55);
            }
            if from_bottom < far[cx] {
                c = mix(p.far, p.sky_bot, 0.35);
            }
            if from_bottom < mid[cx] {
                c = p.mid;
            }
            // Trees on the near layer
            for &tc in &trees {
                let ground = near[tc];
                if cx == tc && from_bottom >= ground && from_bottom < ground + 3 {
                    c = mix(p.dirt, [0.0, 0.0, 0.0], 0.25);
                }
                if cx.abs_diff(tc) <= 1 && from_bottom >= ground + 2 && from_bottom < ground + 5 && !(cx != tc && from_bottom == ground + 4) {
                    c = p.leaves;
                }
            }
            if from_bottom < near[cx] {
                c = if from_bottom + 1 == near[cx] { p.grass } else { p.dirt };
                // per-block shade variation + top highlight
                let v = rnd(seed ^ 0xB10C, (cx * 977 + cy) as u64) * 0.12 - 0.06;
                c = c.map(|k| k * (1.0 + v));
                if y % b == 0 {
                    c = c.map(|k| k * 1.08);
                }
            }
            // Subtle vignette
            let vx = (x as f32 / w as f32 - 0.5) * 2.0;
            let vy = (y as f32 / h as f32 - 0.5) * 2.0;
            let vig = 1.0 - 0.18 * (vx * vx + vy * vy);
            px[y * w + x] = egui::Color32::from_rgb(
                (c[0] * vig).clamp(0.0, 255.0) as u8,
                (c[1] * vig).clamp(0.0, 255.0) as u8,
                (c[2] * vig).clamp(0.0, 255.0) as u8,
            );
        }
    }
    ColorImage { size: [w, h], pixels: px }
}

pub fn seed_of(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

pub enum Slot {
    Loading,
    Ready(TextureHandle),
    Failed,
}

/// Cover textures. Real artwork (per Minecraft version, see `api::art`) is loaded
/// in the background; until then a procedural placeholder is shown.
pub struct Covers {
    cache: HashMap<(String, u8, usize), TextureHandle>,
    real: HashMap<String, Slot>,
    wanted: Vec<String>,
    pub tx: std::sync::mpsc::Sender<(String, Option<ColorImage>)>,
    rx: std::sync::mpsc::Receiver<(String, Option<ColorImage>)>,
}

impl Default for Covers {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self { cache: HashMap::new(), real: HashMap::new(), wanted: Vec::new(), tx, rx }
    }
}

impl Covers {
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((version, img)) = self.rx.try_recv() {
            let slot = match img {
                Some(i) => Slot::Ready(ctx.load_texture(format!("art_{version}"), i, TextureOptions::LINEAR)),
                None => Slot::Failed,
            };
            self.real.insert(version, slot);
        }
    }

    pub fn take_wanted(&mut self) -> Vec<String> {
        std::mem::take(&mut self.wanted)
    }

    /// Re-queue a version whose art could not be resolved yet (e.g. manifest still loading).
    pub fn retry(&mut self, version: &str) {
        self.real.remove(version);
    }

    /// Real art for a version if ready; returns (texture, is_real).
    fn real(&mut self, version: &str) -> Option<TextureHandle> {
        match self.real.get(version) {
            Some(Slot::Ready(t)) => Some(t.clone()),
            Some(_) => None,
            None => {
                self.real.insert(version.to_owned(), Slot::Loading);
                self.wanted.push(version.to_owned());
                None
            }
        }
    }

    pub fn get(&mut self, ctx: &egui::Context, key: &str, version: &str, era: Era) -> TextureHandle {
        self.real(version).unwrap_or_else(|| self.get_sized(ctx, key, era, 480, 300))
    }

    pub fn get_wide(&mut self, ctx: &egui::Context, key: &str, version: &str, era: Era) -> TextureHandle {
        self.real(version).unwrap_or_else(|| self.get_sized(ctx, key, era, 1280, 340))
    }

    /// Wide banners get their own render so blocks keep their size instead of being cropped huge.
    pub fn get_sized(&mut self, ctx: &egui::Context, key: &str, era: Era, w: usize, h: usize) -> TextureHandle {
        let k = (key.to_owned(), era as u8, w);
        self.cache
            .entry(k)
            .or_insert_with(|| ctx.load_texture(format!("cover_{key}_{w}"), render(seed_of(key), era, w, h), TextureOptions::NEAREST))
            .clone()
    }
}
