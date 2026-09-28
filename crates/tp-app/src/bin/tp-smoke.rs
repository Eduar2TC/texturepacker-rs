//! Smoke-test automático de la app: compila un binario que **abre un
//! proyecto real y verifica la vista previa sin interacción humana**.
//!
//! Qué hace:
//!
//! 1. Genera en un directorio temporal sprites de ejemplo (PNG con alpha)
//!    y un proyecto `.tpproj` (escrito con `ProjectConfig::to_toml()`, el
//!    mismo serializador de la app).
//! 2. Construye la app real (`tp_app::App`) sobre un `egui::Context`
//!    headless —mismo `App::new` que usa la ventana nativa— y la **abre**
//!    con `open_project`.
//! 3. Conduce frames reales con `App::run_frame` hasta que la vista previa
//!    (el mismo pipeline `run_preview` de «Publicar») termina.
//! 4. Simula un arrastre completo con eventos de puntero reales
//!    (`RawInput`): press en la fila de `hero.png` del panel izquierdo,
//!    move en línea recta hasta el centro del lienzo y release dentro de
//!    él. Es el mismo camino que recorre un usuario: `drag_started` →
//!    payload `SpriteDrag` → fantasma → drop → repack.
//! 5. Comprueba la vista: página del atlas, relleno > 0, sprites/aliases,
//!    texturas cargadas, píxeles reales en la página, zoom de encuadre y
//!    que el Log de la app no registra errores.
//!
//! Devuelve 0 si todo pasa (o `--json` imprime un resumen legible);
//! sale con error si algún paso falla. Pensado para CI:
//! `cargo run -p tp-app --bin tp-smoke` o `cargo test --workspace` (el test
//! `tp_smoke_binary_runs` ejecuta este mismo binario).

use eframe::egui;
use std::path::Path;

fn main() {
    let json = std::env::args().any(|a| a == "--json");
    match smoke_run(json) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("tp-smoke: {e}");
            std::process::exit(1);
        }
    }
}

fn smoke_run(json: bool) -> Result<(), String> {
    let started = std::time::Instant::now();

    // 1) Proyecto de ejemplo en un directorio temporal propio del proceso.
    let tmp = std::env::temp_dir().join(format!(
        "tp_smoke_{}_{}",
        std::process::id(),
        started.elapsed().as_millis()
    ));
    let sprites = tmp.join("sprites");
    std::fs::create_dir_all(&sprites)
        .map_err(|e| format!("no se pudo crear el dir temporal {}: {e}", tmp.display()))?;
    let project = tp_app::testing::create_example_project(&tmp, &sprites)?;
    println!("proyecto de ejemplo: {}", project.display());

    // 2) App real sobre un egui::Context headless (sin ventana ni GPU).
    let ctx = egui::Context::default();
    let mut app = tp_app::App::new_for_testing(ctx.clone(), Some(project));
    println!("app construida y proyecto abierto (App::new_for_testing)");

    // 3) Conducir frames hasta que la vista previa (pipeline en un hilo)
    //    llegue y se aplique; la app real reprograma repacks ella sola.
    let mut frames = 0usize;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut first_frame = true;
    loop {
        if app.result().is_some() {
            break;
        }
        if std::time::Instant::now() > deadline {
            return Err(format!(
                "tiempo agotado: la vista previa no llegó tras {frames} frames (¿falló el pipeline?)"
            ));
        }
        let input = if first_frame {
            first_frame = false;
            wake_input()
        } else {
            default_input()
        };
        let _ = app.run_frame(&ctx, input);
        frames += 1;
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    println!("vista previa lista tras {frames} frames");

    // 4) Un par de frames más para asentar la UI (barra de estado, zoom...).
    for _ in 0..2 {
        let _ = app.run_frame(&ctx, default_input());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // 5) Arrastre REAL con eventos de puntero: press en la fila del panel
    //    izquierdo → move hacia el lienzo → release sobre el centro.
    //    Cada evento va en su propio frame (como en la ventana real): uno
    //    previo coloca el puntero sobre la fila y cada move avanza un frame
    //    con el botón ya pulsado, atravesando el umbral de click→drag.
    let hero = "hero.png";
    let row = app
        .sprite_row_rects()
        .iter()
        .find(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some(hero))
        .map(|(_, r)| *r)
        .ok_or_else(|| {
            let vistos: Vec<String> = app
                .sprite_row_rects()
                .iter()
                .filter_map(|(p, _)| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .collect();
            format!("la fila de {hero} no está registrada; filas: {vistos:?}")
        })?;
    let canvas = app
        .canvas_rect()
        .ok_or("el lienzo del atlas no se dibujó: no hay rect")?;
    if !canvas.is_positive() {
        return Err(format!("rect del lienzo inválido: {canvas:?}"));
    }
    let from = row.center();
    let to = egui::pos2(canvas.center().x, canvas.center().y);
    println!(
        "drag: fila de {hero} en {:?} → lienzo {:?}",
        (from.x, from.y),
        (to.x, to.y)
    );
    let _ = app.run_frame(&ctx, input_with(vec![egui::Event::PointerMoved(from)]));
    let _ = app.run_frame(
        &ctx,
        input_with(vec![egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        }]),
    );
    const STEPS: usize = 6;
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let p = egui::pos2(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        let _ = app.run_frame(&ctx, input_with(vec![egui::Event::PointerMoved(p)]));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = app.run_frame(&ctx, release(to));
    println!(
        "drag: release sobre el lienzo en ({:.0}, {:.0})",
        to.x, to.y
    );

    // El drop activa el algoritmo Manual y fija posiciones manuales.
    if app.config().algorithm != tp_core::config::PackingAlgorithm::Manual {
        return Err(format!(
            "el drop no activó el algoritmo Manual (es {:?})",
            app.config().algorithm
        ));
    }
    if !app.repack_requested() {
        return Err("el drop no programó el repack (after_workspace_change)".into());
    }
    // Conducir frames hasta que el repack del drop aterrice: el nuevo
    // resultado debe colocar el sprite en la posición imantada.
    let (bp, pad) = (
        app.config().border_padding.max(0),
        app.config().padding.max(0),
    );
    let grid = app
        .config()
        .manual_grid
        .as_ref()
        .map(|g| g.step)
        .unwrap_or(1);
    let zoom = app.preview_zoom().max(0.0001);
    let ax = ((to.x - canvas.min.x) / zoom) as i32;
    let ay = ((to.y - canvas.min.y) / zoom) as i32;
    let snap = |v: i32| {
        let r = v.rem_euclid(grid);
        if r * 2 < grid {
            v - r
        } else {
            v + (grid - r)
        }
    };
    // Posición manual imantada (la que fija el drop y promete el fantasma).
    let expect_x = snap((ax - bp - pad).max(0));
    let expect_y = snap((ay - bp - pad).max(0));
    // El frame del pack incluye el padding; los píxeles visibles empiezan
    // en manual + borde + padding (así lo monta el pipeline).
    let expect_vis = (expect_x + bp + pad, expect_y + bp + pad);
    let mut frames_after = 0usize;
    let hero_id = app
        .result()
        .unwrap()
        .result
        .sprites
        .iter()
        .find(|s| {
            Path::new(&s.source_path)
                .file_name()
                .and_then(|n| n.to_str())
                == Some(hero)
        })
        .map(|s| s.id.clone())
        .ok_or("hero.png no está en el resultado: no se puede seguir el sprite")?;
    let hero_pos = loop {
        let Some(out) = app.result() else {
            return Err("la vista previa desapareció tras el drop".into());
        };
        if let Some(s) = out.result.sprites.iter().find(|s| s.id == hero_id) {
            if (s.visible_frame.x, s.visible_frame.y) != expect_vis {
                if frames_after > 400 {
                    return Err(format!(
                        "hero.png quedó en ({}, {}) tras el drop; esperado {expect_vis:?}",
                        s.visible_frame.x, s.visible_frame.y
                    ));
                }
            } else {
                break (s.visible_frame.x, s.visible_frame.y);
            }
        } else if frames_after > 400 {
            return Err(format!("{hero_id} desapareció del resultado tras el drop"));
        }
        let _ = app.run_frame(&ctx, default_input());
        frames_after += 1;
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    println!(
        "drag: hero.png colocado por el drop: manual ({expect_x}, {expect_y}), \
         píxeles visibles ({}, {}) tras {frames_after} frames",
        hero_pos.0, hero_pos.1
    );

    // 6) Verificaciones sobre el estado real de la app.
    let (total, aliases, pages) = app.pack_summary();
    let out = app
        .result()
        .ok_or("la vista previa desapareció tras aplicarse")?;
    if out.pages.is_empty() {
        return Err("la vista previa no tiene páginas de atlas".into());
    }
    if total != 5 {
        return Err(format!("se esperaban 5 sprites en la vista, hay {total}"));
    }
    if aliases != 2 {
        return Err(format!(
            "se esperaban 2 aliases (dedup por hash de píxeles), hay {aliases}"
        ));
    }
    let page0 = &out.pages[0];
    if page0.width <= 0 || page0.height <= 0 {
        return Err(format!(
            "la página 0 del atlas tiene tamaño inválido: {}x{}",
            page0.width, page0.height
        ));
    }
    let non_empty = out
        .pages
        .iter()
        .any(|p| p.pixels.chunks_exact(4).any(|px| px[3] != 0));
    if !non_empty {
        return Err("ninguna página del atlas tiene píxeles no transparentes".into());
    }
    if app.texture_count() != out.pages.len() {
        return Err(format!(
            "texturas egui cargadas ({}) != páginas del atlas ({})",
            app.texture_count(),
            out.pages.len()
        ));
    }
    let fill_sum: f32 = out.result.pages.iter().map(|p| p.fill_ratio).sum();
    if fill_sum <= 0.0 {
        return Err("el relleno del atlas es 0%".into());
    }
    if app.zoom() <= 0.0 {
        return Err("el zoom de encuadre no se calculó".into());
    }
    let logs = app.log_texts();
    let errors: Vec<&String> = logs
        .iter()
        .filter(|t| t.starts_with("E ") || t.contains("fallido") || t.contains("inesperadamente"))
        .collect();
    if !errors.is_empty() {
        return Err(format!("el Log de la app registra errores: {errors:?}"));
    }
    if app.config().manual_positions.get(&hero_id) != Some(&(expect_x, expect_y)) {
        return Err(format!(
            "manual_positions[{hero_id}] = {:?}, esperado ({expect_x}, {expect_y})",
            app.config().manual_positions.get(&hero_id)
        ));
    }

    if json {
        println!(
            "{{\"ok\":true,\"sprites\":{total},\"aliases\":{aliases},\"pages\":{},\"frames\":{frames},\"drag\":\"hero.png→({expect_x},{expect_y})\",\"ms\":{}}}",
            pages.len(),
            started.elapsed().as_millis()
        );
    } else {
        println!(
            "atlas: {} página(s), {total} sprites ({aliases} aliases), relleno medio {:.0}%",
            pages.len(),
            fill_sum / pages.len() as f32 * 100.0
        );
        println!(
            "SMOKE PASS — la app abre el proyecto, muestra la vista previa y el drag real \"
             press→move→release coloca el sprite ({} ms)",
            started.elapsed().as_millis()
        );
    }
    std::fs::remove_dir_all(&tmp).ok();
    Ok(())
}

/// Input por defecto para un frame (tamaño de ventana simulado 1360x860,
/// el mismo de la ventana nativa).
fn default_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1360.0, 860.0),
        )),
        ..egui::RawInput::default()
    }
}

/// Frame con eventos de puntero concretos sobre el input por defecto.
fn input_with(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        events,
        ..default_input()
    }
}

/// Soltar el botón primario en `pos` (fin del drag).
fn release(pos: egui::Pos2) -> egui::RawInput {
    input_with(vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    }])
}

/// Un `Time` adelantado: despierta sondeos internos de egui en el primer
/// frame (los `request_repaint_after` no aplican al primer frame).
fn wake_input() -> egui::RawInput {
    egui::RawInput {
        time: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        ),
        ..default_input()
    }
}
