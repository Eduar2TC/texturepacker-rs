//! La vista previa de animación reproduce lo que promete su tooltip
//! («de los sprites seleccionados»): con selección en el panel izquierdo,
//! solo esos sprites; sin selección, todos los publicados.
//!
//! Se prueba contra la app real headless (`App::new_for_testing` + `run_frame`
//! con clics de verdad sobre las filas del panel).

use eframe::egui;
use std::path::Path;
use tp_app::{testing::create_example_project, App};

fn screen_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1360.0, 860.0))
}

/// App con el proyecto de ejemplo ya publicado y las filas del panel registradas.
fn boot(name: &str) -> (App, egui::Context) {
    let tmp = std::env::temp_dir().join(format!(
        "tp_anim_{name}_{}_{}",
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
        frame(&mut app, &ctx, egui::Modifiers::default(), vec![]);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.result().is_some(), "la vista previa debería llegar");
    frame(&mut app, &ctx, egui::Modifiers::default(), vec![]);
    assert!(
        !app.animation_frame_ids().is_empty(),
        "la secuencia por defecto no puede estar vacía"
    );
    (app, ctx)
}

fn frame(app: &mut App, ctx: &egui::Context, mods: egui::Modifiers, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(screen_rect()),
        modifiers: mods,
        events,
        ..egui::RawInput::default()
    };
    let _ = app.run_frame(ctx, input);
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Id de un archivo del panel (el motor usa el stem, sin extensión).
fn id_of(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn row_names(app: &App) -> Vec<String> {
    app.sprite_row_rects()
        .iter()
        .map(|(p, _)| file_name(p))
        .collect()
}

/// Centro en pantalla de la fila de un sprite (se lee justo antes del clic).
fn center_of(app: &App, name: &str) -> egui::Pos2 {
    app.sprite_row_rects()
        .iter()
        .find(|(p, _)| file_name(p) == name)
        .map(|(_, r)| r.center())
        .unwrap_or_else(|| panic!("no hay fila para {name}; filas: {:?}", row_names(app)))
}

fn click_row(app: &mut App, ctx: &egui::Context, name: &str, mods: egui::Modifiers) {
    let pos = center_of(app, name);
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            mods,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: mods,
            }],
        );
    }
}

fn press(app: &mut App, ctx: &egui::Context, key: egui::Key, mods: egui::Modifiers) {
    frame(
        app,
        ctx,
        mods,
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: mods,
        }],
    );
}

const CTRL: egui::Modifiers = egui::Modifiers {
    alt: false,
    ctrl: false,
    shift: false,
    command: true,
    mac_cmd: false,
};

#[test]
fn animation_preview_plays_only_the_selected_sprites() {
    let (mut app, ctx) = boot("selection");

    // Sin selección: todos los sprites publicados.
    let all = app.animation_frame_ids();
    assert!(
        all.len() >= 3,
        "el proyecto de ejemplo debería publicar varios sprites: {all:?}"
    );

    // Un solo sprite seleccionado → una sola entrada en la secuencia.
    let rows = row_names(&app);
    let pick = rows
        .iter()
        .find(|n| *n == "bg.png")
        .cloned()
        .unwrap_or_else(|| rows[0].clone());
    click_row(&mut app, &ctx, &pick, egui::Modifiers::default());
    assert_eq!(
        app.selected_paths().len(),
        1,
        "el clic debe dejar una sola fila seleccionada"
    );
    let seq = app.animation_frame_ids();
    assert_eq!(
        seq,
        vec![id_of(&pick)],
        "con selección, la vista previa reproduce solo el sprite elegido"
    );

    // Seleccionar todas (Ctrl+A) → la secuencia vuelve a ser la completa.
    press(&mut app, &ctx, egui::Key::A, CTRL);
    assert_eq!(
        app.animation_frame_ids(),
        all,
        "con toda la lista seleccionada se reproducen todos los sprites"
    );

    // Otra fila suelta → otra vez un solo fotograma (el grupo elegido en la
    // ventana no debe dejar la secuencia vacía).
    let other = rows
        .iter()
        .find(|n| *n == "hero.png")
        .cloned()
        .unwrap_or_else(|| rows[1].clone());
    click_row(&mut app, &ctx, &other, egui::Modifiers::default());
    assert_eq!(
        app.animation_frame_ids(),
        vec![id_of(&other)],
        "cambiar la selección cambia la secuencia"
    );
}

#[test]
fn animation_sequence_keeps_every_selected_sprite_in_group_order() {
    let (mut app, ctx) = boot("order");

    let rows = row_names(&app);
    let (a, b) = (
        rows.iter().find(|n| *n == "bg.png").cloned(),
        rows.iter().find(|n| *n == "hero.png").cloned(),
    );
    let (Some(a), Some(b)) = (a, b) else {
        // Filas inesperadas: no se puede fijar una pareja determinista.
        return;
    };

    // Dos sprites de grupos distintos: ambos entran en la secuencia.
    click_row(&mut app, &ctx, &a, egui::Modifiers::default());
    click_row(&mut app, &ctx, &b, CTRL);
    assert_eq!(
        app.selected_paths().len(),
        2,
        "el Ctrl+clic debe acumular la segunda fila"
    );

    let mut expected = vec![id_of(&a), id_of(&b)];
    expected.sort();
    assert_eq!(
        app.animation_frame_ids(),
        expected,
        "la selección multi-sprite se reproduce en orden de grupo"
    );

    // Quitar la selección (Ctrl+A sobre la lista ya seleccionada la reduce a
    // todo; Escape no está atado), y comprobar que nada se pierde.
    press(&mut app, &ctx, egui::Key::A, CTRL);
    let all = app.animation_frame_ids();
    assert!(
        all.len() >= expected.len(),
        "al ampliar la selección no se pierden sprites: {all:?}"
    );
}
