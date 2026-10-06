//! Floating "Sprite settings" window: pivot editor for the selected sprites.

use super::{App, LogKind};
use crate::i18n::t;
use eframe::egui;
use tp_core::types::Point2D;

fn presets() -> [(&'static str, f32, f32); 9] {
    [
        (t!("Centro"), 0.5, 0.5),
        (t!("Arriba izq."), 0.0, 0.0),
        (t!("Arriba"), 0.5, 0.0),
        (t!("Arriba der."), 1.0, 0.0),
        (t!("Izq."), 0.0, 0.5),
        (t!("Der."), 1.0, 0.5),
        (t!("Abajo izq."), 0.0, 1.0),
        (t!("Abajo"), 0.5, 1.0),
        (t!("Abajo der."), 1.0, 1.0),
    ]
}

pub(super) fn sprite_settings_window(app: &mut App, ctx: &egui::Context) {
    let mut open = app.show_sprite_settings;
    egui::Window::new(t!("Ajustes de sprite"))
        .open(&mut open)
        .default_width(360.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(t!("Pivot por defecto para sprites nuevos"));
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
                    egui::RichText::new(t!(
                        "Selecciona sprites en el panel izquierdo o en la vista previa."
                    ))
                    .weak(),
                );
                return;
            }
            ui.label(t!("{} sprite(s) seleccionado(s)", indices.len()));

            let (mut px, mut py) = match app.result.as_ref() {
                Some(out) => match out.result.sprites.get(indices[0]) {
                    Some(sprite) => (sprite.pivot.x, sprite.pivot.y),
                    None => return,
                },
                None => return,
            };
            // Vista del sprite con el pivot arrastrable: el original abre un
            // editor visual y la cruz se mueve con el ratón en píxeles.
            let view = pivot_view(app, indices[0]);
            if let Some(view) = view {
                let tex_id = app.textures.get(view.page).map(|t| t.id());
                if let Some((nx, ny)) = pivot_preview(ui, &view, tex_id) {
                    apply_pivot(app, &indices, nx, ny);
                    px = nx;
                    py = ny;
                }
            }
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
            // Píxeles absolutos sobre la imagen original (la unidad que usa
            // el original para describir un pivot).
            if let Some(view) = view {
                ui.horizontal(|ui| {
                    let (mut px_px, mut py_px) = (px * view.raw_w as f32, py * view.raw_h as f32);
                    let x = ui
                        .add(
                            egui::DragValue::new(&mut px_px)
                                .range(0.0..=view.raw_w as f32)
                                .speed(1.0)
                                .fixed_decimals(0)
                                .prefix("X ")
                                .suffix(" px"),
                        )
                        .changed();
                    let y = ui
                        .add(
                            egui::DragValue::new(&mut py_px)
                                .range(0.0..=view.raw_h as f32)
                                .speed(1.0)
                                .fixed_decimals(0)
                                .prefix("Y ")
                                .suffix(" px"),
                        )
                        .changed();
                    if x || y {
                        apply_pivot(
                            app,
                            &indices,
                            (px_px / view.raw_w as f32).clamp(0.0, 1.0),
                            (py_px / view.raw_h as f32).clamp(0.0, 1.0),
                        );
                    }
                    ui.label(
                        egui::RichText::new(t!("Imagen de {} × {} px", view.raw_w, view.raw_h))
                            .weak(),
                    );
                });
            }

            ui.label(t!("Posiciones predefinidas"));
            ui.horizontal_wrapped(|ui| {
                for (label, x, y) in presets() {
                    if ui.button(label).clicked() {
                        apply_pivot(app, &indices, x, y);
                    }
                }
            });

            ui.separator();

            // -------- 9-patch / 3-patch --------
            ui.heading(t!("Bordes 9-patch"));
            ui.label(
                egui::RichText::new(t!(
                    "Barras [izq, arriba, der, abajo] en píxeles de la imagen original. \
                     Todo a 0 desactiva el 9-patch."
                ))
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
                if ui.button(t!("9-patch centro")).clicked() {
                    border = [8, 8, 8, 8];
                    border_changed = true;
                }
                if ui.button(t!("3-patch horizontal")).clicked() {
                    border = [8, 0, 8, 0];
                    border_changed = true;
                }
                if ui.button(t!("3-patch vertical")).clicked() {
                    border = [0, 8, 0, 8];
                    border_changed = true;
                }
                if ui.button(t!("Sin bordes")).clicked() {
                    border = [0; 4];
                    border_changed = true;
                }
            });
            if ui
                .button(t!("🔎 Detectar barras sólidas"))
                .on_hover_text(t!(
                    "Detecta los bordes 9-patch analizando las filas/columnas de color \
                     sólido del sprite original (ignora márgenes transparentes)"
                ))
                .clicked()
            {
                app.detect_borders();
            }
            if border_changed {
                apply_border(app, &indices, border);
            }

            ui.separator();
            if ui
                .button(t!("💾 Guardar pivots en pivots.json"))
                .on_hover_text(t!(
                    "Escribe pivots.json y borders.json junto a los sprites, para que \
                     el CLI (y otros proyectos) usen los mismos pivots. En la GUI ya \
                     se aplican solos y viajan en el .tpproj."
                ))
                .clicked()
            {
                app.save_pivots();
            }
        });
    app.show_sprite_settings = open;
}

pub(super) fn apply_pivot(app: &mut App, indices: &[usize], x: f32, y: f32) {
    let pivot = Point2D::new(x, y);
    if let Some(out) = &mut app.result {
        for &i in indices {
            if let Some(sprite) = out.result.sprites.get_mut(i) {
                sprite.pivot = pivot;
                app.config.pivot_overrides.insert(sprite.id.clone(), pivot);
            }
        }
    } else {
        app.aviso(
            LogKind::Warning,
            t!("Publica el atlas antes de editar pivots.").into(),
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
                match border {
                    Some(b) => {
                        app.config.border_overrides.insert(sprite.id.clone(), b);
                    }
                    None => {
                        app.config.border_overrides.remove(sprite.id.as_str());
                    }
                }
            }
        }
    } else {
        app.aviso(
            LogKind::Warning,
            t!("Publica el atlas antes de editar bordes 9-patch.").into(),
        );
    }
}

// ---------------------------------------------------------------------------
// Vista del pivot
// ---------------------------------------------------------------------------

/// Datos que la vista del pivot necesita para dibujarse: todo copiado del
/// resultado para no mantener un préstamo sobre `app.result` mientras la
/// ventana edita valores.
#[derive(Clone, Copy)]
struct PivotView {
    /// Tamaño de la imagen original (el pivot se mide dentro de él).
    raw_w: i32,
    raw_h: i32,
    /// Origen del recorte dentro de la imagen original.
    offset_x: i32,
    offset_y: i32,
    /// Tamaño del recorte en orientación de imagen.
    trimmed_w: i32,
    trimmed_h: i32,
    /// Frame visible dentro del atlas.
    frame: tp_core::types::Rect,
    /// Página del atlas que contiene el frame y sus dimensiones.
    page: usize,
    page_w: i32,
    page_h: i32,
    rotated: bool,
    pivot: Point2D,
}

fn pivot_view(app: &App, index: usize) -> Option<PivotView> {
    let out = app.result.as_ref()?;
    let sprite = out.result.sprites.get(index)?;
    let page = sprite.atlas_page_index.max(0) as usize;
    let info = out.pages.get(page)?;
    let trimmed = sprite.trimmed_bounds;
    Some(PivotView {
        raw_w: sprite.raw_width.max(1),
        raw_h: sprite.raw_height.max(1),
        offset_x: sprite.offset_x,
        offset_y: sprite.offset_y,
        trimmed_w: trimmed.width.max(0),
        trimmed_h: trimmed.height.max(0),
        frame: sprite.visible_frame,
        page,
        page_w: info.width.max(1),
        page_h: info.height.max(1),
        rotated: sprite.is_rotated,
        pivot: sprite.pivot,
    })
}

/// Posición del pivot dentro de la imagen original, en píxeles.
fn pivot_px(pivot: Point2D, raw_w: i32, raw_h: i32) -> (f32, f32) {
    (pivot.x * raw_w.max(1) as f32, pivot.y * raw_h.max(1) as f32)
}

/// Dibuja el sprite con su marco original, el recorte y la cruz del pivot.
/// Devuelve el pivot nuevo (normalizado) si el usuario lo arrastró.
fn pivot_preview(
    ui: &mut egui::Ui,
    view: &PivotView,
    tex: Option<egui::TextureId>,
) -> Option<(f32, f32)> {
    let height = 190.0_f32.min(ui.available_height().max(90.0));
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(160.0), height),
        egui::Sense::drag(),
    );
    response
        .clone()
        .on_hover_text(t!("Arrastra para colocar el pivot."))
        .on_hover_cursor(egui::CursorIcon::Crosshair);

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(45));

    let (rw, rh) = (view.raw_w as f32, view.raw_h as f32);
    let scale = (((rect.width() - 24.0) / rw).min((rect.height() - 24.0) / rh))
        .clamp(0.05, 16.0)
        .min(rect.width().max(1.0));
    let orig = egui::Rect::from_center_size(rect.center(), egui::vec2(rw * scale, rh * scale));

    // Marco de la imagen original: dentro vive el pivot.
    painter.rect_stroke(
        orig,
        0.0,
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(120)),
        egui::StrokeKind::Inside,
    );

    // Recorte visible, colocado en su sitio dentro de la imagen original.
    if view.trimmed_w > 0 && view.trimmed_h > 0 {
        let dst = egui::Rect::from_min_size(
            orig.min
                + egui::vec2(
                    view.offset_x.max(0) as f32 * scale,
                    view.offset_y.max(0) as f32 * scale,
                ),
            egui::vec2(view.trimmed_w as f32 * scale, view.trimmed_h as f32 * scale),
        );
        if let Some(tex) = tex {
            draw_pivot_sprite(&painter, tex, view, dst);
        }
    }

    // Cruz del pivot (roja con borde blanco para verse sobre cualquier fondo).
    let (px, py) = pivot_px(view.pivot, view.raw_w, view.raw_h);
    let p = orig.min + egui::vec2(px * scale, py * scale);
    let cross = egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(255, 60, 60));
    painter.line_segment(
        [egui::pos2(p.x - 8.0, p.y), egui::pos2(p.x + 8.0, p.y)],
        cross,
    );
    painter.line_segment(
        [egui::pos2(p.x, p.y - 8.0), egui::pos2(p.x, p.y + 8.0)],
        cross,
    );
    painter.circle_filled(p, 3.0, egui::Color32::from_rgb(255, 60, 60));
    painter.circle_stroke(p, 3.0, egui::Stroke::new(1.0_f32, egui::Color32::WHITE));

    if !response.dragged() {
        return None;
    }
    let pos = response.interact_pointer_pos()?;
    let nx = ((pos.x - orig.min.x) / (rw * scale)).clamp(0.0, 1.0);
    let ny = ((pos.y - orig.min.y) / (rh * scale)).clamp(0.0, 1.0);
    // Umbral: un tirón de menos de medio píxel no reescribe nada.
    let half = 0.5 / (rw * scale).max(1.0);
    let half_y = 0.5 / (rh * scale).max(1.0);
    if (nx - view.pivot.x).abs() < half && (ny - view.pivot.y).abs() < half_y {
        return None;
    }
    Some((nx, ny))
}

/// Pinta el frame dentro de la vista (con la rotación de 90° del atlas).
fn draw_pivot_sprite(
    painter: &egui::Painter,
    tex: egui::TextureId,
    view: &PivotView,
    dst: egui::Rect,
) {
    let f = view.frame;
    if f.width <= 0 || f.height <= 0 {
        return;
    }
    let (pw, ph) = (view.page_w as f32, view.page_h as f32);
    if !view.rotated {
        let uv = egui::Rect::from_min_max(
            egui::pos2(f.x as f32 / pw, f.y as f32 / ph),
            egui::pos2((f.x + f.width) as f32 / pw, (f.y + f.height) as f32 / ph),
        );
        painter.image(tex, dst, uv, egui::Color32::WHITE);
        return;
    }
    // Rotado 90° en el atlas: malla con UV por esquina, como en la vista de
    // animación (local (lx, ly) → atlas (x + H − ly, y + lx)).
    let (x, y) = (f.x as f32, f.y as f32);
    let (w, h) = (view.trimmed_w as f32, view.trimmed_h as f32);
    let uv = |ax: f32, ay: f32| egui::pos2((x + ax) / pw, (y + ay) / ph);
    let corners = [
        (dst.min, uv(h, 0.0)),
        (egui::pos2(dst.max.x, dst.min.y), uv(h, w)),
        (dst.max, uv(0.0, w)),
        (egui::pos2(dst.min.x, dst.max.y), uv(0.0, 0.0)),
    ];
    let vertices: Vec<egui::epaint::Vertex> = corners
        .iter()
        .map(|(pos, uv)| egui::epaint::Vertex {
            pos: *pos,
            uv: *uv,
            color: egui::Color32::WHITE,
        })
        .collect();
    painter.add(egui::Mesh {
        vertices,
        indices: vec![0, 1, 2, 0, 2, 3],
        texture_id: tex,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pivot_px_convierte_entre_normalizado_y_pixeles() {
        assert_eq!(pivot_px(Point2D::new(0.5, 0.5), 64, 32), (32.0, 16.0));
        assert_eq!(pivot_px(Point2D::new(0.0, 0.0), 64, 32), (0.0, 0.0));
        assert_eq!(pivot_px(Point2D::new(1.0, 1.0), 64, 32), (64.0, 32.0));
    }

    #[test]
    fn pivot_px_sobrevive_tamannos_degenerados() {
        assert_eq!(
            pivot_px(Point2D::new(0.25, 0.75), 0, -4),
            (0.25, 0.75),
            "tamaño 0 no produce NaN ni división por cero"
        );
    }

    #[test]
    fn las_nueve_posiciones_predefinidas_cubren_las_esquinas() {
        let mut seen = std::collections::HashSet::new();
        for (_, x, y) in presets() {
            assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y));
            // Cuantizadas a centésimas: f32 no es Hash.
            seen.insert(((x * 100.0) as i32, (y * 100.0) as i32));
        }
        assert_eq!(seen.len(), 9, "las nueve posiciones son distintas");
    }
}
