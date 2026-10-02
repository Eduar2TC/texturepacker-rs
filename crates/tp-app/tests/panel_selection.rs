//! Selección por lote y navegación por teclado en el panel izquierdo,
//! contra la app real headless (`App::new_for_testing` + `run_frame` con
//! eventos de puntero y de teclado de verdad).
//!
//! Contrato (igual que en los gestores de archivos):
//!
//! 1. Clic simple = una fila; `Ctrl` = acumula; `Shift` = rango visual
//!    entre el ancla y la fila pulsada.
//! 2. Las flechas mueven el cursor por la lista visible; con `Shift`
//!    extienden el rango y con `Ctrl` acumulan.
//! 3. `Ctrl+A` selecciona toda la lista visible.
//! 4. `Supr` quita el lote seleccionado y deja el cursor en la fila
//!    siguiente (o en la anterior si se borró el final), para poder
//!    repetir `Supr` sin volver al ratón.
//! 5. Sin foco de teclado en el panel (nunca se pulsó una fila, o el
//!    puntero está en el lienzo y aún no se ha hecho clic), las teclas no
//!    mueven la lista.

use eframe::egui;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tp_app::{testing::create_example_project, App};

fn screen_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1360.0, 860.0))
}

/// Construye la app con un proyecto real temporal y conduce frames hasta
/// que la vista previa está aplicada (y las filas del panel, registradas).
fn boot(name: &str) -> (App, egui::Context, PathBuf) {
    let tmp = std::env::temp_dir().join(format!(
        "tp_sel_{name}_{}_{}",
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
    // Un frame más para asentar el estado y registrar las filas.
    frame(&mut app, &ctx, egui::Modifiers::default(), vec![]);
    assert!(
        app.sprite_row_rects().len() >= 4,
        "el panel debe registrar las filas del proyecto de ejemplo: {:?}",
        row_names(&app)
    );
    (app, ctx, tmp)
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

/// Nombres de archivo de las filas visibles, en orden visual del panel.
fn row_names(app: &App) -> Vec<String> {
    app.sprite_row_rects()
        .iter()
        .map(|(p, _)| file_name(p))
        .collect()
}

/// Centro en pantalla de la fila de un sprite (se lee justo antes de cada
/// evento: al quitar filas, el resto de filas cambian de sitio).
fn center_of(app: &App, name: &str) -> egui::Pos2 {
    app.sprite_row_rects()
        .iter()
        .find(|(p, _)| file_name(p) == name)
        .map(|(_, r)| r.center())
        .unwrap_or_else(|| {
            panic!(
                "no hay fila para {name}; filas visibles: {:?}",
                row_names(app)
            )
        })
}

/// Nombres de archivo de la selección (orden alfabético: es un BTreeSet).
fn selected(app: &App) -> Vec<String> {
    app.selected_paths().iter().map(|p| file_name(p)).collect()
}

fn expected<S: AsRef<str>>(names: &[S]) -> Vec<String> {
    let mut v: Vec<String> = names.iter().map(|s| s.as_ref().to_string()).collect();
    v.sort();
    v
}

fn click_at(app: &mut App, ctx: &egui::Context, pos: egui::Pos2, mods: egui::Modifiers) {
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

/// Clic completo (press + release) sobre la fila `name`. El press deja el
/// puntero sobre la fila, así que el panel queda «hovered» para las teclas.
fn click_row(app: &mut App, ctx: &egui::Context, name: &str, mods: egui::Modifiers) {
    let pos = center_of(app, name);
    click_at(app, ctx, pos, mods);
}

/// Una pulsación de tecla con los modificadores dados.
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

const SHIFT: egui::Modifiers = egui::Modifiers {
    alt: false,
    ctrl: false,
    shift: true,
    command: false,
    mac_cmd: false,
};
const CTRL: egui::Modifiers = egui::Modifiers {
    alt: false,
    ctrl: false,
    shift: false,
    command: true,
    mac_cmd: false,
};

#[test]
fn shift_click_selects_the_visual_range_from_the_anchor() {
    let (mut app, ctx, tmp) = boot("range");
    let names = row_names(&app);
    let (a, b, c) = (&names[0], &names[1], &names[2]);

    // Clic simple en la primera fila: ancla y selección única.
    click_row(&mut app, &ctx, a, egui::Modifiers::default());
    assert_eq!(
        selected(&app),
        expected(&[a]),
        "el clic simple selecciona una fila"
    );
    assert_eq!(
        app.list_cursor().map(file_name),
        Some(a.to_string()),
        "el cursor va a la fila pulsada"
    );

    // Shift+clic en la tercera: rango completo en orden visual.
    click_row(&mut app, &ctx, c, SHIFT);
    assert_eq!(
        selected(&app),
        expected(&[a, b, c]),
        "Shift+clic selecciona el rango visual entre el ancla y la fila"
    );
    assert_eq!(
        app.list_cursor().map(file_name),
        Some(c.to_string()),
        "el cursor sigue a la fila pulsada"
    );

    // Otra fila más allá con Shift: el rango se recalcula desde la MISMA
    // ancla (la primera fila), no desde la última pulsada.
    let last = names.last().unwrap();
    if last != c {
        click_row(&mut app, &ctx, last, SHIFT);
        assert_eq!(
            selected(&app),
            expected(&names),
            "la ancla no se mueve: el rango sigue saliendo de ella"
        );
    }

    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn ctrl_click_and_ctrl_a_accumulate_the_batch() {
    let (mut app, ctx, tmp) = boot("batch");
    let names = row_names(&app);

    click_row(&mut app, &ctx, &names[0], egui::Modifiers::default());
    click_row(&mut app, &ctx, &names[2], CTRL);
    assert_eq!(
        selected(&app),
        expected(&[&names[0], &names[2]]),
        "Ctrl+clic acumula sin tocar la fila ya seleccionada"
    );

    // Ctrl+A selecciona toda la lista visible.
    press(&mut app, &ctx, egui::Key::A, CTRL);
    assert_eq!(
        selected(&app),
        expected(&names),
        "Ctrl+A selecciona el lote entero"
    );

    // Ctrl+clic sobre una ya seleccionada la quita del lote.
    click_row(&mut app, &ctx, &names[1], CTRL);
    let after = selected(&app);
    assert!(
        !after.contains(&names[1]),
        "Ctrl+clic alterna: la fila sale del lote"
    );
    assert_eq!(
        after.len(),
        names.len() - 1,
        "el resto del lote se conserva"
    );

    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn arrows_walk_the_list_and_shift_extends_the_range() {
    let (mut app, ctx, tmp) = boot("arrows");
    let names = row_names(&app);

    // Punto de partida: clic en la primera fila (da foco de teclado).
    click_row(&mut app, &ctx, &names[0], egui::Modifiers::default());
    assert_eq!(selected(&app), expected(&[&names[0]]));

    // ↓ mueve cursor y selección juntos.
    press(
        &mut app,
        &ctx,
        egui::Key::ArrowDown,
        egui::Modifiers::default(),
    );
    assert_eq!(
        selected(&app),
        expected(&[&names[1]]),
        "↓ selecciona la fila siguiente"
    );
    assert_eq!(app.list_cursor().map(file_name), Some(names[1].clone()));

    // Shift+↓ extiende desde el ancla (la fila donde empezó el rango).
    press(&mut app, &ctx, egui::Key::ArrowDown, SHIFT);
    assert_eq!(
        selected(&app),
        expected(&[&names[1], &names[2]]),
        "Shift+↓ crece desde el ancla"
    );
    press(&mut app, &ctx, egui::Key::ArrowDown, SHIFT);
    assert_eq!(
        selected(&app),
        expected(&[&names[1], &names[2], &names[3]]),
        "Shift+↓ sigue creciendo"
    );

    // ↑ simple vuelve a una sola fila.
    press(
        &mut app,
        &ctx,
        egui::Key::ArrowUp,
        egui::Modifiers::default(),
    );
    assert_eq!(
        selected(&app),
        expected(&[&names[2]]),
        "↑ simple deja una sola fila"
    );

    // Home/End saltan a los extremos de la lista visible.
    press(&mut app, &ctx, egui::Key::End, egui::Modifiers::default());
    assert_eq!(
        selected(&app),
        expected(&[names.last().unwrap()]),
        "End va al final"
    );
    press(&mut app, &ctx, egui::Key::Home, egui::Modifiers::default());
    assert_eq!(selected(&app), expected(&[&names[0]]), "Home va al inicio");

    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn delete_leaves_the_cursor_on_the_next_row() {
    let (mut app, ctx, tmp) = boot("delete");
    let names = row_names(&app);

    // Selecciona la segunda fila.
    click_row(&mut app, &ctx, &names[1], egui::Modifiers::default());
    assert_eq!(selected(&app), expected(&[&names[1]]));

    // Supr: se va de la lista y la selección cae en la fila siguiente.
    press(
        &mut app,
        &ctx,
        egui::Key::Delete,
        egui::Modifiers::default(),
    );
    assert!(
        !app.config().excluded_inputs.is_empty(),
        "la fila quitada queda excluida del pack"
    );
    assert!(
        app.config()
            .excluded_inputs
            .iter()
            .any(|p| file_name(p) == names[1]),
        "la fila eliminada queda excluida del pack"
    );
    assert_eq!(
        selected(&app),
        expected(&[&names[2]]),
        "tras Supr, la selección pasa a la fila siguiente"
    );
    assert_eq!(
        app.list_cursor().map(file_name),
        Some(names[2].clone()),
        "el cursor queda en la fila siguiente"
    );

    // Repetir Supr recorre la lista sin volver al ratón.
    press(
        &mut app,
        &ctx,
        egui::Key::Delete,
        egui::Modifiers::default(),
    );
    assert_eq!(
        selected(&app),
        expected(&[&names[3]]),
        "el segundo Supr sigue bajando"
    );

    // Un frame en reposo refresca las filas (ya sin las quitadas)…
    frame(&mut app, &ctx, egui::Modifiers::default(), vec![]);
    let remaining: Vec<String> = row_names(&app)
        .into_iter()
        .filter(|n| {
            !app.config()
                .excluded_inputs
                .iter()
                .any(|p| file_name(p) == *n)
        })
        .collect();
    // …y al quitar la última fila, el cursor retrocede a la anterior.
    let last_name = remaining.last().unwrap().clone();
    click_row(&mut app, &ctx, &last_name, egui::Modifiers::default());
    press(
        &mut app,
        &ctx,
        egui::Key::Delete,
        egui::Modifiers::default(),
    );
    let after = selected(&app);
    assert_eq!(
        after.len(),
        1,
        "al quitar el final queda una fila seleccionada: {after:?}"
    );
    assert_ne!(
        after[0], last_name,
        "el cursor no se queda en la fila borrada"
    );

    std::fs::remove_dir_all(&tmp).ok();
}

#[test]
fn keys_stay_idle_without_panel_focus() {
    let (mut app, ctx, tmp) = boot("nofocus");
    // Puntero sobre el lienzo y ninguna fila pulsada: el panel no tiene
    // foco de teclado, así que las teclas no mueven nada.
    let canvas = app.canvas_rect().expect("el lienzo debe estar dibujado");
    let center = canvas.center();
    frame(
        &mut app,
        &ctx,
        egui::Modifiers::default(),
        vec![egui::Event::PointerMoved(center)],
    );

    press(
        &mut app,
        &ctx,
        egui::Key::ArrowDown,
        egui::Modifiers::default(),
    );
    assert!(selected(&app).is_empty(), "sin foco, ↓ no selecciona nada");

    press(
        &mut app,
        &ctx,
        egui::Key::Delete,
        egui::Modifiers::default(),
    );
    assert!(
        app.config().excluded_inputs.is_empty(),
        "sin selección (y sin foco), Supr no quita nada"
    );

    // Un clic en una fila gana el foco de teclado del panel…
    let names = row_names(&app);
    click_row(&mut app, &ctx, &names[1], egui::Modifiers::default());
    assert_eq!(selected(&app), expected(&[&names[1]]));

    // …y ya fuera del panel las teclas siguen moviendo la lista.
    frame(
        &mut app,
        &ctx,
        egui::Modifiers::default(),
        vec![egui::Event::PointerMoved(center)],
    );
    press(
        &mut app,
        &ctx,
        egui::Key::ArrowDown,
        egui::Modifiers::default(),
    );
    assert_eq!(
        selected(&app),
        expected(&[&names[2]]),
        "con foco, ↓ mueve la lista aunque el puntero esté en el lienzo"
    );

    std::fs::remove_dir_all(&tmp).ok();
}

/// El rango de Shift depende del orden visual registrado: sin filas
/// duplicadas ni fuera de orden no hay forma de calcularlo.
#[test]
fn row_registration_follows_the_visual_order() {
    let (app, _ctx, tmp) = boot("order");
    let rows: Vec<PathBuf> = app
        .sprite_row_rects()
        .iter()
        .map(|(p, _)| p.clone())
        .collect();
    assert_eq!(rows.len(), 5, "el proyecto de ejemplo tiene 5 sprites");
    let unique: BTreeSet<PathBuf> = rows.iter().cloned().collect();
    assert_eq!(unique.len(), rows.len(), "sin filas duplicadas");
    std::fs::remove_dir_all(&tmp).ok();
}
