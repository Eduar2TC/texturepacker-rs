//! Ayuda de atajos de teclado (review UI/UX I2).
//!
//! La barra enseña el atajo de cada botón en su tooltip, pero los atajos
//! que no viven en un botón —zoom, flechas de la lista, Supr, Esc— no se
//! podían descubrir desde dentro de la app: no había menú, ni ayuda, ni
//! un «?». Esta ventana los lista de un vistazo y se abre con `F1`, con
//! `?` o con el botón «?» de la barra; `Esc` la cierra.

use super::App;
use crate::i18n::t;
use eframe::egui;

/// Una fila del diálogo: la acción y la tecla que la dispara. Las teclas
/// dicen lo que está escrito en el teclado, así que casi no se traducen;
/// sólo cambian las que tienen nombre («Supr», «AvPág»…), que sí.
type Fila = (&'static str, &'static str);

/// Ventana de ayuda. Como el resto de ventanas de la app, se va sola si
/// no está abierta y su `open` manda sobre el estado (el aspa la cierra).
pub(super) fn shortcuts_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_shortcuts {
        return;
    }
    let mut open = app.show_shortcuts;
    egui::Window::new(t!("Atajos de teclado"))
        .id(egui::Id::new("shortcuts_help"))
        .open(&mut open)
        .default_width(430.0)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            seccion(
                ui,
                t!("Proyecto"),
                &[
                    (t!("Abrir proyecto"), "Ctrl + O"),
                    (t!("Guardar proyecto"), "Ctrl + S"),
                    (t!("Publicar"), "Ctrl + P"),
                ],
            );
            seccion(
                ui,
                t!("Edición"),
                &[(t!("Deshacer el último cambio"), "Ctrl + Z")],
            );
            seccion(
                ui,
                t!("Vista"),
                &[
                    (t!("Acercar"), "+"),
                    (t!("Alejar"), "−"),
                    (t!("Zoom al 100% (tamaño real)"), "0"),
                    (t!("Ajustar"), "F"),
                    (t!("Zoom con la rueda"), t!("Ctrl + rueda")),
                    (t!("Mostrar Ajustes"), "F9"),
                ],
            );
            seccion(
                ui,
                t!("Lista de sprites"),
                &[
                    (t!("Mover el cursor"), t!("Flechas")),
                    (t!("Extender la selección"), t!("Mayús + flechas")),
                    (t!("Acumular la selección"), t!("Ctrl + flechas")),
                    (t!("Primera o última fila"), t!("Inicio / Fin")),
                    (t!("Página arriba o abajo"), t!("AvPág / RePág")),
                    (t!("Seleccionar todo"), "Ctrl + A"),
                    (t!("Quitar los sprites seleccionados"), t!("Supr")),
                ],
            );
            seccion(
                ui,
                t!("General"),
                &[
                    (t!("Cerrar la ventana"), "Esc"),
                    (t!("Esta ayuda"), "F1 / ?"),
                ],
            );
            ui.label(
                egui::RichText::new(t!(
                    "Mientras escribes en un campo, las teclas de la lista no se usan."
                ))
                .weak(),
            );
        });
    app.show_shortcuts = open;
}

/// Un bloque: título y sus filas en dos columnas (acción a la izquierda,
/// tecla a la derecha, en monoespaciada como en el resto de la app).
fn seccion(ui: &mut egui::Ui, titulo: &str, filas: &[Fila]) {
    ui.strong(titulo);
    egui::Grid::new(("atajos", titulo))
        .num_columns(2)
        .spacing([28.0, 4.0])
        .show(ui, |ui| {
            for (accion, tecla) in filas {
                ui.label(*accion);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(*tecla).monospace());
                });
                ui.end_row();
            }
        });
    // Entre secciones, no dentro: con ocho el diálogo ya no cabía en la
    // ventana mínima (M4) una vez añadida la fila de F9, y cuatro siguen
    // separando lo suficiente como para que se lean bloques.
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::texto_pintado;

    fn frame(
        app: &mut App,
        ctx: &egui::Context,
        mods: egui::Modifiers,
        eventos: Vec<egui::Event>,
    ) -> egui::FullOutput {
        app.run_frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1360.0, 860.0),
                )),
                modifiers: mods,
                events: eventos,
                ..egui::RawInput::default()
            },
        )
    }

    /// Una pulsación de tecla con los modificadores que la acompañan.
    fn press(app: &mut App, ctx: &egui::Context, key: egui::Key) {
        let mods = egui::Modifiers {
            shift: key == egui::Key::Questionmark,
            ..egui::Modifiers::NONE
        };
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

    /// I2: la ayuda no aparece sola, `F1` y «?» la abren y `Esc` (o `F1`
    /// otra vez) la cierran, que es como se comporta una ayuda de verdad.
    #[test]
    fn la_ayuda_se_abre_con_f1_o_interrogante_y_se_cierra_con_escape() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        assert!(!app.show_shortcuts, "la app arranca sin la ayuda");

        press(&mut app, &ctx, egui::Key::F1);
        assert!(app.show_shortcuts, "F1 debe abrir la ayuda");

        press(&mut app, &ctx, egui::Key::Escape);
        assert!(!app.show_shortcuts, "Esc debe cerrarla");

        press(&mut app, &ctx, egui::Key::Questionmark);
        assert!(app.show_shortcuts, "«?» debe abrirla");

        press(&mut app, &ctx, egui::Key::F1);
        assert!(
            !app.show_shortcuts,
            "F1 también la cierra: es un conmutador"
        );
    }

    /// La ayuda enseña los atajos que la app maneja de verdad: los del
    /// `Ctrl` de la barra y los del zoom, que no viven en ningún botón.
    #[test]
    fn el_dialogo_enseña_los_atajos_que_la_app_maneja() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);

        let cerrado = texto_pintado(&frame(&mut app, &ctx, egui::Modifiers::NONE, vec![]));
        assert!(
            !cerrado.contains("Ctrl + O"),
            "sin abrir no debe pintarse la ayuda: {cerrado}"
        );

        press(&mut app, &ctx, egui::Key::F1);
        let texto = texto_pintado(&frame(&mut app, &ctx, egui::Modifiers::NONE, vec![]));
        for atajo in [
            "Ctrl + O",
            "Ctrl + S",
            "Ctrl + P",
            "Ctrl + Z",
            "Ctrl + A",
            "Mayús + flechas",
            "Esc",
            "F1 / ?",
            "F9",
        ] {
            assert!(texto.contains(atajo), "falta el atajo {atajo}: {texto}");
        }
        for accion in [
            "Abrir proyecto",
            "Guardar proyecto",
            "Publicar",
            "Deshacer el último cambio",
            "Ajustar",
            "Seleccionar todo",
            "Esta ayuda",
        ] {
            assert!(texto.contains(accion), "falta la acción {accion}: {texto}");
        }
    }
}
