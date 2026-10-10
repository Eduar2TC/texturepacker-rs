//! Barra de herramientas: sprites, herramientas del atlas y publicación.
//!
//! Convenciones UX (rediseño F1): aquí sólo vive lo que se hace sobre los
//! sprites —lo del proyecto está en la barra de menús y lo de ayuda en
//! «Ayuda», cada acción en una sola sede—, iconos con texto (descubribles
//! sin hover), acción principal destacada a la derecha y atajos en los
//! tooltips.

use super::{App, LogKind};
use crate::i18n::t;
use eframe::egui;

/// Documentación que abre «Ayuda → Tutorial»: el README del proyecto,
/// en una pestaña del navegador del usuario.
pub(crate) const TUTORIAL_URL: &str = "https://github.com/Eduar2TC/texturepacker-rs#readme";

pub(super) fn toolbar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            // --- Sprites ---
            if ui
                .button(t!("➕ Añadir sprites…"))
                .on_hover_text(t!("Añadir sprites al workspace"))
                .clicked()
            {
                add_sprites_dialog(app);
            }
            let can_remove = !app.selected_paths.is_empty();
            if ui
                .add_enabled(can_remove, egui::Button::new(t!("➖ Quitar")))
                .on_hover_text(t!("Quitar los sprites seleccionados"))
                .on_disabled_hover_text(t!("Selecciona sprites en el panel izquierdo"))
                .clicked()
            {
                quitar_seleccionados(app);
            }
            if ui
                .button(t!("📁 Añadir carpeta…"))
                .on_hover_text(t!(
                    "Añadir carpeta inteligente (se sincroniza con el disco)"
                ))
                .clicked()
            {
                add_smart_folder_dialog(app);
            }
            super::separador(ui);

            // --- Herramientas ---
            let sprite_settings =
                egui::Button::new(t!("⚙ Sprite")).selected(app.show_sprite_settings);
            if ui
                .add(sprite_settings)
                .on_hover_text(t!("Ajustes de sprite (pivots y bordes 9-patch)"))
                .clicked()
            {
                app.show_sprite_settings = !app.show_sprite_settings;
            }
            let split = egui::Button::new(t!("✂ Dividir")).selected(app.show_split);
            if ui
                .add(split)
                .on_hover_text(t!(
                    "Dividir una hoja (sprite sheet) en sprites individuales"
                ))
                .clicked()
            {
                app.show_split = !app.show_split;
            }
            let has_sprites = app
                .result
                .as_ref()
                .map(|o| !o.result.sprites.is_empty())
                .unwrap_or(false);
            if ui
                .add_enabled(has_sprites, egui::Button::new(t!("▶ Animación")))
                .on_hover_text(t!("Vista previa de animación de los sprites seleccionados"))
                .on_disabled_hover_text(t!("Publica el sprite sheet para ver la animación"))
                .clicked()
            {
                app.show_animation = true;
            }

            // --- Acción principal, destacada a la derecha ---
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let publishing = app.running.is_some();
                let publish = egui::Button::new(
                    egui::RichText::new(if publishing {
                        t!("… Publicando")
                    } else {
                        t!("⏏ Publicar")
                    })
                    .strong(),
                );
                if ui
                    .add_enabled(!publishing, publish)
                    .on_hover_text(t!("Empaquetar y exportar el sprite sheet — Ctrl+P"))
                    .clicked()
                {
                    app.start_pack(false);
                }
                // «…» reúne lo que no cabe en el botón de al lado. Tenía
                // también «Publicar», a 200 px del botón «⏏ Publicar» —dos
                // asientos para la misma acción—; se quita (rediseño F1) y
                // queda «Forzar publicación», que no tiene botón propio.
                // El glifo «⋯» no existe en las fuentes de egui —lo ha
                // cazado `glyph_guard`— y «☰» se leía como menú de la app.
                ui.menu_button("…", |ui| {
                    let publishing = app.running.is_some();
                    if ui
                        .add_enabled(!publishing, egui::Button::new(t!("Forzar publicación")))
                        .on_hover_text(t!("Reescribe los ficheros aunque nada haya cambiado"))
                        .clicked()
                    {
                        ui.close();
                        app.start_pack(true);
                    }
                })
                .response
                .on_hover_text(t!("Más opciones de publicación"));
                if publishing {
                    ui.spinner();
                }
            });
        });
    });
}

/// Quita los sprites seleccionados respetando el orden en que se ven en
/// el panel: tras quitar, el cursor queda en la fila siguiente (igual que
/// con la tecla `Supr`). Lo usan la barra de herramientas y el menú
/// «Edición», para que la misma acción no tenga dos dueños (rediseño F1).
pub(super) fn quitar_seleccionados(app: &mut App) {
    let order: Vec<_> = app
        .sprite_row_rects()
        .iter()
        .map(|(p, _)| p.clone())
        .collect();
    app.remove_selected(&order);
}

pub(super) fn add_sprites_dialog(app: &mut App) {
    let Some(files) = rfd::FileDialog::new()
        .add_filter(
            t!("Imágenes"),
            &[
                "png", "webp", "jpg", "jpeg", "tga", "bmp", "gif", "ico", "tiff", "tif", "dds",
                "qoi", "pbm", "pgm", "ppm", "pnm", "xbm", "xpm", "astc", "ktx", "ktx2", "basis",
                "psd", "svg", "svgz", "pkm", "pvr", "pvrtc", "ccz", "gz",
            ],
        )
        .pick_files()
    else {
        return;
    };
    let mut added = 0;
    for file in files {
        if app.add_input(file) {
            added += 1;
        }
    }
    if added > 0 {
        app.aviso(LogKind::Info, t!("{} sprite(s) añadido(s).", added));
    } else {
        app.aviso(
            LogKind::Warning,
            t!("No se añadieron sprites nuevos.").into(),
        );
    }
}

pub(super) fn add_smart_folder_dialog(app: &mut App) {
    let Some(dir) = rfd::FileDialog::new().pick_folder() else {
        return;
    };
    if app.add_input(dir) {
        app.aviso(LogKind::Info, t!("Carpeta inteligente añadida.").into());
    } else {
        app.aviso(
            LogKind::Warning,
            t!("La carpeta ya está en el proyecto.").into(),
        );
    }
}

/// Tira toda la configuración del proyecto: se pregunta con el diálogo
/// nativo del sistema y sólo si el usuario acepta se toca nada. El registro
/// de la app deja constancia del cambio en `reset_defaults` (review UI/UX
/// C5). Se invoca desde «Archivo → Restablecer la configuración», que es
/// donde vive desde el rediseño F1 (en la barra estaba junto a «Guardar»).
pub(super) fn confirm_reset(app: &mut App) {
    let respuesta = rfd::MessageDialog::new()
        .set_title(t!("Restablecer la configuración"))
        .set_description(t!(
            "Se descartarán los ajustes actuales y volverán a los valores por defecto. Los sprites no se tocan."
        ))
        .set_level(rfd::MessageLevel::Warning)
        .set_buttons(rfd::MessageButtons::OkCancel)
        .show();
    if respuesta == rfd::MessageDialogResult::Ok {
        app.reset_defaults();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::texto_pintado;

    #[test]
    fn tutorial_url_points_at_the_project_readme() {
        assert_eq!(
            TUTORIAL_URL,
            "https://github.com/Eduar2TC/texturepacker-rs#readme"
        );
    }

    /// F1: una acción no puede tener dos botones. Con los menús cerrados,
    /// lo que se pinta de «📂 Abrir», «💾 Guardar», «Tutorial» o «↺» sólo
    /// puede salir de la barra de herramientas: si vuelven a aparecer, es
    /// que se han vuelto a duplicar con la barra de menús.
    #[test]
    fn la_toolbar_no_repite_lo_que_esta_en_los_menus() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        let out = app.run_frame(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1360.0, 860.0),
                )),
                ..egui::RawInput::default()
            },
        );
        let pintado = texto_pintado(&out);
        for duplicado in ["📂 Abrir", "💾 Guardar", "Tutorial", "↺"] {
            assert!(
                !pintado.contains(duplicado),
                "«{duplicado}» está en la barra de herramientas y también en un \
                 menú —una acción, un asiento—: {pintado}"
            );
        }
        assert!(
            pintado.contains("⏏ Publicar"),
            "falta la acción principal de la barra: {pintado}"
        );
        assert!(
            pintado.contains("Añadir sprites"),
            "falta el primer botón de sprites: {pintado}"
        );
    }
}
