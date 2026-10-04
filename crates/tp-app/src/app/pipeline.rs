//! Preview/publish engine: debounce, workspace snapshot, autowatch and the
//! repaint scheduling that keeps the atlas fresh without publishing.

use super::{
    App, LogKind, PreviewState, RunMessage, WorkspaceSnapshot, JUST_ADDED_HL, PREVIEW_DEBOUNCE_MS,
    SNAPSHOT_POLL_MS,
};
use crate::i18n::t;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use tp_core::pipeline::PipelineOutput;

// Los tests de repaint bombean la app con los mismos atajos que la UI.
#[cfg(test)]
use super::drag::begin_sprite_drag;
#[cfg(test)]
use tp_core::config::ProjectConfig;

impl App {
    /// Request a (debounced) in-memory repack so the workspace reacts
    /// immediately to added/removed sprites or changed settings. Files are
    /// only written by the explicit «Publicar» action.
    pub(super) fn request_preview(&mut self, debounce: bool) {
        // Un trabajo pendiente de exportación tiene prioridad. El marcador se
        // retira: si queda puesto, `auto_repaint` pediría frames para un cambio
        // que este camino ya no va a programar (lo retoma el sondeo del
        // snapshot al terminar la exportación).
        if self.running.is_some() {
            self.pending_seq = None;
            return;
        }
        if self.pending.is_some() {
            // Ya hay una vista previa en marcha: reprogramar en vez de
            // saturar de trabajos (el snapshot se recalculará al terminar).
            self.pending_seq = Some((self.change_seq, std::time::Instant::now()));
            return;
        }
        // Workspace vacío: vaciar también la vista (sin trabajo ni errores).
        if self.config.input_directory.as_os_str().is_empty() && self.config.extra_inputs.is_empty()
        {
            if self.result.is_some() {
                self.result = None;
            }
            self.pending_seq = None;
            return;
        }
        let seq = self.change_seq;
        if debounce {
            match self.pending_seq {
                Some((s, at)) if s == seq => {
                    if at.elapsed() < std::time::Duration::from_millis(PREVIEW_DEBOUNCE_MS) {
                        return;
                    }
                }
                _ => {
                    self.pending_seq = Some((seq, std::time::Instant::now()));
                    return;
                }
            }
        }
        // La bandera se consume al pasar el debounce: los cambios de la UI
        // evitan el re-escaneo del disco aunque el slider tarde en parar.
        let force = std::mem::take(&mut self.pending_force);
        if !force && !self.snapshot_changed() {
            // Nada que reempaquetar: retire el marcador para que
            // `auto_repaint` deje de programar frames.
            self.pending_seq = None;
            return;
        }
        self.pending_seq = None;
        self.commit_paths();
        self.parse_variants();
        let snapshot = self.workspace_snapshot();
        let (tx, rx) = std::sync::mpsc::channel();
        let cfg = self.config.clone();
        let started = std::time::Instant::now();
        let grouped = self.groups_active();
        std::thread::spawn(move || {
            let result = if grouped {
                tp_core::pipeline::run_grouped_preview(&cfg)
            } else {
                tp_core::pipeline::run_preview(&cfg)
            };
            let _ = tx.send(RunMessage {
                elapsed_ms: started.elapsed().as_millis(),
                result,
            });
        });
        self.pending = Some(rx);
        self.packed_snapshot = Some(snapshot);
    }

    /// ¿Hay grupos con nombre y algún sprite asignado? En ese caso el pack
    /// se divide por carpetas de salida y la vista fusiona todas las hojas.
    pub fn groups_active(&self) -> bool {
        self.config
            .folder_groups
            .iter()
            .any(|g| !g.name.is_empty() && !g.sprites.is_empty())
    }

    /// Estado de la vista previa para el indicador de la barra de zoom:
    /// reempaquetando, actualizando, desactualizada o al día.
    pub(crate) fn preview_state(&self) -> PreviewState {
        if self.running.is_some() {
            PreviewState::Publishing
        } else if self.pending.is_some() {
            PreviewState::Updating
        } else if self.preview_stale {
            PreviewState::Stale
        } else {
            PreviewState::Ok
        }
    }

    /// Map a source path to its ingested sprite id via the current result.
    pub(super) fn sprite_id_for_path(&self, path: &Path) -> Option<String> {
        self.result
            .as_ref()?
            .result
            .sprites
            .iter()
            .find(|s| Path::new(&s.source_path) == path || s.source_path == path.to_string_lossy())
            .map(|s| s.id.clone())
    }

    /// Poll the pending preview job and apply its result.
    pub(super) fn poll_pending(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.pending else { return };
        match rx.try_recv() {
            Ok(msg) => {
                self.pending = None;
                match msg.result {
                    Ok(out) => {
                        self.apply_output(ctx, out, false);
                        self.log(
                            LogKind::Info,
                            t!("Vista previa actualizada en {} ms.", msg.elapsed_ms),
                        );
                        // El resultado acabamos de aplicarlo: el indicador de
                        // frescura solo sigue en «Desactualizado» si el disco
                        // cambió mientras empaquetaba (y en ese caso hay que
                        // reprogramar; si no, la app se quedaría quieta con la
                        // bandera mintiendo hasta la próxima interacción).
                        self.preview_stale = self.snapshot_changed();
                        if self.preview_stale {
                            self.request_preview(false);
                        }
                    }
                    Err(e) => {
                        self.log(
                            LogKind::Error,
                            t!("Vista previa: {}", crate::i18n::tr(&e.to_string())),
                        );
                        // El atlas en pantalla ya no refleja el workspace.
                        self.preview_stale = true;
                        // Un único reintento: con autowatch el fichero pudo
                        // leerse a medio escribir; si vuelve a fallar, se
                        // espera una acción o un cambio nuevo en disco.
                        if !self.preview_retry_used {
                            self.preview_retry_used = true;
                            self.packed_snapshot = None;
                        }
                    }
                }
            }
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                self.log(
                    LogKind::Error,
                    t!("El hilo de vista previa terminó inesperadamente.").into(),
                );
            }
        }
    }

    /// Snapshot of the workspace inputs: sprite set, every packing setting
    /// and the mtime of every sprite file on disk (autowatch).
    pub(super) fn workspace_snapshot(&self) -> WorkspaceSnapshot {
        let mut sprites: Vec<String> = self
            .config
            .extra_inputs
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        sprites.push(self.config.input_directory.display().to_string());
        sprites.extend(
            self.config
                .excluded_inputs
                .iter()
                .map(|p| p.display().to_string()),
        );
        sprites.sort();
        WorkspaceSnapshot {
            sprites,
            config_toml: self.config.to_toml().unwrap_or_default(),
            files: self.collect_input_files(),
        }
    }

    /// Every sprite file on disk with its mtime (ms) and size, sorted:
    /// detects sprites edited, added or removed on the input directories.
    pub(super) fn collect_input_files(&self) -> Vec<(String, u128, u64)> {
        let mut files: Vec<PathBuf> = Vec::new();
        if !self.config.input_directory.as_os_str().is_empty() {
            collect_images(&self.config.input_directory, &mut files);
        }
        for p in &self.config.extra_inputs {
            if p.is_dir() {
                collect_images(p, &mut files);
            } else if p.is_file() {
                files.push(p.clone());
            }
        }
        files.sort();
        files.dedup();
        files
            .into_iter()
            .map(|p| {
                let meta = std::fs::metadata(&p).ok();
                let mtime = meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                (p.display().to_string(), mtime, size)
            })
            .collect()
    }

    /// Watch the input directory and every smart folder (recursive) so sprite
    /// edits on disk refresh the preview automatically. A dedicated thread
    /// wakes the UI on every event (even with the window idle); the UI side
    /// detects the real change via the mtime snapshot in `poll_changes`.
    pub(super) fn start_watcher(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |res| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        }) {
            Ok(w) => w,
            Err(_) => {
                self.watcher = None;
                return;
            }
        };
        use notify::Watcher;
        let mut roots: Vec<PathBuf> = Vec::new();
        if !self.config.input_directory.as_os_str().is_empty() {
            roots.push(self.config.input_directory.clone());
        }
        for p in &self.config.extra_inputs {
            if p.is_dir() {
                roots.push(p.clone());
            } else if let Some(parent) = p.parent() {
                if !parent.as_os_str().is_empty() {
                    roots.push(parent.to_path_buf());
                }
            }
        }
        roots.sort();
        roots.dedup();
        for root in roots {
            if root.exists() {
                let _ = watcher.watch(&root, notify::RecursiveMode::Recursive);
            }
        }
        self.watcher = Some(watcher); // al soltarlo muere el hilo anterior

        // Hilo que despierta la UI con cada evento del disco. El hilo es el
        // único consumidor del canal: drena hasta 500 ms de calma y devuelve
        // el control; el cambio real lo detecta el snapshot de mtimes.
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            while let Ok(_event) = rx.recv() {
                ctx.request_repaint();
                loop {
                    match rx.recv_timeout(std::time::Duration::from_millis(500)) {
                        Ok(_) => ctx.request_repaint(),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
            }
        });
    }

    pub(super) fn snapshot_changed(&self) -> bool {
        match &self.packed_snapshot {
            None => true,
            Some(old) => {
                let new = self.workspace_snapshot();
                new.sprites != old.sprites
                    || new.config_toml != old.config_toml
                    || new.files != old.files
            }
        }
    }

    /// Apply a pipeline result: refresh textures and keep the selection
    /// stable. Pivots and 9-patch borders already come from the project
    /// (`pivot_overrides` / `border_overrides`), so nothing is reapplied here.
    pub(super) fn apply_output(
        &mut self,
        ctx: &egui::Context,
        out: PipelineOutput,
        files_written: bool,
    ) {
        self.selected_page = self.selected_page.min(out.pages.len().saturating_sub(1));
        self.files_written = files_written;
        self.result = Some(out);
        self.preview_retry_used = false;
        self.rebuild_textures(ctx);
    }

    /// Keep the UI painting while a preview job or an unmet change is
    /// outstanding. The change marker must outlive the debounce window: the
    /// job is spawned by the snapshot poll (SNAPSHOT_POLL_MS), which needs
    /// frames to run, so releasing the marker at PREVIEW_DEBOUNCE_MS would
    /// freeze the atlas until the next user event («solo se ve al pulsar
    /// Publicar»).
    pub(super) fn auto_repaint(&self, ctx: &egui::Context) {
        let needs = self.pending.is_some()
            || self
                .pending_seq
                .map(|(s, _)| s == self.change_seq)
                .unwrap_or(false);
        if needs {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    /// Workspace changed by user action: bump the sequence and repack.
    pub(super) fn after_workspace_change(&mut self) {
        self.change_seq += 1;
        self.request_preview(true);
    }

    /// Input fields of the Settings panel changed → commit and repack.
    pub(super) fn on_paths_edited(&mut self) {
        self.commit_paths();
        self.after_workspace_change();
    }

    /// Workspace changed via the settings panel: commit and repack at once,
    /// without waiting for the passive snapshot poll (ni re-escanear el
    /// disco: el propio widget ya avisó de que cambió). `request_preview`
    /// coalesces los arrastres de sliders (un cambio por frame).
    ///
    /// Los controles de Ajustes no están obligados a llamarla: el sondeo de
    /// [`Self::poll_changes`] detecta por huella cualquier `config` mutado
    /// sin avisar. Avisar sigue siendo mejor (notifica en el mismo frame).
    pub fn on_config_changed(&mut self) {
        self.pending_force = true;
        self.commit_paths();
        self.after_workspace_change();
    }

    /// Per-frame detection of passive edits: directory fields, variants,
    /// config changes and on-disk sprite edits refresh the preview.
    pub(super) fn poll_changes(&mut self, ctx: &egui::Context) {
        // Red de seguridad para el panel de Ajustes: si un control muta
        // `config` sin avisar con `on_config_changed`, la huella del último
        // sondeo no cuadra y se notifica aquí (frame siguiente). Coste: una
        // serialización de `config` por frame.
        let actual = format!("{:?}", self.config);
        if self.config_fingerprint.as_ref() != Some(&actual) {
            if self.config_fingerprint.is_some() {
                // `commit_paths` (dentro de la notificación) repone la huella.
                self.on_config_changed();
            } else {
                // Primer frame: solo tomar muestra, sin notificar.
                self.config_fingerprint = Some(actual);
            }
        }
        if self.input_dir_text.trim() != self.config.input_directory.display().to_string()
            || self.output_dir_text.trim() != self.config.output_directory.display().to_string()
        {
            self.on_paths_edited();
        }
        let variants_joined = self
            .config
            .scale_variants
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        if self.variants_text.trim() != variants_joined && self.parse_variants() {
            self.after_workspace_change();
        }
        // El snapshot (mtimes incluidos) se recalcula como mucho cada
        // SNAPSHOT_POLL_MS: los cambios de ajustes llegan por
        // `on_config_changed` o por la huella de `config` de arriba, así que
        // este sondeo solo vigila los ficheros en disco (autowatch).
        if self.last_snapshot_poll.elapsed() >= std::time::Duration::from_millis(SNAPSHOT_POLL_MS) {
            self.last_snapshot_poll = std::time::Instant::now();
            self.preview_stale = self.snapshot_changed();
            if self.preview_stale {
                self.request_preview(true);
            }
        }
        if self
            .just_added_at
            .is_some_and(|at| at.elapsed() >= JUST_ADDED_HL)
        {
            self.just_added.clear();
            self.just_added_at = None;
            ctx.request_repaint();
        }
        self.auto_repaint(ctx);
    }

    pub(super) fn parse_variants(&mut self) -> bool {
        let parsed: Vec<f32> = self
            .variants_text
            .split([',', ';', ' '])
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .filter(|v| *v > 0.0 && *v <= 8.0)
            .collect();
        // `true` solo si cambia algo real: texto aún canónico distinto
        // («1.0» vs «1») o inválido mientras se escribe no debe bumpear la
        // secuencia, o `poll_changes` reprogramaría el preview sin fin.
        if parsed.is_empty() || parsed == self.config.scale_variants {
            false
        } else {
            self.config.scale_variants = parsed;
            // Las opciones de escalas que ya no existen se descartan para
            // que no queden filas huérfanas en el proyecto.
            self.config.variant_options.retain(|o| {
                self.config
                    .scale_variants
                    .iter()
                    .any(|s| (s - o.scale).abs() < 1e-6)
            });
            // Escribe en `config`: repón la huella para que el sondeo por
            // frame no vuelva a notificar este mismo cambio.
            self.refresh_config_fingerprint();
            true
        }
    }

    /// Publica la hoja.
    ///
    /// `force` reescribe los ficheros aunque nada haya cambiado desde la
    /// última publicación (la entrada «Forzar publicación» del menú del
    /// original); sin ella, una hoja ya al día se deja como está.
    pub(super) fn start_pack(&mut self, force: bool) {
        self.commit_paths();
        self.parse_variants();
        if let Err(e) = self.config.validate() {
            self.log(LogKind::Error, e.to_string());
            return;
        }
        if self.config.input_directory.as_os_str().is_empty() && self.config.extra_inputs.is_empty()
        {
            self.log(
                LogKind::Error,
                t!("Selecciona un directorio de entrada o añade sprites.").into(),
            );
            return;
        }
        if !force && self.files_written && !self.snapshot_changed() {
            self.log(
                LogKind::Info,
                t!("Nada ha cambiado desde la última publicación; usa «Forzar publicación» para reescribir los ficheros.")
                    .into(),
            );
            return;
        }
        // Cancel any pending preview: the export replaces it.
        self.pending = None;
        self.pending_seq = None;
        self.preview_retry_used = false;
        let snapshot = self.workspace_snapshot();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut cfg = self.config.clone();
        // La fuerza es un ajuste de ejecución: no se persiste en el .tpproj
        // (por eso no toca `self.config`, que es lo que firma el snapshot).
        cfg.force_publish = force;
        let grouped = self.groups_active();
        std::thread::spawn(move || {
            let result = if grouped {
                tp_core::pipeline::run_grouped(&cfg)
            } else {
                tp_core::pipeline::run(&cfg)
            };
            let _ = tx.send(RunMessage {
                elapsed_ms: 0,
                result,
            });
        });
        self.running = Some(rx);
        self.packed_snapshot = Some(snapshot);
        self.change_seq += 1;
        let origin = if self.config.input_directory.as_os_str().is_empty() {
            t!("sprites añadidos").to_string()
        } else {
            self.config.input_directory.display().to_string()
        };
        self.log(LogKind::Info, t!("Publicando desde {} ...", origin));
    }

    pub(super) fn rebuild_textures(&mut self, ctx: &egui::Context) {
        self.textures.clear();
        let Some(out) = &self.result else {
            return;
        };
        for page in &out.pages {
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [page.width as usize, page.height as usize],
                &page.pixels,
            );
            let name = format!("page_{}", page.index);
            self.textures
                .push(ctx.load_texture(name, img, egui::TextureOptions::NEAREST));
        }
    }

    pub(super) fn handle_run_result(&mut self, ctx: &egui::Context, msg: RunMessage) {
        self.running = None;
        match msg.result {
            Ok(out) => {
                let total = out.result.total_sprites;
                let aliases = out.result.alias_count;
                let pages = out.pages.len();
                self.log(
                    LogKind::Info,
                    t!(
                        "Publicación completa: {} sprites ({} aliases), {} página(s).",
                        total,
                        aliases,
                        pages
                    ),
                );
                for w in &out.result.warnings {
                    self.log(LogKind::Warning, w.clone());
                }
                for (stage, ms) in &out.result.stage_times_ms {
                    self.log(LogKind::Info, t!("  [{}] {} ms", stage, ms));
                }
                for f in &out.result.output_files {
                    self.log(LogKind::Info, t!("  ➡ {}", f));
                }
                self.apply_output(ctx, out, true);
                self.fit_zoom();
            }
            Err(e) => {
                self.log(
                    LogKind::Error,
                    t!("Empaquetado fallido: {}", crate::i18n::tr(&e.to_string())),
                );
            }
        }
    }

    pub(super) fn fit_zoom(&mut self) {
        let Some(out) = &self.result else {
            return;
        };
        let Some(page) = out.pages.get(self.selected_page) else {
            return;
        };
        let avail = self.preview_size - egui::vec2(32.0, 32.0);
        if page.width <= 0 || page.height <= 0 || avail.x <= 0.0 || avail.y <= 0.0 {
            return;
        }
        self.zoom = (avail.x / page.width as f32)
            .min(avail.y / page.height as f32)
            .clamp(0.05, 16.0);
    }
}

pub(super) fn collect_images(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_images(&path, out);
        } else if tp_core::ingest::is_image_file(&path) {
            out.push(path);
        }
    }
}

/// La ventana real solo pinta cuando egui lo pide; los tests que bombean
/// frames a mano enmascaran fallos de programación del preview. Estas
/// pruebas avanzan la app **bajo demanda**: si la app deja de pedir
/// repaints, la UI queda congelada («solo se actualiza al pulsar Publicar»).
#[cfg(test)]
mod on_demand_tests {
    use super::*;
    use crate::testing::{create_example_project, pump_on_demand};
    use std::time::Duration;

    const TIMEOUT: Duration = Duration::from_secs(15);

    type RepaintSlot = std::sync::Arc<std::sync::Mutex<Option<Duration>>>;

    fn demo(tag: &str) -> (App, egui::Context, PathBuf) {
        let tmp = std::env::temp_dir().join(format!(
            "tp_ondemand_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sprites = tmp.join("sprites");
        std::fs::create_dir_all(&sprites).unwrap();
        let project = create_example_project(&tmp, &sprites).expect("proyecto de ejemplo");
        let ctx = egui::Context::default();
        let app = App::new_for_testing(ctx.clone(), Some(project));
        (app, ctx, tmp)
    }

    fn preview_updates(app: &App) -> usize {
        app.log_texts()
            .iter()
            .filter(|t| t.starts_with("I Vista previa actualizada"))
            .count()
    }

    /// Conduce hasta que el preview vuelve a estar al día (sin Publicar).
    fn pump_until_fresh(app: &mut App, ctx: &egui::Context) -> crate::testing::PumpOutcome {
        pump_on_demand(
            app,
            ctx,
            |a| {
                a.pending.is_none()
                    && a.running.is_none()
                    && !a.snapshot_changed()
                    && !a.preview_stale
            },
            TIMEOUT,
        )
    }

    #[test]
    fn preview_refreshes_after_settings_change_without_publishing() {
        let (mut app, ctx, tmp) = demo("settings");
        let boot = pump_on_demand(&mut app, &ctx, |a| a.result().is_some(), TIMEOUT);
        assert!(
            app.result().is_some(),
            "el preview de arranque debe llegar solo: {boot:?}"
        );
        let before = preview_updates(&app);

        // Mismo camino que los widgets del panel de Ajustes.
        app.config.padding += 4;
        app.on_config_changed();

        let out = pump_until_fresh(&mut app, &ctx);
        assert!(
            !out.stuck,
            "la app dejó de pedir repaints tras cambiar un ajuste \
             (frames={}, idle={}): la UI solo se actualizaría con Publicar",
            out.frames, out.idle_frames
        );
        assert!(
            preview_updates(&app) > before,
            "el preview debe reempaquetar tras cambiar un ajuste"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Un control que muta `config` **sin avisar** debe seguir llegando al
    /// preview: es la garantía que da `config_fingerprint` y la razón por la
    /// que el panel de Ajustes ya no repite `on_config_changed` en cada
    /// widget. El aviso sale en el propio frame siguiente.
    #[test]
    fn preview_refreshes_when_a_setting_changes_without_notifying() {
        let (mut app, ctx, tmp) = demo("settings-silent");
        let boot = pump_on_demand(&mut app, &ctx, |a| a.result().is_some(), TIMEOUT);
        assert!(app.result().is_some(), "preview de arranque: {boot:?}");
        let before = preview_updates(&app);

        let seq = app.change_seq;
        app.config.padding += 4;
        app.run_frame(&ctx, crate::testing::idle_input());
        assert!(
            app.change_seq > seq,
            "el sondeo por frame debe notificar el cambio silencioso (seq {seq} -> {})",
            app.change_seq
        );

        let out = pump_until_fresh(&mut app, &ctx);
        assert!(
            !out.stuck,
            "la app dejó de pedir repaints tras un cambio sin avisar \
             (frames={}, idle={})",
            out.frames, out.idle_frames
        );
        assert!(
            preview_updates(&app) > before,
            "el preview debe reempaquetar aunque el control no haya avisado"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn preview_refreshes_after_adding_a_sprite_without_publishing() {
        let (mut app, ctx, tmp) = demo("add");
        let boot = pump_on_demand(&mut app, &ctx, |a| a.result().is_some(), TIMEOUT);
        assert!(app.result().is_some(), "preview de arranque: {boot:?}");
        let before = preview_updates(&app);
        let total_before = app.result().unwrap().result.total_sprites;

        let extra = tmp.join("sprites").join("extra.png");
        let img = image::RgbaImage::from_pixel(20, 20, image::Rgba([10, 200, 30, 255]));
        img.save(&extra).unwrap();
        assert!(app.add_input(extra), "el sprite debe añadirse al workspace");

        let out = pump_until_fresh(&mut app, &ctx);
        assert!(
            !out.stuck,
            "la app dejó de pedir repaints tras añadir un sprite \
             (frames={}, idle={})",
            out.frames, out.idle_frames
        );
        assert!(
            preview_updates(&app) > before,
            "el preview debe recalcularse al añadir un sprite"
        );
        let total_after = app.result().unwrap().result.total_sprites;
        assert!(
            total_after > total_before,
            "el nuevo sprite debe aparecer en la vista ({total_before} -> {total_after})"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn canvas_drop_repaints_without_publishing() {
        let (mut app, ctx, tmp) = demo("drop");
        let boot = pump_on_demand(&mut app, &ctx, |a| a.result().is_some(), TIMEOUT);
        assert!(app.result().is_some(), "preview de arranque: {boot:?}");
        let before = preview_updates(&app);

        let ids: Vec<String> = app
            .result()
            .unwrap()
            .result
            .sprites
            .iter()
            .filter(|s| !s.is_alias)
            .map(|s| s.id.clone())
            .take(1)
            .collect();
        begin_sprite_drag(&app, &ctx, ids.clone());
        let placed = app.drop_sprites_on_canvas(egui::pos2(120.0, 80.0));
        assert_eq!(placed, 1, "el drop debe colocar el sprite");

        let out = pump_until_fresh(&mut app, &ctx);
        assert!(
            !out.stuck,
            "la app dejó de pedir repaints tras soltar en el lienzo \
             (frames={}, idle={}): el drop no se vería hasta Publicar",
            out.frames, out.idle_frames
        );
        assert!(
            preview_updates(&app) > before,
            "el drop debe reempaquetar al instante"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Registra los `request_repaint` del `ctx` en un slot compartido.
    fn record_repaints(ctx: &egui::Context) -> RepaintSlot {
        let slot: RepaintSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
        let record = slot.clone();
        ctx.set_request_repaint_callback(move |info| {
            let mut g = record.lock().unwrap();
            *g = Some(match *g {
                Some(prev) => prev.min(info.delay),
                None => info.delay,
            });
        });
        slot
    }

    /// Apaga el watcher de disco y corre frames hasta que dos seguidos no piden
    /// repaint, dejando el slot registrado y a `None`. Así el único repaint que
    /// se pueda ver en el frame siguiente es el que pida ese frame: sin esta
    /// disciplina los eventos del watcher (incluso los `Open` que genera el
    /// propio sondeo de mtimes) enmascararían cualquier regresión.
    fn settle_until_quiet(app: &mut App, ctx: &egui::Context) -> RepaintSlot {
        app.watcher = None;
        std::thread::sleep(Duration::from_millis(600));
        let slot = record_repaints(ctx);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut quiet = 0;
        while std::time::Instant::now() < deadline {
            let _ = app.run_frame(ctx, crate::testing::idle_input());
            let mut g = slot.lock().unwrap();
            if g.is_none() {
                quiet += 1;
                if quiet >= 2 {
                    drop(g);
                    return slot;
                }
            } else {
                quiet = 0;
                *g = None;
            }
        }
        panic!("la app nunca deja de pedir repaints: no se puede aislar el frame");
    }

    /// Mientras el SO tiene ficheros sobre la ventana, el overlay solo se
    /// pinta si cada frame programa el siguiente.
    #[test]
    fn hover_keeps_painting_while_files_are_over_the_window() {
        let (mut app, ctx, tmp) = demo("hover");
        let boot = pump_until_fresh(&mut app, &ctx);
        assert!(!boot.stuck, "preview de arranque: {boot:?}");

        let slot = settle_until_quiet(&mut app, &ctx);

        let mut input = crate::testing::idle_input();
        input.hovered_files = vec![egui::HoveredFile {
            path: Some(tmp.join("sprites").join("hero.png")),
            mime: String::new(),
        }];
        let _ = app.run_frame(&ctx, input);
        assert!(
            slot.lock().unwrap().is_some(),
            "el frame del hover no programó el siguiente repaint: el overlay \
             «Suelta para añadir al workspace» no llegaría a verse"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Camino real de eframe: el `RawInput` del frame trae los ficheros
    /// soltados por el SO y `handle_global_file_drop` los procesa DENTRO del
    /// frame. `poll_changes` (que programa el repack) corre al inicio del
    /// frame, antes que el drop, así que el frame del drop tiene que pedir él
    /// mismo el siguiente repaint; si no, la ventana real se queda quieta y el
    /// atlas solo cambia al pulsar Publicar.
    #[test]
    fn so_drop_schedules_next_frame_and_repaints_without_publishing() {
        let (mut app, ctx, tmp) = demo("so_drop");
        let boot = pump_until_fresh(&mut app, &ctx);
        assert!(!boot.stuck, "preview de arranque: {boot:?}");
        let before = preview_updates(&app);
        let total_before = app.result().unwrap().result.total_sprites;

        // Fuera de los directorios vigilados: el hilo del watcher repinta con
        // cada evento de disco y enmascararía el repaint del frame del drop.
        let fuera = tmp.join("fuera");
        std::fs::create_dir_all(&fuera).unwrap();
        let extra = fuera.join("soltado_del_so.png");
        image::RgbaImage::from_pixel(18, 18, image::Rgba([200, 40, 90, 255]))
            .save(&extra)
            .unwrap();
        let rechazado = fuera.join("no_es_imagen.txt");
        std::fs::write(&rechazado, "no soy un sprite").unwrap();

        let slot = settle_until_quiet(&mut app, &ctx);

        // Fichero rechazado: no añade nada ni arranca el watcher, así que el
        // único repaint posible en su frame es el que pide el propio drop.
        let mut input = crate::testing::idle_input();
        input.dropped_files = vec![egui::DroppedFile {
            path: Some(rechazado.clone()),
            ..Default::default()
        }];
        let _ = app.run_frame(&ctx, input);
        assert!(
            !app.config.extra_inputs.iter().any(|p| p == &rechazado),
            "un fichero que no es imagen no debe entrar al workspace"
        );
        assert!(
            slot.lock().unwrap().is_some(),
            "el frame del drop no programó el siguiente repaint: la ventana \
             real se quedaría quieta y el atlas solo se vería con Publicar"
        );

        // Drop real: entra en el workspace.
        let mut input = crate::testing::idle_input();
        input.dropped_files = vec![egui::DroppedFile {
            path: Some(extra.clone()),
            ..Default::default()
        }];
        let _ = app.run_frame(&ctx, input);
        assert!(
            app.config.extra_inputs.iter().any(|p| p == &extra),
            "el fichero soltado debe entrar en el workspace"
        );

        // Pasado el debounce, el marcador del cambio sigue vivo: sin eso,
        // `auto_repaint` dejaría de pedir frames antes de que el sondeo del
        // snapshot alcance a lanzar el repack.
        std::thread::sleep(Duration::from_millis(PREVIEW_DEBOUNCE_MS + 80));
        let probe = egui::Context::default();
        let probe_slot = record_repaints(&probe);
        app.auto_repaint(&probe);
        assert!(
            probe_slot.lock().unwrap().is_some(),
            "pasado el debounce, con el cambio aún pendiente, auto_repaint \
             dejó de programar frames: el repack no llegaría a lanzarse"
        );

        let out = pump_until_fresh(&mut app, &ctx);
        assert!(
            !out.stuck,
            "la app dejó de pedir repaints tras soltar ficheros del SO \
             (frames={}, idle={})",
            out.frames, out.idle_frames
        );
        assert!(
            preview_updates(&app) > before,
            "el preview debe recalcularse solo, sin pulsar Publicar"
        );
        let total_after = app.result().unwrap().result.total_sprites;
        assert!(
            total_after > total_before,
            "el sprite soltado debe aparecer en la vista ({total_before} -> {total_after})"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Soltar un proyecto sobre la ventana lo abre: `.tps` (el del original)
    /// y `.tpproj` (el nuestro). Antes `add_input` los descartaba por no ser
    /// imágenes, así que el gesto no hacía nada. Un `.toml` que no es de
    /// proyecto no debe tocar la configuración cargada.
    #[test]
    fn soltar_un_proyecto_lo_abre() {
        let (mut app, ctx, tmp) = demo("drop_proyecto");
        let boot = pump_until_fresh(&mut app, &ctx);
        assert!(!boot.stuck, "preview de arranque: {boot:?}");

        // Un .tps del original: marcas reconocibles y ruta relativa.
        let cfg = ProjectConfig {
            base_file_name: "soltado".into(),
            input_directory: PathBuf::from("sprites"),
            ..ProjectConfig::default()
        };
        let tps = tmp.join("soltado.tps");
        tp_core::tps::save_tps(&cfg, &tps).expect("escribir el .tps");

        let slot = settle_until_quiet(&mut app, &ctx);
        let mut input = crate::testing::idle_input();
        input.dropped_files = vec![egui::DroppedFile {
            path: Some(tps.clone()),
            ..Default::default()
        }];
        let _ = app.run_frame(&ctx, input);

        assert_eq!(
            app.project_path.as_deref(),
            Some(tps.as_path()),
            "un .tps soltado debe abrirse como proyecto"
        );
        assert_eq!(
            app.config.base_file_name, "soltado",
            "los ajustes del .tps deben cargarse"
        );
        assert_eq!(
            app.config.input_directory,
            tmp.join("sprites"),
            "las rutas relativas se resuelven contra la carpeta del .tps"
        );
        assert!(
            !app.config.extra_inputs.iter().any(|p| p == &tps),
            "un proyecto no debe entrar al workspace como sprite"
        );
        assert!(
            slot.lock().unwrap().is_some(),
            "el frame del drop no programó el siguiente repaint: el proyecto              recién cargado no se vería hasta la siguiente interacción"
        );

        // El TOML propio, mismo camino.
        let cfg2 = ProjectConfig {
            base_file_name: "tpp".into(),
            ..ProjectConfig::default()
        };
        let tpproj = tmp.join("otro.tpproj");
        std::fs::write(&tpproj, cfg2.to_toml().expect("toml")).unwrap();
        let mut input = crate::testing::idle_input();
        input.dropped_files = vec![egui::DroppedFile {
            path: Some(tpproj.clone()),
            ..Default::default()
        }];
        let _ = app.run_frame(&ctx, input);
        assert_eq!(
            app.project_path.as_deref(),
            Some(tpproj.as_path()),
            "un .tpproj soltado debe abrirse como proyecto"
        );
        assert_eq!(app.config.base_file_name, "tpp");

        // Un toml cualquiera no es un proyecto: se ignora.
        let ajeno = tmp.join("ui.toml");
        std::fs::write(&ajeno, "lang = \"es\"\n").unwrap();
        let mut input = crate::testing::idle_input();
        input.dropped_files = vec![egui::DroppedFile {
            path: Some(ajeno.clone()),
            ..Default::default()
        }];
        let _ = app.run_frame(&ctx, input);
        assert_eq!(
            app.project_path.as_deref(),
            Some(tpproj.as_path()),
            "un .toml que no es de proyecto no debe abrirse"
        );
        assert!(
            !app.config.extra_inputs.iter().any(|p| p == &ajeno),
            "un .toml tampoco es un sprite"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// La app arrancada con un `.tps` del original —el mismo camino que usan
    /// el argumento de la línea de comandos y el diálogo de abrir— carga los
    /// ajustes y deja la vista previa al día.
    #[test]
    fn arranca_con_un_tps_del_original() {
        let tmp = std::env::temp_dir().join(format!(
            "tp_ondemand_tps_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sprites = tmp.join("sprites");
        std::fs::create_dir_all(&sprites).unwrap();
        let tpproj = create_example_project(&tmp, &sprites).expect("proyecto de ejemplo");
        let toml = std::fs::read_to_string(&tpproj).expect("leer el .tpproj");
        let cfg = ProjectConfig::from_toml(&toml).expect("config del .tpproj");
        let tps = tmp.join("demo.tps");
        tp_core::tps::save_tps(&cfg, &tps).expect("escribir el .tps");

        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), Some(tps.clone()));
        assert_eq!(
            app.project_path.as_deref(),
            Some(tps.as_path()),
            "la app debe quedar sobre el .tps"
        );
        assert_eq!(
            app.config.input_directory, sprites,
            "los ajustes del .tps deben cargarse"
        );
        let boot = pump_until_fresh(&mut app, &ctx);
        assert!(!boot.stuck, "preview del .tps: {boot:?}");
        let out = app.result().expect("el .tps debe producir vista previa");
        assert!(
            out.result.total_sprites > 0,
            "el .tps apunta a la carpeta de sprites: deben empaquetarse"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Carpeta temporal propia de cada test (se borra al terminar).
    fn carpeta_temporal(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tp_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    /// Ensayo de rendimiento de la app completa, tal como corre la suite
    /// (compilación de debug): arranque con proyecto, frames de UI en
    /// marcha, repack al cambiar un ajuste y publicación a disco. Imprime
    /// la tabla de tiempos y marca techos laxos: solo deben saltar con
    /// regresiones de orden de magnitud, no con el ruido de la máquina.
    #[test]
    fn rendimiento_de_la_app() {
        let tmp = carpeta_temporal("rendimiento");
        let sprites = tmp.join("sprites");
        std::fs::create_dir_all(&sprites).unwrap();
        let project = create_example_project(&tmp, &sprites).expect("demo");
        let ctx = egui::Context::default();

        // 1) Arranque con proyecto + primer preview.
        let t = std::time::Instant::now();
        let mut app = App::new_for_testing(ctx.clone(), Some(project));
        let boot = pump_until_fresh(&mut app, &ctx);
        let arranque_ms = t.elapsed().as_millis();
        assert!(!boot.stuck, "primer preview: {boot:?}");
        let total = app
            .result()
            .expect("el arranque deja atlas")
            .result
            .total_sprites;
        assert!(total > 0, "la demo tiene sprites: {total}");

        // 2) Frames de UI con la app ya caliente (sin trabajo pendiente).
        let n = 30u128;
        let t = std::time::Instant::now();
        for _ in 0..n {
            let _ = app.run_frame(&ctx, crate::testing::idle_input());
        }
        let frame_ms = t.elapsed().as_millis() / n;

        // 3) Repack tras cambiar un ajuste real.
        let antes = preview_updates(&app);
        app.config.padding = 7;
        app.after_workspace_change();
        let t = std::time::Instant::now();
        let out = pump_until_fresh(&mut app, &ctx);
        let repack_ms = t.elapsed().as_millis();
        assert!(!out.stuck, "repack: {out:?}");
        assert!(
            preview_updates(&app) > antes,
            "el cambio de ajuste debe reempaquetar"
        );

        // 4) Publicación real a disco (el hilo va en paralelo: se bombea
        //    hasta que termina, con un tope por si nunca acaba).
        let salida = app.config.output_directory.clone();
        let t = std::time::Instant::now();
        app.start_pack(true);
        let deadline = std::time::Instant::now() + TIMEOUT;
        while app.running.is_some() && std::time::Instant::now() < deadline {
            let _ = app.run_frame(&ctx, crate::testing::idle_input());
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            app.running.is_none(),
            "la publicación no terminó en {TIMEOUT:?}"
        );
        let publicar_ms = t.elapsed().as_millis();
        let hojas: Vec<PathBuf> = std::fs::read_dir(&salida)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("png"))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            !hojas.is_empty(),
            "la publicación debe escribir la hoja en {}",
            salida.display()
        );

        println!(
            "rendimiento ({total} sprites, debug):\n  \
             arranque + primer preview: {arranque_ms} ms\n  \
             frame medio de UI: {frame_ms} ms\n  \
             repack tras cambio de ajuste: {repack_ms} ms\n  \
             publicación a disco: {publicar_ms} ms"
        );
        assert!(
            arranque_ms < 15_000,
            "arranque y primer preview demasiado lentos: {arranque_ms} ms"
        );
        assert!(
            frame_ms < 150,
            "frame de UI medio demasiado lento: {frame_ms} ms"
        );
        assert!(
            repack_ms < 10_000,
            "repack tras cambio de ajuste demasiado lento: {repack_ms} ms"
        );
        assert!(
            publicar_ms < 10_000,
            "publicación a disco demasiado lenta: {publicar_ms} ms"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// El mismo ensayo con un workspace grande (150 sprites de 32×32):
    /// el pipeline debe seguir en el mismo orden de magnitud, no al
    /// cuadrado del número de sprites.
    #[test]
    fn rendimiento_con_workspace_grande() {
        let tmp = carpeta_temporal("rendimiento_grande");
        let sprites = tmp.join("sprites");
        std::fs::create_dir_all(&sprites).unwrap();
        for i in 0..150u32 {
            let mut px = vec![0u8; 32 * 32 * 4];
            for (j, p) in px.chunks_exact_mut(4).enumerate() {
                p.copy_from_slice(&[
                    (i % 251) as u8,
                    ((i * 7 + j as u32) % 241) as u8,
                    (j % 97) as u8,
                    255,
                ]);
            }
            image::RgbaImage::from_raw(32, 32, px)
                .expect("buffer cuadra")
                .save(sprites.join(format!("sprite_{i:03}.png")))
                .expect("guardar sprite");
        }
        let cfg = ProjectConfig {
            input_directory: sprites.clone(),
            output_directory: tmp.join("out"),
            ..ProjectConfig::default()
        };
        let project = tmp.join("grande.tpproj");
        std::fs::write(&project, cfg.to_toml().expect("toml")).expect("escribir");

        let ctx = egui::Context::default();
        let t = std::time::Instant::now();
        let mut app = App::new_for_testing(ctx.clone(), Some(project));
        let boot = pump_until_fresh(&mut app, &ctx);
        let preview_ms = t.elapsed().as_millis();
        assert!(!boot.stuck, "preview grande: {boot:?}");
        let total = app.result().expect("atlas grande").result.total_sprites;
        assert_eq!(total, 150, "los 150 sprites deben empaquetarse");

        let n = 10u128;
        let t = std::time::Instant::now();
        for _ in 0..n {
            let _ = app.run_frame(&ctx, crate::testing::idle_input());
        }
        let frame_ms = t.elapsed().as_millis() / n;

        println!(
            "rendimiento (150 sprites, debug):\n  \
             primer preview: {preview_ms} ms\n  \
             frame medio de UI: {frame_ms} ms"
        );
        assert!(
            preview_ms < 60_000,
            "el preview de 150 sprites tardó demasiado: {preview_ms} ms"
        );
        assert!(
            frame_ms < 150,
            "frame de UI con atlas grande demasiado lento: {frame_ms} ms"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// El binario arranca **sin proyecto cargado**: `variants_text` nace como
    /// `"1.0"` mientras `scale_variants` se serializa como `"1"`. Si la
    /// comparación es textual, `poll_changes` repite `after_workspace_change`
    /// en cada frame, `request_preview` renueva `pending_seq` antes de poder
    /// leer el debounce y el repack **nunca** se lanza (a la vez, `auto_repaint`
    /// mantiene la ventana repintando a 50 fps para siempre).
    #[test]
    fn arranque_sin_proyecto_no_satura_la_secuencia() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        let seq0 = app.change_seq;
        for _ in 0..8 {
            let _ = app.run_frame(&ctx, crate::testing::idle_input());
        }
        assert_eq!(
            app.change_seq, seq0,
            "frames en reposo bumpearon change_seq ({seq0} -> {}): la \
             comparación de variantes es textual y se repite sin fin",
            app.change_seq
        );
        assert!(
            app.variants_text.trim()
                == app
                    .config
                    .scale_variants
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
                || app.variants_text != "1.0",
            "el texto de variantes debe arrancar canónico para no desincronizarse"
        );
    }

    /// Flujo real del usuario: abre el binario sin proyecto, suelta una
    /// imagen del SO y espera a que el espacio de trabajo se pinte solo.
    #[test]
    fn drop_sin_proyecto_pinta_el_atlas_sin_publicar() {
        let tmp = std::env::temp_dir().join(format!(
            "tp_noproject_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sprites = tmp.join("sprites");
        std::fs::create_dir_all(&sprites).unwrap();
        create_example_project(&tmp, &sprites).expect("proyecto de ejemplo");
        let soltado = sprites.join("coin.png");

        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        let before = preview_updates(&app);

        let mut input = crate::testing::idle_input();
        input.dropped_files = vec![egui::DroppedFile {
            path: Some(soltado.clone()),
            ..Default::default()
        }];
        let _ = app.run_frame(&ctx, input);
        assert!(
            app.config.extra_inputs.iter().any(|p| p == &soltado),
            "el fichero soltado debe entrar en el workspace"
        );
        assert!(
            app.has_inputs() && app.result.is_none(),
            "el lienzo debe estar en estado de cálculo (no en el vacío) tras el drop"
        );
        assert!(
            app.is_just_added(&soltado),
            "la ruta soltada debe quedar marcada como recién añadida (feedback)"
        );
        assert!(
            app.pending.is_some(),
            "el drop debe lanzar el empaquetado en el mismo frame, sin esperar \
             al debounce: si no, el lienzo tarda ~300 ms en moverse"
        );

        let out = pump_until_fresh(&mut app, &ctx);
        assert!(
            !out.stuck,
            "el arranque sin proyecto dejó de pedir repaints (frames={}, idle={}): \
             el espacio de trabajo se quedaría en «Aún no hay sprite sheet». Log: {:?}",
            out.frames,
            out.idle_frames,
            app.log_texts()
        );
        assert!(
            preview_updates(&app) > before,
            "la vista previa debe calcularse sola sin proyecto y sin Publicar"
        );
        assert!(
            app.result().is_some(),
            "el lienzo debe dejar de mostrar el estado vacío tras el drop"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }
}
