//! Center preview panel (sprite sheet) plus the bottom zoom bar.

use super::App;
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
    // Banda 9-patch que se está arrastrando (None = nada).
    let mut drag: Option<(Edge, i32)> = None;
    let mut border_released = false;

    egui::ScrollArea::both()
        .id_salt("preview_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
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

            let sprites: Vec<_> = out
                .result
                .sprites
                .iter()
                .filter(|s| s.atlas_page_index as usize == selected_page)
                .collect();

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

    if let Some((edge, value)) = drag {
        set_selected_border_edge(app, edge, value);
        // Al soltar la banda, persistir borders.json (si hay directorio).
        if border_released {
            app.auto_save_borders();
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
    l = l.clamp(0, v.width);
    r = r.clamp(0, v.width);
    t = t.clamp(0, v.height);
    b = b.clamp(0, v.height);
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
