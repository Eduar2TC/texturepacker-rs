//! Project file actions: save, open (`.tpproj` / `.tps`) and reset.

use super::{App, LogKind};
use crate::i18n::t;
use eframe::egui;
use std::path::PathBuf;
use tp_core::config::ProjectConfig;

/// El motivo de un guardado que falla, con la acción delante. El motor ya
/// envuelve el detalle con su propio prefijo («Configuración inválida: …»,
/// «Error TOML: …»), así que aquí no se le pone otro parecido: el envoltorio
/// anterior —«Config inválida: {motivo}»— dejaba el prefijo dos veces, y en
/// inglés salía «Invalid config: Invalid configuration: …» (re-review RB5).
/// El aviso traduce el texto que recibe, pero los argumentos de `t!` van
/// crudos (no se traducen solos), así que el motivo del motor se traduce
/// aquí a mano; criterio de [`crate::i18n::io_motivo`] y de `motivo_clave`.
fn motivo_de_guardado(e: &tp_core::TpError) -> String {
    t!("No se pudo guardar: {}", crate::i18n::tr(&e.to_string()))
}

impl App {
    pub(super) fn save_project(&mut self) {
        self.commit_paths();
        let path = self.project_path.clone().or_else(|| {
            rfd::FileDialog::new()
                .add_filter(t!("Proyecto"), &["tpproj", "tps"])
                .save_file()
        });
        let Some(path) = path else { return };
        self.parse_variants();
        // `.tps` escribe el XML del original; cualquier otra extensión TOML.
        let text = if path.extension().and_then(|e| e.to_str()) == Some("tps") {
            Some(tp_core::tps::write_tps(&self.config))
        } else {
            match self.config.to_toml() {
                Ok(text) => Some(text),
                Err(e) => {
                    self.aviso(LogKind::Error, motivo_de_guardado(&e));
                    None
                }
            }
        };
        if let Some(text) = text {
            match std::fs::write(&path, text) {
                Ok(_) => {
                    self.project_path = Some(path.clone());
                    // A partir de aquí lo que hay en memoria es lo que hay en
                    // disco: no quedan cambios pendientes (C2).
                    self.saved_config = self.config.to_toml().unwrap_or_default();
                    self.aviso(LogKind::Info, t!("Proyecto guardado en {}", path.display()));
                }
                Err(e) => self.aviso(
                    LogKind::Error,
                    t!("No se pudo guardar: {}", crate::i18n::io_motivo(&e)),
                ),
            }
        }
    }

    pub(super) fn load_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(t!("Proyecto"), &["tpproj", "tps"])
            .pick_file()
        else {
            return;
        };
        self.open_project(path);
    }

    pub(super) fn open_project(&mut self, path: PathBuf) {
        let is_tps = path.extension().and_then(|e| e.to_str()) == Some("tps");
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                // El .tps del original es XML: se traduce a la configuración
                // de aquí y se cuentan sus avisos en el registro.
                let loaded = if is_tps {
                    tp_core::tps::parse_tps(&text, path.parent())
                        .map(|tps| (tps.config, tps.warnings))
                } else {
                    ProjectConfig::from_toml(&text).map(|cfg| (cfg, Vec::new()))
                };
                match loaded {
                    Ok((mut cfg, warnings)) => {
                        // Las rutas guardadas en relativo van contra el
                        // propio proyecto, no contra el directorio de
                        // trabajo desde el que se lanzó la app (C3).
                        cfg.resolve_relative_paths(path.parent());
                        self.config = cfg;
                        self.saved_config = self.config.to_toml().unwrap_or_default();
                        self.sync_variants();
                        self.sync_paths();
                        self.selected_paths.clear();
                        self.selected_sprite = None;
                        self.selection_anchor = None;
                        self.list_cursor = None;
                        self.tree_kb_focus = false;
                        // Proyecto nuevo, vista nueva: el encuadre se vuelve
                        // a hacer solo con la primera vista previa.
                        self.auto_fit = true;
                        self.start_watcher();
                        self.project_path = Some(path.clone());
                        self.aviso(LogKind::Info, t!("Proyecto cargado: {}", path.display()));
                        for warning in warnings {
                            self.log(LogKind::Warning, crate::i18n::tr(&warning));
                        }
                    }
                    Err(e) => self.aviso(
                        LogKind::Error,
                        t!("Proyecto inválido: {}", crate::i18n::tr(&e.to_string())),
                    ),
                }
            }
            Err(e) => self.aviso(
                LogKind::Error,
                t!("No se pudo leer: {}", crate::i18n::io_motivo(&e)),
            ),
        }
    }

    pub(super) fn reset_defaults(&mut self) {
        self.anotar_deshacer();
        self.config = ProjectConfig::default();
        self.sync_variants();
        self.sync_paths();
        self.selected_paths.clear();
        self.selected_sprite = None;
        self.selection_anchor = None;
        self.list_cursor = None;
        self.tree_kb_focus = false;
        self.start_watcher();
        self.aviso(
            LogKind::Info,
            t!("Configuración restablecida a los valores por defecto.").into(),
        );
        self.after_workspace_change();
    }

    /// `true` cuando la configuración en memoria se ha separado de la última
    /// versión escrita (o cargada) en disco: hay cambios sin guardar.
    ///
    /// Se compara por la misma huella TOML que escribe el `.tpproj`, de modo
    /// que lo que no se serializa tampoco cuenta como cambio pendiente. La
    /// huella es estable mientras la configuración no cambia, porque la
    /// serialización recorre el mismo `HashMap` de siempre.
    pub(super) fn is_dirty(&self) -> bool {
        self.config
            .to_toml()
            .map(|t| t != self.saved_config)
            .unwrap_or(true)
    }

    /// Diálogo «hay cambios sin guardar»: se abre cuando el sistema pide
    /// cerrar la ventana con la configuración tocada y sólo se cierra con una
    /// decisión. Guardar puede cancelarse (diálogo de archivo del sistema o
    /// error de escritura), en cuyo caso la ventana sigue abierta (C2).
    ///
    /// El orden y el peso son los de un diálogo de confirmación de GTK
    /// (re-review D6): los botones van alineados a la derecha, la
    /// cancelación a la izquierda del grupo, la destructiva en el centro
    /// con su color y la sugerida la última, con el acento. Antes salían
    /// tres `ui.button` idénticos, a la izquierda y con «Guardar y salir»
    /// el primero de todos, que es justo el revés.
    pub(super) fn exit_dialog(&mut self, ctx: &egui::Context) {
        if !self.exit_pending {
            return;
        }
        let respuesta = egui::Modal::new(egui::Id::new("exit_dialog")).show(ctx, |ui| {
            ui.set_min_width(360.0);
            ui.label(egui::RichText::new(t!("Hay cambios sin guardar")).heading());
            ui.add_space(8.0);
            ui.label(t!("¿Guardar los cambios antes de salir?"));
            ui.add_space(16.0);
            let visuals = ui.visuals().clone();
            let destructivo = visuals.error_fg_color;
            let sugerido = visuals.selection.bg_fill;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let guardar = ui.add(
                    egui::Button::new(
                        egui::RichText::new(t!("💾 Guardar y salir"))
                            .color(super::theme::texto_sobre(sugerido)),
                    )
                    .fill(sugerido),
                );
                let tirar = ui.add(
                    egui::Button::new(
                        egui::RichText::new(t!("Salir sin guardar"))
                            .color(super::theme::texto_sobre(destructivo)),
                    )
                    .fill(destructivo),
                );
                if ui.button(t!("Cancelar")).clicked() {
                    self.exit_pending = false;
                }
                if tirar.clicked() {
                    self.exit_pending = false;
                    self.exit_confirmed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if guardar.clicked() {
                    self.save_project();
                    self.exit_pending = false;
                    // Si el guardado llegó a completarse no queda nada
                    // pendiente y la salida puede tramitarse.
                    self.exit_confirmed = !self.is_dirty();
                    if self.exit_confirmed {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            });
        });
        if respuesta.should_close() {
            // Esc o clic fuera del diálogo: se trata como «Cancelar».
            self.exit_pending = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Restablecer es destructivo y hasta ahora silencioso: sin este registro
    /// el usuario no tendría forma de saber que la app le acaba de tirar la
    /// configuración entera (review UI/UX C5).
    #[test]
    fn reset_deja_constancia_en_el_registro() {
        let ctx = eframe::egui::Context::default();
        let mut app = App::new_for_testing(ctx, None);
        app.config.padding = 7;
        assert_ne!(
            app.config.padding,
            ProjectConfig::default().padding,
            "el ajuste debe estar cambiado para que el test signifique algo"
        );

        app.reset_defaults();

        assert_eq!(
            app.config.padding,
            ProjectConfig::default().padding,
            "reset_defaults debe restaurar los valores por defecto"
        );
        assert!(
            app.logs.iter().any(|e| e.text.contains("restablecida")),
            "reset_defaults debe dejar constancia en el registro"
        );
    }

    /// M7: el `io::Error` crudo del sistema («No such file or directory
    /// (os error 2)») salía en inglés en mitad de un mensaje español. Al
    /// abrir un proyecto inexistente el registro debe enseñar el motivo
    /// localizado, que es lo que da pie a corregir la ruta.
    #[test]
    fn abrir_un_proyecto_inexistente_no_ensena_el_error_crudo_del_sistema() {
        let ctx = eframe::egui::Context::default();
        let mut app = App::new_for_testing(ctx, None);
        let ausente =
            std::env::temp_dir().join(format!("tp_no_existe_{}.tpproj", std::process::id()));
        assert!(!ausente.exists(), "el fichero no debe existir: {ausente:?}");

        app.open_project(ausente);

        let texto = app
            .logs
            .last()
            .map(|e| e.text.clone())
            .expect("abrir un fichero inexistente debe dejar constancia");
        assert!(
            texto.contains("No se pudo leer: el fichero o la carpeta no existe"),
            "mensaje localizado esperado, llegó: {texto}"
        );
        assert!(
            !texto.contains("os error") && !texto.contains("No such file"),
            "no debe quedar el texto crudo del sistema: {texto}"
        );
    }

    /// C3: una ruta escrita en relativo en el `.tpproj` significa «de junto
    /// al proyecto», no «de junto al directorio de trabajo» (que en las
    /// pruebas es la raíz del crate, donde no hay ningún `sprites`).
    #[test]
    fn las_rutas_relativas_se_resuelven_contra_el_proyecto() {
        let tmp = std::env::temp_dir().join(format!(
            "tp_rutas_rel_{}_{}",
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

        // Se reescribe el `.tpproj` con la ruta de entrada en relativo, como
        // si lo hubiera escrito a mano para compartir la carpeta.
        let texto = std::fs::read_to_string(&proyecto).expect("lee el .tpproj");
        let texto = texto.replace(&sprites.display().to_string(), "sprites");
        // TOML escoge cadena literal («'…'») cuando el valor lleva barras
        // invertidas, que es el caso normal en Windows: la comprobación no
        // puede depender del estilo de comillas que el serializador elija.
        let relativa = texto.contains("input_directory = \"sprites\"")
            || texto.contains("input_directory = 'sprites'");
        assert!(relativa, "el reemplazo a relativo no cuadró: {texto}");
        std::fs::write(&proyecto, texto).expect("reescribe el .tpproj");

        let ctx = eframe::egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), Some(proyecto));
        assert_eq!(
            app.config.input_directory, sprites,
            "la ruta relativa debe resolverse contra la carpeta del .tpproj"
        );

        // …y con eso la vista previa encuentra los sprites.
        let outcome = crate::testing::pump_on_demand(
            &mut app,
            &ctx,
            |a| a.result.is_some(),
            std::time::Duration::from_secs(30),
        );
        let n = app
            .result
            .as_ref()
            .map(|o| o.result.sprites.len())
            .unwrap_or(0);
        assert!(
            n >= 4,
            "la vista previa debe encontrar los sprites (hay {n}): {outcome:?}"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Proyecto temporal con su `.tpproj` ya en disco: `save_project` escribe
    /// en él sin necesidad de abrir el diálogo de archivo del sistema.
    fn proyecto_temp(tag: &str) -> (PathBuf, PathBuf) {
        let tmp = std::env::temp_dir().join(format!(
            "tp_dirty_{tag}_{}_{}",
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
        (tmp, proyecto)
    }

    /// Un frame en el que el sistema pide cerrar la ventana.
    fn input_cierre() -> egui::RawInput {
        let info = egui::ViewportInfo {
            events: vec![egui::ViewportEvent::Close],
            ..Default::default()
        };
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_max(
                egui::Pos2::ZERO,
                egui::pos2(1360.0, 860.0),
            )),
            viewports: [(egui::ViewportId::ROOT, info)].into_iter().collect(),
            ..egui::RawInput::default()
        }
    }

    fn cancela_el_cierre(out: &egui::FullOutput) -> bool {
        out.viewport_output.values().any(|v| {
            v.commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::CancelClose))
        })
    }

    /// RB5: el envoltorio repetía el prefijo que el motor ya pone sobre el
    /// detalle: «Config inválida: Configuración inválida: …» en español y
    /// «Invalid config: Invalid configuration: …» en inglés.
    #[test]
    fn el_motivo_de_un_guardado_fallido_no_repite_el_prefijo() {
        let detalle = "max_texture_size debe ser una potencia de dos positiva (se obtuvo 3000)";
        let motor = tp_core::TpError::Config(detalle.to_string());
        assert_eq!(
            motivo_de_guardado(&motor),
            format!("No se pudo guardar: Configuración inválida: {detalle}")
        );

        // El fallo que `to_toml` puede devolver de verdad es de
        // serialización: la acción se cuenta una vez y el motivo queda tal
        // como lo escribe el motor, con su prefijo y sin repetirlo.
        let claves: std::collections::HashMap<i32, u8> = [(1, 2)].into();
        let ser = toml::to_string_pretty(&claves).expect_err("las claves enteras no son TOML");
        let texto = motivo_de_guardado(&tp_core::TpError::TomlSer(ser));
        assert!(
            texto.starts_with("No se pudo guardar: Error TOML: "),
            "acción y motivo, un solo prefijo por nivel: {texto}"
        );
    }

    /// C2: la huella de «sin guardar» acompaña a la configuración: cambia
    /// cuando se toca y se borra cuando se escribe en disco.
    #[test]
    fn los_cambios_sin_guardar_se_boran_al_guardar() {
        let (tmp, proyecto) = proyecto_temp("huella");
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx, Some(proyecto.clone()));
        assert!(!app.is_dirty(), "recién abierto no hay nada pendiente");

        app.config.padding = 5;
        assert!(
            app.is_dirty(),
            "tocar la configuración deja cambios sin guardar"
        );

        app.save_project();
        assert!(!app.is_dirty(), "guardar los debe borrar");

        // Un cambio posterior sigue pendiente en esta sesión, pero no
        // contamina al fichero: lo que quedó escrito no es un cambio al
        // volver a abrirlo.
        app.config.padding = 9;
        assert!(app.is_dirty(), "los cambios posteriores siguen pendientes");
        let otra = App::new_for_testing(egui::Context::default(), Some(proyecto));
        assert!(
            !otra.is_dirty(),
            "lo recién guardado no es un cambio pendiente"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// C2: cerrar con cambios pendientes no se tramita solo: el cierre se
    /// cancela y se abre el diálogo de confirmación.
    #[test]
    fn cerrar_con_cambios_pendientes_se_cancela_y_se_pregunta() {
        let (tmp, proyecto) = proyecto_temp("cierre");
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), Some(proyecto));
        app.config.padding = 5;
        assert!(app.is_dirty(), "el test sólo tiene sentido con cambios");

        let out = app.run_frame(&ctx, input_cierre());

        assert!(app.exit_pending, "debe abrirse el diálogo de confirmación");
        assert!(
            cancela_el_cierre(&out),
            "el cierre debe cancelarse hasta que el usuario decida"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// D6: el diálogo de salida llevaba tres `ui.button` idénticos, a la
    /// izquierda y con «Guardar y salir» el primero de todos: el revés
    /// del orden de un diálogo de confirmación de GTK —cancelación a la
    /// izquierda del grupo, destructiva en el centro con su color y
    /// sugerida la última, alineadas a la derecha—. Se mide sobre el
    /// frame pintado, no sobre el código.
    #[test]
    fn el_dialogo_de_salida_sigue_el_orden_y_el_peso_de_gtk() {
        let (tmp, proyecto) = proyecto_temp("orden_salida");
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), Some(proyecto));
        app.config.padding = 5;
        assert!(app.is_dirty(), "el test sólo tiene sentido con cambios");

        let _ = app.run_frame(&ctx, input_cierre());
        assert!(app.exit_pending, "el diálogo debe estar abierto");
        // El modal se pinta en el frame siguiente al que tramita el
        // cierre pedido por el sistema.
        let out = app.run_frame(&ctx, crate::testing::idle_input());

        let caja = |texto: &str| {
            crate::testing::galones_pintados(&out)
                .into_iter()
                .find(|(t, _)| t.trim() == texto)
                .map(|(_, rect)| rect)
                .unwrap_or_else(|| panic!("«{texto}» no está pintado"))
        };
        // El relleno más pequeño que contiene al rótulo es el propio
        // botón: es la misma inspección que `controles_de`, sin salir
        // del crate.
        let relleno_en = |punto: egui::Pos2| {
            crate::testing::rellenos_pintados(&out)
                .into_iter()
                .filter(|(_, rect)| rect.contains(punto))
                .min_by(|a, b| a.1.area().total_cmp(&b.1.area()))
                .map(|(color, _)| color)
                .unwrap_or_else(|| panic!("no hay relleno pintado que contenga a {punto:?}"))
        };

        let cancelar = caja("Cancelar");
        let tirar = caja("Salir sin guardar");
        let guardar = caja("💾 Guardar y salir");
        assert!(
            cancelar.center().x < tirar.center().x && tirar.center().x < guardar.center().x,
            "el orden debe ser «Cancelar», «Salir sin guardar», «Guardar y salir»: \
             {:?}, {:?}, {:?}",
            cancelar.center(),
            tirar.center(),
            guardar.center()
        );
        // Los tres en la mitad derecha del diálogo, como en GTK: la fila
        // no empieza pegada al borde izquierdo de la ventana.
        assert!(
            cancelar.left() > 360.0 / 2.0,
            "el grupo de botones va alineado a la derecha, no a la izquierda: \
             arranca en {}",
            cancelar.left()
        );

        let v = ctx.style().visuals.clone();
        assert_eq!(
            relleno_en(tirar.center()),
            v.error_fg_color,
            "la acción destructiva lleva el color de error del tema"
        );
        assert_eq!(
            relleno_en(guardar.center()),
            v.selection.bg_fill,
            "la acción sugerida lleva el acento del tema"
        );
        assert_ne!(
            relleno_en(cancelar.center()),
            relleno_en(guardar.center()),
            "la cancelación no puede pesar lo mismo que la sugerida"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Y cerrar sin nada pendiente sigue siendo instantáneo.
    #[test]
    fn cerrar_sin_cambios_pendientes_no_molesta() {
        let (tmp, proyecto) = proyecto_temp("cierre_limpio");
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), Some(proyecto));
        assert!(!app.is_dirty(), "no debe haber nada pendiente");

        let out = app.run_frame(&ctx, input_cierre());

        assert!(
            !app.exit_pending,
            "si no hay cambios no debe abrirse el diálogo"
        );
        assert!(!cancela_el_cierre(&out), "el cierre debe pasar sin más");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
