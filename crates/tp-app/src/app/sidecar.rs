//! Sidecar files next to the sprites: `pivots.json`, `borders.json` and the
//! 9-patch border auto-detect.

use super::{App, LogKind};
use crate::i18n::t;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tp_core::types::Point2D;

impl App {
    /// Directory that receives the 9-patch/pivot sidecar files (the smart
    /// folder of the first sprite when there is no main input directory).
    pub(super) fn sidecar_dir(&self) -> Option<PathBuf> {
        if !self.config.input_directory.as_os_str().is_empty() {
            return Some(self.config.input_directory.clone());
        }
        self.config
            .extra_inputs
            .iter()
            .find(|p| p.is_file())
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| {
                self.config
                    .extra_inputs
                    .iter()
                    .find(|p| p.is_dir())
                    .cloned()
            })
    }

    /// Auto-detect 9-patch borders on the selected sprites from their source
    /// pixels (solid frame analysis) and apply them to the pack result.
    pub(super) fn detect_borders(&mut self) {
        let indices = self.selected_sprite_indices();
        if indices.is_empty() {
            self.log(
                LogKind::Warning,
                t!("Selecciona sprites antes de detectar bordes.").into(),
            );
            return;
        }
        let Some(mut out) = self.result.take() else {
            self.log(
                LogKind::Warning,
                t!("Publica el atlas antes de detectar bordes.").into(),
            );
            return;
        };
        let threshold = self.config.trim_threshold.clamp(0, 255) as u8;
        let tolerance = self.config.detect_border_tolerance;
        let max_search = self.config.detect_border_max_search;
        // (index, id, borde detectado)
        let mut results: Vec<(usize, String, Option<[i32; 4]>)> = Vec::new();
        for &i in &indices {
            let Some(sprite) = out.result.sprites.get(i) else {
                continue;
            };
            let path = std::path::PathBuf::from(&sprite.source_path);
            // Los normal maps comparten frame: no detectar sobre ellos.
            if tp_core::ingest::is_normal_file(&path, &self.config.normal_map_suffix) {
                continue;
            }
            let id = sprite.id.clone();
            let (w, h, rgba) = match tp_core::ingest::load_image_rgba(&path) {
                Ok(v) => v,
                Err(e) => {
                    self.log(
                        LogKind::Warning,
                        t!(
                            "No se pudo leer {} para detectar bordes: {}",
                            path.display(),
                            crate::i18n::tr(&e.to_string())
                        ),
                    );
                    continue;
                }
            };
            let b =
                tp_core::ingest::detect_borders_auto(&rgba, w, h, threshold, tolerance, max_search);
            let border = if b == [0, 0, 0, 0] { None } else { Some(b) };
            results.push((i, id, border));
        }
        let mut detected = 0usize;
        for (i, id, border) in results {
            if let Some(sprite) = out.result.sprites.get_mut(i) {
                sprite.border = border;
            }
            match border {
                Some(b) => {
                    detected += 1;
                    self.config.border_overrides.insert(id.clone(), b);
                    self.log(
                        LogKind::Info,
                        t!("{}: bordes detectados {}", id, format!("{b:?}")),
                    );
                }
                None => {
                    self.config.border_overrides.remove(&id);
                    self.log(
                        LogKind::Warning,
                        t!("{}: sin barras sólidas, se quita el 9-patch", id),
                    );
                }
            }
        }
        self.log(
            LogKind::Info,
            t!(
                "Detección de bordes 9-patch: {} de {} sprite(s) con barras sólidas",
                detected,
                indices.len()
            ),
        );
        self.result = Some(out);
    }

    pub(super) fn save_pivots(&mut self) {
        let Some(dir) = self.sidecar_dir() else {
            self.log(
                LogKind::Error,
                t!("Añade sprites primero: los archivos se guardan junto a ellos.").into(),
            );
            return;
        };
        let Some(out) = &self.result else {
            self.log(
                LogKind::Error,
                t!("Publica primero el atlas antes de guardar pivots.").into(),
            );
            return;
        };
        let map: HashMap<&str, Point2D> = out
            .result
            .sprites
            .iter()
            .map(|s| (s.id.as_str(), s.pivot))
            .collect();
        let borders: HashMap<String, [i32; 4]> = out
            .result
            .sprites
            .iter()
            .filter_map(|s| s.border.map(|b| (s.id.clone(), b)))
            .collect();
        let path = dir.join("pivots.json");
        match serde_json::to_string_pretty(&map) {
            Ok(text) => match std::fs::write(&path, text) {
                Ok(_) => self.aviso(LogKind::Info, t!("Pivots guardados en {}", path.display())),
                Err(e) => self.aviso(LogKind::Error, t!("No se pudo guardar: {}", e)),
            },
            Err(e) => self.aviso(LogKind::Error, t!("No se pudo serializar: {}", e)),
        }
        // Bordes 9-patch a un archivo propio (solo sprites con bordes).
        self.write_borders_file(&dir, &borders);
    }

    /// Write `borders.json` next to the input sprites and log the result.
    pub(super) fn write_borders_file(&mut self, dir: &Path, borders: &HashMap<String, [i32; 4]>) {
        let bpath = dir.join("borders.json");
        match serde_json::to_string_pretty(borders) {
            Ok(text) => match std::fs::write(&bpath, text) {
                Ok(_) => {
                    if !borders.is_empty() {
                        self.log(
                            LogKind::Info,
                            t!(
                                "Bordes 9-patch guardados en {} ({} sprite(s))",
                                bpath.display(),
                                borders.len()
                            ),
                        );
                    }
                }
                Err(e) => self.log(LogKind::Error, t!("No se pudo guardar borders.json: {}", e)),
            },
            Err(e) => self.log(
                LogKind::Error,
                t!("No se pudo serializar borders.json: {}", e),
            ),
        }
    }

    /// Persist `borders.json` without user action (drag release / Detect).
    /// Silent when there is no sprite directory or nothing to save.
    pub(super) fn auto_save_borders(&mut self) {
        let Some(dir) = self.sidecar_dir() else {
            return;
        };
        let Some(out) = &self.result else {
            return;
        };
        let borders: HashMap<String, [i32; 4]> = out
            .result
            .sprites
            .iter()
            .filter_map(|s| s.border.map(|b| (s.id.clone(), b)))
            .collect();
        if borders.is_empty() {
            return; // nada que guardar: no crear archivos vacíos ni avisar
        }
        let path = dir.join("borders.json");
        if let Ok(text) = serde_json::to_string_pretty(&borders) {
            if std::fs::write(&path, text).is_ok() {
                self.log(
                    LogKind::Info,
                    t!(
                        "borders.json actualizado automáticamente ({} borde(s))",
                        borders.len()
                    ),
                );
            }
        }
    }
}
