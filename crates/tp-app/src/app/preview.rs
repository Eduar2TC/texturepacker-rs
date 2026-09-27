//! Center preview panel (sprite sheet) plus the bottom zoom bar.

use super::{App, PreviewState};
use eframe::egui;

const ZOOM_STEPS: [f32; 7] = [0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0];

pub(super) fn preview_ui(app: &mut App, ui: &mut egui::Ui) {
    egui::TopBottomPanel::bottom("zoom_bar")
        .resizable(false)
        .exact_height(32.0)
        .show_inside(ui, |ui| zoom_bar(app, ui));
    egui::CentralPanel::default().show_inside(ui, |ui| preview_area(app, ui));
}

fn zoom_bar(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        // Indicador de frescura de la vista previa.
        match app.preview_state() {
            PreviewState::Publishing => {
                ui.spinner();
                ui.label(
                    egui::RichText::new("Publicando…")
                        .color(egui::Color32::from_rgb(120, 170, 255)),
                );
            }
            PreviewState::Updating => {
                ui.spinner();
                ui.label(
                    egui::RichText::new("Actualizando…")
                        .color(egui::Color32::from_rgb(120, 170, 255)),
                );
            }
            PreviewState::Stale => {
                ui.label(
                    egui::RichText::new("● Desactualizado")
                        .color(egui::Color32::from_rgb(230, 180, 60)),
                );
            }
            PreviewState::Ok => {}
        }
        ui.separator();
        let pages = app.result.as_ref().map(|o| o.pages.len()).unwrap_or(0);
        if pages > 1 {
            if ui.button("◀").clicked() && app.selected_page > 0 {
                app.selected_page -= 1;
            }
            ui.label(format!("Página {}/{}", app.selected_page + 1, pages));
            if ui.button("▶").clicked() && app.selected_page + 1 < pages {
                app.selected_page += 1;
            }
            ui.separator();
        }

        if ui.button("−").on_hover_text("Alejar").clicked() {
            zoom_step(app, -1);
        }
        ui.add(
            egui::Slider::new(&mut app.zoom, 0.05..=8.0)
                .logarithmic(true)
                .text("Zoom"),
        );
        if ui.button("+").on_hover_text("Acercar").clicked() {
            zoom_step(app, 1);
        }
        if ui.button("1:1").on_hover_text("Zoom al 100%").clicked() {
            app.zoom = 1.0;
        }
        if ui
            .button("Ajustar")
            .on_hover_text("Encuadrar el atlas en la vista")
            .clicked()
        {
            app.fit_zoom();
        }

        ui.separator();
        ui.checkbox(&mut app.show_outlines, "Mostrar contornos")
            .on_hover_text("Marcos y triangulación de los sprites");
        ui.checkbox(&mut app.show_pivots, "Pivots");
        ui.checkbox(&mut app.show_borders, "Bordes 9-patch")
            .on_hover_text("Barras verdes de los bordes 9-patch de cada sprite");

        // Controles del algoritmo Manual: imán de rejilla y limpiar todo.
        if app.config.effective_algorithm() == tp_core::config::PackingAlgorithm::Manual {
            ui.separator();
            let mut grid_changed = false;
            let mut grid_on = app.config.manual_grid.is_some();
            if ui
                .checkbox(&mut grid_on, "Rejilla")
                .on_hover_text("Imanta el arrastre a una rejilla fija; con «Rejilla en filas» también ordena los sprites sueltos")
                .changed()
            {
                app.config.manual_grid = if grid_on {
                    Some(tp_core::config::ManualGrid::new(16, true))
                } else {
                    None
                };
                grid_changed = true;
            }
            if let Some(g) = &mut app.config.manual_grid {
                grid_changed |= ui
                    .add(
                        egui::Slider::new(&mut g.step, 2..=256)
                            .logarithmic(true)
                            .text("Paso"),
                    )
                    .on_hover_text("Separación de la rejilla en píxeles del atlas")
                    .changed();
                grid_changed |= ui
                    .checkbox(&mut g.snap_flow, "Rejilla en filas")
                    .on_hover_text("Los sprites sin posición fija también se alinean a la rejilla")
                    .changed();
            }
            if grid_changed {
                app.after_workspace_change();
            }
            if ui
                .button("Limpiar posiciones")
                .on_hover_text("Borra todas las posiciones manuales: los sprites vuelven al flujo automático")
                .clicked()
            {
                let cleared = app.config.manual_positions.len();
                app.config.manual_positions.clear();
                if cleared > 0 {
                    app.log(
                        super::LogKind::Info,
                        format!("{cleared} posición(es) manual(es) eliminada(s)."),
                    );
                    app.after_workspace_change();
                }
            }
        }

        if let Some(name) = &app.selected_sprite {
            ui.separator();
            ui.add(
                egui::Label::new(egui::RichText::new(format!("Seleccionado: {name}")).strong())
                    .truncate(),
            );
        }
    });
}

fn zoom_step(app: &mut App, dir: i32) {
    if dir > 0 {
        app.zoom = ZOOM_STEPS
            .iter()
            .copied()
            .find(|&z| z > app.zoom + 1e-3)
            .unwrap_or(app.zoom);
    } else {
        app.zoom = ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|&z| z < app.zoom - 1e-3)
            .unwrap_or(app.zoom);
    }
}

fn preview_area(app: &mut App, ui: &mut egui::Ui) {
    app.preview_size = ui.available_size();

    let Some(out) = &app.result else {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("Aún no hay sprite sheet.\nAñade sprites y pulsa «Publicar».")
                    .weak(),
            );
        });
        return;
    };
    if out.pages.is_empty() {
        ui.label("El resultado no tiene páginas.");
        return;
    }
    if app.selected_page >= out.pages.len() {
        app.selected_page = 0;
    }

    let page = &out.pages[app.selected_page];
    let tex = &app.textures[app.selected_page];
    let zoom = app.zoom;
    let size = egui::vec2(page.width as f32 * zoom, page.height as f32 * zoom);
    let selected_page = app.selected_page;
    let show_outlines = app.show_outlines;
    let show_pivots = app.show_pivots;
    let show_borders = app.show_borders;
    let mut picked: Option<String> = None;
    // Sprite bajo el cursor (resaltado + tooltip) y zoom con Ctrl+rueda.
    let mut hovered: Option<String> = None;
    let mut zoom_delta: Option<f32> = None;
    // Banda 9-patch que se está arrastrando (None = nada).
    let mut drag: Option<(Edge, i32)> = None;
    let mut border_released = false;
    // Arrastre manual (algoritmo Manual): (id, x, y) destino del sprite.
    let mut manual_moved: Option<(String, i32, i32)> = None;
    let mut manual_stopped = false;
    // Geometría compartida para mapear posiciones manuales ↔ atlas.
    let (bp, pad) = (app.config.border_padding.max(0), app.config.padding.max(0));
    let canvas_w = if app.config.fixed_width > 0 {
        app.config.fixed_width
    } else {
        app.config.max_texture_size
    };
    let canvas_h = if app.config.fixed_height > 0 {
        app.config.fixed_height
    } else {
        app.config.max_texture_size
    };
    let manual_mode = app.config.effective_algorithm() == tp_core::config::PackingAlgorithm::Manual;

    egui::ScrollArea::both()
        .id_salt("preview_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, egui::Color32::from_gray(30));
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );

            let to_screen = |x: i32, y: i32| {
                egui::pos2(rect.min.x + x as f32 * zoom, rect.min.y + y as f32 * zoom)
            };

            // Rejilla visual del modo Manual (bajo los sprites): líneas en
            // cada múltiplo del paso dentro del lienzo.
            if manual_mode {
                if let Some(g) = app.config.manual_grid {
                    if g.step > 1 {
                        let step = g.step as f32 * zoom;
                        if step >= 4.0 {
                            let color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 22);
                            let mut gx = rect.min.x;
                            let mut i = 0.0;
                            while gx <= rect.max.x + 0.5 {
                                painter.line_segment(
                                    [egui::pos2(gx, rect.min.y), egui::pos2(gx, rect.max.y)],
                                    (1.0, color),
                                );
                                i += 1.0;
                                gx = rect.min.x + i * step;
                            }
                            let mut gy = rect.min.y;
                            let mut j = 0.0;
                            while gy <= rect.max.y + 0.5 {
                                painter.line_segment(
                                    [egui::pos2(rect.min.x, gy), egui::pos2(rect.max.x, gy)],
                                    (1.0, color),
                                );
                                j += 1.0;
                                gy = rect.min.y + j * step;
                            }
                        }
                    }
                }
            }

            let sprites: Vec<_> = out
                .result
                .sprites
                .iter()
                .filter(|s| s.atlas_page_index as usize == selected_page)
                .collect();

            // Interactividad: sprite bajo el cursor (tooltip + resalte) y
            // zoom con Ctrl+rueda sobre la vista.
            if response.hovered() {
                let (with_ctrl, scroll_y) = ui.input(|i| {
                    let ctrl = i.modifiers.ctrl || i.modifiers.command;
                    (ctrl, i.raw_scroll_delta.y)
                });
                if with_ctrl && scroll_y != 0.0 {
                    zoom_delta = Some((1.0 - scroll_y * 0.0015).clamp(0.5, 2.0));
                }
                if let Some(pos) = ui.input(|i| i.pointer.latest_pos()) {
                    let px = ((pos.x - rect.min.x) / zoom) as i32;
                    let py = ((pos.y - rect.min.y) / zoom) as i32;
                    let probe = tp_core::types::Rect::new(px, py, 1, 1);
                    hovered = sprites
                        .iter()
                        .find(|s| s.visible_frame.contains(&probe))
                        .map(|s| s.id.clone());
                }
                if let Some(id) = &hovered {
                    if let Some(s) = sprites.iter().find(|s| s.id == *id) {
                        let f = s.visible_frame;
                        response.clone().on_hover_ui(|ui| {
                            ui.strong(&s.id);
                            ui.label(format!(
                                "{}x{} px{}",
                                s.raw_width,
                                s.raw_height,
                                if s.is_rotated { " · rotado 90°" } else { "" }
                            ));
                            ui.label(format!(
                                "frame ({}, {}) {}x{} · página {}",
                                f.x,
                                f.y,
                                f.width,
                                f.height,
                                s.atlas_page_index + 1
                            ));
                            if s.is_alias {
                                ui.label(format!(
                                    "alias → {}",
                                    s.alias_target_id.as_deref().unwrap_or("?")
                                ));
                            }
                            ui.label(egui::RichText::new("Ctrl+rueda: zoom").weak());
                        });
                    }
                }
            }

            if show_outlines {
                for sprite in &sprites {
                    let f = sprite.visible_frame;
                    let color = if app.selected_sprite.as_deref() == Some(sprite.id.as_str()) {
                        egui::Color32::YELLOW
                    } else if sprite.is_alias {
                        egui::Color32::GRAY
                    } else {
                        egui::Color32::from_rgb(0, 220, 255)
                    };
                    let r = egui::Rect::from_min_max(
                        to_screen(f.x, f.y),
                        to_screen(f.x + f.width, f.y + f.height),
                    );
                    painter.rect_stroke(
                        r,
                        0.0,
                        egui::Stroke::new(1.0_f32, color),
                        egui::StrokeKind::Inside,
                    );
                    if zoom > 1.5 {
                        painter.text(
                            to_screen(f.x + 1, f.y + 10),
                            egui::Align2::LEFT_TOP,
                            &sprite.id,
                            egui::FontId::proportional(9.0),
                            color,
                        );
                    }
                    draw_mesh(painter, sprite, rect, zoom, page.width, page.height);
                }
                // Resaltar el sprite bajo el cursor (sin llegar a seleccionar).
                if let Some(id) = &hovered {
                    if let Some(sprite) = sprites
                        .iter()
                        .find(|s| s.id == *id)
                        .filter(|s| app.selected_sprite.as_deref() != Some(s.id.as_str()))
                    {
                        let f = sprite.visible_frame;
                        let r = egui::Rect::from_min_max(
                            to_screen(f.x, f.y),
                            to_screen(f.x + f.width, f.y + f.height),
                        );
                        painter.rect_stroke(
                            r,
                            0.0,
                            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(255, 255, 255)),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }

            if show_pivots {
                for sprite in &sprites {
                    if sprite.is_alias {
                        continue;
                    }
                    let v = sprite.visible_frame;
                    let px = v.x as f32 + sprite.pivot.x * v.width as f32;
                    let py = v.y as f32 + sprite.pivot.y * v.height as f32;
                    let p = to_screen(px as i32, py as i32);
                    painter.circle_filled(p, 3.0, egui::Color32::RED);
                }
            }

            if show_borders {
                let is_selected = |id: &str| app.selected_sprite.as_deref() == Some(id);
                for sprite in &sprites {
                    let interactive = is_selected(&sprite.id) && !sprite.is_alias;
                    if let Some((edge, value, released)) =
                        draw_borders(ui, painter, sprite, &to_screen, zoom, interactive)
                    {
                        drag = Some((edge, value));
                        border_released |= released;
                    }
                }
            }

            // Algoritmo Manual: arrastrar el sprite seleccionado fija su
            // posición en el atlas (config.manual_positions).
            if manual_mode && drag.is_none() {
                if let Some(id) = app.selected_sprite.clone() {
                    if let Some(sprite) =
                        sprites.iter().find(|s| s.id == id).filter(|s| !s.is_alias)
                    {
                        let f = sprite.visible_frame;
                        let r = egui::Rect::from_min_max(
                            to_screen(f.x, f.y),
                            to_screen(f.x + f.width, f.y + f.height),
                        );
                        let dresp = ui
                            .interact(
                                r,
                                egui::Id::new(("manual_drag", sprite.id.as_str())),
                                egui::Sense::drag(),
                            )
                            .on_hover_cursor(egui::CursorIcon::Grab);
                        if dresp.dragged() {
                            if let Some(pos) = dresp.interact_pointer_pos() {
                                let px = ((pos.x - rect.min.x) / zoom) as i32 - bp - pad;
                                let py = ((pos.y - rect.min.y) / zoom) as i32 - bp - pad;
                                // Rejilla opcional: vista viva imantada mientras
                                // se arrastra; el motor confita el mismo snap.
                                let (px, py) = match &app.config.manual_grid {
                                    Some(g) => g.snap_pos((px, py)),
                                    None => (px, py),
                                };
                                let max_x = (canvas_w - 2 * bp - (f.width + 2 * pad)).max(0);
                                let max_y = (canvas_h - 2 * bp - (f.height + 2 * pad)).max(0);
                                manual_moved = Some((
                                    sprite.id.clone(),
                                    px.clamp(0, max_x),
                                    py.clamp(0, max_y),
                                ));
                            }
                        }
                        if dresp.drag_stopped() {
                            manual_stopped = true;
                        }
                    }
                }
            }

            // Un click que completa un arrastre de banda no selecciona sprites.
            if response.clicked() && drag.is_none() {
                let pos = ui.input(|i| i.pointer.latest_pos());
                if let Some(pos) = pos {
                    let px = ((pos.x - rect.min.x) / zoom) as i32;
                    let py = ((pos.y - rect.min.y) / zoom) as i32;
                    let probe = tp_core::types::Rect::new(px, py, 1, 1);
                    if let Some(sprite) = sprites
                        .iter()
                        .find(|s| s.visible_frame.contains(&probe) && !s.is_alias)
                        .or_else(|| sprites.iter().find(|s| s.visible_frame.contains(&probe)))
                    {
                        picked = Some(sprite.id.clone());
                    }
                }
            }
        });

    if let Some(factor) = zoom_delta {
        app.zoom = (app.zoom * factor).clamp(0.05, 8.0);
    }
    if let Some((edge, value)) = drag {
        set_selected_border_edge(app, edge, value);
        // Al soltar la banda, persistir borders.json (si hay directorio).
        if border_released {
            app.auto_save_borders();
        }
    }
    if let Some((id, nx, ny)) = manual_moved {
        let delta: (i32, i32) = match app.config.manual_positions.get(&id).copied() {
            Some((ox, oy)) => (nx - ox, ny - oy),
            None => app
                .result
                .as_ref()
                .and_then(|o| o.result.sprites.iter().find(|s| s.id == id))
                .map(|s| {
                    (
                        nx + bp + pad - s.visible_frame.x,
                        ny + bp + pad - s.visible_frame.y,
                    )
                })
                .unwrap_or((0, 0)),
        };
        let moved = delta != (0, 0);
        app.config.manual_positions.insert(id.clone(), (nx, ny));
        if moved {
            // Feedback inmediato: desplazar el frame del resultado actual;
            // la vista previa en memoria confirmará la posición final.
            if let Some(out) = &mut app.result {
                if let Some(s) = out.result.sprites.iter_mut().find(|s| s.id == id) {
                    s.visible_frame.x += delta.0;
                    s.visible_frame.y += delta.1;
                    s.allocated_frame.x += delta.0;
                    s.allocated_frame.y += delta.1;
                }
            }
            app.change_seq += 1;
            app.request_preview(false);
        }
        if manual_stopped {
            let grid_note = match app.config.manual_grid {
                Some(g) => format!(" (rejilla: {} px)", g.step),
                None => String::new(),
            };
            app.log(
                super::LogKind::Info,
                format!("Posición manual fijada{grid_note} (se guarda con el proyecto)."),
            );
        }
    }
    if let Some(id) = picked {
        app.select_sprite(&id);
    }
}

/// Overwrite a single side of the selected sprite's 9-patch border.
/// All-zero results collapse to `None` (no 9-patch), consistent with the
/// settings editor.
fn set_selected_border_edge(app: &mut App, edge: Edge, value: i32) {
    let Some(out) = &mut app.result else {
        return;
    };
    let Some(idx) = out
        .result
        .sprites
        .iter()
        .position(|s| app.selected_sprite.as_deref() == Some(s.id.as_str()))
    else {
        return;
    };
    let Some(sprite) = out.result.sprites.get_mut(idx) else {
        return;
    };
    let mut b = sprite.border.unwrap_or([0; 4]);
    let v = value.max(0);
    match edge {
        Edge::Left => b[0] = v,
        Edge::Top => b[1] = v,
        Edge::Right => b[2] = v,
        Edge::Bottom => b[3] = v,
    }
    sprite.border = if b == [0; 4] { None } else { Some(b) };
    // Recordar la edición para sobrevivir a los reempaquetados automáticos.
    match sprite.border {
        Some(b) => {
            app.border_edits.insert(sprite.id.clone(), b);
        }
        None => {
            app.border_edits.remove(sprite.id.as_str());
        }
    }
}

fn draw_mesh(
    painter: &egui::Painter,
    sprite: &tp_core::SpriteAsset,
    rect: egui::Rect,
    zoom: f32,
    page_w: i32,
    page_h: i32,
) {
    let Some(mesh) = &sprite.mesh else {
        return;
    };
    if mesh.uvs.len() != mesh.vertices.len() || mesh.indices.is_empty() {
        return;
    }
    let to_screen = |u: f32, v: f32| {
        egui::pos2(
            rect.min.x + u * page_w as f32 * zoom,
            rect.min.y + v * page_h as f32 * zoom,
        )
    };
    let stroke = egui::Stroke::new(0.6_f32, egui::Color32::from_rgb(140, 140, 140));
    for tri in mesh.indices.chunks_exact(3) {
        let a = to_screen(mesh.uvs[tri[0] as usize].x, mesh.uvs[tri[0] as usize].y);
        let b = to_screen(mesh.uvs[tri[1] as usize].x, mesh.uvs[tri[1] as usize].y);
        let c = to_screen(mesh.uvs[tri[2] as usize].x, mesh.uvs[tri[2] as usize].y);
        painter.line_segment([a, b], stroke);
        painter.line_segment([b, c], stroke);
        painter.line_segment([c, a], stroke);
    }
}

/// One draggable side of the 9-patch border (in atlas pixel space).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    Left,
    Top,
    Right,
    Bottom,
}

impl Edge {
    fn cursor(self) -> egui::CursorIcon {
        match self {
            Edge::Left | Edge::Right => egui::CursorIcon::ResizeHorizontal,
            Edge::Top | Edge::Bottom => egui::CursorIcon::ResizeVertical,
        }
    }
}

/// Tolerance (in screen pixels) for grabbing a green band with the mouse.
const EDGE_GRAB_TOLERANCE: f32 = 4.0;

/// Draw the green 9-patch border guides of one sprite.
/// Lines are drawn inside the visible frame: `l`/`r` pixels from the left/right
/// edges, `t`/`b` from the top/bottom. When `interactive` (selected sprite),
/// each band can be grabbed and dragged with the mouse; returns the dragged
/// edge and its new value so the caller can persist it on the sprite.
fn draw_borders(
    ui: &egui::Ui,
    painter: &egui::Painter,
    sprite: &tp_core::types::SpriteAsset,
    to_screen: &impl Fn(i32, i32) -> egui::Pos2,
    zoom: f32,
    interactive: bool,
) -> Option<(Edge, i32, bool)> {
    let v = sprite.visible_frame;
    if v.width <= 0 || v.height <= 0 {
        return None;
    }
    let border = sprite.border.unwrap_or([0; 4]);
    let [mut l, mut t, mut r, mut b] = border;
    // Los valores se miden sobre la imagen original (convención de
    // borders.json): descontar el margen transparente que se recortó al
    // empaquetar para dibujar las bandas dentro del frame visible.
    let (margin_left, margin_top) = (sprite.offset_x.max(0), sprite.offset_y.max(0));
    let (margin_right, margin_bottom) = (
        (sprite.raw_width - sprite.offset_x - sprite.trimmed_bounds.width).max(0),
        (sprite.raw_height - sprite.offset_y - sprite.trimmed_bounds.height).max(0),
    );
    l = (l - margin_left).clamp(0, v.width);
    r = (r - margin_right).clamp(0, v.width);
    t = (t - margin_top).clamp(0, v.height);
    b = (b - margin_bottom).clamp(0, v.height);
    let (stroke, thick) = if interactive {
        (
            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(0, 255, 80)),
            6.0,
        )
    } else {
        (
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0, 255, 80)),
            2.0,
        )
    };

    // (edge, from, to, horizontal?): las bandas horizontales son líneas
    // verticales que se mueven en X (cursor |←→|) y viceversa.
    let bands = [
        (Edge::Left, (v.x + l, v.y), (v.x + l, v.y + v.height)),
        (
            Edge::Right,
            (v.x + v.width - r, v.y),
            (v.x + v.width - r, v.y + v.height),
        ),
        (Edge::Top, (v.x, v.y + t), (v.x + v.width, v.y + t)),
        (
            Edge::Bottom,
            (v.x, v.y + v.height - b),
            (v.x + v.width, v.y + v.height - b),
        ),
    ];

    let mut dragged: Option<(Edge, i32)> = None;
    let mut released = false;
    for (edge, from, to) in bands {
        // Solo dibujar bandas con valor > 0 (las ocultas no se pueden arrastrar
        // hasta darles valor en el editor).
        let hidden = matches!(
            (edge, border),
            (Edge::Left, [0, _, _, _])
                | (Edge::Top, [_, 0, _, _])
                | (Edge::Right, [_, _, 0, _])
                | (Edge::Bottom, [_, _, _, 0])
        );
        if hidden {
            continue;
        }
        let a = to_screen(from.0, from.1);
        let c = to_screen(to.0, to.1);
        painter.line_segment([a, c], stroke);

        if !interactive {
            continue;
        }

        // Zona de agarre centrada en la banda, a lo largo de todo el borde.
        let band = if a.x == c.x {
            egui::Rect::from_center_size(
                egui::pos2(a.x, (a.y + c.y) * 0.5),
                egui::vec2(thick, (c.y - a.y).abs()),
            )
        } else {
            egui::Rect::from_center_size(
                egui::pos2((a.x + c.x) * 0.5, a.y),
                egui::vec2((c.x - a.x).abs(), thick),
            )
        };
        let response = ui
            .interact(
                band.expand(EDGE_GRAB_TOLERANCE),
                egui::Id::new(("9patch", sprite.id.as_str(), edge as u8)),
                egui::Sense::drag(),
            )
            .on_hover_cursor(edge.cursor());

        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                // Origen en pantalla del frame del sprite:
                let origin = to_screen(v.x, v.y);
                let value = if a.x == c.x {
                    ((pos.x - origin.x) / zoom).round() as i32
                } else {
                    ((pos.y - origin.y) / zoom).round() as i32
                };
                let new_value = match edge {
                    Edge::Left | Edge::Top => value,
                    Edge::Right => v.width - value,
                    Edge::Bottom => v.height - value,
                };
                if new_value
                    != match edge {
                        Edge::Left => l,
                        Edge::Top => t,
                        Edge::Right => r,
                        Edge::Bottom => b,
                    }
                {
                    dragged = Some((edge, new_value));
                }
                if response.drag_stopped() {
                    released = true;
                }
            }
        }
    }
    dragged.map(|(edge, value)| (edge, value, released))
}
