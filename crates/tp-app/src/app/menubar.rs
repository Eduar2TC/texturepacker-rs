//! Barra de menús y ventana «Acerca de» (review UI/UX M5; rediseño F1).
//!
//! Cuatro menús con **dueño único** de cada acción: «Archivo» para el ciclo
//! de vida del proyecto, «Edición» para deshacer y quitar, «Ver» para la
//! vista y «Ayuda» para lo que hay que descubrir. Nada de esto se repite en
//! la barra de herramientas, que sólo lleva acciones sobre los sprites.
//!
//! De aquí salen además tres cosas que no existían en ninguna parte de la
//! app: **«Salir»** (sólo la ✕ de la ventana cerraba), **«Acerca de»** con
//! la **versión** del programa, y la ayuda de atajos como entrada de menú
//! además de tecla.

use super::{
    preview,
    toolbar::{confirm_reset, TUTORIAL_URL},
    App, LogKind,
};
use crate::i18n::t;
use eframe::egui;

/// Nombre del programa. Es constante porque es el mismo en los dos
/// idiomas, y así la UI no pinta un literal crudo.
const NOMBRE: &str = "TexturePacker-RS";

/// Versión que enseña «Acerca de»: la misma que usan la CLI (`--version`)
/// y el exportador KTX, para que no se contradigan entre sí.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Repositorio del proyecto: issues, releases y código fuente.
const REPO_URL: &str = "https://github.com/Eduar2TC/texturepacker-rs";

/// Barra de menús, por encima de la barra de herramientas. Es la única
/// sede de las acciones que no son de sprite: la barra de herramientas de
/// abajo sólo lleva añadir/quitar, herramientas del atlas y publicar.
pub(super) fn menubar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(t!("Archivo"), |ui| archivo(app, ui));
            ui.menu_button(t!("Edición"), |ui| edicion(app, ui));
            ui.menu_button(t!("Ver"), |ui| ver(app, ui));
            ui.menu_button(t!("Ayuda"), |ui| ayuda(app, ui));
        });
    });
}

fn archivo(app: &mut App, ui: &mut egui::Ui) {
    if ui
        .add(egui::Button::new(t!("Abrir proyecto")).shortcut_text("Ctrl + O"))
        .clicked()
    {
        ui.close();
        app.load_project();
    }
    if ui
        .add(egui::Button::new(t!("Guardar proyecto")).shortcut_text("Ctrl + S"))
        .clicked()
    {
        ui.close();
        app.save_project();
    }
    ui.separator();
    // «Restablecer» vive aquí y no en la barra de herramientas: es
    // destructivo, pide confirmación (C5) y no es una acción del día a
    // día —junto a «Guardar» se podía golpear por reflejo (rediseño F1).
    if ui
        .add(egui::Button::new(t!("Restablecer la configuración")))
        .on_hover_text(t!(
            "Restablecer todos los ajustes del proyecto a los valores por defecto"
        ))
        .clicked()
    {
        ui.close();
        confirm_reset(app);
    }
    ui.separator();
    // «Salir» no cierra: pide el cierre a la ventana, que es quien
    // decide si aún hay cambios sin guardar por preguntar (C2).
    if ui.button(t!("Salir")).clicked() {
        ui.close();
        salir(ui.ctx());
    }
}

/// «Edición»: el deshacer no tenía ninguna sede visible —sólo `Ctrl+Z`—
/// y quitar sprites se hacía desde la barra de herramientas o con `Supr`
/// (rediseño F1). No se ofrece «Rehacer» porque el código no lo tiene.
fn edicion(app: &mut App, ui: &mut egui::Ui) {
    if ui
        .add_enabled(
            !app.deshacer.is_empty(),
            egui::Button::new(t!("Deshacer el último cambio")).shortcut_text("Ctrl + Z"),
        )
        .clicked()
    {
        ui.close();
        app.deshacer();
    }
    ui.separator();
    if ui
        .add_enabled(
            !app.selected_paths.is_empty(),
            egui::Button::new(t!("Quitar los sprites seleccionados")).shortcut_text(t!("Supr")),
        )
        .clicked()
    {
        ui.close();
        super::toolbar::quitar_seleccionados(app);
    }
}

/// «Ver»: zoom de la vista y el panel inferior. El chevron del panel
/// sigue ahí como control local —como en GIMP o Krita—, pero el menú es
/// la vía canónica y descubrible (rediseño F1).
fn ver(app: &mut App, ui: &mut egui::Ui) {
    if ui
        .add(egui::Button::new(t!("Acercar")).shortcut_text("+"))
        .clicked()
    {
        ui.close();
        preview::zoom_step(app, 1);
    }
    if ui
        .add(egui::Button::new(t!("Alejar")).shortcut_text("−"))
        .clicked()
    {
        ui.close();
        preview::zoom_step(app, -1);
    }
    if ui
        .add(egui::Button::new(t!("Zoom al 100% (tamaño real)")).shortcut_text("0"))
        .clicked()
    {
        ui.close();
        app.zoom = 1.0;
        app.auto_fit = false;
    }
    if ui
        .add(egui::Button::new(t!("Ajustar")).shortcut_text("F"))
        .clicked()
    {
        ui.close();
        app.fit_zoom();
        app.auto_fit = false;
    }
    ui.separator();
    // El menú marca lo que se ve, no lo que se pliega: la casilla está
    // «marcada» mientras el panel esté a la vista.
    let mut visible = !app.bottom_collapsed;
    if ui.checkbox(&mut visible, t!("Panel inferior")).changed() {
        app.bottom_collapsed = !visible;
    }
    // Mismo patrón que la de arriba: la casilla refleja el dock tal y
    // como está pintado, que es lo que decide si se plegó solo por ancho.
    let mut dock = app.show_settings;
    if ui.checkbox(&mut dock, t!("Mostrar Ajustes")).changed() {
        app.show_settings = dock;
    }
}

fn ayuda(app: &mut App, ui: &mut egui::Ui) {
    if ui
        .add(egui::Button::new(t!("Atajos de teclado")).shortcut_text("F1"))
        .clicked()
    {
        ui.close();
        app.show_shortcuts = true;
    }
    if ui
        .add(egui::Button::new(t!("Tutorial")))
        .on_hover_text(t!("Abre la documentación del proyecto en el navegador"))
        .clicked()
    {
        ui.close();
        ui.ctx().open_url(egui::OpenUrl::new_tab(TUTORIAL_URL));
        app.aviso(
            LogKind::Info,
            t!("Abriendo la documentación en el navegador.").into(),
        );
    }
    ui.separator();
    if ui.button(t!("Acerca de")).clicked() {
        ui.close();
        app.show_about = true;
    }
}

/// «Salir»: pide a la ventana que se cierre y deja que el resto del camino
/// haga su trabajo. Tramitarlo aquí a pelo se saltaría el diálogo «hay
/// cambios sin guardar» del C2, que vive en la respuesta a ese pedido.
pub(super) fn salir(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
}

/// «Acerca de»: nombre, versión, qué es esto, enlaces y licencia. Es el
/// hueco del review M5 —«sin «Acerca de» ni versión visible»—.
pub(super) fn about_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_about {
        return;
    }
    let mut open = app.show_about;
    egui::Window::new(t!("Acerca de"))
        .id(egui::Id::new("about"))
        .open(&mut open)
        .default_width(380.0)
        // Primera apertura en el centro de la ventana, como un diálogo de
        // información que se cierra y no se arrastra (si el usuario lo
        // mueve, egui recuerda su sitio).
        .default_pos(ctx.content_rect().center() - egui::vec2(190.0, 140.0))
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.heading(NOMBRE);
            ui.label(format!("{} {VERSION}", t!("Versión")));
            ui.add_space(8.0);
            ui.label(t!(
                "Una aplicación de escritorio en Rust para generar atlas de texturas: se maneja desde la CLI, desde la interfaz o desde un archivo de proyecto."
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.hyperlink_to(t!("Repositorio"), REPO_URL);
                ui.hyperlink_to(t!("Documentación"), TUTORIAL_URL);
            });
            ui.add_space(4.0);
            ui.label(egui::RichText::new(t!("Licencia MIT")).weak());
        });
    app.show_about = open;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::texto_pintado;

    fn frame(app: &mut App, ctx: &egui::Context) -> egui::FullOutput {
        app.run_frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1360.0, 860.0),
                )),
                ..egui::RawInput::default()
            },
        )
    }

    /// Un frame en el que el sistema pide cerrar la ventana (lo que haría
    /// el backend tras recibir el pedido de «Salir»).
    fn input_cierre() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1360.0, 860.0),
            )),
            viewports: [(
                egui::ViewportId::ROOT,
                egui::ViewportInfo {
                    events: vec![egui::ViewportEvent::Close],
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..egui::RawInput::default()
        }
    }

    fn pide(out: &egui::FullOutput, cmd: egui::ViewportCommand) -> bool {
        out.viewport_output
            .values()
            .any(|v| v.commands.contains(&cmd))
    }

    /// M5: la barra de menús existe y «Acerca de» enseña el nombre y la
    /// versión del programa. La versión no aparece en ningún otro sitio, así
    /// que si se cae del diálogo, la prueba lo ve.
    #[test]
    fn el_acerca_de_enseña_el_nombre_y_la_versión() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);

        let barra = texto_pintado(&frame(&mut app, &ctx));
        assert!(barra.contains("Archivo"), "falta «Archivo»: {barra}");
        assert!(barra.contains("Ayuda"), "falta «Ayuda»: {barra}");
        assert!(
            !barra.contains(VERSION),
            "sin abrir «Acerca de» no debe verse la versión"
        );

        app.show_about = true;
        // La ventana se asienta en su primer frame —sólo deja sus
        // placeholders— y el contenido se ve a partir del siguiente, que es
        // el camino que recorre la ventana real.
        let _ = frame(&mut app, &ctx);
        let texto = texto_pintado(&frame(&mut app, &ctx));
        assert!(
            texto.contains(VERSION),
            "el «Acerca de» debe enseñar la versión: {texto}"
        );
        assert!(
            texto.contains(NOMBRE),
            "el «Acerca de» debe enseñar el nombre: {texto}"
        );
        assert!(
            texto.contains("Acerca de"),
            "el diálogo debe llevar su título: {texto}"
        );
    }

    /// M5: «Salir» pide el cierre a la ventana en vez de cerrarla a pelo,
    /// de modo que con cambios sin guardar manda el diálogo del C2 y no la
    /// app: el cierre se rechaza hasta que el usuario decida.
    #[test]
    fn salir_pide_cerrar_y_con_cambios_pendientes_no_se_salta_la_pregunta() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        app.config.padding = 5;
        assert!(app.is_dirty(), "el test sólo tiene sentido con cambios");

        salir(&ctx);
        assert!(
            !app.exit_confirmed,
            "«Salir» no debe dar por bueno el cierre: eso saltaría la pregunta",
        );

        let out = app.run_frame(&ctx, input_cierre());

        assert!(
            pide(&out, egui::ViewportCommand::Close),
            "«Salir» debe pedir a la ventana que se cierre"
        );
        assert!(
            pide(&out, egui::ViewportCommand::CancelClose),
            "con cambios sin guardar ese cierre debe rechazarse"
        );
        assert!(app.exit_pending, "y debe abrirse la pregunta");
    }

    /// Pinta sólo el contenido de un menú —sin el botón que lo abre— para
    /// poder mirar sus entradas. Los textos se unen con espacios porque un
    /// rótulo largo puede llegar a egui en varios trozos.
    fn pinta_menu(app: &mut App, ctx: &egui::Context, menu: fn(&mut App, &mut egui::Ui)) -> String {
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1360.0, 860.0),
                )),
                ..egui::RawInput::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| menu(app, ui));
            },
        );
        crate::testing::textos_pintados(&out)
            .into_iter()
            .map(|(texto, _, _)| texto)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// F1: «↺ Restablecer» estaba en la barra de herramientas junto a
    /// «Guardar» —fácil de golpear por reflejo— y «Salir» sólo existía en
    /// el menú. El menú es ahora la única sede de las acciones de proyecto.
    #[test]
    fn el_menu_archivo_lleva_abrir_guardar_restablecer_y_salir() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        let texto = pinta_menu(&mut app, &ctx, archivo);
        for esperado in [
            "Abrir proyecto",
            "Guardar proyecto",
            "Restablecer la configuración",
            "Salir",
        ] {
            assert!(
                texto.contains(esperado),
                "«{esperado}» falta en «Archivo»: {texto}"
            );
        }
    }

    /// F1: el deshacer (C6) no tenía ningún botón —sólo `Ctrl+Z`— y quitar
    /// sprites se hacía desde la barra o con `Supr`. El menú «Edición» es
    /// su sede visible, y con la pila vacía la entrada se pinta deshabilitada.
    #[test]
    fn el_menu_edicion_expone_el_deshacer_que_no_tiene_boton() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        assert!(
            app.deshacer.is_empty(),
            "la prueba empieza sin nada que deshacer"
        );
        let texto = pinta_menu(&mut app, &ctx, edicion);
        for esperado in [
            "Deshacer el último cambio",
            "Ctrl + Z",
            "Quitar los sprites seleccionados",
            "Supr",
        ] {
            assert!(
                texto.contains(esperado),
                "«{esperado}» falta en «Edición»: {texto}"
            );
        }
    }

    /// F1: el zoom sólo se controlaba con rueda y botones de la barra de
    /// zoom, y el panel inferior sólo con su chevron. «Ver» reúne las dos
    /// cosas con sus atajos, que es como se descubren. F4 añade el dock
    /// de Ajustes, que hasta entonces no se podía ocultar.
    #[test]
    fn el_menu_ver_expone_el_zoom_el_panel_inferior_y_el_dock() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        let texto = pinta_menu(&mut app, &ctx, ver);
        for esperado in [
            "Acercar",
            "Alejar",
            "Zoom al 100%",
            "Ajustar",
            "Panel inferior",
            "Mostrar Ajustes",
        ] {
            assert!(
                texto.contains(esperado),
                "«{esperado}» falta en «Ver»: {texto}"
            );
        }
        assert!(
            !app.bottom_collapsed,
            "el panel inferior arranca visible y el menú debe marcarlo"
        );
        assert!(
            app.show_settings,
            "el dock de Ajustes arranca visible y el menú debe marcarlo"
        );
    }

    /// F4: la casilla no sólo se pinta —si no gira el dock, el menú miente.
    /// Se localiza su rectángulo en un frame y se le manda un clic real
    /// (movimiento + pulsación + levantación) en el siguiente.
    #[test]
    fn la_casilla_de_ver_gira_el_dock_de_ajustes() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        assert!(app.show_settings, "el dock arranca a la vista");

        let base = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1360.0, 860.0),
            )),
            ..egui::RawInput::default()
        };
        let pinta = |app: &mut App, entrada: egui::RawInput| {
            ctx.run(entrada, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| ver(app, ui));
            })
        };

        let pintado = pinta(&mut app, base());
        let (_, rect, _) = crate::testing::textos_pintados(&pintado)
            .into_iter()
            .find(|(texto, _, _)| texto.trim() == "Mostrar Ajustes")
            .expect("la casilla «Mostrar Ajustes» debe pintarse en «Ver»");
        let pos = rect.center();

        for (vez, esperado) in [(1, false), (2, true)] {
            let mut entrada = base();
            entrada.events = vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            let _ = pinta(&mut app, entrada);
            assert_eq!(
                app.show_settings, esperado,
                "el clic {vez} de la casilla no gira el dock"
            );
        }
    }
}
