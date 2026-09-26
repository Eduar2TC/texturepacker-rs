//! Bottom panel: log, output details, sprite table and mesh views.

use super::{App, BottomTab, LogKind};
use eframe::egui;

pub(super) fn bottom_ui(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Log, "Log");
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Output, "Salida");
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Sprites, "Sprites");
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Mesh, "Malla");
        if let Some(out) = &app.result {
            ui.separator();
            ui.label(
                egui::RichText::new(format!(
                    "{} sprites · {} aliases · {} página(s)",
                    out.result.total_sprites,
                    out.result.alias_count,
                    out.pages.len()
                ))
                .weak(),
            );
        }
    });
    ui.separator();

    match app.bottom_tab {
        BottomTab::Log => log_view(app, ui),
        BottomTab::Output => output_view(app, ui),
        BottomTab::Sprites => sprites_view(app, ui),
        BottomTab::Mesh => mesh_view(app, ui),
    }
}

fn log_view(app: &App, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .id_salt("log_scroll")
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in &app.logs {
                let color = match entry.kind {
                    LogKind::Info => egui::Color32::from_gray(200),
                    LogKind::Warning => egui::Color32::from_rgb(255, 200, 80),
                    LogKind::Error => egui::Color32::from_rgb(255, 100, 100),
                };
                ui.colored_label(color, &entry.text);
            }
        });
}

fn output_view(app: &App, ui: &mut egui::Ui) {
    let Some(out) = &app.result else {
        ui.centered_and_justified(|ui| ui.label("Ejecuta un empaquetado primero."));
        return;
    };
    let result = &out.result;
    egui::ScrollArea::vertical()
        .id_salt("output_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Archivos generados");
                for f in &result.output_files {
                    ui.monospace(f);
                }
            });
            ui.horizontal(|ui| {
                ui.strong("Tiempos por etapa");
                for (stage, ms) in &result.stage_times_ms {
                    ui.monospace(format!("{ms:>6} ms"));
                    ui.label(stage);
                }
            });
            ui.horizontal(|ui| {
                ui.strong("Avisos");
                if result.warnings.is_empty() {
                    ui.label("(ninguno)");
                }
                for w in &result.warnings {
                    ui.colored_label(egui::Color32::from_rgb(255, 200, 80), w);
                }
            });
            for p in &result.pages {
                ui.label(format!(
                    "Página {}: {}x{} · relleno {:.1}% · {}",
                    p.index,
                    p.width,
                    p.height,
                    p.fill_ratio * 100.0,
                    p.file_name
                ));
            }
        });
}

fn sprites_view(app: &mut App, ui: &mut egui::Ui) {
    let Some(out) = &app.result else {
        ui.centered_and_justified(|ui| ui.label("Ejecuta un empaquetado primero."));
        return;
    };
    let mut selected: Option<String> = None;
    egui::ScrollArea::both()
        .id_salt("sprites_table")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("sprites")
                .striped(true)
                .min_col_width(70.0)
                .show(ui, |ui| {
                    ui.strong("id");
                    ui.strong("tamaño");
                    ui.strong("recortado");
                    ui.strong("frame (x,y,w,h)");
                    ui.strong("rot");
                    ui.strong("página");
                    ui.strong("alias");
                    ui.strong("pivot");
                    ui.strong("malla");
                    ui.end_row();
                    for s in &out.result.sprites {
                        let is_sel = app.selected_sprite.as_deref() == Some(s.id.as_str());
                        if ui.selectable_label(is_sel, &s.id).clicked() {
                            selected = Some(s.id.clone());
                        }
                        ui.label(format!("{}x{}", s.raw_width, s.raw_height));
                        ui.label(format!(
                            "{}x{}@({},{})",
                            s.trimmed_bounds.width, s.trimmed_bounds.height, s.offset_x, s.offset_y
                        ));
                        let f = s.allocated_frame;
                        ui.label(format!("({},{},{},{})", f.x, f.y, f.width, f.height));
                        ui.label(if s.is_rotated { "90°" } else { "—" });
                        ui.label(s.atlas_page_index.to_string());
                        ui.label(if s.is_alias {
                            format!("→ {}", s.alias_target_id.as_deref().unwrap_or("?"))
                        } else {
                            "—".into()
                        });
                        ui.label(format!("({:.2},{:.2})", s.pivot.x, s.pivot.y));
                        ui.label(if s.mesh.is_some() { "✓" } else { "—" });
                        ui.end_row();
                    }
                });
        });

    if let Some(id) = selected {
        app.select_sprite(&id);
    }
}

fn mesh_view(app: &App, ui: &mut egui::Ui) {
    let Some(out) = &app.result else {
        ui.centered_and_justified(|ui| ui.label("Ejecuta un empaquetado primero."));
        return;
    };
    let Some(sel) = &app.selected_sprite else {
        ui.centered_and_justified(|ui| {
            ui.label("Selecciona un sprite en la vista previa o en la tabla Sprites.");
        });
        return;
    };
    let Some(sprite) = out.result.sprites.iter().find(|s| &s.id == sel) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Malla de «{sel}»")).strong());
        if sprite.is_alias {
            ui.label(egui::RichText::new("(alias — reutiliza el frame de su objetivo)").weak());
        }
    });
    match &sprite.mesh {
        None => {
            ui.label("Este sprite no tiene malla (modo polígono desactivado o sprite alias).");
        }
        Some(mesh) => {
            ui.label(format!(
                "Vértices: {} · Triángulos: {} · Contornos: {}",
                mesh.vertices.len(),
                mesh.indices.len() / 3,
                sprite.contours.len()
            ));

            let tw = sprite.trimmed_bounds.width.max(1) as f32;
            let th = sprite.trimmed_bounds.height.max(1) as f32;
            let scale = 220.0 / tw.max(th);
            let size = egui::vec2(tw * scale, th * scale);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 2.0, egui::Color32::from_gray(20));
            for contour in &sprite.contours {
                let pts: Vec<egui::Pos2> = contour
                    .points
                    .iter()
                    .map(|p| egui::pos2(rect.min.x + p.x * scale, rect.min.y + p.y * scale))
                    .collect();
                if pts.len() >= 2 {
                    let color = if contour.is_hole {
                        egui::Color32::from_rgb(255, 120, 120)
                    } else {
                        egui::Color32::from_rgb(120, 220, 255)
                    };
                    for w in pts.windows(2) {
                        painter.line_segment([w[0], w[1]], egui::Stroke::new(1.5_f32, color));
                    }
                    if let Some(last) = pts.last() {
                        painter.line_segment([*last, pts[0]], egui::Stroke::new(1.5_f32, color));
                    }
                }
            }
            for tri in mesh.indices.chunks_exact(3) {
                let a = mesh.vertices[tri[0] as usize];
                let b = mesh.vertices[tri[1] as usize];
                let c = mesh.vertices[tri[2] as usize];
                let pa = egui::pos2(rect.min.x + a.x * scale, rect.min.y + a.y * scale);
                let pb = egui::pos2(rect.min.x + b.x * scale, rect.min.y + b.y * scale);
                let pc = egui::pos2(rect.min.x + c.x * scale, rect.min.y + c.y * scale);
                let stroke = egui::Stroke::new(0.5_f32, egui::Color32::from_rgb(90, 90, 90));
                painter.line_segment([pa, pb], stroke);
                painter.line_segment([pb, pc], stroke);
                painter.line_segment([pc, pa], stroke);
            }
            ui.label(
                egui::RichText::new("Azul: contorno exterior · Rojo: agujeros · Gris: triángulos")
                    .weak(),
            );
        }
    }
}
