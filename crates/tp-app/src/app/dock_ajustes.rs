//! F4: el dock de Ajustes se puede apartar, y su sitio lo recoge el lienzo.
//!
//! Con los dos paneles laterales abiertos el lienzo se quedaba en menos de
//! la mitad de la ventana y no había manera de ocultar el derecho. Aquí se
//! comprueba que quitar el dock le devuelve el ancho al lienzo, que `F9`
//! (y la casilla del menú «Ver») es la manera de hacerlo, y que por debajo
//! del umbral se pliega solo **una vez** —si se plegara en cada frame, la
//! misma tecla que lo devuelve no serviría para nada—.
//!
//! El pliegue por ancho es el único caso en que la app se quita un panel
//! sin que nadie lo pida, y no deja rastro alguno: aquí también se prueba
//! el aviso in-situ que lo cuenta con la tecla que lo devuelve (4.2).
//!
//! El test de que la ventana mínima sigue siendo usable sin el dock vive
//! en [`super::ventana_minima`].

use super::*;
use crate::testing::{rellenos_pintados, textos_pintados};

/// Un `RawInput` con la geometría dada y el reloj de `idle_input`.
fn pantalla(ancho: f32, alto: f32) -> eframe::egui::RawInput {
    let mut input = crate::testing::idle_input();
    input.screen_rect = Some(eframe::egui::Rect::from_min_size(
        eframe::egui::Pos2::ZERO,
        eframe::egui::vec2(ancho, alto),
    ));
    input
}

/// Un frame con la geometría dada y el reloj puesto a mano: el aviso del
/// pliegue vive de tiempo, así que el test lo lleva.
fn pantalla_en(ancho: f32, alto: f32, t: f64) -> eframe::egui::RawInput {
    let mut input = pantalla(ancho, alto);
    input.time = Some(t);
    input
}

fn frame(app: &mut App, ctx: &egui::Context, ancho: f32, alto: f32) -> eframe::egui::FullOutput {
    app.run_frame(ctx, pantalla(ancho, alto))
}

/// Un frame con una tecla pulsada (mismo mecanismo que los tests de F1).
fn pulsa(
    app: &mut App,
    ctx: &egui::Context,
    key: egui::Key,
    ancho: f32,
    alto: f32,
) -> eframe::egui::FullOutput {
    let mut input = pantalla(ancho, alto);
    input.events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    app.run_frame(ctx, input)
}

/// ¿Se pinta el título del dock? Es texto exacto: «Ajustes de sprite» es
/// otra ventana y no debe contar.
fn dock_pintado(salida: &eframe::egui::FullOutput) -> bool {
    textos_pintados(salida)
        .iter()
        .any(|(texto, _, _)| texto.trim() == "Ajustes")
}

/// El ancho de lo pintado con el color del lienzo: la zona que gana el
/// canvas cuando el dock se aparta (mismo mirador que el test de
/// superficies, que exige ≥25 % de la ventana).
fn ancho_lienzo(salida: &eframe::egui::FullOutput, lienzo: egui::Color32) -> f32 {
    rellenos_pintados(salida)
        .iter()
        .filter(|(color, _)| *color == lienzo)
        .map(|(_, rect)| rect.width())
        .fold(0.0, f32::max)
}

/// F4: el dock no es una pared fija: se aparta con `F9` y el ancho que
/// ocupaba vuelve al lienzo, que es de donde sale el espacio.
#[test]
fn quitar_el_dock_devuelve_su_sitio_al_lienzo_y_f9_lo_vuelve_a_poner() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);
    assert!(app.show_settings, "el dock arranca a la vista");

    let con_dock = frame(&mut app, &ctx, 1360.0, 860.0);
    assert!(dock_pintado(&con_dock), "si está pedido, se pinta");
    let superficie = superficies(ctx.style().visuals.dark_mode);
    let ancho_con = ancho_lienzo(&con_dock, superficie.lienzo);

    let sin_dock = pulsa(&mut app, &ctx, egui::Key::F9, 1360.0, 860.0);
    assert!(!app.show_settings, "F9 debe ocultar el dock");
    assert!(
        !dock_pintado(&sin_dock),
        "oculto no debe pintarse ni su título"
    );
    let ancho_sin = ancho_lienzo(&sin_dock, superficie.lienzo);
    assert!(
        ancho_sin >= ancho_con + 300.0,
        "el lienzo debe ganar el ancho del dock: {ancho_con:.0} de {ancho_sin:.0} px"
    );

    let otra_vez = pulsa(&mut app, &ctx, egui::Key::F9, 1360.0, 860.0);
    assert!(app.show_settings, "F9 es un conmutador, no sólo un apagado");
    assert!(dock_pintado(&otra_vez), "y el dock vuelve pintado");
    assert!(
        ancho_lienzo(&otra_vez, superficie.lienzo) <= ancho_sin,
        "con el dock de vuelta el lienzo no puede seguir ensanchado"
    );
}

/// F4: por debajo del umbral el dock se pliega solo al *cruzar* la raya.
/// Si se plegara en cada frame `F9` no serviría para recuperarlo con la
/// ventana todavía estrecha, que es justo lo que se le pide.
#[test]
fn el_dock_se_pliega_al_cruzar_el_umbral_y_f9_lo_devuelve() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);

    frame(&mut app, &ctx, 1360.0, 860.0);
    assert!(app.show_settings, "por encima del umbral se ve");

    let estrecho = frame(&mut app, &ctx, 900.0, 600.0);
    assert!(!app.show_settings, "al cruzar a 900 px se pliega solo");
    assert!(!dock_pintado(&estrecho), "plegado no se pinta ni el título");

    let devuelto = pulsa(&mut app, &ctx, egui::Key::F9, 900.0, 600.0);
    assert!(
        app.show_settings,
        "F9 debe devolverlo aunque la ventana siga estrecha"
    );
    assert!(dock_pintado(&devuelto), "y se pinta");

    let sigue = frame(&mut app, &ctx, 900.0, 600.0);
    assert!(
        app.show_settings,
        "no se vuelve a plegar hasta que se cruce de nuevo el umbral"
    );
    assert!(dock_pintado(&sigue), "sigue pintado en el frame siguiente");

    // Cruzar de nuevo —ancha y otra vez estrecha— vuelve a plegarlo.
    frame(&mut app, &ctx, 1360.0, 860.0);
    frame(&mut app, &ctx, 900.0, 600.0);
    assert!(
        !app.show_settings,
        "el segundo cruce del umbral vuelve a plegarlo"
    );
}

/// Texto del aviso del pliegue (4.2). El mismo que va al log, así que en
/// pantalla sólo se distingue por dónde cae: el registro vive abajo a la
/// izquierda y el recuadro, al pie derecha.
const AVISO: &str = "Ajustes oculto — F9";

/// 4.2: el pliegue por ancho es el único cambio que la app hace sola, y
/// no deja rastro —se va el dock, se va el botón y se va el título—: si
/// no avisa, el usuario no sabe ni que estaba ahí ni que `F9` lo devuelve.
/// El recuadro cae al pie derecho, lejos de la línea del log que dice lo
/// mismo (que es el registro, y sigue ahí).
#[test]
fn el_pliegue_por_ancho_deja_un_aviso_con_la_tecla_que_lo_devuelve() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);

    let ancha = frame(&mut app, &ctx, 1360.0, 860.0);
    assert!(
        app.aviso.is_none(),
        "por encima del umbral no hay nada que avisar"
    );
    assert!(
        textos_pintados(&ancha)
            .iter()
            .all(|(t, _, _)| !t.contains(AVISO)),
        "…ni nada de aviso pintado"
    );

    let estrecho = frame(&mut app, &ctx, 900.0, 600.0);
    assert!(!app.show_settings, "el dock se pliega solo");
    assert!(app.aviso.is_some(), "y el pliegue tiene que avisar");
    assert!(
        app.logs.iter().any(|l| l.text.contains(AVISO)),
        "como todo aviso, se apunta también en el log"
    );
    assert!(
        textos_pintados(&estrecho)
            .iter()
            .all(|(t, _, _)| !t.contains(AVISO)),
        "el recuadro mide su caja en este frame, así que aún no se ve"
    );

    let con_aviso = frame(&mut app, &ctx, 900.0, 600.0);
    let (_, rect, _) = textos_pintados(&con_aviso)
        .into_iter()
        .find(|(texto, rect, _)| texto.contains(AVISO) && rect.min.x > 500.0)
        .unwrap_or_else(|| panic!("el aviso debe salir al pie derecho de la ventana"));
    assert!(
        rect.min.y > 300.0,
        "…y abajo, no en mitad de la pantalla: {rect:?}"
    );
    assert!(
        app.logs.iter().any(|l| l.text.contains(AVISO)),
        "y la línea del log sigue siendo el registro"
    );
}

/// `F9` —y la casilla del menú «Ver», que gira el mismo conmutador— es un
/// gesto consciente: quien acaba de pulsarlo sabe qué hizo y no necesita
/// que la app se lo recuerde. El aviso es sólo para lo que nadie pidió.
#[test]
fn f9_no_avisa_de_lo_que_el_usuario_acaba_de_pedir() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);
    frame(&mut app, &ctx, 1360.0, 860.0);

    let oculto = pulsa(&mut app, &ctx, egui::Key::F9, 1360.0, 860.0);
    assert!(!app.show_settings, "F9 debe ocultar el dock");
    assert!(app.aviso.is_none(), "…sin avisar, que lo ha pedido él");
    assert!(
        textos_pintados(&oculto)
            .iter()
            .all(|(t, _, _)| !t.contains(AVISO)),
        "ni pintado"
    );

    let visible = pulsa(&mut app, &ctx, egui::Key::F9, 1360.0, 860.0);
    assert!(app.show_settings, "y lo vuelve a poner");
    assert!(app.aviso.is_none(), "volver a enseñarlo tampoco es noticia");
    assert!(
        textos_pintados(&visible)
            .iter()
            .all(|(t, _, _)| !t.contains(AVISO)),
        "ni pintado"
    );
}

/// 4.2: el aviso del pliegue no acompaña a un gesto sino a un cambio, y
/// el tiempo que pide no es el de leerlo sólo —que es lo que cuestan los
/// de 2,5 s— sino el de leerlo y decidir si se va a por `F9` mientras la
/// ventana sigue estrecha. Se va solo, sin que nadie toque nada.
#[test]
fn el_aviso_del_pliegue_dura_tres_segundos_y_se_va_solo() {
    let ctx = egui::Context::default();
    let mut app = App::new_for_testing(ctx.clone(), None);

    app.run_frame(&ctx, pantalla_en(1360.0, 860.0, 100.0));
    app.run_frame(&ctx, pantalla_en(900.0, 600.0, 100.2)); // el cruce
    assert!(app.aviso.is_some(), "el pliegue avisa");

    app.run_frame(&ctx, pantalla_en(900.0, 600.0, 103.1)); // 2,9 s después
    assert!(
        app.aviso.is_some(),
        "a los 2,9 s sigue: el de un gesto (2,5 s) ya se habría ido"
    );

    app.run_frame(&ctx, pantalla_en(900.0, 600.0, 103.4)); // 3,2 s después
    assert!(app.aviso.is_none(), "y a los 3,2 s se va solo");
}
