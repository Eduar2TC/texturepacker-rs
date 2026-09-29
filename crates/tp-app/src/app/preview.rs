//! Center preview panel (sprite sheet) plus the bottom zoom bar.

use super::{App, PreviewState};
use eframe::egui;
use std::path::PathBuf;

const ZOOM_STEPS: [f32; 7] = [0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0];

pub(super) fn preview_ui(app: &mut App, ui: &mut egui::Ui) {
    // Soltar sprites arrastrados del panel: convertir pantalla → píxeles del
    // atlas y colocarlos ahí (consumo del payload en el frame de release).
    if ui.input(|i| i.pointer.any_released()) {
        if let (Some(_drag), Some(rect)) =
            (crate::app::SpriteDrag::payload(ui.ctx()), app.canvas_rect)
        {
            if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
                if rect.contains(p) {
                    let ax = ((p.x - rect.min.x) / app.preview_zoom.max(0.0001)) as i32;
                    let ay = ((p.y - rect.min.y) / app.preview_zoom.max(0.0001)) as i32;
                    let n = app.drop_sprites_on_canvas(egui::pos2(ax as f32, ay as f32));
                    if n == 0 {
                        crate::app::SpriteDrag::clear(ui.ctx());
                    }
                }
            }
        }
    }
    egui::TopBottomPanel::bottom("zoom_bar")
        .resizable(false)
        .exact_height(32.0)
        .show_inside(ui, |ui| zoom_bar(app, ui));
    egui::CentralPanel::default().show_inside(ui, |ui| preview_area(app, ui));
}

fn zoom_bar(app: &mut App, ui: &mut egui::Ui) {
    // La barra nunca desborda: si no cabe, aparece scroll horizontal.
    egui::ScrollArea::horizontal()
        .id_salt("zoom_bar_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            zoom_bar_inner(app, ui);
        });
}

fn zoom_bar_inner(app: &mut App, ui: &mut egui::Ui) {
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
        if ui.button("1:1").on_hover_text("Zoom al 100% (tamaño real)").clicked() {
            app.zoom = 1.0;
        }
        if ui
            .button("Ajustar")
            .on_hover_text("Encuadrar el atlas completo en la vista")
            .clicked()
        {
            app.fit_zoom();
        }

        ui.separator();
        ui.menu_button("Vista", |ui| {
            ui.checkbox(&mut app.show_outlines, "Mostrar contornos")
                .on_hover_text("Marcos y triangulación de los sprites");
            ui.checkbox(&mut app.show_pivots, "Pivots");
            ui.checkbox(&mut app.show_borders, "Bordes 9-patch")
                .on_hover_text("Barras verdes de los bordes 9-patch de cada sprite");
        });

        // Controles del algoritmo Manual, en un menú para no saturar la barra.
        if app.config.effective_algorithm() == tp_core::config::PackingAlgorithm::Manual {
            ui.separator();
            let grid_label = if let Some(g) = &app.config.manual_grid {
                format!("Rejilla: {} px", g.step)
            } else {
                "Rejilla: no".to_string()
            };
            ui.menu_button(grid_label, |ui| {
                let mut grid_changed = false;
                let mut grid_on = app.config.manual_grid.is_some();
                if ui
                    .checkbox(&mut grid_on, "Imán de rejilla")
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
                ui.separator();
                if ui
                    .button("Limpiar posiciones manuales")
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
                    ui.close();
                }
            });
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

/// Rect en pantalla del fantasma de un sprite soltado en `(px, py)` del
/// atlas: aplica zoom y el tamaño mínimo visible (`GHOST_MIN_SIZE_PX`).
/// Comparte la misma transformación que `to_screen`, pero con el tamaño
/// acotado para que el fantasma no desaparezca a zoom bajo.
fn ghost_rect(px: i32, py: i32, w: i32, h: i32, zoom: f32, rect: egui::Rect) -> (egui::Rect, f32) {
    let zoom = zoom.max(0.0001);
    let min = super::GHOST_MIN_SIZE_PX;
    let sw = (w as f32 * zoom).max(min);
    let sh = (h as f32 * zoom).max(min);
    let gx = rect.min.x + px as f32 * zoom;
    let gy = rect.min.y + py as f32 * zoom;
    (
        egui::Rect::from_min_size(egui::pos2(gx, gy), egui::vec2(sw, sh)),
        sw.min(sh),
    )
}

pub(super) fn zoom_step(app: &mut App, dir: i32) {
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

/// Diana grande de suelta: el objetivo del workspace vacío.
fn drop_target(ui: &mut egui::Ui, hovering: bool) {
    let tint = if hovering {
        egui::Color32::from_rgba_unmultiplied(120, 200, 255, 45)
    } else {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 22)
    };
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(520.0), 84.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(1.5_f32, tint),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "⤵  Arrastra aquí imágenes o carpetas",
        egui::FontId::proportional(16.0),
        if hovering {
            egui::Color32::from_rgb(160, 215, 255)
        } else {
            egui::Color32::from_gray(150)
        },
    );
    ui.weak("PNG · WebP · JPG · TGA · BMP · GIF · DDS · QOI");
}

/// Tira compacta de suelta: sigue visible mientras se calcula el atlas para
/// poder seguir arrastrando sin perder el objetivo de encaje.
fn drop_strip(ui: &mut egui::Ui, hovering: bool) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(520.0), 44.0),
        egui::Sense::hover(),
    );
    let tint = if hovering {
        egui::Color32::from_rgba_unmultiplied(120, 200, 255, 60)
    } else {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18)
    };
    ui.painter().rect_filled(rect, 8.0, tint);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        if hovering {
            "Suelta para añadir al workspace"
        } else {
            "⤵  Sigue soltando imágenes o carpetas"
        },
        egui::FontId::proportional(14.0),
        if hovering {
            egui::Color32::from_rgb(180, 225, 255)
        } else {
            egui::Color32::from_gray(150)
        },
    );
}

fn preview_area(app: &mut App, ui: &mut egui::Ui) {
    app.preview_size = ui.available_size();
    app.canvas_rect = None;
    app.preview_zoom = app.zoom;

    let Some(out) = &app.result else {
        let (hovered, _dropped) =
            ui.input(|i| (i.raw.hovered_files.clone(), i.raw.dropped_files.clone()));
        let hovering = !hovered.is_empty();

        // Ya hay sprites pero todavía no hay atlas: el usuario acaba de
        // soltar archivos y espera ver movimiento. Aquí manda el estado de
        // cálculo («se está haciendo»), no el vacío: el vacío solo es
        // honesto cuando el workspace está de verdad vacío.
        if app.has_inputs() {
            let elems = app.config.extra_inputs.len()
                + usize::from(!app.config.input_directory.as_os_str().is_empty());
            let frescos = app.just_added_names();
            let resumen = if frescos.len() > 6 {
                format!("{}, +{} más", frescos[..6].join(", "), frescos.len() - 6)
            } else {
                frescos.join(", ")
            };
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.spinner();
                    ui.add_space(10.0);
                    ui.heading("Preparando el sprite sheet…");
                    ui.label(
                        egui::RichText::new(format!(
                            "{elems} elemento(s) en el workspace · la vista previa se calcula sola"
                        ))
                        .weak(),
                    );
                    if !resumen.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(format!("✓ Recién añadido: {resumen}"))
                                .color(super::JUST_ADDED_COLOR),
                        );
                    }
                    ui.add_space(16.0);
                    drop_strip(ui, hovering);
                });
            });
            return;
        }

        // Primera experiencia: el vacío es accionable, no solo texto.
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.heading("Aún no hay sprite sheet");
                ui.label("Añade sprites y la vista previa se calculará al momento.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("➕ Añadir sprites…").clicked() {
                        super::toolbar::add_sprites_dialog(app);
                    }
                    if ui.button("📁 Añadir carpeta…").clicked() {
                        super::toolbar::add_smart_folder_dialog(app);
                    }
                });
                ui.add_space(18.0);
                // Suelta de ficheros del SO sobre el espacio de trabajo.
                drop_target(ui, hovering);
            });
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
    // El cursor está sobre una banda 9-patch (para no pisar su cursor).
    let mut band_hovered = false;
    // Arrastre manual (algoritmo Manual): (id, x, y) destino del sprite.
    let mut manual_moved: Option<(String, i32, i32)> = None;
    let mut manual_stopped = false;
    // Arrastre de un sprite (cualquier modo): (id, frame original).
    let mut drag_source: Option<(String, tp_core::types::Rect)> = None;
    // Vista fantasma del arrastre fuera del modo Manual.
    let mut drag_ghost: Option<(tp_core::types::Rect, f32, f32)> = None;
    // Selección por rectángulo (marquee) sobre el atlas.
    let mut marquee_origin: Option<egui::Pos2> = None;
    let mut marquee_select: Option<(i32, i32, i32, i32)> = None;
    // Clic en zona vacía: deselecciona (convención estándar).
    let mut clicked_empty = false;
    // Supr con la vista bajo el cursor: quita los sprites seleccionados.
    let mut delete_in_preview = false;
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
            // El lienzo es destino de soltado de sprites del panel izquierdo:
            // registrar geometría y consumir el drop (ver post-frame abajo).
            app.canvas_rect = Some(rect);
            let painter = ui.painter();
            // Fondo y textura del lienzo ANTES que el resalte del arrastre:
            // pintados después, el relleno gris y la imagen taparían el
            // resalte y la fantasma de soltado del panel izquierdo.
            painter.rect_filled(rect, 0.0, egui::Color32::from_gray(30));
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            let panel_drag = crate::app::SpriteDrag::payload(ui.ctx());
            if panel_drag.is_some() && ui.rect_contains_pointer(rect) {
                // Resalte del lienzo como zona de destino válida.
                painter.rect_filled(
                    rect,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(120, 200, 255, 22),
                );
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(
                        2.0_f32,
                        egui::Color32::from_rgba_unmultiplied(120, 200, 255, 160),
                    ),
                    egui::StrokeKind::Inside,
                );
                // Fantasma bajo el cursor: el MISMO plan del drop (misma
                // fuente de verdad), así lo que se ve es lo que queda al
                // soltar: anclado a la esquina, con snap y clamp de lienzo.
                if let (Some(drag), Some(p)) =
                    (panel_drag.as_deref(), ui.input(|i| i.pointer.latest_pos()))
                {
                    let ax = ((p.x - rect.min.x) / zoom.max(0.0001)) as i32;
                    let ay = ((p.y - rect.min.y) / zoom.max(0.0001)) as i32;
                    let plan = App::plan_canvas_drop(
                        &drag.ids,
                        &drag.frames,
                        drag.first_frame,
                        egui::pos2(ax as f32, ay as f32),
                        &app.config,
                    );
                    let first = plan
                        .first()
                        .map(|(_, (px, py))| (*px, *py))
                        .unwrap_or((ax, ay));
                    for (i, (id, (px, py))) in plan.iter().enumerate() {
                        let f = drag
                            .frames
                            .get(id)
                            .copied()
                            .unwrap_or(tp_core::types::Rect::new(0, 0, 32, 32));
                        let (g, _) = ghost_rect(*px, *py, f.width, f.height, zoom, rect);
                        painter.rect_filled(
                            g,
                            0.0,
                            egui::Color32::from_rgba_unmultiplied(
                                120,
                                200,
                                255,
                                if i == 0 { 40 } else { 26 },
                            ),
                        );
                        painter.rect_stroke(
                            g,
                            0.0,
                            egui::Stroke::new(
                                1.5_f32,
                                egui::Color32::from_rgba_unmultiplied(160, 220, 255, 220),
                            ),
                            egui::StrokeKind::Inside,
                        );
                    }
                    // Etiqueta centrada bajo el sprite principal (siempre
                    // visible, también con el fantasma mínimo de 24 px).
                    let f = drag
                        .first_frame
                        .unwrap_or(tp_core::types::Rect::new(0, 0, 64, 64));
                    let (g, _) = ghost_rect(first.0, first.1, f.width, f.height, zoom, rect);
                    let label = if drag.ids.len() == 1 {
                        "Soltar para colocar aquí".to_string()
                    } else {
                        format!("Soltar {} sprites aquí", drag.ids.len())
                    };
                    painter.text(
                        egui::pos2(g.center().x, g.max.y + 14.0),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(12.0),
                        egui::Color32::from_rgba_unmultiplied(200, 235, 255, 240),
                    );
                }
            }
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
            // zoom con Ctrl+rueda / gesto de pinza (trackpad).
            if response.hovered() {
                let (with_ctrl, zoom_delta_input) = ui.input(|i| {
                    let ctrl = i.modifiers.ctrl || i.modifiers.command;
                    (ctrl, i.zoom_delta())
                });
                if with_ctrl && zoom_delta_input != 1.0 {
                    zoom_delta = Some(zoom_delta_input.clamp(0.5, 2.0));
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
                            ui.label(egui::RichText::new("Ctrl+rueda o pinza: zoom").weak());
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
                    if let Some((edge, value, released)) = draw_borders(
                        ui,
                        painter,
                        sprite,
                        &to_screen,
                        zoom,
                        interactive,
                        &mut band_hovered,
                    ) {
                        drag = Some((edge, value));
                        border_released |= released;
                    }
                }
            }

            // Interacción unificada sobre la respuesta del lienzo (un solo
            // widget: sin robo de clics). Arrastrar un sprite lo mueve en el
            // modo Manual o muestra una vista fantasma en los demás; arrastrar
            // desde zona vacía selecciona por rectángulo.
            let pointer = response.interact_pointer_pos();
            if response.drag_started() {
                if let Some(p) = pointer {
                    let px = ((p.x - rect.min.x) / zoom) as i32;
                    let py = ((p.y - rect.min.y) / zoom) as i32;
                    let probe = tp_core::types::Rect::new(px, py, 1, 1);
                    drag_source = sprites
                        .iter()
                        .find(|s| s.visible_frame.contains(&probe) && !s.is_alias)
                        .map(|s| (s.id.clone(), s.visible_frame));
                    if drag_source.is_none() {
                        marquee_origin = Some(p);
                    }
                }
            }
            if response.dragged() {
                if let Some((id, f)) = &drag_source {
                    if let Some(p) = pointer {
                        if manual_mode {
                            let px = ((p.x - rect.min.x) / zoom) as i32 - bp - pad;
                            let py = ((p.y - rect.min.y) / zoom) as i32 - bp - pad;
                            // Rejilla opcional: vista viva imantada mientras
                            // se arrastra; el motor aplica el mismo snap.
                            let (px, py) = match &app.config.manual_grid {
                                Some(g) => g.snap_pos((px, py)),
                                None => (px, py),
                            };
                            let max_x = (canvas_w - 2 * bp - (f.width + 2 * pad)).max(0);
                            let max_y = (canvas_h - 2 * bp - (f.height + 2 * pad)).max(0);
                            manual_moved =
                                Some((id.clone(), px.clamp(0, max_x), py.clamp(0, max_y)));
                        } else {
                            // Vista fantasma: el frame sigue al puntero
                            // (centro anclado), invitando al modo Manual.
                            let cx = rect.min.x + (f.x as f32 + f.width as f32 * 0.5) * zoom;
                            let cy = rect.min.y + (f.y as f32 + f.height as f32 * 0.5) * zoom;
                            drag_ghost = Some((*f, (p.x - cx) / zoom, (p.y - cy) / zoom));
                        }
                    }
                } else if let (Some(o), Some(p)) = (marquee_origin, pointer) {
                    // Umbral propio de la marquesina: un micro-movimiento no
                    // dibuja ni selecciona (el clic simple deselecciona y no
                    // debe convertirse en selección de un píxel).
                    if o.distance(p) <= super::DRAG_THRESHOLD_PX {
                        drag_source = None;
                        marquee_origin = None;
                    }
                    let r = egui::Rect::from_two_pos(o, p);
                    painter.rect_filled(
                        r,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(120, 200, 255, 26),
                    );
                    painter.rect_stroke(
                        r,
                        0.0,
                        egui::Stroke::new(
                            1.0_f32,
                            egui::Color32::from_rgba_unmultiplied(120, 200, 255, 170),
                        ),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            if response.drag_stopped() {
                if drag_source.is_some() {
                    if manual_mode {
                        manual_stopped = true;
                    }
                } else if let (Some(o), Some(p)) = (marquee_origin, pointer) {
                    if o.distance(p) > super::DRAG_THRESHOLD_PX {
                        let r = egui::Rect::from_two_pos(o, p);
                        let ax0 = ((r.min.x - rect.min.x) / zoom) as i32;
                        let ay0 = ((r.min.y - rect.min.y) / zoom) as i32;
                        let ax1 = ((r.max.x - rect.min.x) / zoom) as i32;
                        let ay1 = ((r.max.y - rect.min.y) / zoom) as i32;
                        marquee_select = Some((ax0, ay0, ax1, ay1));
                    }
                }
                drag_source = None;
                marquee_origin = None;
            }

            // Cursor de agarre al pasar sobre un sprite (sin pisar el
            // cursor de resize de las bandas 9-patch).
            if hovered.is_some() && drag_source.is_none() && drag.is_none() && !band_hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }

            // Vista fantasma del arrastre fuera del modo Manual.
            if let Some((f, dx, dy)) = drag_ghost {
                let gx = f.x as f32 + dx;
                let gy = f.y as f32 + dy;
                let g = egui::Rect::from_min_max(
                    to_screen(gx.round() as i32, gy.round() as i32),
                    to_screen(gx.round() as i32 + f.width, gy.round() as i32 + f.height),
                );
                painter.rect_stroke(
                    g,
                    0.0,
                    egui::Stroke::new(
                        1.5_f32,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 110),
                    ),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    g.left_top() + egui::vec2(0.0, -6.0),
                    egui::Align2::LEFT_BOTTOM,
                    "Algoritmo «Manual» para fijar posición",
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 185),
                );
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
                    } else {
                        clicked_empty = true;
                    }
                }
            }

            // Supr con el cursor sobre la vista: quita los seleccionados.
            delete_in_preview |=
                response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Delete));
            // Un clic en el lienzo devuelve el foco de teclado a la vista:
            // las flechas dejan de mover la lista del panel izquierdo.
            if response.hovered() && ui.input(|i| i.pointer.any_pressed()) {
                app.tree_kb_focus = false;
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
    if clicked_empty {
        app.selected_sprite = None;
        app.selected_paths.clear();
        app.list_cursor = None;
        app.selection_anchor = None;
    }
    if let Some((x0, y0, x1, y1)) = marquee_select {
        // Rectángulo degenerado (clic sin arrastre): no altera la selección.
        if x1 - x0 > 1 || y1 - y0 > 1 {
            select_sprites_in_rect(app, x0, y0, x1, y1);
        }
    }
    if delete_in_preview {
        // Orden visual del panel: Supr deja el cursor en la fila siguiente.
        let order: Vec<PathBuf> = app.sprite_rows.iter().map(|(p, _)| p.clone()).collect();
        app.remove_selected(&order);
    }
}

/// Selecciona todos los sprites cuyo frame visible toca el rectángulo del
/// atlas dado (selección múltiple por marquee).
fn select_sprites_in_rect(app: &mut App, x0: i32, y0: i32, x1: i32, y1: i32) {
    let Some(out) = &app.result else {
        return;
    };
    let region =
        tp_core::types::Rect::new(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
    let page = app.selected_page;
    let ids: Vec<String> = out
        .result
        .sprites
        .iter()
        .filter(|s| s.atlas_page_index as usize == page && s.visible_frame.intersects(&region))
        .map(|s| s.id.clone())
        .collect();
    if ids.is_empty() {
        return;
    }
    app.selected_paths.clear();
    app.selected_sprite = None;
    let mut first = None;
    for id in &ids {
        if let Some(s) = out.result.sprites.iter().find(|s| &s.id == id) {
            let path = PathBuf::from(&s.source_path);
            if first.is_none() {
                first = Some(path.clone());
            }
            app.selected_paths.insert(path);
        }
    }
    // El marquee manda el cursor/la ancla a lo seleccionado: las flechas
    // siguen desde ahí.
    app.list_cursor = first.clone();
    app.selection_anchor = first;
    if ids.len() == 1 {
        app.selected_sprite = Some(ids[0].clone());
    }
    app.log(
        super::LogKind::Info,
        format!("{} sprite(s) seleccionados por rectángulo.", ids.len()),
    );
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
    band_hovered: &mut bool,
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
        *band_hovered |= response.hovered();

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
