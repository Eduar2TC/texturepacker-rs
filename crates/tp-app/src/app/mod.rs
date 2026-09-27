//! Main application state and panel orchestration.
//!
//! Layout:
//!
//! - **Tool bar** (`toolbar`): open/save, add/remove sprites, sprite settings, publish
//! - **Sprites panel** (`sprites_panel`, left): folder/sprite tree with drag & drop
//! - **Preview panel** (`preview`, center) + **zoom bar** at its bottom
//! - **Settings panel** (`settings`, right): basic / advanced options
//! - **Bottom panel** (`bottom`): log, output, sprite table and mesh views
//! - **Sprite settings** (`sprite_settings`): pivot editor window

mod animation;
mod bottom;
mod preview;
mod settings;
mod split_sheet;
mod sprite_settings;
mod sprites_panel;
mod toolbar;

use eframe::egui;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use tp_core::config::ProjectConfig;
use tp_core::pipeline::PipelineOutput;
use tp_core::types::Point2D;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BottomTab {
    Log,
    Output,
    Sprites,
    Mesh,
}

pub(crate) enum LogKind {
    Info,
    Warning,
    Error,
}

pub(crate) struct LogEntry {
    pub kind: LogKind,
    pub text: String,
}

struct RunMessage {
    elapsed_ms: u128,
    result: Result<PipelineOutput, tp_core::TpError>,
}

/// Snapshot of the workspace inputs (sprite set + packing settings) used to
/// decide whether the in-memory preview is stale.
#[derive(PartialEq)]
struct WorkspaceSnapshot {
    sprites: Vec<String>,
    config_toml: String,
    /// `(ruta, mtime_ms, tamaño)` de cada sprite en disco (autowatch).
    files: Vec<(String, u128, u64)>,
}

pub struct App {
    config: ProjectConfig,
    input_dir_text: String,
    output_dir_text: String,
    variants_text: String,
    result: Option<PipelineOutput>,
    textures: Vec<egui::TextureHandle>,
    running: Option<Receiver<RunMessage>>,
    selected_page: usize,
    zoom: f32,
    show_outlines: bool,
    show_pivots: bool,
    show_borders: bool,
    preview_size: egui::Vec2,
    selected_paths: BTreeSet<PathBuf>,
    selected_sprite: Option<String>,
    advanced_settings: bool,
    show_sprite_settings: bool,
    /// Whether the animation preview window is open.
    show_animation: bool,
    /// Playback state of the animation preview window.
    anim: animation::AnimState,
    /// Whether the split-sheet window is open.
    show_split: bool,
    /// Session state of the split-sheet window.
    split: split_sheet::SplitState,
    bottom_tab: BottomTab,
    logs: Vec<LogEntry>,
    project_path: Option<PathBuf>,
    /// Text filter applied to the sprites tree.
    tree_filter: String,
    /// Whether the tree filter has keyboard focus (blocks the Delete key).
    tree_filter_focused: bool,
    /// `Some(true)` opens every folder, `Some(false)` closes them (one frame).
    tree_force_open: Option<bool>,
    /// Pending preview job (dynamic workspace): packed in memory, no files.
    pending: Option<Receiver<RunMessage>>,
    /// Counter used to dedupe pending preview jobs.
    change_seq: u64,
    /// Trigger for the debounce: last observed change + timestamp.
    pending_seq: Option<(u64, std::time::Instant)>,
    /// Snapshot of the inputs of the last packed run (dynamic workspace).
    packed_snapshot: Option<WorkspaceSnapshot>,
    /// Pivots/borders edited in the GUI, reapplied on every repack.
    pivot_edits: HashMap<String, Point2D>,
    border_edits: HashMap<String, [i32; 4]>,
    /// Filesystem watcher (autowatch): edits on disk refresh the preview.
    watcher: Option<notify::RecommendedWatcher>,
    /// Clonable handle to wake the UI from the watcher thread.
    egui_ctx: egui::Context,
    /// Throttle for the (mtime-based) workspace snapshot poll.
    last_snapshot_poll: std::time::Instant,
    /// One automatic preview retry per successful cycle (mid-write reads).
    preview_retry_used: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_project: Option<PathBuf>) -> Self {
        let mut app = Self {
            config: ProjectConfig::default(),
            input_dir_text: String::new(),
            output_dir_text: String::new(),
            variants_text: "1.0".to_string(),
            result: None,
            textures: Vec::new(),
            running: None,
            selected_page: 0,
            zoom: 1.0,
            show_outlines: true,
            show_pivots: true,
            show_borders: true,
            preview_size: egui::Vec2::ZERO,
            selected_paths: BTreeSet::new(),
            selected_sprite: None,
            advanced_settings: false,
            show_sprite_settings: false,
            show_animation: false,
            anim: animation::AnimState::default(),
            show_split: false,
            split: split_sheet::SplitState::default(),
            bottom_tab: BottomTab::Log,
            logs: Vec::new(),
            project_path: None,
            tree_filter: String::new(),
            tree_filter_focused: false,
            tree_force_open: None,
            pending: None,
            change_seq: 0,
            pending_seq: None,
            packed_snapshot: None,
            pivot_edits: HashMap::new(),
            border_edits: HashMap::new(),
            watcher: None,
            egui_ctx: cc.egui_ctx.clone(),
            last_snapshot_poll: std::time::Instant::now(),
            preview_retry_used: false,
        };
        app.log(
            LogKind::Info,
            "Bienvenido a TexturePacker-RS. Añade sprites y pulsa «Publicar».".into(),
        );
        app.sync_paths();
        app.start_watcher();
        if let Some(path) = initial_project {
            app.open_project(path);
        }
        app
    }

    fn log(&mut self, kind: LogKind, text: String) {
        self.logs.push(LogEntry { kind, text });
        if self.logs.len() > 2000 {
            self.logs.drain(0..self.logs.len() - 2000);
        }
    }

    fn sync_paths(&mut self) {
        self.input_dir_text = self.config.input_directory.display().to_string();
        self.output_dir_text = self.config.output_directory.display().to_string();
    }

    fn sync_variants(&mut self) {
        self.variants_text = self
            .config
            .scale_variants
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ");
    }

    /// Keep the text fields in sync with the config before running/saving.
    fn commit_paths(&mut self) {
        self.config.input_directory = PathBuf::from(self.input_dir_text.trim());
        self.config.output_directory = PathBuf::from(self.output_dir_text.trim());
    }

    /// Request a (debounced) in-memory repack so the workspace reacts
    /// immediately to added/removed sprites or changed settings. Files are
    /// only written by the explicit «Publicar» action.
    fn request_preview(&mut self, debounce: bool) {
        // Un trabajo pendiente de exportación tiene prioridad.
        if self.running.is_some() {
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
            return;
        }
        let seq = self.change_seq;
        if !self.snapshot_changed() {
            return;
        }
        if debounce {
            match self.pending_seq {
                Some((s, at)) if s == seq => {
                    if at.elapsed() < std::time::Duration::from_millis(400) {
                        return;
                    }
                }
                _ => {
                    self.pending_seq = Some((seq, std::time::Instant::now()));
                    return;
                }
            }
        }
        self.pending_seq = None;
        self.commit_paths();
        self.parse_variants();
        let snapshot = self.workspace_snapshot();
        let (tx, rx) = std::sync::mpsc::channel();
        let cfg = self.config.clone();
        let started = std::time::Instant::now();
        std::thread::spawn(move || {
            let result = tp_core::pipeline::run_preview(&cfg);
            let _ = tx.send(RunMessage {
                elapsed_ms: started.elapsed().as_millis(),
                result,
            });
        });
        self.pending = Some(rx);
        self.packed_snapshot = Some(snapshot);
    }

    /// Poll the pending preview job and apply its result.
    fn poll_pending(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.pending else { return };
        match rx.try_recv() {
            Ok(msg) => {
                self.pending = None;
                match msg.result {
                    Ok(out) => {
                        self.apply_output(ctx, out);
                        self.log(
                            LogKind::Info,
                            format!("Vista previa actualizada en {} ms.", msg.elapsed_ms),
                        );
                        // ¿Cambió algo mientras empaquetaba? Reprogramar.
                        if self.snapshot_changed() {
                            self.request_preview(false);
                        }
                    }
                    Err(e) => {
                        self.log(LogKind::Error, format!("Vista previa: {e}"));
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
                    "El hilo de vista previa terminó inesperadamente.".into(),
                );
            }
        }
    }

    /// Snapshot of the workspace inputs: sprite set, every packing setting
    /// and the mtime of every sprite file on disk (autowatch).
    fn workspace_snapshot(&self) -> WorkspaceSnapshot {
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
    fn collect_input_files(&self) -> Vec<(String, u128, u64)> {
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
    fn start_watcher(&mut self) {
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

    fn snapshot_changed(&self) -> bool {
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

    /// Apply a pipeline result: reapply GUI edits (pivots / 9-patch borders
    /// lost by the repack), refresh textures and keep the selection stable.
    fn apply_output(&mut self, ctx: &egui::Context, mut out: PipelineOutput) {
        for sprite in &mut out.result.sprites {
            if let Some(p) = self.pivot_edits.get(&sprite.id) {
                sprite.pivot = *p;
            }
            if let Some(b) = self.border_edits.get(&sprite.id) {
                sprite.border = Some(*b);
            }
        }
        self.selected_page = self.selected_page.min(out.pages.len().saturating_sub(1));
        self.result = Some(out);
        self.preview_retry_used = false;
        self.rebuild_textures(ctx);
    }

    /// Remember the current pivots/borders as GUI edits before a repack.
    fn snapshot_edits(&mut self) {
        let Some(out) = &self.result else { return };
        for s in &out.result.sprites {
            self.pivot_edits.insert(s.id.clone(), s.pivot);
            if let Some(b) = s.border {
                self.border_edits.insert(s.id.clone(), b);
            }
        }
    }

    /// Repaint soon while there is a pending preview job or debounce.
    fn auto_repaint(&self, ctx: &egui::Context) {
        let needs = self.pending.is_some()
            || self
                .pending_seq
                .map(|(s, at)| {
                    s == self.change_seq && at.elapsed() < std::time::Duration::from_millis(400)
                })
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
    fn on_paths_edited(&mut self) {
        self.commit_paths();
        self.after_workspace_change();
    }

    /// Per-frame detection of passive edits: directory fields, variants,
    /// config changes and on-disk sprite edits refresh the preview.
    fn poll_changes(&mut self, ctx: &egui::Context) {
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
        if self.variants_text.trim() != variants_joined {
            self.parse_variants();
            self.after_workspace_change();
        }
        // El snapshot (mtimes incluidos) se recalcula como mucho cada 250 ms.
        if self.last_snapshot_poll.elapsed() >= std::time::Duration::from_millis(250) {
            self.last_snapshot_poll = std::time::Instant::now();
            if self.snapshot_changed() {
                self.request_preview(true);
            }
        }
        self.auto_repaint(ctx);
    }

    fn parse_variants(&mut self) {
        let parsed: Vec<f32> = self
            .variants_text
            .split([',', ';', ' '])
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .filter(|v| *v > 0.0 && *v <= 8.0)
            .collect();
        if !parsed.is_empty() {
            self.config.scale_variants = parsed;
        }
    }

    fn start_pack(&mut self) {
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
                "Selecciona un directorio de entrada o añade sprites.".into(),
            );
            return;
        }
        // Cancel any pending preview: the export replaces it.
        self.pending = None;
        self.pending_seq = None;
        self.preview_retry_used = false;
        self.snapshot_edits();
        let snapshot = self.workspace_snapshot();
        let (tx, rx) = std::sync::mpsc::channel();
        let cfg = self.config.clone();
        std::thread::spawn(move || {
            let result = tp_core::pipeline::run(&cfg);
            let _ = tx.send(RunMessage {
                elapsed_ms: 0,
                result,
            });
        });
        self.running = Some(rx);
        self.packed_snapshot = Some(snapshot);
        self.change_seq += 1;
        let origin = if self.config.input_directory.as_os_str().is_empty() {
            "sprites añadidos".to_string()
        } else {
            self.config.input_directory.display().to_string()
        };
        self.log(LogKind::Info, format!("Publicando desde {origin} ..."));
    }

    /// Directory that receives the 9-patch/pivot sidecar files (the smart
    /// folder of the first sprite when there is no main input directory).
    fn sidecar_dir(&self) -> Option<PathBuf> {
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

    fn rebuild_textures(&mut self, ctx: &egui::Context) {
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

    fn handle_run_result(&mut self, ctx: &egui::Context, msg: RunMessage) {
        self.running = None;
        match msg.result {
            Ok(out) => {
                let total = out.result.total_sprites;
                let aliases = out.result.alias_count;
                let pages = out.pages.len();
                self.log(
                    LogKind::Info,
                    format!(
                        "Publicación completa: {total} sprites ({aliases} aliases), {pages} página(s)."
                    ),
                );
                for w in &out.result.warnings {
                    self.log(LogKind::Warning, w.clone());
                }
                for (stage, ms) in &out.result.stage_times_ms {
                    self.log(LogKind::Info, format!("  [{stage}] {ms} ms"));
                }
                for f in &out.result.output_files {
                    self.log(LogKind::Info, format!("  → {f}"));
                }
                self.apply_output(ctx, out);
                self.fit_zoom();
            }
            Err(e) => {
                self.log(LogKind::Error, format!("Empaquetado fallido: {e}"));
            }
        }
    }

    /// Add a dropped/picked file or folder to the sprite set.
    /// Returns `true` when the workspace changed and a repack is scheduled.
    fn add_input(&mut self, path: PathBuf) -> bool {
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
        self.config.extra_inputs.push(path);
        self.start_watcher();
        self.change_seq += 1;
        self.request_preview(true);
        true
    }

    /// Remove the selected sprites/folders from the sprite set.
    fn remove_selected(&mut self) {
        let selected: Vec<PathBuf> = self.selected_paths.iter().cloned().collect();
        if selected.is_empty() {
            return;
        }
        let mut removed = 0usize;
        for path in selected {
            removed += self.remove_path(&path);
        }
        self.selected_paths.clear();
        self.selected_sprite = None;
        self.log(LogKind::Info, format!("{removed} sprite(s) quitado(s)."));
        self.change_seq += 1;
        self.request_preview(true);
    }

    /// Exclude a single file, or every image inside a directory.
    /// Returns how many sprites were hidden.
    fn remove_path(&mut self, path: &Path) -> usize {
        if path.is_dir() {
            let mut files = Vec::new();
            collect_images(path, &mut files);
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
    fn remove_smart_folder(&mut self, dir: &Path) {
        let norm = tp_core::ingest::normalize_path(dir);
        if tp_core::ingest::normalize_path(&self.config.input_directory) == norm {
            self.log(
                LogKind::Warning,
                "Ese directorio es el de entrada principal; quítalo en Ajustes > Datos.".into(),
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
        self.log(
            LogKind::Info,
            format!("Carpeta inteligente quitada: {}", dir.display()),
        );
        self.start_watcher();
        self.change_seq += 1;
        self.request_preview(true);
    }

    fn exclude(&mut self, path: &Path) {
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

    fn restore_excluded(&mut self) {
        let count = self.config.excluded_inputs.len();
        if count == 0 {
            return;
        }
        self.config.excluded_inputs.clear();
        self.log(LogKind::Info, format!("{count} sprite(s) restaurado(s)."));
        self.change_seq += 1;
        self.request_preview(true);
    }

    fn hidden_count(&self) -> usize {
        self.config.excluded_inputs.len()
    }

    /// Highlight a sprite from the preview/table and sync the sprites panel.
    fn select_sprite(&mut self, id: &str) {
        self.selected_sprite = Some(id.to_string());
        if let Some(out) = &self.result {
            if let Some(sprite) = out.result.sprites.iter().find(|s| s.id == id) {
                if sprite.atlas_page_index >= 0 {
                    self.selected_page = sprite.atlas_page_index as usize;
                }
                let path = PathBuf::from(&sprite.source_path);
                self.selected_paths.clear();
                self.selected_paths.insert(path);
            }
        }
    }

    /// Indices into the packed sprite list that match the current selection.
    fn selected_sprite_indices(&self) -> Vec<usize> {
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

    fn fit_zoom(&mut self) {
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

    /// Auto-detect 9-patch borders on the selected sprites from their source
    /// pixels (solid frame analysis) and apply them to the pack result.
    fn detect_borders(&mut self) {
        let indices = self.selected_sprite_indices();
        if indices.is_empty() {
            self.log(
                LogKind::Warning,
                "Selecciona sprites antes de detectar bordes.".into(),
            );
            return;
        }
        let Some(mut out) = self.result.take() else {
            self.log(
                LogKind::Warning,
                "Publica el atlas antes de detectar bordes.".into(),
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
            if tp_core::ingest::is_normal_file(&path) {
                continue;
            }
            let id = sprite.id.clone();
            let (w, h, rgba) = match tp_core::ingest::load_image_rgba(&path) {
                Ok(v) => v,
                Err(e) => {
                    self.log(
                        LogKind::Warning,
                        format!(
                            "No se pudo leer {} para detectar bordes: {e}",
                            path.display()
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
                    self.border_edits.insert(id.clone(), b);
                    self.log(LogKind::Info, format!("{id}: bordes detectados {b:?}"));
                }
                None => {
                    self.border_edits.remove(&id);
                    self.log(
                        LogKind::Warning,
                        format!("{id}: sin barras sólidas, se quita el 9-patch"),
                    );
                }
            }
        }
        self.log(
            LogKind::Info,
            format!(
                "Detección de bordes 9-patch: {detected} de {} sprite(s) con barras sólidas",
                indices.len()
            ),
        );
        self.result = Some(out);
    }

    fn save_pivots(&mut self) {
        let Some(dir) = self.sidecar_dir() else {
            self.log(
                LogKind::Error,
                "Añade sprites primero: los archivos se guardan junto a ellos.".into(),
            );
            return;
        };
        let Some(out) = &self.result else {
            self.log(
                LogKind::Error,
                "Publica primero el atlas antes de guardar pivots.".into(),
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
                Ok(_) => self.log(
                    LogKind::Info,
                    format!("Pivots guardados en {}", path.display()),
                ),
                Err(e) => self.log(LogKind::Error, format!("No se pudo guardar: {e}")),
            },
            Err(e) => self.log(LogKind::Error, format!("No se pudo serializar: {e}")),
        }
        // Bordes 9-patch a un archivo propio (solo sprites con bordes).
        self.write_borders_file(&dir, &borders);
    }

    /// Write `borders.json` next to the input sprites and log the result.
    fn write_borders_file(&mut self, dir: &Path, borders: &HashMap<String, [i32; 4]>) {
        let bpath = dir.join("borders.json");
        match serde_json::to_string_pretty(borders) {
            Ok(text) => match std::fs::write(&bpath, text) {
                Ok(_) => {
                    if !borders.is_empty() {
                        self.log(
                            LogKind::Info,
                            format!(
                                "Bordes 9-patch guardados en {} ({} sprite(s))",
                                bpath.display(),
                                borders.len()
                            ),
                        );
                    }
                }
                Err(e) => self.log(
                    LogKind::Error,
                    format!("No se pudo guardar borders.json: {e}"),
                ),
            },
            Err(e) => self.log(
                LogKind::Error,
                format!("No se pudo serializar borders.json: {e}"),
            ),
        }
    }

    /// Persist `borders.json` without user action (drag release / Detect).
    /// Silent when there is no sprite directory or nothing to save.
    fn auto_save_borders(&mut self) {
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
                    format!(
                        "borders.json actualizado automáticamente ({} borde(s))",
                        borders.len()
                    ),
                );
            }
        }
    }

    fn save_project(&mut self) {
        self.commit_paths();
        let path = self.project_path.clone().or_else(|| {
            rfd::FileDialog::new()
                .add_filter("Proyecto", &["tpproj"])
                .save_file()
        });
        let Some(path) = path else { return };
        self.parse_variants();
        match self.config.to_toml() {
            Ok(text) => match std::fs::write(&path, text) {
                Ok(_) => {
                    self.project_path = Some(path.clone());
                    self.log(
                        LogKind::Info,
                        format!("Proyecto guardado en {}", path.display()),
                    );
                }
                Err(e) => self.log(LogKind::Error, format!("No se pudo guardar: {e}")),
            },
            Err(e) => self.log(LogKind::Error, format!("Config inválida: {e}")),
        }
    }

    fn load_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Proyecto", &["tpproj"])
            .pick_file()
        else {
            return;
        };
        self.open_project(path);
    }

    fn open_project(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path) {
            Ok(text) => match ProjectConfig::from_toml(&text) {
                Ok(cfg) => {
                    self.config = cfg;
                    self.sync_variants();
                    self.sync_paths();
                    self.selected_paths.clear();
                    self.selected_sprite = None;
                    self.pivot_edits.clear();
                    self.border_edits.clear();
                    self.start_watcher();
                    self.project_path = Some(path.clone());
                    self.log(
                        LogKind::Info,
                        format!("Proyecto cargado: {}", path.display()),
                    );
                }
                Err(e) => self.log(LogKind::Error, format!("Proyecto inválido: {e}")),
            },
            Err(e) => self.log(LogKind::Error, format!("No se pudo leer: {e}")),
        }
    }

    fn reset_defaults(&mut self) {
        self.config = ProjectConfig::default();
        self.sync_variants();
        self.sync_paths();
        self.selected_paths.clear();
        self.selected_sprite = None;
        self.pivot_edits.clear();
        self.border_edits.clear();
        self.start_watcher();
        self.after_workspace_change();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.running {
            match rx.try_recv() {
                Ok(msg) => self.handle_run_result(ctx, msg),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
                Err(TryRecvError::Disconnected) => {
                    self.running = None;
                    self.log(
                        LogKind::Error,
                        "El hilo de empaquetado terminó inesperadamente.".into(),
                    );
                }
            }
        }

        self.poll_pending(ctx);
        self.poll_changes(ctx);

        toolbar::toolbar(self, ctx);

        egui::TopBottomPanel::bottom("bottom_panel")
            .resizable(true)
            .default_height(180.0)
            .min_height(64.0)
            .show(ctx, |ui| bottom::bottom_ui(self, ui));

        egui::SidePanel::left("sprites_panel")
            .resizable(true)
            .default_width(250.0)
            .min_width(180.0)
            .show(ctx, |ui| sprites_panel::sprites_ui(self, ui));

        egui::SidePanel::right("settings_panel")
            .resizable(true)
            .default_width(330.0)
            .min_width(260.0)
            .show(ctx, |ui| settings::settings_ui(self, ui));

        egui::CentralPanel::default().show(ctx, |ui| preview::preview_ui(self, ui));

        sprite_settings::sprite_settings_window(self, ctx);
        animation::animation_window(self, ctx);
        split_sheet::split_window(self, ctx);
    }
}

fn collect_images(dir: &Path, out: &mut Vec<PathBuf>) {
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
