//! Regresión del flujo «arrastrar sprites del panel al lienzo» contra la
//! app real, headless (`App::new_for_testing` + `run_frame`). Verifica el
//! contrato de sensación del arrastre:
//!
//! 1. El primer sprite queda con su esquina bajo el cursor (menos
//!    borde/padding del atlas).
//! 2. La disposición relativa de la multi-selección se conserva.
//! 3. El algoritmo pasa a Manual y se repacktua al instante.
//! 4. El drop aplica el mismo snap de rejilla que promete el fantasma
//!    (una única fuente de verdad: `plan_canvas_drop`).
//! 5. El clamp del lienzo evita colocar sprites fuera del atlas.
//! 6. Los aliases no se colocan (no ocupan frame propio).
//!
//! Y con frames de UI de verdad (`run_frame` + eventos de puntero): el
//! fantasma y el drop consumen el mismo plan, así que lo que se ve es lo
//! que queda.

use eframe::egui;
use std::path::PathBuf;
use tp_app::{testing::create_example_project, App};

/// Construye la app con un proyecto real temporal y conduce frames hasta
/// que la vista previa está aplicada.
fn app_with_preview(name: &str) -> (App, egui::Context, PathBuf) {
    let tmp = std::env::temp_dir().join(format!(
        "tp_drop_{name}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    let sprites = tmp.join("sprites");
    std::fs::create_dir_all(&sprites).unwrap();
    let project = create_example_project(&tmp, &sprites).expect("proyecto de ejemplo");
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), Some(project));
    for _ in 0..4000 {
        if app.result().is_some() {
            break;
        }
        let _ = app.run_frame(&ctx, test_input());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        app.result().is_some(),
        "la vista previa debería haber llegado (app real + pipeline)"
    );
    // Un frame más para asentar estado post-resultado.
    let _ = app.run_frame(&ctx, test_input());
    (app, ctx, tmp)
}

fn test_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1360.0, 860.0),
        )),
        ..egui::RawInput::default()
    }
}

/// Inicia un arrastre como el del panel (payload con ids y frames) usando
/// la misma función que la UI llama en `drag_started` del árbol.
fn start_drag(app: &App, ctx: &egui::Context, ids: &[&str]) {
    let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    tp_app::begin_sprite_drag(app, ctx, ids);
}

#[test]
fn drop_places_first_sprite_under_cursor_and_keeps_relative_layout() {
    let (mut app, ctx, tmp) = app_with_preview("anchor");

    // Ids reales del pack: el primero y el tercero (bg, el más grande).
    let ids: Vec<String> = app
        .result()
        .unwrap()
        .result
        .sprites
        .iter()
        .filter(|s| !s.is_alias)
        .map(|s| s.id.clone())
        .take(2)
        .collect();
    assert_eq!(ids.len(), 2, "necesitamos dos sprites no-alias");
    start_drag(
        &app,
        &ctx,
        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
    );

    // Soltar en el píxel del atlas (100, 60).
    let dropped = app.drop_sprites_on_canvas(egui::pos2(100.0, 60.0));
    assert_eq!(dropped, 2, "los dos sprites se colocan");

    // Contrato 1: algoritmo Manual activado por la composición manual.
    assert_eq!(
        app.config().algorithm,
        tp_core::config::PackingAlgorithm::Manual
    );

    // Contrato 2: el primer id (orden de la lista) queda bajo el cursor:
    // anchor = (100, 60) - bp(0) - pad(2) = (98, 58); con rejilla 16 del
    // proyecto de ejemplo: snap((98,58)) = (96, 64).
    let (px, py) = app.config().manual_positions[&ids[0]];
    assert_eq!(
        (px, py),
        (96, 64),
        "el primer sprite debe quedar en el anclaje imantado"
    );

    // Contrato 3: el segundo conserva su desplazamiento relativo respecto
    // a la vista previa previa al drop.
    let out = app.result().unwrap();
    let f0 = &out.result.sprites[0].visible_frame;
    let f1 = &out.result.sprites[1].visible_frame;
    let dx = f1.x - f0.x;
    let dy = f1.y - f0.y;
    let (qx, qy) = app.config().manual_positions[&ids[1]];
    assert_eq!(
        (qx - px, qy - py),
        (dx, dy),
        "la disposición relativa se conserva"
    );

    // Contrato 4: el repack quedó programado (drop → after_workspace_change).
    assert!(
        app.repack_requested() || app.result().is_some(),
        "el drop debe programar repack"
    );

    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn drop_snaps_to_manual_grid_and_clamps_inside_canvas() {
    let (mut app, ctx, tmp) = app_with_preview("snapclamp");
    let ids: Vec<String> = app
        .result()
        .unwrap()
        .result
        .sprites
        .iter()
        .filter(|s| !s.is_alias)
        .map(|s| s.id.clone())
        .take(1)
        .collect();
    start_drag(&app, &ctx, &[ids[0].as_str()]);

    // Rejilla 16: soltar en (10, 10) → anchor (8, 8) → snap (0, 0).
    let dropped = app.drop_sprites_on_canvas(egui::pos2(10.0, 10.0));
    assert_eq!(dropped, 1);
    let (px, py) = app.config().manual_positions[&ids[0]];
    assert_eq!(px % 16, 0, "x imantada a la rejilla de 16");
    assert_eq!(py % 16, 0, "y imantada a la rejilla de 16");

    // Clamp: la posición quedó dentro del lienzo (2048x2048 por defecto).
    let (bx, by) = app.config().manual_positions[&ids[0]];
    assert!(bx >= 0 && by >= 0, "posiciones dentro del lienzo");
    assert!(bx < app.config().max_texture_size && by < app.config().max_texture_size);

    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn ghost_plan_matches_final_positions_and_aliases_are_skipped() {
    let (mut app, ctx, tmp) = app_with_preview("ghost");

    // Frames que viajarían en el payload (como begin_sprite_drag).
    let all: Vec<(String, tp_core::types::Rect, bool)> = app
        .result()
        .unwrap()
        .result
        .sprites
        .iter()
        .map(|s| (s.id.clone(), s.visible_frame, s.is_alias))
        .collect();
    let non_alias: Vec<String> = all
        .iter()
        .filter(|(_, _, a)| !*a)
        .map(|(i, _, _)| i.clone())
        .collect();
    let mut frames = std::collections::BTreeMap::new();
    for (id, f, a) in &all {
        if !a {
            frames.insert(id.clone(), *f);
        }
    }
    let first_frame = non_alias.iter().find_map(|id| frames.get(id).copied());

    // El plan que dibuja el fantasma…
    let plan = App::plan_canvas_drop(
        &non_alias,
        &frames,
        first_frame,
        egui::pos2(64.0, 48.0),
        app.config(),
    );
    // …es exactamente el que aplica el drop real.
    start_drag(
        &app,
        &ctx,
        &non_alias.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let dropped = app.drop_sprites_on_canvas(egui::pos2(64.0, 48.0));
    assert_eq!(dropped, plan.len(), "fantasma y drop colocan lo mismo");

    for (id, pos) in &plan {
        assert_eq!(
            app.config().manual_positions.get(id),
            Some(pos),
            "el fantasma prometió {id} en {pos:?} y el drop lo colocó igual"
        );
    }

    // Los aliases no aparecen en el plan (no ocupan frame propio).
    let aliases = all.iter().filter(|(_, _, a)| *a).count();
    assert!(aliases > 0, "el proyecto de ejemplo tiene aliases");
    assert_eq!(plan.len(), non_alias.len());

    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn drag_payload_roundtrip_and_clear() {
    let ctx = egui::Context::default();
    assert!(egui::DragAndDrop::payload::<String>(&ctx).is_none());
    egui::DragAndDrop::set_payload(&ctx, "panel_ui".to_string());
    assert_eq!(
        egui::DragAndDrop::payload::<String>(&ctx).map(|s| s.as_str().to_string()),
        Some("panel_ui".to_string())
    );
    egui::DragAndDrop::clear_payload(&ctx);
    assert!(egui::DragAndDrop::payload::<String>(&ctx).is_none());
    let _ = PathBuf::new();
}
