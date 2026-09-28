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

/// Retardo tras el último cambio antes de reempaquetar (ajuste de sliders).
const PREVIEW_DEBOUNCE_MS: u64 = 120;
/// Cadencia de sondeo del snapshot (mtimes de los sprites en disco).
const SNAPSHOT_POLL_MS: u64 = 150;

/// Umbral (px) a partir del cual el panel inferior se considera abierto.
const BOTTOM_OPEN_HEIGHT: f32 = 180.0;

/// Estilo visual global: tema oscuro con esquinas suaves, acento cian y
/// sliders rellenos. Llamado una vez por frame; solo construye el estilo
/// nuevo la primera vez.
fn apply_theme(ctx: &egui::Context) {
    if ctx.memory(|m| m.data.get_temp::<bool>(egui::Id::new("tp_theme"))) == Some(true) {
        return;
    }
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut(|style| {
        let v = &mut style.visuals;
        v.window_corner_radius = 8.into();
        v.menu_corner_radius = 6.into();
        v.widgets.noninteractive.corner_radius = 4.into();
        v.widgets.inactive.corner_radius = 5.into();
        v.widgets.hovered.corner_radius = 5.into();
        v.widgets.active.corner_radius = 5.into();
        v.widgets.open.corner_radius = 5.into();
        v.selection.bg_fill = egui::Color32::from_rgb(38, 98, 115);
        v.selection.stroke.width = 1.0;
        v.hyperlink_color = egui::Color32::from_rgb(97, 175, 239);
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.4 };
        v.collapsing_header_frame = true;
    });
    ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new("tp_theme"), true));
}
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use tp_core::config::ProjectConfig;
use tp_core::pipeline::PipelineOutput;
use tp_core::types::Point2D;

/// Estado de frescura de la vista previa (indicador de la barra de zoom).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewState {
    /// Publicando (export a disco en marcha).
    Publishing,
    /// Recalculando la vista previa (en memoria).
    Updating,
    /// La vista no refleja el workspace actual.
    Stale,
    /// Al día.
    Ok,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BottomTab {
    Log,
    Output,
    Sprites,
    Mesh,
}

/// Umbral mínimo de arrastre (px) para decisiones de interacción propias:
/// la zona vacía del lienzo no inicia marquee hasta superar esta distancia
/// (evita marquesinas accidentales con micro-movimientos; egui ya exige
/// `max_click_dist` = 6 px para convertir un press en drag).
pub(crate) const DRAG_THRESHOLD_PX: f32 = 6.0;

/// Tamaño mínimo en pantalla del fantasma de arrastre (px): a zoom bajo un
/// sprite de 8 px sería invisible; se dibuja al menos tan grande como esto.
pub(crate) const GHOST_MIN_SIZE_PX: f32 = 24.0;

/// Arrastre de sprites del panel izquierdo hacia el lienzo: los ids
/// arrastrados (multi-selección incluida) y el frame visible de cada uno
/// (para el fantasma y el plan de colocación). Viaja como payload de
/// DragAndDrop.
pub(crate) struct SpriteDrag {
    pub ids: Vec<String>,
    pub first_frame: Option<tp_core::types::Rect>,
    /// Frame visible de cada id arrastrado (para previsualizar la
    /// disposición completa de la multi-selección).
    pub frames: std::collections::BTreeMap<String, tp_core::types::Rect>,
}

impl SpriteDrag {
    pub(crate) fn payload(ctx: &egui::Context) -> Option<std::sync::Arc<SpriteDrag>> {
        egui::DragAndDrop::payload::<SpriteDrag>(ctx)
    }

    pub(crate) fn clear(ctx: &egui::Context) {
        egui::DragAndDrop::clear_payload(ctx);
    }

    /// ¿Hay un arrastre de sprites en marcha (cualquier tipo de destino)?
    pub(crate) fn has_payload(ctx: &egui::Context) -> bool {
        egui::DragAndDrop::has_payload_of_type::<SpriteDrag>(ctx)
    }

    /// Recupera y consume el payload (para destinos que no sean el lienzo).
    pub(crate) fn take(ctx: &egui::Context) -> Option<std::sync::Arc<SpriteDrag>> {
        egui::DragAndDrop::take_payload::<SpriteDrag>(ctx)
    }
}

/// Registra el inicio de un arrastre de sprites (desde el árbol) con los ids
/// arrastrados: si la fila arrastrada pertenece a la selección multi, viajan
/// todos los seleccionados.
/// Inicia un arrastre de sprites desde el árbol hacia el lienzo. `pub`
/// (no solo `pub(crate)`) para que los tests de integración conduzcan el
/// mismo camino exacto que la UI.
pub fn begin_sprite_drag(app: &App, ctx: &egui::Context, mut ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    ids.sort();
    let mut frames = std::collections::BTreeMap::new();
    if let Some(out) = app.result.as_ref() {
        for s in &out.result.sprites {
            if ids.contains(&s.id) && !s.is_alias {
                frames.insert(s.id.clone(), s.visible_frame);
            }
        }
    }
    let first_frame = ids.iter().find_map(|id| frames.get(id).copied());
    // OJO: set_payload ya envuelve en Arc internamente; pasar el struct
    // directamente (no un Arc propio) para que payload::<SpriteDrag>()
    // haga downcast correctamente.
    egui::DragAndDrop::set_payload(
        ctx,
        SpriteDrag {
            ids,
            first_frame,
            frames,
        },
    );
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
    /// La configuración cambió por UI: el próximo `request_preview` no
    /// necesita comparar snapshots (ahorra re-escanear el disco por frame).
    pending_force: bool,
    /// Pivots/borders edited in the GUI, reapplied on every repack.
    pivot_edits: HashMap<String, Point2D>,
    border_edits: HashMap<String, [i32; 4]>,
    /// Filesystem watcher (autowatch): edits on disk refresh the preview.
    watcher: Option<notify::RecommendedWatcher>,
    /// Clonable handle to wake the UI from the watcher thread.
    egui_ctx: egui::Context,
    /// Throttle for the (mtime-based) workspace snapshot poll.
    last_snapshot_poll: std::time::Instant,
    /// Cached result of the last snapshot freshness check (for the zoom-bar
    /// indicator between polls).
    preview_stale: bool,
    /// Panel inferior plegado (gana espacio para la vista del atlas).
    bottom_collapsed: bool,
    /// Última altura abierta del panel inferior (se restaura al desplegar).
    bottom_height: f32,
    /// Rect en pantalla del lienzo del atlas (para el drop del panel).
    canvas_rect: Option<egui::Rect>,
    /// Zoom actual de la vista del atlas (convierte pantalla → píxeles).
    preview_zoom: f32,
    /// Rects en pantalla de las filas de sprite del panel izquierdo
    /// (ruta del fichero → rect de la fila), registrados durante el último
    /// frame. Permite a las pruebas apuntar eventos de puntero a la fila
    /// exacta de un sprite (autotest `tp-smoke`).
    sprite_rows: Vec<(PathBuf, egui::Rect)>,
    /// Último título de ventana aplicado (evita comandos repetidos).
    last_title: String,
    /// One automatic preview retry per successful cycle (mid-write reads).
    preview_retry_used: bool,
}

impl App {
    /// Constructor para la ventana nativa (necesita el `CreationContext`
    /// de eframe para temas, fuentes y storage).
    pub fn new(cc: &eframe::CreationContext<'_>, initial_project: Option<PathBuf>) -> Self {
        Self::build(cc.egui_ctx.clone(), initial_project)
    }

    /// Ejecuta un frame completo de la app sobre un `egui::Context` dado.
    /// Es el mismo camino que toma la ventana nativa (`eframe::App::update`
    /// con un `Frame` de testing), así que sirve para conducir la app real
    /// en pruebas headless y en el autotest `tp-smoke` sin interacción
    /// humana: pasa `RawInput` y obtiene los shapes/resultados del frame.
    pub fn run_frame(&mut self, ctx: &egui::Context, input: egui::RawInput) -> egui::FullOutput {
        let mut frame = eframe::Frame::_new_kittest();
        ctx.run(input, |ctx| {
            eframe::App::update(self, ctx, &mut frame);
        })
    }

    /// Constructor interno compartido: funciona con cualquier `egui::Context`,
    /// incluido uno puro de pruebas (sin ventana ni GPU).
    fn build(egui_ctx: egui::Context, initial_project: Option<PathBuf>) -> Self {
        let cc = eframe::CreationContext::_new_kittest(egui_ctx.clone());
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
            pending_force: false,
            pivot_edits: HashMap::new(),
            border_edits: HashMap::new(),
            watcher: None,
            egui_ctx: cc.egui_ctx.clone(),
            last_snapshot_poll: std::time::Instant::now(),
            preview_stale: false,
            bottom_collapsed: false,
            last_title: String::new(),
            preview_retry_used: false,
            bottom_height: BOTTOM_OPEN_HEIGHT,
            canvas_rect: None,
            preview_zoom: 1.0,
            sprite_rows: Vec::new(),
        };
        apply_theme(&egui_ctx);
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

    /// Igual que `App::new` pero aceptando un `egui::Context` cualquiera
    /// (típicamente uno headless de pruebas): mismo arranque que la ventana
    /// nativa —tema, log de bienvenida, watcher y proyecto inicial—.
    pub fn new_for_testing(egui_ctx: egui::Context, initial_project: Option<PathBuf>) -> Self {
        Self::build(egui_ctx, initial_project)
    }

    /// Vista previa actual aplicada a la UI (`None` hasta el primer pack).
    pub fn result(&self) -> Option<&PipelineOutput> {
        self.result.as_ref()
    }

    /// Texturas egui cargadas (una por página del atlas).
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// (sprites totales, aliases, páginas) del último resultado.
    pub fn pack_summary(&self) -> (usize, usize, &[tp_core::types::PageInfo]) {
        match &self.result {
            Some(out) => (
                out.result.total_sprites,
                out.result.alias_count,
                &out.result.pages,
            ),
            None => (0, 0, &[]),
        }
    }

    /// Nivel de zoom actual de la vista del atlas.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Configuración del proyecto (solo lectura; para tests e inspector).
    pub fn config(&self) -> &ProjectConfig {
        &self.config
    }

    /// ¿Hay un repack solicitado y aún no lanzado (debounce pendiente)?
    pub fn repack_requested(&self) -> bool {
        self.pending_force
    }

    /// Rects en pantalla de las filas de sprite del panel izquierdo tal y
    /// como quedaron en el último frame dibujado (para pruebas de puntero).
    pub fn sprite_row_rects(&self) -> &[(PathBuf, egui::Rect)] {
        &self.sprite_rows
    }

    /// Rect en pantalla del lienzo del atlas (última pasada de la vista);
    /// `None` si aún no hay vista o la página no se dibujó este frame.
    pub fn canvas_rect(&self) -> Option<egui::Rect> {
        self.canvas_rect
    }

    /// Zoom de la vista del atlas: pantalla → píxeles del atlas es dividir
    /// por este valor (es el que usan el fantasma y el drop).
    pub fn preview_zoom(&self) -> f32 {
        self.preview_zoom
    }

    /// Contenido del Log en formato «I/W/E texto» (para inspección en pruebas).
    pub fn log_texts(&self) -> Vec<String> {
        self.logs
            .iter()
            .map(|l| {
                let tag = match l.kind {
                    LogKind::Info => "I",
                    LogKind::Warning => "W",
                    LogKind::Error => "E",
                };
                format!("{tag} {}", l.text)
            })
            .collect()
    }

    fn log(&mut self, kind: LogKind, text: String) {
        // Espejo opcional del Log a stderr (TP_LOG_STDERR=1): permite seguir
        // la sesión desde un terminal o en CI sin depender de la GUI.
        static MIRROR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *MIRROR.get_or_init(|| std::env::var_os("TP_LOG_STDERR").is_some()) {
            let tag = match kind {
                LogKind::Info => "I",
                LogKind::Warning => "W",
                LogKind::Error => "E",
            };
            eprintln!("[{tag}] {text}");
        }
        // Un error salta al Log: que no pase desapercibido en otra pestaña.
        if matches!(kind, LogKind::Error) {
            self.bottom_tab = BottomTab::Log;
        }
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
    fn sprite_id_for_path(&self, path: &Path) -> Option<String> {
        self.result
            .as_ref()?
            .result
            .sprites
            .iter()
            .find(|s| Path::new(&s.source_path) == path || s.source_path == path.to_string_lossy())
            .map(|s| s.id.clone())
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
                    s == self.change_seq
                        && at.elapsed() < std::time::Duration::from_millis(PREVIEW_DEBOUNCE_MS)
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

    /// Suelta sprites arrastrados del panel sobre el lienzo: activa el
    /// algoritmo Manual si hace falta, fija las posiciones relativas entre
    /// sí (con el mismo snap y clamp que promete el fantasma) y repacktua
    /// al instante. Devuelve cuántos sprites se colocaron.
    pub fn drop_sprites_on_canvas(&mut self, atlas_pos: egui::Pos2) -> usize {
        let Some(drag) = SpriteDrag::payload(&self.egui_ctx) else {
            return 0;
        };
        let ids = drag.ids.clone();
        let frames = drag.frames.clone();
        let first_frame = drag.first_frame;
        SpriteDrag::clear(&self.egui_ctx);
        if ids.is_empty() {
            return 0;
        }
        // Se necesitan frames conocidos: sin resultado previo no hay nada
        // que colocar (los sprites recién añadidos se empaquetan solos).
        if self.result.is_none() {
            self.log(
                LogKind::Warning,
                "Espera a que la vista se calcule antes de colocar sprites.".into(),
            );
            return 0;
        }

        // Activar Manual si no lo está: el usuario está componiendo a mano.
        if self.config.effective_algorithm() != tp_core::config::PackingAlgorithm::Manual {
            self.config.algorithm = tp_core::config::PackingAlgorithm::Manual;
            self.log(
                LogKind::Info,
                "Algoritmo cambiado a Manual: los sprites soltados fijan su posición.".into(),
            );
        }

        // Fantasma y drop comparten el plan: lo que se ve es lo que queda.
        let placed_list =
            Self::plan_canvas_drop(&ids, &frames, first_frame, atlas_pos, &self.config);
        for (id, (px, py)) in &placed_list {
            self.config.manual_positions.insert(id.clone(), (*px, *py));
        }
        let placed = placed_list.len();
        let first_pos = placed_list.first().map(|(_, p)| *p);

        if placed > 0 {
            // Selección visible: los colocados quedan seleccionados.
            self.selected_paths.clear();
            for id in &ids {
                if let Some(s) = self
                    .result
                    .as_ref()
                    .and_then(|o| o.result.sprites.iter().find(|s| &s.id == id))
                {
                    self.selected_paths.insert(PathBuf::from(&s.source_path));
                }
            }
            let n = placed;
            let (px, py) = first_pos.unwrap_or((0, 0));
            self.selected_sprite = ids
                .iter()
                .find(|id| self.config.manual_positions.contains_key(*id))
                .cloned();
            self.log(
                LogKind::Info,
                format!("{n} sprite(s) colocados en ({px}, {py}): se reempaqueta al instante."),
            );
            self.pending_force = true;
            self.after_workspace_change();
        }
        placed
    }

    /// Plan de colocación de un drop en el lienzo: para cada id arrastrado
    /// colocado (no alias, con frame conocido), la posición manual `(x, y)`
    /// resultante. Único camino para el fantasma del arrastre y para el drop
    /// real: el fantasma dibuja exactamente estas posiciones.
    ///
    /// - El frame del sprite primero queda bajo el cursor (esquina superior
    ///   izquierda), menos borde/padding del atlas: el píxel, no la caja.
    /// - La disposición relativa entre los sprites se conserva.
    /// - Con rejilla Manual activa, el anclaje se imanta al paso (el motor
    ///   aplica el mismo snap al empaquetar: consistencia garantizada).
    /// - Cada sprite se clampea para que quepa dentro del lienzo (igual que
    ///   el arrastre en vivo: soltar en el borde no sale del atlas).
    pub fn plan_canvas_drop(
        ids: &[String],
        frames: &std::collections::BTreeMap<String, tp_core::types::Rect>,
        first_frame: Option<tp_core::types::Rect>,
        atlas_pos: egui::Pos2,
        config: &ProjectConfig,
    ) -> Vec<(String, (i32, i32))> {
        let (bp, pad) = (config.border_padding.max(0), config.padding.max(0));
        let mut anchor_x = (atlas_pos.x as i32 - bp - pad).max(0);
        let mut anchor_y = (atlas_pos.y as i32 - bp - pad).max(0);
        if let Some(g) = &config.manual_grid {
            let (sx, sy) = g.snap_pos((anchor_x, anchor_y));
            anchor_x = sx;
            anchor_y = sy;
        }
        let frame0 = first_frame.unwrap_or(tp_core::types::Rect::new(0, 0, 0, 0));
        let origin = (frame0.x, frame0.y);
        let canvas_w = if config.fixed_width > 0 {
            config.fixed_width
        } else {
            config.max_texture_size
        };
        let canvas_h = if config.fixed_height > 0 {
            config.fixed_height
        } else {
            config.max_texture_size
        };

        let mut out = Vec::new();
        for id in ids {
            // Los aliases no ocupan frame propio: quedan donde el motor los
            // ponga (superpuestos a su objetivo).
            let Some(frame) = frames.get(id) else {
                continue;
            };
            let px = anchor_x + frame.x - origin.0;
            let py = anchor_y + frame.y - origin.1;
            let max_x = (canvas_w - 2 * bp - (frame.width + 2 * pad)).max(0);
            let max_y = (canvas_h - 2 * bp - (frame.height + 2 * pad)).max(0);
            out.push((id.clone(), (px.clamp(0, max_x), py.clamp(0, max_y))));
        }
        out
    }

    /// Workspace changed via the settings panel: commit and repack at once,
    /// without waiting for the passive snapshot poll (ni re-escanear el
    /// disco: el propio widget ya avisó de que cambió). `request_preview`
    /// coalesces los arrastres de sliders (un cambio por frame).
    pub fn on_config_changed(&mut self) {
        self.pending_force = true;
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
        // El snapshot (mtimes incluidos) se recalcula como mucho cada
        // SNAPSHOT_POLL_MS: los cambios de ajustes llegan por on_config_changed,
        // así que este sondeo solo vigila los ficheros en disco (autowatch).
        if self.last_snapshot_poll.elapsed() >= std::time::Duration::from_millis(SNAPSHOT_POLL_MS) {
            self.last_snapshot_poll = std::time::Instant::now();
            self.preview_stale = self.snapshot_changed();
            if self.preview_stale {
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
        apply_theme(ctx);

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
        handle_shortcuts(self, ctx);

        // Registro fresco cada frame: las filas que no se dibujen este
        // frame (filtro, panel colapsado…) desaparecen del registro.
        self.sprite_rows.clear();

        // Soltar ficheros del SO en CUALQUIER parte de la ventana (centro,
        // paneles, barra): comportamiento estándar de las apps del estilo.
        handle_global_file_drop(self, ctx);

        toolbar::toolbar(self, ctx);

        // Barra de estado (abajo del todo, declarada primero): datos del
        // atlas actual. Responde a «¿dónde están mis datos?» sin robar
        // altura a la vista.
        egui::TopBottomPanel::bottom("status_bar")
            .resizable(false)
            .exact_height(22.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(out) = &self.result {
                        let pages = out.pages.len();
                        if let Some(p) = out.pages.get(self.selected_page) {
                            let fill = out
                                .result
                                .pages
                                .iter()
                                .find(|pi| pi.index == p.index)
                                .map(|pi| pi.fill_ratio)
                                .unwrap_or(0.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Página {} · {}×{} px · relleno {:.0}%",
                                    p.index + 1,
                                    p.width,
                                    p.height,
                                    fill * 100.0
                                ))
                                .weak(),
                            );
                            if pages > 1 {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "· {}/{}",
                                        self.selected_page + 1,
                                        pages
                                    ))
                                    .weak(),
                                );
                            }
                        }
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!(
                                "{} sprites · {} aliases",
                                out.result.total_sprites, out.result.alias_count
                            ))
                            .weak(),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("Sin atlas — añade sprites".to_string()).weak(),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.project_path.is_none() {
                            ui.label(
                                egui::RichText::new("proyecto sin guardar — Ctrl+S")
                                    .weak()
                                    .italics(),
                            );
                        }
                    });
                });
            });

        // Panel inferior plegable: colapsado deja el atlas como protagonista.
        // Conserva la última altura abierta para restaurarla al desplegar.
        let bottom_open = self.bottom_height.max(64.0);
        let resizable = !self.bottom_collapsed;
        let bottom = egui::TopBottomPanel::bottom("bottom_panel")
            .resizable(resizable)
            .default_height(bottom_open)
            .height_range(28.0..=f32::INFINITY);
        let bottom = if self.bottom_collapsed {
            bottom.exact_height(28.0)
        } else {
            bottom
        };
        bottom.show(ctx, |ui| {
            if !self.bottom_collapsed {
                self.bottom_height = ui.available_height();
            }
            bottom::bottom_ui(self, ui);
        });

        // Fondo ligeramente distinto en los paneles laterales: separa
        // herramientas (izquierda/derecha) del lienzo (centro).
        let panel_fill = ctx.style().visuals.panel_fill;
        let side_fill = egui::Color32::from_rgba_unmultiplied(
            panel_fill.r().saturating_sub(6),
            panel_fill.g().saturating_sub(6),
            panel_fill.b().saturating_sub(6),
            255,
        );

        egui::SidePanel::left("sprites_panel")
            .resizable(true)
            .default_width(250.0)
            .min_width(180.0)
            .frame(egui::Frame::default().fill(side_fill))
            .show(ctx, |ui| sprites_panel::sprites_ui(self, ui));

        egui::SidePanel::right("settings_panel")
            .resizable(true)
            .default_width(330.0)
            .min_width(260.0)
            .frame(egui::Frame::default().fill(side_fill))
            .show(ctx, |ui| settings::settings_ui(self, ui));

        egui::CentralPanel::default().show(ctx, |ui| preview::preview_ui(self, ui));

        sprite_settings::sprite_settings_window(self, ctx);
        animation::animation_window(self, ctx);
        split_sheet::split_window(self, ctx);

        // La ruta del proyecto vive en el título de la ventana, no en la
        // barra de herramientas (evita truncamientos y ruido visual).
        let title = match &self.project_path {
            Some(p) => format!("{} — TexturePacker-RS", p.display()),
            None => "TexturePacker-RS".to_string(),
        };
        if self.last_title != title {
            self.last_title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }
}

/// Soltar ficheros del SO en cualquier parte de la ventana: los añade al
/// workspace. Devuelve un overlay azul mientras el arrastre esté sobre la
/// app (feedback estándar) y registra en el Log lo añadido.
fn handle_global_file_drop(app: &mut App, ctx: &egui::Context) {
    let (hovered, dropped) =
        ctx.input(|i| (i.raw.hovered_files.clone(), i.raw.dropped_files.clone()));
    if !hovered.is_empty() {
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            "drop_overlay".into(),
        ));
        painter.rect_filled(
            screen.shrink(2.0),
            6.0,
            egui::Color32::from_rgba_unmultiplied(90, 180, 255, 28),
        );
        painter.rect_stroke(
            screen.shrink(4.0),
            6.0,
            egui::Stroke::new(
                2.0,
                egui::Color32::from_rgba_unmultiplied(120, 200, 255, 200),
            ),
            egui::StrokeKind::Outside,
        );
        let text = if hovered.len() == 1 {
            "Suelta para añadir al workspace".to_string()
        } else {
            format!("Suelta para añadir {} elementos", hovered.len())
        };
        painter.text(
            screen.center() + egui::vec2(0.0, -16.0),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(22.0),
            egui::Color32::from_rgba_unmultiplied(230, 245, 255, 255),
        );
        painter.text(
            screen.center() + egui::vec2(0.0, 14.0),
            egui::Align2::CENTER_CENTER,
            "(sprites o carpetas; se empaquetan al instante)",
            egui::FontId::proportional(13.0),
            egui::Color32::from_rgba_unmultiplied(180, 210, 235, 220),
        );
    }
    if dropped.is_empty() {
        return;
    }
    let mut added = 0;
    for file in dropped {
        if let Some(path) = file.path {
            if app.add_input(path) {
                added += 1;
            }
        }
    }
    if added > 0 {
        app.log(LogKind::Info, format!("{added} sprite(s) añadido(s)."));
    } else {
        app.log(LogKind::Warning, "No se añadieron sprites nuevos.".into());
    }
}

/// Atajos de teclado globales (estilo estándar de herramientas de escritorio):
/// Ctrl+O abrir, Ctrl+S guardar, Ctrl+P publicar, Supr quitar selección,
/// +/-/0 zoom, F ajustar, Esc cierra ventanas flotantes.
fn handle_shortcuts(app: &mut App, ctx: &egui::Context) {
    let consume = |ctx: &egui::Context, key: egui::Key| {
        ctx.input(|i| {
            let mods = i.modifiers;
            i.key_pressed(key) && (mods.ctrl || mods.command)
        })
    };
    if consume(ctx, egui::Key::O) {
        app.load_project();
    }
    if consume(ctx, egui::Key::S) {
        app.save_project();
    }
    if consume(ctx, egui::Key::P) && app.running.is_none() {
        app.start_pack();
    }

    // Zoom de teclado (sin modificadores, como en Figma/Photoshop):
    // +/- acercan y alejan, 0 restaura 1:1 y F encuadra.
    let (zoom_in, zoom_out, zoom_reset, zoom_fit) = ctx.input(|i| {
        let blocked = i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt;
        (
            !blocked && i.key_pressed(egui::Key::Equals),
            !blocked && i.key_pressed(egui::Key::Minus),
            !blocked && i.key_pressed(egui::Key::Num0),
            !blocked && i.key_pressed(egui::Key::F),
        )
    });
    if zoom_in {
        preview::zoom_step(app, 1);
    }
    if zoom_out {
        preview::zoom_step(app, -1);
    }
    if zoom_reset {
        app.zoom = 1.0;
    }
    if zoom_fit {
        app.fit_zoom();
    }

    // Esc cierra la ventana flotante activa (convención estándar).
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if esc {
        if app.show_animation {
            app.show_animation = false;
        } else if app.show_split {
            app.show_split = false;
        } else if app.show_sprite_settings {
            app.show_sprite_settings = false;
        }
    }

    // Cursor de «copiando» mientras se arrastra un sprite del árbol hacia
    // el lienzo o una hoja (feedback estándar de arrastrar-y-soltar).
    if SpriteDrag::has_payload(ctx) {
        ctx.set_cursor_icon(egui::CursorIcon::Copy);
    }
}

/// Conduce la app real (la misma que abre la ventana) durante `frames`
/// frames sobre un `egui::Context` headless. Utilidad para pruebas:
/// `App::new_for_testing` + `run_frame` sin repetir la receta.
#[cfg_attr(not(test), allow(dead_code))]
pub fn run_headless(app: &mut App, ctx: &egui::Context, mut frames: usize) {
    while frames > 0 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1360.0, 860.0),
            )),
            ..egui::RawInput::default()
        };
        let _ = app.run_frame(ctx, input);
        frames -= 1;
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

#[cfg(test)]
mod drag_payload_tests {
    use super::*;

    #[test]
    fn payload_set_then_read_without_frames() {
        let ctx = egui::Context::default();
        let mut app = App::build(ctx.clone(), None);
        begin_sprite_drag(&app, &ctx, vec!["a".into(), "b".into()]);
        let got = SpriteDrag::payload(&ctx);
        assert!(
            got.is_some(),
            "payload debe sobrevivir sin frames intermedios"
        );
        SpriteDrag::clear(&ctx);
        assert!(SpriteDrag::payload(&ctx).is_none());
        let _ = &mut app;
    }

    #[test]
    fn payload_survives_a_frame_without_release() {
        let ctx = egui::Context::default();
        let mut app = App::build(ctx.clone(), None);
        begin_sprite_drag(&app, &ctx, vec!["a".into()]);
        let _ = app.run_frame(&ctx, egui::RawInput::default());
        assert!(
            SpriteDrag::payload(&ctx).is_some(),
            "payload debe sobrevivir a un frame sin release"
        );
    }
}
