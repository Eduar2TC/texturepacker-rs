//! Floating "Sprite settings" window: pivot editor for the selected sprites.

use super::{App, LogKind};
use eframe::egui;
use tp_core::types::Point2D;

const PRESETS: [(&str, f32, f32); 9] = [
    ("Centro", 0.5, 0.5),
    ("Arriba izq.", 0.0, 0.0),
    ("Arriba", 0.5, 0.0),
    ("Arriba der.", 1.0, 0.0),
    ("Izq.", 0.0, 0.5),
    ("Der.", 1.0, 0.5),
    ("Abajo izq.", 0.0, 1.0),
    ("Abajo", 0.5, 1.0),
    ("Abajo der.", 1.0, 1.0),
];

pub(super) fn sprite_settings_window(app: &mut App, ctx: &egui::Context) {
    let mut open = app.show_sprite_settings;
    egui::Window::new("Ajustes de sprite")
        .open(&mut open)
        .default_width(360.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("Pivot por defecto para sprites nuevos");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut app.config.default_pivot_x)
                        .range(0.0..=1.0)
                        .speed(0.01)
                        .prefix("X "),
                );
                ui.add(
                    egui::DragValue::new(&mut app.config.default_pivot_y)
                        .range(0.0..=1.0)
                        .speed(0.01)
                        .prefix("Y "),
                );
            });
            ui.separator();

            let indices = app.selected_sprite_indices();
            if indices.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Selecciona sprites en el panel izquierdo o en la vista previa.",
                    )
                    .weak(),
                );
                return;
            }
            ui.label(format!("{} sprite(s) seleccionado(s)", indices.len()));

            let (mut px, mut py) = match app.result.as_ref() {
                Some(out) => match out.result.sprites.get(indices[0]) {
                    Some(sprite) => (sprite.pivot.x, sprite.pivot.y),
                    None => return,
                },
                None => return,
            };
            ui.horizontal(|ui| {
                let x = ui
                    .add(
                        egui::DragValue::new(&mut px)
                            .range(0.0..=1.0)
                            .speed(0.01)
                            .prefix("X "),
                    )
                    .changed();
                let y = ui
                    .add(
                        egui::DragValue::new(&mut py)
                            .range(0.0..=1.0)
                            .speed(0.01)
                            .prefix("Y "),
                    )
                    .changed();
                if x || y {
                    apply_pivot(app, &indices, px, py);
                }
            });

            ui.label("Posiciones predefinidas");
            ui.horizontal_wrapped(|ui| {
                for (label, x, y) in PRESETS {
                    if ui.button(label).clicked() {
                        apply_pivot(app, &indices, x, y);
                    }
                }
            });

            ui.separator();

            // -------- 9-patch / 3-patch --------
            ui.heading("Bordes 9-patch");
            ui.label(
                egui::RichText::new(
                    "Barras [izq, arriba, der, abajo] en píxeles de la imagen original. \
                     Todo a 0 desactiva el 9-patch.",
                )
                .weak(),
            );
            let mut border = app
                .result
                .as_ref()
                .and_then(|out| out.result.sprites.get(indices[0]))
                .and_then(|s| s.border)
                .unwrap_or([0; 4]);
            let mut border_changed = false;
            ui.horizontal(|ui| {
                for (i, name) in ["L", "T", "R", "B"].iter().enumerate() {
                    border_changed |= ui
                        .add(
                            egui::DragValue::new(&mut border[i])
                                .range(0..=4096)
                                .prefix(format!("{name} ")),
                        )
                        .changed();
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("9-patch centro").clicked() {
                    border = [8, 8, 8, 8];
                    border_changed = true;
                }
                if ui.button("3-patch horizontal").clicked() {
                    border = [8, 0, 8, 0];
                    border_changed = true;
                }
                if ui.button("3-patch vertical").clicked() {
                    border = [0, 8, 0, 8];
                    border_changed = true;
                }
                if ui.button("Sin bordes").clicked() {
                    border = [0; 4];
                    border_changed = true;
                }
            });
            if ui
                .button("🔎 Detectar barras sólidas")
                .on_hover_text(
                    "Detecta los bordes 9-patch analizando las filas/columnas de color \
                     sólido del sprite original (ignora márgenes transparentes)",
                )
                .clicked()
            {
                app.detect_borders();
            }
            if border_changed {
                apply_border(app, &indices, border);
            }

            ui.separator();
            if ui
                .button("💾 Guardar pivots en pivots.json")
                .on_hover_text(
                    "Los pivots y los bordes 9-patch se reutilizarán en el siguiente publicado",
                )
                .clicked()
            {
                app.save_pivots();
            }
        });
    app.show_sprite_settings = open;
}

fn apply_pivot(app: &mut App, indices: &[usize], x: f32, y: f32) {
    let pivot = Point2D::new(x, y);
    if let Some(out) = &mut app.result {
        for &i in indices {
            if let Some(sprite) = out.result.sprites.get_mut(i) {
                sprite.pivot = pivot;
            }
        }
    } else {
        app.log(
            LogKind::Warning,
            "Publica el atlas antes de editar pivots.".into(),
        );
    }
}

/// Apply 9-patch borders to the selected sprites (all-zero clears the border).
fn apply_border(app: &mut App, indices: &[usize], border: [i32; 4]) {
    let border = if border == [0; 4] { None } else { Some(border) };
    if let Some(out) = &mut app.result {
        for &i in indices {
            if let Some(sprite) = out.result.sprites.get_mut(i) {
                sprite.border = border;
            }
        }
    } else {
        app.log(
            LogKind::Warning,
            "Publica el atlas antes de editar bordes 9-patch.".into(),
        );
    }
}
