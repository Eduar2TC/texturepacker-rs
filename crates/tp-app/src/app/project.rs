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
                    Ok((cfg, warnings)) => {
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
        self.after_workspace_change();
    }
}
