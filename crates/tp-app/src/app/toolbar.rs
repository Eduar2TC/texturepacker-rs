//! Top tool bar: project actions, sprite set actions and publishing.

use super::{App, LogKind};
use eframe::egui;

pub(super) fn toolbar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.strong("TexturePacker-RS");
            ui.separator();

            if ui
                .button("Abrir")
                .on_hover_text("Abrir proyecto (.tpproj)")
                .clicked()
            {
                app.load_project();
            }
            if ui
                .button("Guardar")
                .on_hover_text("Guardar proyecto (.tpproj)")
                .clicked()
            {
                app.save_project();
            }
            if ui
                .button("↺")
                .on_hover_text("Restablecer la configuración")
                .clicked()
            {
                app.reset_defaults();
            }
            ui.separator();

            if ui.button("➕").on_hover_text("Añadir sprites").clicked() {
                add_sprites_dialog(app);
            }
            let can_remove = !app.selected_paths.is_empty();
            if ui
                .add_enabled(can_remove, egui::Button::new("➖"))
                .on_hover_text("Quitar los sprites seleccionados")
                .on_disabled_hover_text("Selecciona sprites en el panel izquierdo")
                .clicked()
            {
                app.remove_selected();
            }
            if ui
                .button("Carpeta")
                .on_hover_text("Añadir carpeta inteligente")
                .clicked()
            {
                add_smart_folder_dialog(app);
            }
            ui.separator();

            let sprite_settings = egui::Button::new("⚙").selected(app.show_sprite_settings);
            if ui
                .add(sprite_settings)
                .on_hover_text("Ajustes de sprite (pivots)")
                .clicked()
            {
                app.show_sprite_settings = !app.show_sprite_settings;
            }
            ui.separator();

            if ui
                .add_enabled(app.running.is_none(), egui::Button::new("Publicar"))
                .on_hover_text("Empaquetar y exportar el sprite sheet")
                .clicked()
            {
                app.start_pack();
            }
            if app.running.is_some() {
                ui.spinner();
                ui.label("Publicando…");
            }

            ui.separator();
            let split = egui::Button::new("✂").selected(app.show_split);
            if ui
                .add(split)
                .on_hover_text("Dividir hoja (sprite sheet) en sprites individuales")
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
                .add_enabled(has_sprites, egui::Button::new("▶"))
                .on_hover_text("Vista previa de animación")
                .on_disabled_hover_text("Publica el sprite sheet para ver la animación")
                .clicked()
            {
                app.show_animation = true;
            }

            if let Some(path) = &app.project_path {
                ui.separator();
                ui.label(egui::RichText::new(path.display().to_string()).weak());
            }
        });
    });
}

fn add_sprites_dialog(app: &mut App) {
    let Some(files) = rfd::FileDialog::new()
        .add_filter(
            "Imágenes",
            &[
                "png", "webp", "jpg", "jpeg", "tga", "bmp", "gif", "ico", "tiff", "tif", "dds",
                "qoi",
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

fn add_smart_folder_dialog(app: &mut App) {
    let Some(dir) = rfd::FileDialog::new().pick_folder() else {
        return;
    };
    if app.add_input(dir.clone()) {
        app.log(
            LogKind::Info,
            format!("Carpeta inteligente añadida: {}", dir.display()),
        );
    } else {
        app.log(LogKind::Warning, "La carpeta ya estaba añadida.".into());
    }
}
