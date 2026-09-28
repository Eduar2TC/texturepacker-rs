//! Utilidades para pruebas headless: generación de un proyecto de ejemplo
//! (sprites PNG + `.tpproj`) que usan el autotest `tp-smoke` y los tests de
//! integración que conducen la app real.

use std::path::{Path, PathBuf};

/// Crea sprites de ejemplo en `sprites_dir` y un proyecto `demo.tpproj` en
/// `tmp` (escrito con `ProjectConfig::to_toml()`, el mismo serializador de
/// la app), con aliasing activado: dos ficheros son copias exactas, así que
/// el pack produce 2 aliases. Devuelve la ruta del `.tpproj`.
///
/// Sprites: hero 32×32, coin 16×16, bg 64×48 y sus copias `*_alt.png`.
pub fn create_example_project(tmp: &Path, sprites_dir: &Path) -> Result<PathBuf, String> {
    let mut cfg = tp_core::config::ProjectConfig {
        input_directory: sprites_dir.to_path_buf(),
        output_directory: tmp.join("out"),
        enable_aliasing: true,
        ..tp_core::config::ProjectConfig::default()
    };
    // Rejilla Manual de ejemplo: también ejercita el snap del motor.
    cfg.manual_grid = Some(tp_core::config::ManualGrid::new(16, true));
    let toml = cfg
        .to_toml()
        .map_err(|e| format!("no se pudo serializar el proyecto: {e}"))?;
    let project = tmp.join("demo.tpproj");
    std::fs::write(&project, toml)
        .map_err(|e| format!("no se pudo escribir {}: {e}", project.display()))?;

    let base = |w: u32, h: u32, rgb: [u8; 3]| {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for p in px.chunks_exact_mut(4) {
            p.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        image::RgbaImage::from_raw(w, h, px).expect("buffer cuadra con w*h")
    };
    let save = |name: &str, img: image::RgbaImage| -> Result<(), String> {
        img.save(sprites_dir.join(name))
            .map_err(|e| format!("no se pudo guardar {name}: {e}"))
    };
    save("hero.png", base(32, 32, [220, 60, 60]))?;
    save("coin.png", base(16, 16, [240, 200, 60]))?;
    save("bg.png", base(64, 48, [60, 120, 200]))?;
    // Copias exactas: el motor las detecta como aliases (mismo hash).
    save("hero_alt.png", base(32, 32, [220, 60, 60]))?;
    save("coin_alt.png", base(16, 16, [240, 200, 60]))?;
    Ok(project)
}
