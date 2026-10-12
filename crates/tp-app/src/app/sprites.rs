//! Workspace (the sprite set): add/remove inputs, selection sync, folder
//! groups and the «just added» feedback.

use super::pipeline::collect_images;
use super::{App, LogKind, JUST_ADDED_HL};
use crate::i18n::t;
use std::path::{Path, PathBuf};

impl App {
    /// Mueve unos sprites (por id) a la hoja indicada: los quita de todas
    /// las hojas menos la de destino y los asigna a ella si no estaban ya.
    /// Devuelve cuántos cambiaron.
    pub fn move_sprites_to_group(&mut self, ids: &[String], group_index: usize) -> usize {
        let mut moved = 0usize;
        for id in ids {
            // Retirarlo sólo de las demás hojas: si el `retain` lo quitara
            // también del destino, soltar un sprite sobre su propia hoja lo
            // devolvería a la hoja principal en lugar de no hacer nada.
            let mut ya_estaba = false;
            for (i, g) in self.config.folder_groups.iter_mut().enumerate() {
                if i == group_index {
                    ya_estaba = g.sprites.iter().any(|s| s == id);
                } else {
                    g.sprites.retain(|s| s != id);
                }
            }
            if ya_estaba {
                continue;
            }
            if let Some(g) = self.config.folder_groups.get_mut(group_index) {
                g.sprites.push(id.clone());
                moved += 1;
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
            // Mismo criterio que move_sprites_to_group: quitarlo sólo de
            // los demás grupos, que reasignar la selección a su propio
            // grupo no la borre.
            let mut ya_estaba = false;
            for (i, g) in self.config.folder_groups.iter_mut().enumerate() {
                if i == group_index {
                    ya_estaba = g.sprites.iter().any(|s| s == &id);
                } else {
                    g.sprites.retain(|s| s != &id);
                }
            }
            if ya_estaba {
                continue;
            }
            if let Some(g) = self.config.folder_groups.get_mut(group_index) {
                g.sprites.push(id.clone());
                moved += 1;
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

    /// Quita un sprite —o una carpeta entera— desde el árbol del panel.
    ///
    /// Un paso de deshacer por gesto, y sólo si el gesto quita algo
    /// (re-review B4): el árbol borraba sin anotar nada, de modo que
    /// `Ctrl+Z` aplicaba el paso anterior —el del gesto de antes—, que es
    /// un deshacer peor que no tener.
    pub(super) fn remove_from_tree(&mut self, path: &Path) {
        self.anotar_deshacer();
        let removed = self.remove_path(path);
        if removed == 0 {
            // Carpeta sin imágenes: el paso en blanco no debe consumir el
            // primer `Ctrl+Z`.
            self.desanotar_deshacer();
        }
        self.selected_paths.remove(path);
        self.aviso(LogKind::Info, t!("{} sprite(s) quitado(s).", removed));
        self.after_workspace_change();
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
        self.anotar_deshacer();
        self.config
            .extra_inputs
            .retain(|p| tp_core::ingest::normalize_path(p) != norm);
        if self.config.extra_inputs.len() == before {
            // No estaba: el paso no debe quedarse en la pila consumiendo el
            // primer Ctrl+Z (re-review B4).
            self.desanotar_deshacer();
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
        self.anotar_deshacer();
        self.config.excluded_inputs.clear();
        self.aviso(LogKind::Info, t!("{} sprite(s) restaurado(s).", count));
        self.change_seq += 1;
        self.request_preview(true);
    }

    pub(super) fn hidden_count(&self) -> usize {
        self.config.excluded_inputs.len()
    }

    /// Borra un grupo del proyecto: sus sprites vuelven a la hoja principal
    /// por el reparto de `run_groups`.
    pub(super) fn delete_group(&mut self, index: usize) {
        let Some(g) = self.config.folder_groups.get(index) else {
            return;
        };
        let name = g.name.clone();
        self.anotar_deshacer();
        self.config.folder_groups.remove(index);
        self.aviso(LogKind::Info, t!("Grupo «{}» eliminado.", name));
        self.after_workspace_change();
    }

    /// Vacía un grupo: sus sprites vuelven a la hoja principal.
    ///
    /// Era el único gesto destructivo de la app que no dejaba rastro
    /// alguno —ni aviso ni paso de deshacer— (re-review B3/B4). Ahora
    /// avisa con el nombre del grupo y con cuántos sprites devuelve, y
    /// `Ctrl+Z` recupera la asignación.
    pub(super) fn empty_group(&mut self, index: usize) {
        let Some(g) = self.config.folder_groups.get_mut(index) else {
            return;
        };
        if g.name.is_empty() || g.sprites.is_empty() {
            return;
        }
        let name = g.name.clone();
        let cuantos = g.sprites.len();
        self.anotar_deshacer();
        self.config.folder_groups[index].sprites.clear();
        self.aviso(
            LogKind::Info,
            t!(
                "Grupo «{}» vaciado: {} sprite(s) devuelto(s) a la hoja principal.",
                name,
                cuantos
            ),
        );
        self.after_workspace_change();
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

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;
    use std::time::Duration;

    fn app() -> App {
        let ctx = egui::Context::default();
        App::new_for_testing(ctx.clone(), None)
    }

    /// Soltar sprites sobre su propia hoja no los borra: `retain` los quita
    /// de todas las hojas y, sin el guardia, no se volvían a añadir, con lo
    /// que desaparecían del atlas.
    #[test]
    fn mover_a_su_propio_grupo_no_lo_borra() {
        let mut app = app();
        app.config.folder_groups = vec![
            tp_core::config::FolderGroup {
                name: "ui".into(),
                sprites: vec!["a".into(), "b".into()],
            },
            tp_core::config::FolderGroup {
                name: "fx".into(),
                sprites: vec!["c".into()],
            },
        ];

        let moved = app.move_sprites_to_group(&["a".into()], 0);

        assert_eq!(moved, 0, "mover a su propio grupo no mueve nada");
        assert_eq!(
            app.config.folder_groups[0].sprites,
            vec!["a".to_string(), "b".to_string()],
            "el sprite sigue en su hoja"
        );
        assert_eq!(
            app.config.folder_groups[1].sprites,
            vec!["c".to_string()],
            "las demás hojas no se tocan"
        );
    }

    /// Mismo guardia en el camino del menú («↪ Selección»): reasignar la
    /// selección a su propio grupo es un no-op, no un borrado.
    #[test]
    fn reasignar_la_seleccion_a_su_propio_grupo_no_lo_borra() {
        let tmp = std::env::temp_dir().join(format!(
            "tp_yathere_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sprites = tmp.join("sprites");
        std::fs::create_dir_all(&sprites).unwrap();
        let project = crate::testing::create_example_project(&tmp, &sprites).expect("proyecto");
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), Some(project));
        // Necesita el atlas para resolver ruta → id, como en la app real.
        let _ = crate::testing::pump_on_demand(
            &mut app,
            &ctx,
            |a| a.result().is_some(),
            Duration::from_secs(15),
        );
        let sprite = app
            .result()
            .expect("la preview de arranque debe dar atlas")
            .result
            .sprites
            .first()
            .cloned()
            .expect("al menos un sprite");
        app.config.folder_groups = vec![
            tp_core::config::FolderGroup {
                name: "ui".into(),
                sprites: vec![sprite.id.clone()],
            },
            tp_core::config::FolderGroup {
                name: "fx".into(),
                sprites: vec![],
            },
        ];
        app.selected_paths = std::iter::once(PathBuf::from(&sprite.source_path)).collect();

        let moved = app.assign_selected_to_group(0);

        assert_eq!(moved, 0, "reasignar a su propio grupo no mueve nada");
        assert_eq!(
            app.config.folder_groups[0].sprites,
            vec![sprite.id],
            "el sprite sigue en su hoja"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }
}
