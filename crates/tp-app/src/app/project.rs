//! Project file actions: save, open (`.tpproj` / `.tps`) and reset.

use super::{App, LogKind};
use crate::i18n::t;
use std::path::PathBuf;
use tp_core::config::ProjectConfig;

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
                    self.log(
                        LogKind::Error,
                        t!("Config inválida: {}", crate::i18n::tr(&e.to_string())),
                    );
                    None
                }
            }
        };
        if let Some(text) = text {
            match std::fs::write(&path, text) {
                Ok(_) => {
                    self.project_path = Some(path.clone());
                    self.log(LogKind::Info, t!("Proyecto guardado en {}", path.display()));
                }
                Err(e) => self.log(LogKind::Error, t!("No se pudo guardar: {}", e)),
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
                        self.log(LogKind::Info, t!("Proyecto cargado: {}", path.display()));
                        for warning in warnings {
                            self.log(LogKind::Warning, crate::i18n::tr(&warning));
                        }
                    }
                    Err(e) => self.log(
                        LogKind::Error,
                        t!("Proyecto inválido: {}", crate::i18n::tr(&e.to_string())),
                    ),
                }
            }
            Err(e) => self.log(LogKind::Error, t!("No se pudo leer: {}", e)),
        }
    }

    pub(super) fn reset_defaults(&mut self) {
        self.config = ProjectConfig::default();
        self.sync_variants();
        self.sync_paths();
        self.selected_paths.clear();
        self.selected_sprite = None;
        self.selection_anchor = None;
        self.list_cursor = None;
        self.tree_kb_focus = false;
        self.start_watcher();
        self.log(
            LogKind::Info,
            t!("Configuración restablecida a los valores por defecto.").into(),
        );
        self.after_workspace_change();
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
        assert!(
            texto.contains("input_directory = \"sprites\""),
            "el reemplazo a relativo no cuadró: {texto}"
        );
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
}
