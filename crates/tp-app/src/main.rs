//! TexturePacker-RS desktop application (egui/eframe).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("TexturePacker-RS"),
        ..Default::default()
    };
    eframe::run_native(
        "TexturePacker-RS",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
