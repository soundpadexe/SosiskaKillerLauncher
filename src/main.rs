#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app;
mod core;
mod icon;
mod models;
mod ui;

fn main() -> eframe::Result<()> {
    let icon = egui::IconData { rgba: icon::render(256), width: 256, height: 256 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SosiskaKillerLauncher")
            .with_icon(icon)
            .with_inner_size([1180.0, 740.0])
            .with_min_inner_size([960.0, 600.0])
            .with_decorations(false)
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "SosiskaKillerLauncher",
        options,
        Box::new(|cc| Ok(Box::new(app::SklApp::new(cc)))),
    )
}
