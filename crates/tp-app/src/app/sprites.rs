//! Workspace (the sprite set): add/remove inputs, selection sync, folder
//! groups and the «just added» feedback.

use super::pipeline::collect_images;
use super::{App, LogKind, JUST_ADDED_HL};
use crate::i18n::t;
use std::path::{Path, PathBuf};

impl App {
    /// Mueve unos sprites (por id) a la hoja indicada: los quita de todas
    /// las hojas y los asigna a la de destino. Devuelve cuántos cambiaron.
    pub fn move_sprites_to_group(&mut self, ids: &[String], group_index: usize) -> usize {
        let mut moved = 0usize;
        for id in ids {
            // ¿Ya está en el grupo destino? Retirarlo de los demás y, solo si
            // no estaba ya allí, añadirlo (mover a su propio grupo = no-op;
            // sin este guardia, retain+condición lo BORRABA).
            let mut already_there = false;
            for (i, g) in self.config.folder_groups.iter_mut().enumerate() {
                let before = g.sprites.len();
                g.sprites.retain(|s| s != id);
                if i == group_index && before > g.sprites.len() {
                    already_there = true;
                }
            }
            if !already_there {
                if let Some(g) = self.config.folder_groups.get_mut(group_index) {
                    g.sprites.push(id.clone());
                    moved += 1;
                }
            }
        }
        if moved > 0 {
            self.after_workspace_change();
        }
        moved
    }

    /// Reasigna los sprites seleccionados (por ruta) a un grupo del proyecto.
    /// Asignar a un sprite ya asignado lo mueve (un sprite solo vive en un
    /// grupo); devuelve cuántos sprites cambiaron de grupo.
    pub fn assign_selected_to_group(&mut self, group_index: usize) -> usize {
        let mut moved = 0usize;
        for path in self.selected_paths.clone() {
            let Some(id) = self.sprite_id_for_path(&path) else {
                continue;
            };
            // Mismo guardia que move_sprites_to_group: reasignar a su propio
            // grupo es un no-op, no un borrado.
            let mut already_there = false;
            for (i, g) in self.config.folder_groups.iter_mut().enumerate() {
                let before = g.sprites.len();
                g.sprites.retain(|s| s != &id);
                if i == group_index && before > g.sprites.len() {
                    already_there = true;
                }
            }
            if !already_there {
                if let Some(g) = self.config.folder_groups.get_mut(group_index) {
                    g.sprites.push(id.clone());
                    moved += 1;
                }
            }
        }
        if moved > 0 {
            self.after_workspace_change();
        }
        moved
    }

    /// ¿Hay algo que empaquetar? Separa «workspace vacío» (lienzo accionable
    /// con su diana de suelta) de «workspace con sprites, atlas aún sin
    /// calcular» (estado de cálculo), que es donde el usuario percibe que
    /// «no se actualiza».
    pub(crate) fn has_inputs(&self) -> bool {
        !self.config.input_directory.as_os_str().is_empty() || !self.config.extra_inputs.is_empty()
    }

    /// ¿`path` entró hace instantes? Resaltado temporal de feedback.
    pub(crate) fn is_just_added(&self, path: &Path) -> bool {
        let Some(at) = self.just_added_at else {
            return false;
        };
        if at.elapsed() >= JUST_ADDED_HL {
            return false;
        }
        let norm = tp_core::ingest::normalize_path(path);
        self.just_added
            .iter()
            .any(|p| tp_core::ingest::normalize_path(p) == norm)
    }

    /// Nombres de lo recién añadido, para el estado de cálculo del lienzo.
    pub(crate) fn just_added_names(&self) -> Vec<String> {
        let fresh = self
            .just_added_at
            .map(|at| at.elapsed() < JUST_ADDED_HL)
            .unwrap_or(false);
        if !fresh {
            return Vec::new();
        }
        self.just_added
            .iter()
            .map(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string())
            })
            .collect()
    }

    /// Add a dropped/picked file or folder to the sprite set.
    /// Returns `true` when the workspace changed and a repack is scheduled.
    pub(super) fn add_input(&mut self, path: PathBuf) -> bool {
        if path.as_os_str().is_empty() {
            return false;
        }
        let norm = tp_core::ingest::normalize_path(&path);
        if !self.config.input_directory.as_os_str().is_empty()
            && tp_core::ingest::normalize_path(&self.config.input_directory) == norm
        {
            return false;
        }
        if self
            .config
            .extra_inputs
            .iter()
            .any(|p| tp_core::ingest::normalize_path(p) == norm)
        {
            return false;
        }
        if path.is_file() && !tp_core::ingest::is_image_file(&path) {
            return false;
        }
        if path.is_file() {
            let hidden = self
                .config
                .excluded_inputs
                .iter()
                .any(|p| tp_core::ingest::normalize_path(p) == norm);
            if hidden {
                self.config
                    .excluded_inputs
                    .retain(|p| tp_core::ingest::normalize_path(p) != norm);
            }
        }
        self.just_added.push(path.clone());
        if self.just_added.len() > 32 {
            let over = self.just_added.len() - 32;
            self.just_added.drain(0..over);
        }
        self.just_added_at = Some(std::time::Instant::now());
        self.config.extra_inputs.push(path);
        self.start_watcher();
        self.change_seq += 1;
        self.request_preview(true);
        true
    }

    /// Remove the selected sprites/folders from the sprite set.
    ///
    /// `order` es el orden visual de la lista del panel: tras quitar el
    /// lote, el cursor queda en la fila siguiente visible (o en la
    /// anterior, si se borró el final), de modo que repetir Supr recorre
    /// la lista sin volver al ratón.
    pub(super) fn remove_selected(&mut self, order: &[PathBuf]) {
        let selected: Vec<PathBuf> = self.selected_paths.iter().cloned().collect();
        if selected.is_empty() {
            return;
        }
        self.anotar_deshacer();
        // Posición del lote en la lista antes de quitarlo (para el «siguiente»).
        let first = order.iter().position(|p| selected.contains(p));
        let last = order.iter().rposition(|p| selected.contains(p));
        let mut removed = 0usize;
        for path in &selected {
            removed += self.remove_path(path);
        }
        self.selected_paths.clear();
        self.selected_sprite = None;
        // Primera fila que sobrevive tras el lote, buscando primero hacia
        // abajo y luego hacia arriba (estándar de los gestores de archivos).
        let next = match (first, last) {
            (Some(f), Some(l)) => order
                .iter()
                .skip(l + 1)
                .chain(order.iter().take(f).rev())
                .find(|p| !selected.contains(p) && !selected.iter().any(|s| p.starts_with(s)))
                .cloned(),
            _ => None,
        };
        self.list_cursor = next.clone();
        self.selection_anchor = next.clone();
        if let Some(p) = next {
            self.selected_paths.insert(p);
            self.sync_selected_sprite();
        }
        self.aviso(LogKind::Info, t!("{} sprite(s) quitado(s).", removed));
        self.change_seq += 1;
        self.request_preview(true);
    }

    /// Sincroniza `selected_sprite` (id dentro del pack) con `selected_paths`:
    /// solo hay id cuando la selección es exactamente un sprite.
    pub(super) fn sync_selected_sprite(&mut self) {
        if self.selected_paths.len() == 1 {
            let only = self.selected_paths.iter().next().cloned();
            self.selected_sprite = only.and_then(|p| {
                self.result.as_ref().and_then(|out| {
                    out.result
                        .sprites
                        .iter()
                        .find(|s| Path::new(&s.source_path) == p.as_path())
                        .map(|s| s.id.clone())
                })
            });
        } else {
            self.selected_sprite = None;
        }
    }

    /// Exclude a single file, or every image inside a directory.
    /// Returns how many sprites were hidden.
    pub(super) fn remove_path(&mut self, path: &Path) -> usize {
        if path.is_dir() {
            let mut files = Vec::new();
            let fallidos = collect_images(path, &mut files);
            self.report_unreadable(fallidos);
            for file in &files {
                self.exclude(file);
            }
            files.len()
        } else {
            self.exclude(path);
            1
        }
    }

    /// Drop a smart folder from `extra_inputs` (its files come back from disk
    /// the next time a project with that folder is opened).
    pub(super) fn remove_smart_folder(&mut self, dir: &Path) {
        let norm = tp_core::ingest::normalize_path(dir);
        if tp_core::ingest::normalize_path(&self.config.input_directory) == norm {
            self.aviso(
                LogKind::Warning,
                t!("Ese directorio es el de entrada principal; quítalo en Ajustes > Datos.").into(),
            );
            return;
        }
        let before = self.config.extra_inputs.len();
        self.config
            .extra_inputs
            .retain(|p| tp_core::ingest::normalize_path(p) != norm);
        if self.config.extra_inputs.len() == before {
            return;
        }
        // Stale exclusions under the removed folder are meaningless now.
        self.config.excluded_inputs.retain(|p| !p.starts_with(dir));
        self.selected_paths.retain(|p| !p.starts_with(dir));
        self.aviso(
            LogKind::Info,
            t!("Carpeta inteligente quitada: {}", dir.display()),
        );
        self.start_watcher();
        self.change_seq += 1;
        self.request_preview(true);
    }

    pub(super) fn exclude(&mut self, path: &Path) {
        let norm = tp_core::ingest::normalize_path(path);
        if self
            .config
            .excluded_inputs
            .iter()
            .any(|p| tp_core::ingest::normalize_path(p) == norm)
        {
            return;
        }
        self.config.excluded_inputs.push(path.to_path_buf());
    }

    pub(super) fn restore_excluded(&mut self) {
        let count = self.config.excluded_inputs.len();
        if count == 0 {
            return;
        }
        self.config.excluded_inputs.clear();
        self.aviso(LogKind::Info, t!("{} sprite(s) restaurado(s).", count));
        self.change_seq += 1;
        self.request_preview(true);
    }

    pub(super) fn hidden_count(&self) -> usize {
        self.config.excluded_inputs.len()
    }

    /// Highlight a sprite from the preview/table and sync the sprites panel.
    pub(super) fn select_sprite(&mut self, id: &str) {
        self.selected_sprite = Some(id.to_string());
        if let Some(out) = &self.result {
            if let Some(sprite) = out.result.sprites.iter().find(|s| s.id == id) {
                if sprite.atlas_page_index >= 0 {
                    self.selected_page = sprite.atlas_page_index as usize;
                }
                let path = PathBuf::from(&sprite.source_path);
                self.selected_paths.clear();
                self.selected_paths.insert(path.clone());
                self.list_cursor = Some(path.clone());
                self.selection_anchor = Some(path);
            }
        }
    }

    /// Indices into the packed sprite list that match the current selection.
    pub(super) fn selected_sprite_indices(&self) -> Vec<usize> {
        let Some(out) = &self.result else {
            return Vec::new();
        };
        out.result
            .sprites
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                self.selected_sprite.as_deref() == Some(s.id.as_str())
                    || self.selected_paths.contains(Path::new(&s.source_path))
            })
            .map(|(i, _)| i)
            .collect()
    }
}
