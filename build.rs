#[path = "src/icon.rs"]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("skl.ico");
    let frames: Vec<_> = [256u32, 64, 48, 32, 24, 16]
        .iter()
        .map(|&s| image::codecs::ico::IcoFrame::as_png(&icon::render(s), s, s, image::ExtendedColorType::Rgba8).unwrap())
        .collect();
    image::codecs::ico::IcoEncoder::new(std::fs::File::create(&out).unwrap())
        .encode_images(&frames)
        .unwrap();

    let mut res = winresource::WindowsResource::new();
    res.set_icon(out.to_str().unwrap())
        .set("ProductName", "SosiskaKillerLauncher")
        .set("FileDescription", "SosiskaKillerLauncher")
        .set("CompanyName", "SKL");
    res.compile().expect("failed to embed Windows resources");
}
