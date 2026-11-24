//! Local skin & cape files (`<data>/skins/`). Validated on import.
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use sha2::Digest;

use super::paths::Paths;

pub fn dir(paths: &Paths) -> PathBuf {
    paths.root().join("skins")
}


fn decode(bytes: Vec<u8>) -> anyhow::Result<(Vec<u8>, image::RgbaImage)> {
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).context("not a PNG image")?.to_rgba8();
    Ok((bytes, img))
}

/// Accepts modern 64×64 and legacy 64×32 skins. Returns whether the skin looks slim (3px arms).
pub fn import_skin(paths: &Paths, src: &Path) -> anyhow::Result<bool> {
    import_skin_bytes(paths, std::fs::read(src)?)
}

pub fn import_skin_bytes(paths: &Paths, raw: Vec<u8>) -> anyhow::Result<bool> {
    let (bytes, img) = decode(raw)?;
    if !matches!(img.dimensions(), (64, 64) | (64, 32)) {
        bail!("skin must be 64×64 or 64×32, got {}×{}", img.width(), img.height());
    }
    std::fs::create_dir_all(dir(paths))?;
    std::fs::write(dir(paths).join("skin.png"), bytes)?;
    Ok(detect_slim(&img))
}

/// Standard capes are 64×32 (or HD multiples); very old ones 22×17.
pub fn import_cape(paths: &Paths, src: &Path) -> anyhow::Result<()> {
    import_cape_bytes(paths, std::fs::read(src)?)
}

pub fn import_cape_bytes(paths: &Paths, raw: Vec<u8>) -> anyhow::Result<()> {
    let (bytes, img) = decode(raw)?;
    let (w, h) = img.dimensions();
    if !((w % 64 == 0 && h * 2 == w) || (w, h) == (22, 17)) {
        bail!("cape must be 64×32 (or a multiple), got {w}×{h}");
    }
    std::fs::create_dir_all(dir(paths))?;
    std::fs::write(dir(paths).join("cape.png"), bytes)?;
    Ok(())
}

pub fn clear(paths: &Paths, which: &str) {
    let _ = std::fs::remove_file(dir(paths).join(format!("{which}.png")));
}

pub fn read(paths: &Paths, which: &str) -> Option<Vec<u8>> {
    std::fs::read(dir(paths).join(format!("{which}.png"))).ok()
}

/// Slim ("Alex") skins leave the 4th arm column transparent.
pub fn detect_slim(img: &image::RgbaImage) -> bool {
    img.height() == 64 && (20..32).all(|y| img.get_pixel(54, y)[3] == 0 && img.get_pixel(55, y)[3] == 0)
}

/// Pre-1.8 clients read only the top 64×32 half.
pub fn legacy_crop(png: &[u8]) -> Vec<u8> {
    let Ok(img) = image::load_from_memory(png) else { return png.to_vec() };
    if img.height() <= 32 {
        return png.to_vec();
    }
    let top = img.crop_imm(0, 0, 64, 32);
    let mut out = std::io::Cursor::new(Vec::new());
    if top.write_to(&mut out, image::ImageFormat::Png).is_ok() { out.into_inner() } else { png.to_vec() }
}

pub fn hash(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn slim_detection_and_crop() {
        let mut img = image::RgbaImage::from_pixel(64, 64, image::Rgba([200, 100, 50, 255]));
        assert!(!super::detect_slim(&img));
        for y in 20..32 {
            img.put_pixel(54, y, image::Rgba([0, 0, 0, 0]));
            img.put_pixel(55, y, image::Rgba([0, 0, 0, 0]));
        }
        assert!(super::detect_slim(&img));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let cropped = image::load_from_memory(&super::legacy_crop(&png.into_inner())).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (64, 32));
    }
}
