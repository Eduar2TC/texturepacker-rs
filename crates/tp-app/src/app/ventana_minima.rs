//! M4: pruebas de layout en la ventana mínima.
//!
//! Toda la suite vive en 1360×860 —el tamaño de arranque— y por eso
//! nadie miraba qué pasa cuando la ventana está al mínimo que la app
//! promete (`main.rs`): paneles apretados, la barra desbordada o un
//! diálogo más alto que la pantalla. Aquí se comprueba justo eso, con el
//! mismo número que usa `main.rs` ([`crate::VENTANA_MINIMA`]).

use super::*;
use crate::testing::{create_example_project, textos_pintados};
use std::time::Duration;

/// La ventana mínima como rectángulo, con la esquina superior izquierda
/// en el origen.
fn pantalla() -> egui::Rect {
    egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(crate::VENTANA_MINIMA.0, crate::VENTANA_MINIMA.1),
    )
}

/// ¿`dentro` cabe entero en `rect`? (emath no trae `contains(Rect)`.)
fn cabe(rect: egui::Rect, dentro: egui::Rect) -> bool {
    rect.min.x <= dentro.min.x
        && rect.min.y <= dentro.min.y
        && rect.max.x >= dentro.max.x
        && rect.max.y >= dentro.max.y
}

/// Un frame con la ventana al mínimo. Lleva `time`, que la app usa para
/// programar los repints.
fn frame_en(app: &mut App, ctx: &egui::Context, pantalla: egui::Rect) -> egui::FullOutput {
    app.run_frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(pantalla),
            time: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64(),
            ),
            ..egui::RawInput::default()
        },
    )
}

fn frame(app: &mut App, ctx: &egui::Context) -> egui::FullOutput {
    frame_en(app, ctx, pantalla())
}

/// Proyecto de ejemplo ya empaquetado, con la ventana al mínimo todo el
/// rato: el resultado y el encuadre automático se calculan a 900×600.
fn demo(tag: &str) -> (App, egui::Context, PathBuf) {
    let tmp = std::env::temp_dir().join(format!(
        "tp_min_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let sprites = tmp.join("sprites");
    std::fs::create_dir_all(&sprites).expect("crea la carpeta de sprites");
    let proyecto = create_example_project(&tmp, &sprites).expect("proyecto de ejemplo");
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), Some(proyecto));
    for _ in 0..4000 {
        if app.result().is_some() {
            break;
        }
        let _ = frame(&mut app, &ctx);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        app.result().is_some(),
        "el proyecto de ejemplo debe empaquetarse con la ventana al mínimo"
    );
    // Un frame más para asentar el encuadre automático post-resultado.
    let _ = frame(&mut app, &ctx);
    (app, ctx, tmp)
}

/// M4: con la ventana al mínimo el lienzo sigue existiendo, entero y por
/// encima del log —es la zona con la que se trabaja—, y la lista de la
/// izquierda no se queda fuera de pantalla.
#[test]
fn el_lienzo_cabe_entero_en_la_ventana_minima() {
    let (app, _ctx, tmp) = demo("lienzo");

    let rect = app
        .canvas_rect()
        .expect("el lienzo debe dibujarse con la ventana al mínimo");
    assert!(rect.is_positive(), "el lienzo debe tener tamaño: {rect:?}");
    assert!(
        cabe(pantalla(), rect),
        "el lienzo se sale de la ventana mínima: {rect:?} no cabe en {:?}",
        pantalla()
    );
    assert!(
        rect.width() >= 100.0 && rect.height() >= 100.0,
        "el lienzo queda en una tira de {}×{} px",
        rect.width(),
        rect.height()
    );
    // El centro —donde se mira el atlas— no se queda en una tira: con los
    // dos paneles laterales y el log abiertos aún queda sitio de verdad.
    // (Tamaño del lienzo no lo dice: depende del atlas, que puede ser
    // pequeñísimo; el sitio disponible sí depende sólo de la ventana.)
    let vp = app.preview_size;
    assert!(
        vp.x >= 250.0 && vp.y >= 200.0,
        "la vista previa queda en {vp:?} px con la ventana al mínimo"
    );

    let filas = app.sprite_row_rects();
    assert!(
        !filas.is_empty(),
        "la lista de sprites debe seguir pintándose"
    );
    for (ruta, r) in filas {
        assert!(
            cabe(pantalla(), *r),
            "la fila {} se sale de la ventana mínima: {r:?}",
            ruta.display()
        );
    }
    std::fs::remove_dir_all(&tmp).ok();
}

/// Alturas que el rediseño dio con medida fija: la banda de menú y la
/// de herramientas, la de la barra de zoom y la de la tira de estado.
const BANDA_CHROME: f32 = 22.0;
const BANDA_ZOOM: f32 = 32.0;
const BANDA_TIRA: f32 = 22.0;

/// El primer texto pintado que cumple `buscado`, con su rectángulo y el
/// recorte que egui le puso. Falla si no aparece.
fn hallado(
    pintados: &[(String, egui::Rect, egui::Rect)],
    buscado: &str,
    exacto: bool,
) -> (String, egui::Rect, egui::Rect) {
    pintados
        .iter()
        .find(|(texto, _, _)| {
            if exacto {
                texto.trim() == buscado
            } else {
                texto.contains(buscado)
            }
        })
        .map(|(texto, rect, recorte)| (texto.clone(), *rect, *recorte))
        .unwrap_or_else(|| panic!("«{buscado}» no se pintó a la ventana mínima"))
}

/// M4: lo esencial de cada banda se ve entero con la ventana al mínimo.
///
/// Cada control se busca por su texto —los que pueden coincidir con otro
/// sitio, exacto— porque egui *recorta* las formas que quedan
/// enteramente fuera de su panel: si un control se va de la barra no
/// aparece ni siquiera como texto pintado, y que no aparezca es lo que
/// hace fallar la prueba. Lo que queda pintado a medias se descubre con
/// su rectángulo contra el recorte que egui le puso y contra la ventana.
///
/// Los recortes son las propias bandas, así que de paso se comprueba que
/// siguen montadas una sobre otra: la de menú empieza en el borde, cada
/// una continúa donde acaba la anterior y las de medida fija miden lo
/// que tienen que medir.
#[test]
fn lo_esencial_de_la_barra_y_los_paneles_se_ve_entero() {
    let (mut app, ctx, tmp) = demo("barra");
    let out = frame(&mut app, &ctx);
    let pintados = textos_pintados(&out);
    let pantalla = pantalla();

    // (texto buscado, ¿comparación exacta?)
    let buscados = [
        // Barra de menú.
        ("Archivo", false),
        ("Edición", false),
        ("Ver", true), // también está dentro de «Vista previa…»
        ("Ayuda", false),
        // Barra de herramientas: las acciones de un clic.
        ("➕ Añadir sprites…", false),
        ("📁 Añadir carpeta…", false),
        ("⏏ Publicar", false),
        ("…", true), // «Más opciones de publicación»
        // Panel izquierdo.
        ("Sprites (", false),
        // Barra de zoom.
        ("Vista", true), // el log también dice «Vista previa…»
        ("1:1", true),
        ("Ajustar", true),
        // Panel inferior y tira de estado.
        ("Log", true),
        ("ficheros", false),
        // Sin «Ajustes»: a 900 px el dock se pliega solo para que el
        // lienzo no quede en una tira (F4) y se recupera con `F9`. Que
        // no aparezca aquí es lo que comprueba el pliegue, y que vuelva
        // lo comprueba `dock_ajustes`.
    ];
    for (buscado, exacto) in buscados {
        let (texto, rect, recorte) = hallado(&pintados, buscado, exacto);
        assert!(
            cabe(recorte, rect),
            "«{texto}» está cortado por su recorte: {rect:?} no cabe en {recorte:?}"
        );
        assert!(
            cabe(pantalla, rect),
            "«{texto}» se sale de la ventana mínima: {rect:?}"
        );
    }

    // Las bandas de arriba son a pantalla completa y siguen la una a la
    // otra sin dejarse hueco.
    let (_, _, corte_menu) = hallado(&pintados, "Archivo", false);
    assert_eq!(
        corte_menu.min.y, 0.0,
        "la barra de menú no empieza en el borde: {corte_menu:?}"
    );
    assert_eq!(
        corte_menu.width(),
        pantalla.width(),
        "la barra de menú no llega a todo lo ancho: {corte_menu:?}"
    );
    assert_eq!(
        corte_menu.height(),
        BANDA_CHROME,
        "la barra de menú no mide {BANDA_CHROME} px: {corte_menu:?}"
    );

    let (_, _, corte_herr) = hallado(&pintados, "⏏ Publicar", false);
    assert_eq!(
        corte_herr.min.y, corte_menu.max.y,
        "la barra de herramientas no continúa la de menú: {corte_herr:?}"
    );
    assert_eq!(
        corte_herr.width(),
        pantalla.width(),
        "la barra de herramientas no llega a todo lo ancho: {corte_herr:?}"
    );
    assert_eq!(
        corte_herr.height(),
        BANDA_CHROME,
        "la barra de herramientas no mide {BANDA_CHROME} px: {corte_herr:?}"
    );

    // El panel izquierdo llega desde el borde, no se come la mitad de la
    // ventana y acaba donde empieza el panel inferior.
    let (_, _, corte_izq) = hallado(&pintados, "Sprites (", false);
    assert_eq!(
        corte_izq.min.x, 0.0,
        "el panel izquierdo no llega al borde: {corte_izq:?}"
    );
    assert!(
        corte_izq.max.x < pantalla.width() / 2.0,
        "el panel izquierdo se come más de la mitad de la ventana: {corte_izq:?}"
    );

    // El panel inferior llena la ventana de lado a lado.
    let (_, _, corte_panel) = hallado(&pintados, "Log", true);
    assert_eq!(
        corte_panel.width(),
        pantalla.width(),
        "el panel inferior no llega a todo lo ancho: {corte_panel:?}"
    );
    assert_eq!(
        corte_izq.max.y, corte_panel.min.y,
        "el panel izquierdo no acaba donde empieza el inferior: {corte_izq:?} y {corte_panel:?}"
    );

    // La barra de zoom ocupa la columna central —sin invadir el panel
    // izquierdo—, se pega al panel inferior y mide sus 32 px.
    let (_, _, corte_zoom) = hallado(&pintados, "Vista", true);
    assert!(
        corte_zoom.min.x >= corte_izq.max.x,
        "la barra de zoom invade el panel izquierdo: {corte_zoom:?} y {corte_izq:?}"
    );
    assert_eq!(
        corte_zoom.max.y, corte_panel.min.y,
        "la barra de zoom no se pega al panel inferior: {corte_zoom:?} y {corte_panel:?}"
    );
    assert_eq!(
        corte_zoom.height(),
        BANDA_ZOOM,
        "la barra de zoom no mide {BANDA_ZOOM} px: {corte_zoom:?}"
    );

    // La tira cierra contra el borde de abajo y mide sus 22 px.
    let (_, _, corte_tira) = hallado(&pintados, "ficheros", false);
    assert_eq!(
        corte_tira.min.y, corte_panel.max.y,
        "la tira no continúa el panel inferior: {corte_tira:?}"
    );
    assert_eq!(
        corte_tira.max.y, pantalla.max.y,
        "la tira no llega al borde de abajo: {corte_tira:?}"
    );
    assert_eq!(
        corte_tira.height(),
        BANDA_TIRA,
        "la tira no mide {BANDA_TIRA} px: {corte_tira:?}"
    );

    std::fs::remove_dir_all(&tmp).ok();
}

/// M4: las cinco ventanas flotantes caben en la ventana mínima.
///
/// Se abren con la ventana al mínimo y luego se miden en una pantalla
/// grande, donde egui no recorta nada: a 900×600 egui recorta cada
/// ventana a la pantalla —así que «caben» siempre— y lo que se pierde es
/// el contenido de abajo, que es justo lo que hay que evitar.
#[test]
fn las_ventanas_flotantes_caben_en_la_minima() {
    let (mut app, ctx, tmp) = demo("dialogos");
    app.show_shortcuts = true;
    app.show_about = true;
    app.show_animation = true;
    app.show_split = true;
    app.show_sprite_settings = true;

    let ids = [
        (t!("Atajos de teclado"), egui::Id::new("shortcuts_help")),
        (t!("Acerca de"), egui::Id::new("about")),
        (
            t!("Vista previa de animación"),
            egui::Id::new("animation_preview"),
        ),
        (t!("Dividir hoja"), egui::Id::new("split_sheet")),
        (t!("Ajustes de sprite"), egui::Id::new("Ajustes de sprite")),
    ];

    // Dos frames al mínimo (la primera pasada sólo guarda el área).
    let _ = frame(&mut app, &ctx);
    let _ = frame(&mut app, &ctx);
    for (titulo, id) in &ids {
        assert!(
            ctx.memory(|m| m.area_rect(*id)).is_some(),
            "«{titulo}» no abrió con la ventana al mínimo"
        );
    }

    // El tamaño natural, en una pantalla donde nada estorba.
    let sin_recorte = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(crate::VENTANA_MINIMA.0 * 2.0, crate::VENTANA_MINIMA.1 * 3.0),
    );
    let _ = frame_en(&mut app, &ctx, sin_recorte);
    let _ = frame_en(&mut app, &ctx, sin_recorte);

    let (min_ancho, min_alto) = crate::VENTANA_MINIMA;
    for (titulo, id) in &ids {
        let rect = ctx
            .memory(|m| m.area_rect(*id))
            .expect("las ventanas siguen abiertas");
        assert!(
            rect.width() <= min_ancho && rect.height() <= min_alto,
            "«{titulo}» mide {}×{} px y la ventana mínima sólo da {min_ancho}×{min_alto}: \
             lo que sobre se queda sin ver",
            rect.width(),
            rect.height()
        );
    }
    std::fs::remove_dir_all(&tmp).ok();
}

/// M4: `main.rs` promete exactamente este mínimo. Si alguien mueve uno
/// sin el otro, el probado y el prometido se separan —que era justo el
/// defecto—, y esta prueba se entera.
#[test]
fn main_promete_exactamente_la_ventana_que_se_prueba() {
    let main = include_str!("../main.rs");
    assert!(
        main.contains("with_min_inner_size(tp_app::VENTANA_MINIMA)"),
        "main.rs ya no promete `VENTANA_MINIMA`: el mínimo de la ventana \
         y el que prueban estas pruebas han dejado de ser el mismo"
    );
}
