//! Bottom panel: log, output details, sprite table and mesh views.

use super::{App, BottomTab, LogKind};
use crate::i18n::t;
use eframe::egui;

pub(super) fn bottom_ui(app: &mut App, ui: &mut egui::Ui) {
    // El estado con el que `mod.rs` enseñó el panel **este frame**. El clic
    // del chevron llega aquí dentro y cambia `app.bottom_collapsed`, pero
    // lo que se maqueta debajo tiene que corresponder a la identidad con
    // la que egui va a guardar la altura del panel: si el pliegue se
    // aplicara ya, el frame del clic maquetaría sólo la fila donde el
    // panel estaba abierto, egui apuntaría 28 px como su altura y al
    // desplegar no volvería la que tenía —102 px—. El cambio se ve en el
    // frame siguiente, que es indistinguible.
    let plegado = app.bottom_collapsed;
    ui.horizontal(|ui| {
        // Plegar/desplegar el panel (chevron como en cualquier herramienta).
        //
        // Es `button` y no `small_button`: en la misma fila convivían dos
        // alturas, la de las pestañas —`interact_size.y`, 18 px— y la del
        // `small_button`, 15 px, con lo que la caja del chevron se quedaba
        // 3 px por encima y por debajo de la de sus vecinas.
        let chevron = if plegado { "⏵" } else { "⏷" };
        if ui
            .button(chevron)
            .on_hover_text(if plegado {
                t!("Mostrar el panel")
            } else {
                t!("Plegar el panel y dejar la vista del atlas en pantalla completa")
            })
            .clicked()
        {
            app.bottom_collapsed = !plegado;
        }
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Log, "Log");
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Output, t!("Salida"));
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Sprites, "Sprites");
        ui.selectable_value(&mut app.bottom_tab, BottomTab::Mesh, t!("Malla"));
        // Los recuentos viven sólo en la tira de estado (fase 3): aquí se
        // repetían con 120 px de diferencia y el mismo dato se leía dos
        // veces (E1/H3). La tira no se esconde ni con el panel plegado, así
        // que no se pierde nada y la barra de pestañas queda sólo pestañas.
        //
        // Punto de error visible aunque el Log esté plegado o en otra pestaña.
        let has_errors = app.logs.iter().any(|e| matches!(e.kind, LogKind::Error));
        if has_errors {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Mismo motivo que el chevron: aquí también era `small_button`
                // y se salía 3 px de la altura de la fila.
                if ui
                    .button(egui::RichText::new(t!("✖ Error")).color(ui.visuals().error_fg_color))
                    .on_hover_text(t!("Ir al Log"))
                    .clicked()
                {
                    app.bottom_tab = BottomTab::Log;
                    // Igual que el chevron: el cambio de pliegue se nota
                    // en el frame siguiente, nunca en el que lo pide.
                    app.bottom_collapsed = false;
                }
            });
        }
    });
    if plegado {
        return; // solo la tira de pestañas + estado
    }

    // La línea va *después* del retorno. `Frame::show` devuelve como
    // rectángulo del panel lo que maquetó el contenido más sus márgenes y
    // egui guarda ese rectángulo como altura de la identidad con la que se
    // enseñó: maquetando la línea con la banda plegada se apuntaba una
    // altura mayor que la banda —35 px— y era la que después se
    // restauraba al desplegar.
    super::separador(ui);

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
                    // El índice del motor es 0-based; aquí y en la tira la
                    // página se numera desde 1 («Página 0» no es una página).
                    p.index + 1,
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
    use crate::app::{BOTTOM_OPEN_HEIGHT, BOTTOM_PANEL, BOTTOM_PANEL_PLEGADO};
    use crate::testing::{controles_de, idle_input, textos_pintados};

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

    /// En la barra de pestañas sólo puede haber **una** altura. El chevron
    /// de plegar y el botón de error eran `small_button` —15 px—, así que
    /// sus cajas se quedaban 3 px por encima y por debajo de las de las
    /// pestañas, que miden `interact_size.y` —18 px—: dos filas de bordes
    /// distintos dentro de una misma fila.
    #[test]
    fn la_barra_de_pestañas_no_mezcla_alturas() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        // El botón de error sólo se pinta si el registro tiene errores.
        app.log(LogKind::Error, "fallo de prueba".into());
        let _ = app.run_frame(&ctx, idle_input());
        let out = app.run_frame(&ctx, idle_input());

        let pestaña = controles_de(&out, "Log")
            .into_iter()
            .next()
            .expect("la pestaña Log arranca seleccionada y pintada");

        for rotulo in ["⏷", "✖ Error"] {
            let caja = controles_de(&out, rotulo)
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("«{rotulo}» debe estar en la barra"));
            let descuadre = (caja.height() - pestaña.height()).abs();
            assert!(
                descuadre < 0.5,
                "«{rotulo}» mide {:.0} px de alto y la pestaña Log {:.0}: \
                 en una sola fila no puede haber dos alturas",
                caja.height(),
                pestaña.height()
            );
        }
    }

    /// Plegado, el panel es una banda de 22 px: 2 de marco y 18 de fila,
    /// como el menú, la barra de herramientas y la tira de estado. Con
    /// `exact_height(28)` la banda medía 28 y su marco dejaba 6 px de chrome
    /// muerto debajo de la única fila que lleva —1 px de aire entre la línea
    /// de arriba y la fila, 8 entre la fila y la de abajo—, que es justo el
    /// defecto que tenía la barra de zoom.
    #[test]
    fn el_panel_plegado_es_de_22_px_y_llena_su_banda() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        app.bottom_collapsed = true;
        let _ = app.run_frame(&ctx, idle_input());
        let out = app.run_frame(&ctx, idle_input());

        let banda = textos_pintados(&out)
            .into_iter()
            .find(|(texto, _, _)| texto.trim() == "Log")
            .map(|(_, _, recorte)| recorte)
            .expect("la pestaña Log sigue pintándose con el panel plegado");
        let fila = controles_de(&out, "⏵")
            .into_iter()
            .next()
            .expect("el chevron de la barra está en el panel plegado");

        assert!(
            (banda.height() - 22.0).abs() < 0.5,
            "el panel plegado mide {:.0} px: la banda es 2 de marco y 18 de \
             fila, como las demás bandas de 22",
            banda.height()
        );
        let arriba = fila.min.y - banda.min.y;
        let abajo = banda.max.y - fila.max.y;
        assert!(
            (arriba - abajo).abs() < 0.5,
            "la barra de pestañas deja {arriba:.0} px arriba y {abajo:.0} \
             abajo: dentro de su banda tiene que ir centrada"
        );
    }

    /// La altura de la banda inferior: el recorte de la pestaña Log es el
    /// del panel entero.
    fn altura_del_panel(out: &eframe::egui::FullOutput) -> f32 {
        textos_pintados(out)
            .into_iter()
            .filter(|(texto, _, _)| texto.trim() == "Log")
            .map(|(_, _, recorte)| recorte)
            .max_by(|a, b| a.min.y.total_cmp(&b.min.y))
            .expect("la pestaña Log debe seguir pintarse")
            .height()
    }

    /// El chevron de la barra, pulsado y soltado como lo haría el ratón
    /// (en frames distintos, que es como egui distingue un clic de una
    /// pulsación suelta), y dos frames más para que asiente el tamaño que
    /// egui guarda del panel.
    fn clica_el_chevron(app: &mut App, ctx: &egui::Context, glifo: &str) {
        let out = app.run_frame(ctx, idle_input());
        let donde = controles_de(&out, glifo)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("el chevron «{glifo}» debe estar en la barra"))
            .center();
        let boton = |pressed: bool| egui::Event::PointerButton {
            pos: donde,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut entrada = idle_input();
        entrada.events = vec![egui::Event::PointerMoved(donde), boton(true)];
        let _ = app.run_frame(ctx, entrada);
        let mut entrada = idle_input();
        entrada.events = vec![boton(false)];
        let _ = app.run_frame(ctx, entrada);
        let _ = app.run_frame(ctx, idle_input());
        let _ = app.run_frame(ctx, idle_input());
    }

    /// Plegar y desplegar tiene que devolver la altura que el panel tenía.
    ///
    /// El rectángulo que egui guarda para un panel es el que maquetó su
    /// contenido más los márgenes del marco, no el que se le pidió, y el
    /// chevron se toca *dentro* de `bottom.show`: si el pliegue se
    /// aplicara ahí mismo, el frame del clic maquetaría sólo la fila
    /// donde el panel estaba abierto y egui apuntaría esa altura —28 px—
    /// como la del panel abierto. Al desplegar no volvería la de 120 sino
    /// 102, y ya no se recuperaba. Estos tests pulsan el chevron de
    /// verdad, que es por donde entra el fallo.
    #[test]
    fn desplegar_devuelve_la_altura_que_tenia_el_panel() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        // Dos frames: el primero asienta el tamaño del panel.
        let _ = app.run_frame(&ctx, idle_input());
        let out = app.run_frame(&ctx, idle_input());
        let abierta = altura_del_panel(&out);
        assert!(
            (abierta - BOTTOM_OPEN_HEIGHT).abs() < 0.5,
            "el panel arranca con {abierta:.0} px y debe ser {BOTTOM_OPEN_HEIGHT}"
        );

        clica_el_chevron(&mut app, &ctx, "⏷");
        let out = app.run_frame(&ctx, idle_input());
        let plegada = altura_del_panel(&out);
        assert!(
            (plegada - 22.0).abs() < 0.5,
            "la banda plegada mide {plegada:.0} px y debe ser 22"
        );

        clica_el_chevron(&mut app, &ctx, "⏵");
        let out = app.run_frame(&ctx, idle_input());
        let otra_vez = altura_del_panel(&out);
        assert!(
            (otra_vez - abierta).abs() < 0.5,
            "al desplegar el panel mide {otra_vez:.0} px y tenía {abierta:.0}: \
             la altura abierta se guarda antes de plegar y es la que hay que \
             devolver"
        );
    }

    /// Cada estado del panel guarda su altura bajo su propia identidad.
    ///
    /// Es el reverso del test anterior: ahí se mide lo que se ve y aquí lo
    /// que egui persiste, que es de donde sale la altura del frame
    /// siguiente. Con el panel plegado, la identidad abierta tiene que
    /// seguir guardando los 120 px —es la que se restaura— y la plegada
    /// tiene que guardar exactamente su banda: si la fila maqueta la línea
    /// que va debajo, ese sobrante se apunta como altura.
    #[test]
    fn cada_estado_del_panel_guarda_su_propia_altura() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        let _ = app.run_frame(&ctx, idle_input());
        let _ = app.run_frame(&ctx, idle_input());

        clica_el_chevron(&mut app, &ctx, "⏷");

        let estado = |id: &str| {
            egui::containers::panel::PanelState::load(&ctx, egui::Id::new(id))
                .unwrap_or_else(|| panic!("«{id}» no tiene altura guardada"))
                .rect
                .height()
        };
        let plegada = estado(BOTTOM_PANEL_PLEGADO);
        assert!(
            (plegada - 22.0).abs() < 0.5,
            "la banda plegada guarda {plegada:.0} px de estado: maqueta la \
             línea que va debajo de su fila y egui la toma por altura"
        );
        let abierta = estado(BOTTOM_PANEL);
        assert!(
            (abierta - BOTTOM_OPEN_HEIGHT).abs() < 0.5,
            "al plegar la altura abierta pasa a {abierta:.0} px en vez de \
             quedarse en {BOTTOM_OPEN_HEIGHT}: es la que se restaura"
        );
    }

    /// Un proyecto ya empaquetado: los cinco ficheros de ejemplo más uno roto
    /// que el panel lista pero el motor descarta —la diferencia que señalaba
    /// la review (M2). Devuelve también el directorio temporal para que la
    /// prueba lo limpie.
    fn app_empaquetada(prefijo: &str) -> (egui::Context, App, std::path::PathBuf) {
        let tmp = std::env::temp_dir().join(format!(
            "tp_{prefijo}_{}_{}",
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
        (ctx, app, tmp)
    }

    /// M2 + fase 3 (E1/H3): la cabecera del panel izquierdo contaba ficheros
    /// —«Sprites (13)»— y las dos barras de estado repetían el mismo resumen
    /// con 120 px de diferencia. La tira empieza por los ficheros que lista el
    /// panel —los dos números se leen juntos y se ve qué fichero no llega a ser
    /// sprite— y es la **única** que cuenta: la barra de pestañas se quedó sin
    /// resumen.
    #[test]
    fn la_tira_empieza_por_los_ficheros_del_panel_y_es_la_unica_que_cuenta() {
        let (ctx, mut app, tmp) = app_empaquetada("m2");
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
            1,
            "el recuento sólo puede estar una vez, en la tira de estado: {barras:?}"
        );
        let barra = &barras[0];
        let al_mando: usize = barra
            .split_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("la tira no empieza por un recuento: {barra}"));
        assert_eq!(
            al_mando, ficheros,
            "«{barra}» no empieza por los ficheros del panel"
        );
        assert!(
            barra.contains("5 sprites"),
            "…y por los cinco que sí son sprites: {barra}"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Fase 3: la página se numera desde 1 en todas partes. La tira dice
    /// «Página 1» y la pestaña Salida imprimía el índice del motor, que empieza
    /// en 0 («Página 0»): el mismo dato con dos números distintos.
    #[test]
    fn la_salida_numera_las_paginas_como_la_tira() {
        let (ctx, app, tmp) = app_empaquetada("salida");
        // Se pinta la pestaña sola y a pantalla completa: dentro de la barra
        // inferior (120 px) su listado de páginas va el último y queda bajo el
        // pliegue, ni siquiera pintado.
        let out = ctx.run(idle_input(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| output_view(&app, ui));
        });

        let paginas: Vec<String> = textos_pintados(&out)
            .into_iter()
            .filter_map(|(texto, _, _)| texto.trim().strip_prefix("Página ").map(str::to_string))
            .collect();
        assert!(
            paginas.iter().any(|p| p.starts_with("1:")),
            "la pestaña Salida debe empezar la numeración en 1: {paginas:?}"
        );
        assert!(
            !paginas.iter().any(|p| p.starts_with("0:")),
            "ninguna página se llama 0: {paginas:?}"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }
}
