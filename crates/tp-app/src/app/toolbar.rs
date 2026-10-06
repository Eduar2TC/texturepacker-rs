//! Top tool bar: project actions, sprite set actions and publishing.
//!
//! Convenciones UX: iconos con texto (descubribles sin hover), acción
//! principal destacada a la derecha, atajos mostrados en los tooltips.

use super::{App, LogKind};
use crate::i18n::t;
use eframe::egui;

/// Documentación que abre el botón «Tutorial»: el README del proyecto,
/// en una pestaña del navegador del usuario.
pub(crate) const TUTORIAL_URL: &str = "https://github.com/Eduar2TC/texturepacker-rs#readme";

pub(super) fn toolbar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            // --- Proyecto ---
            if ui
                .button(t!("📂 Abrir"))
                .on_hover_text(t!("Abrir proyecto (.tpproj o .tps) — Ctrl+O"))
                .clicked()
            {
                app.load_project();
            }
            if ui
                .button(t!("💾 Guardar"))
                .on_hover_text(t!("Guardar proyecto (.tpproj o .tps) — Ctrl+S"))
                .clicked()
            {
                app.save_project();
            }
            // «↺» tira toda la configuración: separado de «Guardar» para que
            // no se pueda golpear por reflejo y con confirmación previa
            // (review UI/UX C5).
            ui.separator();
            if ui
                .button("↺")
                .on_hover_text(t!(
                    "Restablecer todos los ajustes del proyecto a los valores por defecto"
                ))
                .clicked()
            {
                confirm_reset(app);
            }
            ui.separator();

            // --- Sprites ---
            if ui
                .button(t!("➕ Añadir"))
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
                // Orden visual del panel: tras quitar, el cursor queda en la
                // fila siguiente (igual que con la tecla Supr).
                let order: Vec<_> = app
                    .sprite_row_rects()
                    .iter()
                    .map(|(p, _)| p.clone())
                    .collect();
                app.remove_selected(&order);
            }
            if ui
                .button(t!("📁 Carpeta"))
                .on_hover_text(t!(
                    "Añadir carpeta inteligente (se sincroniza con el disco)"
                ))
                .clicked()
            {
                add_smart_folder_dialog(app);
            }
            ui.separator();

            // --- Herramientas ---
            let sprite_settings = egui::Button::new("⚙ Sprite").selected(app.show_sprite_settings);
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
            // «Tutorial» abre la documentación del proyecto en el
            // navegador (egui-winit → `webbrowser`).
            if ui
                .button(t!("Tutorial"))
                .on_hover_text(t!("Abre la documentación del proyecto en el navegador"))
                .clicked()
            {
                ctx.open_url(egui::OpenUrl::new_tab(TUTORIAL_URL));
                app.aviso(
                    LogKind::Info,
                    t!("Abriendo la documentación en el navegador.").into(),
                );
            }
            // «?» abre la ayuda de atajos (I2): los atajos que no viven en
            // un botón no eran descubribles desde la propia app.
            if ui
                .add(egui::Button::new("?").selected(app.show_shortcuts))
                .on_hover_text(format!("{} — F1", t!("Atajos de teclado")))
                .clicked()
            {
                app.show_shortcuts = !app.show_shortcuts;
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
                // Menú de publicación, como el del original: publicar solo
                // cuando algo cambió y una entrada para forzar la escritura.
                ui.menu_button("☰", |ui| {
                    let publishing = app.running.is_some();
                    if ui
                        .add_enabled(!publishing, egui::Button::new(t!("Publicar")))
                        .on_hover_text(t!("Empaquetar y exportar el sprite sheet — Ctrl+P"))
                        .clicked()
                    {
                        ui.close();
                        app.start_pack(false);
                    }
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

/// «↺» tira toda la configuración del proyecto: se pregunta con el diálogo
/// nativo del sistema y sólo si el usuario acepta se toca nada. El registro
/// de la app deja constancia del cambio en `reset_defaults` (review UI/UX C5).
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
    use super::TUTORIAL_URL;

    #[test]
    fn tutorial_url_points_at_the_project_readme() {
        assert_eq!(
            TUTORIAL_URL,
            "https://github.com/Eduar2TC/texturepacker-rs#readme"
        );
    }
}
