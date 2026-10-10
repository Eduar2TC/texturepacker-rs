//! Utilidades para pruebas headless: generación de un proyecto de ejemplo
//! (sprites PNG + `.tpproj`) que usan el autotest `tp-smoke` y los tests de
//! integración que conducen la app real; y el bombeo de frames **como lo
//! haría la ventana** (solo cuando egui pide repaint), que es lo que
//! destapa fallos de programación del preview que los bucles manuales
//! enmascaran.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::App;

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

/// Resultado del bombeo bajo demanda: `stuck` = la app dejó de pedir
/// repaints (la ventana real se quedaría quieta con la UI sin actualizar).
#[derive(Debug, Clone, Copy)]
pub struct PumpOutcome {
    pub frames: usize,
    pub stuck: bool,
    pub idle_frames: usize,
}

/// Input de un frame con la geometría de la ventana nativa.
pub fn idle_input() -> eframe::egui::RawInput {
    eframe::egui::RawInput {
        screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(1360.0, 860.0),
        )),
        time: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        ),
        ..eframe::egui::RawInput::default()
    }
}

/// Conduce la app como lo haría eframe: un frame solo cuando egui pide
/// repaint (`set_request_repaint_callback`), durmiendo el delay pedido.
/// Devuelve pronto cuando `stop` se cumple (típicamente `app.result().is_some()`).
///
/// El predicado recibe `&mut App` para que también pueda mirar métodos que
/// recorren el disco y actualizan el registro (`snapshot_changed`), no sólo
/// campos.
///
/// A diferencia de los bucles manuales («run_frame + sleep(10 ms)»), este
/// bombeo **no** mantiene viva a la app por su cuenta: si la app no
/// programa repaints, el bucle se para y `stuck` queda en `true`.
pub fn pump_on_demand(
    app: &mut App,
    ctx: &eframe::egui::Context,
    stop: impl Fn(&mut App) -> bool,
    max: Duration,
) -> PumpOutcome {
    let pending: Arc<Mutex<Option<Duration>>> = Arc::new(Mutex::new(None));
    let slot = pending.clone();
    ctx.set_request_repaint_callback(move |info| {
        let mut g = slot.lock().unwrap();
        *g = Some(match *g {
            Some(prev) => prev.min(info.delay),
            None => info.delay,
        });
    });

    let deadline = std::time::Instant::now() + max;
    let mut frames = 0usize;
    let mut idle = 0usize;
    loop {
        if stop(app) {
            return PumpOutcome {
                frames,
                stuck: false,
                idle_frames: idle,
            };
        }
        if std::time::Instant::now() > deadline {
            return PumpOutcome {
                frames,
                stuck: true,
                idle_frames: idle,
            };
        }
        *pending.lock().unwrap() = None;
        let _ = app.run_frame(ctx, idle_input());
        frames += 1;
        let wait = pending.lock().unwrap().take();
        match wait {
            Some(delay) => {
                idle = 0;
                std::thread::sleep(delay.max(Duration::from_millis(1)));
            }
            None => {
                idle += 1;
                if idle >= 3 {
                    return PumpOutcome {
                        frames,
                        stuck: true,
                        idle_frames: idle,
                    };
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if std::time::Instant::now() > deadline {
            return PumpOutcome {
                frames,
                stuck: true,
                idle_frames: idle,
            };
        }
    }
}

/// Texto que un frame dejó pintado. Las ventanas flotantes no guardan
/// estado que leer —lo único que hacen es pintar—, así que las pruebas
/// miran las formas que egui devuelve en el `FullOutput`. Los contenedores
/// anidan formas dentro de formas, de ahí el barrido recursivo.
pub fn texto_pintado(output: &eframe::egui::FullOutput) -> String {
    let mut texto = String::new();
    for (t, _, _) in textos_pintados(output) {
        texto.push_str(&t);
        texto.push('\n');
    }
    texto
}

/// Cada texto que un frame dejó pintado, con su rectángulo en pantalla y
/// el recorte que lo limita: `(texto, rectángulo, recorte)`.
///
/// Hace falta cuando lo que se prueba es de geometría. Un botón que egui
/// empuja fuera de su panel sigue *pintado* —`texto_pintado` lo daría por
/// bueno—, pero el usuario no lo ve: sólo se descubre comparando su
/// rectángulo con el recorte que el `ClippedShape` le asigna. Los hijos de
/// una forma enmascarada (`Shape::Vec`) heredan su recorte.
pub fn textos_pintados(
    output: &eframe::egui::FullOutput,
) -> Vec<(String, eframe::egui::Rect, eframe::egui::Rect)> {
    fn recorre<'a>(
        shapes: impl IntoIterator<Item = &'a eframe::egui::Shape>,
        recorte: eframe::egui::Rect,
        fuera: &mut Vec<(String, eframe::egui::Rect, eframe::egui::Rect)>,
    ) {
        for shape in shapes {
            match shape {
                eframe::egui::Shape::Text(t) => {
                    fuera.push((t.galley.job.text.clone(), t.visual_bounding_rect(), recorte))
                }
                eframe::egui::Shape::Vec(hijos) => recorre(hijos, recorte, fuera),
                _ => {}
            }
        }
    }
    let mut fuera = Vec::new();
    for forma in &output.shapes {
        recorre([&forma.shape], forma.clip_rect, &mut fuera);
    }
    fuera
}

/// Cada rectángulo relleno que un frame dejó pintado, con su color y su
/// rectángulo: `(relleno, rectángulo)`.
///
/// Es la misma inspección que [`textos_pintados`] pero para el **fondo**:
/// los paneles de la ventana no dejan estado que leer, sólo formas, y es
/// por los rellenos por los que se distingue una zona de otra.
pub fn rellenos_pintados(
    output: &eframe::egui::FullOutput,
) -> Vec<(eframe::egui::Color32, eframe::egui::Rect)> {
    fn recorre<'a>(
        shapes: impl IntoIterator<Item = &'a eframe::egui::Shape>,
        fuera: &mut Vec<(eframe::egui::Color32, eframe::egui::Rect)>,
    ) {
        for shape in shapes {
            match shape {
                eframe::egui::Shape::Rect(r) => fuera.push((r.fill, r.rect)),
                eframe::egui::Shape::Vec(hijos) => recorre(hijos, fuera),
                _ => {}
            }
        }
    }
    let mut fuera = Vec::new();
    for forma in &output.shapes {
        recorre([&forma.shape], &mut fuera);
    }
    fuera
}

/// Cada texto que un frame dejó pintado, con la caja del galión entero
/// (la caja de línea) en vez de con la tinta.
///
/// [`textos_pintados`] devuelve `visual_bounding_rect`, que es la tinta:
/// sirve para mirar si algo se ve, pero no para medir. La tinta empieza
/// donde el glifo decide —una «ó» con tilde sube más que una «D», y una
/// «j» baja más—, así que dos rótulos del mismo cuerpo no arrancan a la
/// misma altura y una diferencia de 2 px puede no ser nada. La caja del
/// galión sí mide lo que egui reservó para la línea, y es con lo que se
/// comprueban las alturas de fila y los márgenes de la rejilla (4.3).
pub fn galones_pintados(output: &eframe::egui::FullOutput) -> Vec<(String, eframe::egui::Rect)> {
    fn recorre<'a>(
        shapes: impl IntoIterator<Item = &'a eframe::egui::Shape>,
        fuera: &mut Vec<(String, eframe::egui::Rect)>,
    ) {
        for shape in shapes {
            match shape {
                eframe::egui::Shape::Text(t) => {
                    // `galley.rect` está en el sistema del galión y `pos`
                    // es su origen en pantalla: sin traducir, todos los
                    // rótulos parecen estar en la esquina.
                    fuera.push((
                        t.galley.job.text.clone(),
                        t.galley.rect.translate(t.pos.to_vec2()),
                    ))
                }
                eframe::egui::Shape::Vec(hijos) => recorre(hijos, fuera),
                _ => {}
            }
        }
    }
    let mut fuera = Vec::new();
    for forma in &output.shapes {
        recorre([&forma.shape], &mut fuera);
    }
    fuera
}

/// La caja del control que pinta `glifo`, una por aparición en pantalla.
///
/// La caja de un botón no es texto sino una forma, y egui no devuelve el
/// rectángulo de un widget: lo que sí deja una salida son los glifos de
/// sus rótulos —[`textos_pintados`]— y, pintados a su alrededor, los
/// rellenos de su frame —[`rellenos_pintados`]—. El relleno más pequeño
/// que contiene al glifo es el propio control; el panel que también lo
/// contiene es más grande y, por serlo, pierde.
///
/// Es con lo que se mide si un control alcanza su objetivo (4.3): la
/// medida sale del frame de verdad, sin confiar en el fuente.
pub fn controles_de(output: &eframe::egui::FullOutput, glifo: &str) -> Vec<eframe::egui::Rect> {
    let centros: Vec<_> = textos_pintados(output)
        .iter()
        .filter(|(texto, _, _)| texto.trim() == glifo)
        .map(|(_, rect, _)| rect.center())
        .collect();
    let rellenos: Vec<_> = rellenos_pintados(output)
        .into_iter()
        .map(|(_, rect)| rect)
        .collect();
    centros
        .iter()
        .filter_map(|centro| {
            rellenos
                .iter()
                .filter(|r| r.contains(*centro))
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .copied()
        })
        .collect()
}
