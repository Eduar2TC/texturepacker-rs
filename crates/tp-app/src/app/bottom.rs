//! Bottom panel: log, output details, sprite table and mesh views.

use super::{App, BottomTab, LogKind};
use crate::i18n::t;
use eframe::egui;

pub(super) fn bottom_ui(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        // Plegar/desplegar el panel (chevron como en cualquier herramienta).
        let chevron = if app.bottom_collapsed { "⏵" } else { "⏷" };
        if ui
            .small_button(chevron)
            .on_hover_text(if app.bottom_collapsed {
                t!("Mostrar el panel")
            } else {
                t!("Plegar el panel y dejar la vista del atlas en pantalla completa")
            })
            .clicked()
        {
            app.bottom_collapsed = !app.bottom_collapsed;
        }
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Log, "Log");
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Output, t!("Salida"));
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Sprites, "Sprites");
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Mesh, t!("Malla"));
        // Estado resumido siempre visible, incluso con el panel plegado.
        // Empieza por los ficheros del panel izquierdo: entre ellos y los
        // sprites hay diferencia —un fichero descartado, un mapa de
        // normales— y sin esa cifra los dos recuentos se leían en conflicto
        // (M2).
        let ficheros = app.input_file_count();
        if let Some(out) = &app.result {
            ui.separator();
            ui.label(
                egui::RichText::new(t!(
                    "{} ficheros · {} sprite(s) · {} alias(es) · {} página(s)",
                    ficheros,
                    out.result.total_sprites,
                    out.result.alias_count,
                    out.pages.len()
                ))
                .weak(),
            );
        }
        // Punto de error visible aunque el Log esté plegado o en otra pestaña.
        let has_errors = app.logs.iter().any(|e| matches!(e.kind, LogKind::Error));
        if has_errors {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(
                        egui::RichText::new(t!("✖ Error")).color(ui.visuals().error_fg_color),
                    )
                    .on_hover_text(t!("Ir al Log"))
                    .clicked()
                {
                    app.bottom_tab = BottomTab::Log;
                    app.bottom_collapsed = false;
                }
            });
        }
    });
    ui.separator();

    if app.bottom_collapsed {
        return; // solo la tira de pestañas + estado
    }
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
                let visuals = ui.visuals();
                let color = match entry.kind {
                    LogKind::Info => visuals.text_color(),
                    LogKind::Warning => visuals.warn_fg_color,
                    LogKind::Error => visuals.error_fg_color,
                };
                ui.colored_label(color, &entry.text);
            }
        });
}

fn output_view(app: &App, ui: &mut egui::Ui) {
    let Some(out) = &app.result else {
        ui.centered_and_justified(|ui| ui.label(t!("Ejecuta un empaquetado primero.")));
        return;
    };
    let result = &out.result;
    egui::ScrollArea::vertical()
        .id_salt("output_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let (title, hint) = if app.files_written {
                (
                    t!("Archivos generados"),
                    t!("Escritos en el disco durante la última publicación."),
                )
            } else {
                (
                    t!("Archivos que se publicarán"),
                    t!("Vista previa: aún no existen; esta es la lista exacta que                      escribirá «Publicar»."),
                )
            };
            ui.label(egui::RichText::new(title).strong()).on_hover_text(hint);
            if result.output_files.is_empty() {
                ui.label(t!("(ninguno: añade sprites y publica)"));
            }
            for f in &result.output_files {
                ui.monospace(format!("  ➡ {f}"));
            }
            ui.horizontal(|ui| {
                ui.strong(t!("Tiempos por etapa"));
                for (stage, ms) in &result.stage_times_ms {
                    ui.monospace(format!("{ms:>6} ms"));
                    ui.label(stage);
                }
            });
            ui.horizontal(|ui| {
                ui.strong(t!("Avisos"));
                if result.warnings.is_empty() {
                    ui.label(t!("(ninguno)"));
                }
                for w in &result.warnings {
                    ui.colored_label(ui.visuals().warn_fg_color, crate::i18n::tr(w));
                }
            });
            for p in &result.pages {
                ui.label(t!(
                    "Página {}: {}x{} · relleno {}% · {}",
                    p.index,
                    p.width,
                    p.height,
                    format!("{:.1}", p.fill_ratio * 100.0),
                    p.file_name
                ));
            }
        });
}

fn sprites_view(app: &mut App, ui: &mut egui::Ui) {
    let Some(out) = &app.result else {
        ui.centered_and_justified(|ui| ui.label(t!("Ejecuta un empaquetado primero.")));
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
                    ui.strong(t!("id"));
                    ui.strong(t!("tamaño"));
                    ui.strong(t!("recortado"));
                    ui.strong(t!("frame (x,y,w,h)"));
                    ui.strong(t!("rot"));
                    ui.strong(t!("página"));
                    ui.strong(t!("alias"));
                    ui.strong(t!("pivot"));
                    ui.strong(t!("malla"));
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
                            format!("➡ {}", s.alias_target_id.as_deref().unwrap_or("?"))
                        } else {
                            "—".into()
                        });
                        ui.label(format!("({:.2},{:.2})", s.pivot.x, s.pivot.y));
                        ui.label(if s.mesh.is_some() { "✔" } else { "—" });
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
        ui.centered_and_justified(|ui| ui.label(t!("Ejecuta un empaquetado primero.")));
        return;
    };
    let Some(sel) = &app.selected_sprite else {
        ui.centered_and_justified(|ui| {
            ui.label(t!(
                "Selecciona un sprite en la vista previa o en la tabla Sprites."
            ));
        });
        return;
    };
    let Some(sprite) = out.result.sprites.iter().find(|s| &s.id == sel) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(t!("Malla de «{}»", sel)).strong());
        if sprite.is_alias {
            ui.label(egui::RichText::new(t!("(alias — reutiliza el frame de su objetivo)")).weak());
        }
    });
    match &sprite.mesh {
        None => {
            ui.label(t!(
                "Este sprite no tiene malla (modo polígono desactivado o sprite alias)."
            ));
        }
        Some(mesh) => {
            ui.label(t!(
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
                // Una triangulación malformada no debe tumbar el callback
                // de pintado: el triángulo cuyo índice se sale se ignora
                // (M15).
                let Some((a, b, c)) = mesh.triangle_vertices(tri) else {
                    continue;
                };
                let pa = egui::pos2(rect.min.x + a.x * scale, rect.min.y + a.y * scale);
                let pb = egui::pos2(rect.min.x + b.x * scale, rect.min.y + b.y * scale);
                let pc = egui::pos2(rect.min.x + c.x * scale, rect.min.y + c.y * scale);
                let stroke = egui::Stroke::new(0.5_f32, egui::Color32::from_rgb(90, 90, 90));
                painter.line_segment([pa, pb], stroke);
                painter.line_segment([pb, pc], stroke);
                painter.line_segment([pc, pa], stroke);
            }
            ui.label(
                egui::RichText::new(t!(
                    "Azul: contorno exterior · Rojo: agujeros · Gris: triángulos"
                ))
                .weak(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{idle_input, textos_pintados};

    /// M3: el panel arrancaba con 180 px para tres líneas de log: el hueco
    /// por debajo de lo escrito salía más grande que lo escrito. Con la
    /// altura de arranque, las tres líneas deben verse enteras y no puede
    /// quedar debajo de la última más aire que una línea.
    #[test]
    fn arranca_con_tres_lineas_de_log_y_sin_hueco_debajo() {
        fn dentro_de(contenedor: egui::Rect, r: egui::Rect) -> bool {
            contenedor.contains(r.min) && contenedor.contains(r.max)
        }

        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        app.log(LogKind::Info, "línea uno".into());
        app.log(LogKind::Info, "línea dos".into());
        app.log(LogKind::Info, "línea tres".into());
        // Dos frames: el primero asienta el tamaño del panel.
        let _ = app.run_frame(&ctx, idle_input());
        let out = app.run_frame(&ctx, idle_input());

        // El recorte de la pestaña es el del panel entero.
        let mut panel: Option<egui::Rect> = None;
        let mut lineas = Vec::new();
        for (texto, rect, recorte) in &textos_pintados(&out) {
            if texto.trim() == "Log" && panel.is_none_or(|p| recorte.min.y > p.min.y) {
                panel = Some(*recorte);
            }
            if matches!(texto.trim(), "línea uno" | "línea dos" | "línea tres") {
                lineas.push((texto.clone(), *rect, *recorte));
            }
        }
        let panel = panel.expect("la pestaña Log debe seguir pintándose");
        assert_eq!(lineas.len(), 3, "el registro debe tener sus tres líneas");

        // Las tres se ven enteras…
        for (texto, rect, recorte) in &lineas {
            assert!(
                dentro_de(*recorte, *rect),
                "«{texto}» queda bajo el panel: {rect:?} no cabe en {recorte:?}"
            );
        }
        // …y debajo de la última no queda un hueco más grande que una línea.
        let ultima = lineas
            .iter()
            .map(|(_, rect, _)| rect.max.y)
            .fold(f32::MIN, f32::max);
        let hueco = panel.max.y - ultima;
        assert!(
            (0.0..=30.0).contains(&hueco),
            "{hueco:.0} px de hueco bajo el log: la altura de arranque \
             (BOTTOM_OPEN_HEIGHT) no corresponde con lo que escribe"
        );
    }

    /// M2: la cabecera del panel izquierdo contaba ficheros —«Sprites (13)»—
    /// y la barra de estado contaba sprites —«12 sprites · 1 alias»—, con la
    /// diferencia sin explicar en ninguna parte. Las dos barras de estado
    /// empiezan ahora por los ficheros que lista el panel: los dos números se
    /// leen juntos y se ve qué fichero no llega a ser sprite.
    #[test]
    fn las_barras_de_estado_empiezan_por_los_ficheros_del_panel() {
        let tmp = std::env::temp_dir().join(format!(
            "tp_m2_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("el reloj va hacia delante")
                .as_nanos()
        ));
        let sprites = tmp.join("sprites");
        std::fs::create_dir_all(&sprites).expect("carpeta de sprites");
        let proyecto =
            crate::testing::create_example_project(&tmp, &sprites).expect("proyecto de ejemplo");
        // Un fichero que el motor no puede leer: está en el panel y no llega
        // a ser sprite —es justo la diferencia que señalaba la review.
        std::fs::write(sprites.join("roto.png"), b"esto no es una imagen").expect("el roto");

        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), Some(proyecto));
        for _ in 0..4000 {
            if app.result().is_some() {
                break;
            }
            let _ = app.run_frame(&ctx, idle_input());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(app.result().is_some(), "el proyecto debe empaquetarse");
        let out = app.run_frame(&ctx, idle_input());

        let mut cabecera = None;
        let mut barras = Vec::new();
        for (texto, _, _) in &textos_pintados(&out) {
            if let Some(resto) = texto.trim().strip_prefix("Sprites (") {
                if let Some(n) = resto.strip_suffix(')') {
                    cabecera = n.parse::<usize>().ok();
                }
            }
            if texto.contains("ficheros · ") {
                barras.push(texto.clone());
            }
        }
        let ficheros = cabecera.expect("la cabecera «Sprites (N)» debe pintarse");
        assert_eq!(
            ficheros, 6,
            "el panel lista los seis ficheros de la carpeta (los cinco del \
             proyecto y el roto)"
        );
        assert_eq!(
            barras.len(),
            2,
            "las dos barras de estado deben llevar el recuento de ficheros: {barras:?}"
        );
        for barra in &barras {
            let al_mando: usize = barra
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("la barra no empieza por un recuento: {barra}"));
            assert_eq!(
                al_mando, ficheros,
                "«{barra}» no empieza por los ficheros del panel"
            );
        }
        assert!(
            barras.iter().any(|b| b.contains("5 sprites")),
            "…y por los cinco que sí son sprites: {barras:?}"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }
}
