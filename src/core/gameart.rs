//! Real Minecraft artwork for instance covers, read at runtime from the
//! client jar the user already downloaded (nothing is bundled in the exe):
//! * the version's own title-screen panorama, when it has one;
//! * otherwise a landscape built from that version's real block textures.
use std::io::Read;
use std::path::Path;

use image::{imageops, RgbaImage};

const PANORAMAS: &[&str] = &[
    "assets/minecraft/textures/gui/title/background/panorama_0.png",
    "title/bg/panorama0.png",
];

/// (grass side, dirt, stone) in the layouts used across eras.
const BLOCK_SETS: &[[&str; 3]] = &[
    ["assets/minecraft/textures/block/grass_block_side.png", "assets/minecraft/textures/block/dirt.png", "assets/minecraft/textures/block/stone.png"],
    ["assets/minecraft/textures/blocks/grass_side.png", "assets/minecraft/textures/blocks/dirt.png", "assets/minecraft/textures/blocks/stone.png"],
    ["textures/blocks/grass_side.png", "textures/blocks/dirt.png", "textures/blocks/stone.png"],
];

fn read(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<RgbaImage> {
    let mut e = zip.by_name(name).ok()?;
    let mut buf = Vec::with_capacity(e.size() as usize);
    e.read_to_end(&mut buf).ok()?;
    Some(image::load_from_memory(&buf).ok()?.to_rgba8())
}

/// First 16×16 frame (animated textures are vertical strips).
fn tile(img: &RgbaImage) -> RgbaImage {
    let w = img.width().min(img.height());
    imageops::crop_imm(img, 0, 0, w, w).to_image()
}

pub fn shrink(img: RgbaImage) -> RgbaImage {
    if img.width() > 1024 { imageops::resize(&img, 1024, 1024 * img.height() / img.width(), imageops::FilterType::Triangle) } else { img }
}

/// Panorama stored in the jar itself (≤1.12). Modern jars only hold 1×1 placeholders.
pub fn jar_panorama(jar: &Path) -> Option<RgbaImage> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
    PANORAMAS.iter().find_map(|n| read(&mut zip, n)).filter(|i| i.width() >= 64).map(shrink)
}

/// Landscape from the jar's real block textures (works for every era).
pub fn extract(jar: &Path, seed: u64) -> Option<RgbaImage> {
    if let Some(p) = jar_panorama(jar) {
        return Some(p);
    }
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
    let blocks = BLOCK_SETS
        .iter()
        .find_map(|set| Some([read(&mut zip, set[0])?, read(&mut zip, set[1])?, read(&mut zip, set[2])?].map(|t| tile(&t))))
        .or_else(|| {
            // ≤1.4: one 256×256 atlas, 16 px tiles: stone (1,0), dirt (2,0), grass side (3,0)
            let atlas = read(&mut zip, "terrain.png")?;
            let t = atlas.width() / 16;
            let cut = |c: u32| imageops::crop_imm(&atlas, c * t, 0, t, t).to_image();
            Some([cut(3), cut(2), cut(1)])
        })?;
    Some(landscape(&blocks, seed))
}

fn hash(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB) ^ (x >> 31)
}

/// Rolling hills of real block textures under a soft sky.
fn landscape(blocks: &[RgbaImage; 3], seed: u64) -> RgbaImage {
    let (cols, rows, t) = (30u32, 17u32, 16u32);
    let mut out = RgbaImage::new(cols * t, rows * t);
    for y in 0..out.height() {
        let k = y as f32 / out.height() as f32;
        let sky = [96.0 + 90.0 * k, 146.0 + 70.0 * k, 214.0 + 20.0 * k];
        for x in 0..out.width() {
            out.put_pixel(x, y, image::Rgba([sky[0] as u8, sky[1] as u8, sky[2] as u8, 255]));
        }
    }
    let mut h = 7i32;
    for c in 0..cols {
        let r = hash(seed ^ c as u64) % 6;
        h = (h + match r { 0 => -1, 1 => 1, _ => 0 }).clamp(4, 10);
        for row in 0..rows as i32 {
            let from_bottom = rows as i32 - 1 - row;
            if from_bottom > h {
                continue;
            }
            let tex = if from_bottom == h { &blocks[0] } else if from_bottom > h - 3 { &blocks[1] } else { &blocks[2] };
            let scaled = imageops::resize(tex, t, t, imageops::FilterType::Nearest);
            imageops::overlay(&mut out, &scaled, (c * t) as i64, (row as u32 * t) as i64);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    /// `SKL_ART_OUT=<dir> cargo test -- --ignored art_from_jars --nocapture`
    #[test]
    #[ignore]
    fn art_from_jars() {
        let out = std::path::PathBuf::from(std::env::var("SKL_ART_OUT").unwrap());
        let paths = crate::core::paths::Paths::init().unwrap();
        for e in std::fs::read_dir(paths.versions()).unwrap().flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            let jar = e.path().join(format!("{id}.jar"));
            if !jar.exists() {
                continue;
            }
            match super::extract(&jar, 7) {
                Some(img) => {
                    println!("{id}: {}x{}", img.width(), img.height());
                    img.save(out.join(format!("art_{id}.png"))).unwrap();
                }
                None => println!("{id}: no art"),
            }
        }
    }
}
