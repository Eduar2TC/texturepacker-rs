//! TexturePacker-RS desktop application (egui/eframe).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

/// Icono de ventana (empotrado en el binario, decodificado al arrancar).
/// El PNG maestro vive en `packaging/` y se regenera con
/// `python3 packaging/gen_icon.py`.
const ICON_PNG: &[u8] = include_bytes!("../../../packaging/icon.png");

fn window_icon() -> Option<eframe::egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?.to_rgba8();
    Some(eframe::egui::IconData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

fn main() -> eframe::Result {
    let project = std::env::args().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("TexturePacker-RS")
            .with_icon(window_icon().expect("icono de ventana embebido")),
        ..Default::default()
    };
    eframe::run_native(
        "TexturePacker-RS",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, project)))),
    )
}
