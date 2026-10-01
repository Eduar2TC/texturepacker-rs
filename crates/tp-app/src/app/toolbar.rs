//! Top tool bar: project actions, sprite set actions and publishing.
//!
//! Convenciones UX: iconos con texto (descubribles sin hover), acción
//! principal destacada a la derecha, atajos mostrados en los tooltips.

use super::{App, LogKind};
use eframe::egui;

/// La misma página que abre el botón «Tutorial» del original: la de
/// tutoriales de TexturePacker en el navegador del usuario.
pub(crate) const TUTORIAL_URL: &str = "https://www.codeandweb.com/texturepacker/tutorials";

pub(super) fn toolbar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            // --- Proyecto ---
            if ui
                .button("📂 Abrir")
                .on_hover_text("Abrir proyecto (.tpproj) — Ctrl+O")
                .clicked()
            {
                app.load_project();
            }
            if ui
                .button("💾 Guardar")
                .on_hover_text("Guardar proyecto (.tpproj) — Ctrl+S")
                .clicked()
            {
                app.save_project();
            }
            if ui
                .button("↺")
                .on_hover_text("Restablecer la configuración por defecto")
                .clicked()
            {
                app.reset_defaults();
            }
            ui.separator();

            // --- Sprites ---
            if ui
                .button("➕ Añadir")
                .on_hover_text("Añadir sprites al workspace")
                .clicked()
            {
                add_sprites_dialog(app);
            }
            let can_remove = !app.selected_paths.is_empty();
            if ui
                .add_enabled(can_remove, egui::Button::new("➖ Quitar"))
                .on_hover_text("Quitar los sprites seleccionados")
                .on_disabled_hover_text("Selecciona sprites en el panel izquierdo")
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
                .button("📁 Carpeta")
                .on_hover_text("Añadir carpeta inteligente (se sincroniza con el disco)")
                .clicked()
            {
                add_smart_folder_dialog(app);
            }
            ui.separator();

            // --- Herramientas ---
            let sprite_settings = egui::Button::new("⚙ Sprite").selected(app.show_sprite_settings);
            if ui
                .add(sprite_settings)
                .on_hover_text("Ajustes de sprite (pivots y bordes 9-patch)")
                .clicked()
            {
                app.show_sprite_settings = !app.show_sprite_settings;
            }
            let split = egui::Button::new("✂ Dividir").selected(app.show_split);
            if ui
                .add(split)
                .on_hover_text("Dividir una hoja (sprite sheet) en sprites individuales")
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
                .add_enabled(has_sprites, egui::Button::new("▶ Animación"))
                .on_hover_text("Vista previa de animación de los sprites seleccionados")
                .on_disabled_hover_text("Publica el sprite sheet para ver la animación")
                .clicked()
            {
                app.show_animation = true;
            }
            // Como en el original, «Tutorial» abre la página de tutoriales
            // en el navegador (egui-winit → `webbrowser`).
            if ui
                .button("Tutorial")
                .on_hover_text("Abre la página de tutoriales de TexturePacker en el navegador")
                .clicked()
            {
                ctx.open_url(egui::OpenUrl::new_tab(TUTORIAL_URL));
                app.log(
                    LogKind::Info,
                    "Abriendo la página de tutoriales en el navegador.".into(),
                );
            }

            // --- Acción principal, destacada a la derecha ---
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let publishing = app.running.is_some();
                let publish = egui::Button::new(
                    egui::RichText::new(if publishing {
                        "… Publicando"
                    } else {
                        "⏏ Publicar"
                    })
                    .strong(),
                );
                if ui
                    .add_enabled(!publishing, publish)
                    .on_hover_text("Empaquetar y exportar el sprite sheet — Ctrl+P")
                    .clicked()
                {
                    app.start_pack();
                }
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
            "Imágenes",
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
        app.log(LogKind::Info, format!("{added} sprite(s) añadido(s)."));
    } else {
        app.log(LogKind::Warning, "No se añadieron sprites nuevos.".into());
    }
}

pub(super) fn add_smart_folder_dialog(app: &mut App) {
    let Some(dir) = rfd::FileDialog::new().pick_folder() else {
        return;
    };
    if app.add_input(dir) {
        app.log(LogKind::Info, "Carpeta inteligente añadida.".into());
    } else {
        app.log(
            LogKind::Warning,
            "La carpeta ya está en el proyecto.".into(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::TUTORIAL_URL;

    #[test]
    fn tutorial_url_points_at_the_official_page() {
        assert_eq!(
            TUTORIAL_URL,
            "https://www.codeandweb.com/texturepacker/tutorials"
        );
    }
}
