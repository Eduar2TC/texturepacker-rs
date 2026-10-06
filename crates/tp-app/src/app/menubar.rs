//! Menú de la aplicación y ventana «Acerca de» (review UI/UX M5).
//!
//! La barra de menús clásica del escritorio, con lo que la barra de
//! herramientas no cubre. De aquí salen tres cosas que no existían en
//! ninguna parte de la app: **«Salir»** (sólo la ✕ de la ventana cerraba),
//! **«Acerca de»** con la **versión** del programa, y la ayuda de atajos
//! como entrada de menú además de tecla.

use super::{toolbar::TUTORIAL_URL, App, LogKind};
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

/// Barra de menús, por encima de la barra de herramientas: «Archivo» con
/// las acciones sobre el proyecto y «Ayuda» con lo que hay que descubrir.
pub(super) fn menubar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            archivo(app, ui);
            ayuda(app, ui);
        });
    });
}

fn archivo(app: &mut App, ui: &mut egui::Ui) {
    ui.menu_button(t!("Archivo"), |ui| {
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
        // «Salir» no cierra: pide el cierre a la ventana, que es quien
        // decide si aún hay cambios sin guardar por preguntar (C2).
        if ui.button(t!("Salir")).clicked() {
            ui.close();
            salir(ui.ctx());
        }
    });
}

fn ayuda(app: &mut App, ui: &mut egui::Ui) {
    ui.menu_button(t!("Ayuda"), |ui| {
        if ui
            .add(egui::Button::new(t!("Atajos de teclado")).shortcut_text("F1"))
            .clicked()
        {
            ui.close();
            app.show_shortcuts = true;
        }
        if ui.button(t!("Tutorial")).clicked() {
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
    });
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
}
