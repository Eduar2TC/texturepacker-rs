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
/// Cuánto dura el resaltado de «recién añadido» (árbol y lienzo).
const JUST_ADDED_HL: std::time::Duration = std::time::Duration::from_secs(8);
/// Umbral (px) a partir del cual el panel inferior se considera abierto.
const BOTTOM_OPEN_HEIGHT: f32 = 180.0;

/// Verde del resaltado «recién añadido», legible sobre los dos temas.
pub(crate) fn just_added_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(130, 220, 160)
    } else {
        egui::Color32::from_rgb(0, 132, 74)
    }
}

/// Ámbar de aviso (carpetas inteligentes, «empaqueta sola», avisos).
pub(crate) fn amber_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(230, 190, 60)
    } else {
        egui::Color32::from_rgb(150, 105, 0)
    }
}

/// Azul informativo (carpetas anidadas, «publicando…», «actualizando…»).
pub(crate) fn info_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(120, 170, 255)
    } else {
        egui::Color32::from_rgb(20, 100, 205)
    }
}

/// Gris tenue de los textos de las zonas de arrastre (fondo de panel).
pub(crate) fn muted_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_gray(150)
    } else {
        egui::Color32::from_gray(100)
    }
}

/// El mismo gris, pero en su variante de «pasado por encima».
pub(crate) fn muted_hover_color(v: &egui::Visuals) -> egui::Color32 {
    if v.dark_mode {
        egui::Color32::from_rgb(180, 225, 255)
    } else {
        egui::Color32::from_rgb(20, 100, 205)
    }
}

/// Tinte de fondo de las zonas de arrastre (blanco en oscuro, negro en claro).
pub(crate) fn drop_tint(v: &egui::Visuals, hover: bool) -> egui::Color32 {
    if hover {
        egui::Color32::from_rgba_unmultiplied(120, 200, 255, 60)
    } else if v.dark_mode {
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18)
    } else {
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 14)
    }
}

/// Estilo visual global: esquinas suaves, selección y enlaces teñidos con el
/// acento, sliders rellenos. Se aplica a los estilos oscuro y claro a la vez
/// y sólo la primera vez (los valores no dependen del tema elegido).
fn apply_theme(ctx: &egui::Context, theme: crate::ui_prefs::Theme) {
    if ctx.memory(|m| m.data.get_temp::<bool>(egui::Id::new("tp_theme"))) == Some(true) {
        return;
    }
    theme.apply(ctx);
    // Se tocan los dos estilos (oscuro y claro) a la vez: egui elige uno de
    // ellos según la preferencia, así que un cambio de tema en caliente no
    // pierde estos ajustes.
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        v.window_corner_radius = 8.into();
        v.menu_corner_radius = 6.into();
        v.widgets.noninteractive.corner_radius = 4.into();
        v.widgets.inactive.corner_radius = 5.into();
        v.widgets.hovered.corner_radius = 5.into();
        v.widgets.active.corner_radius = 5.into();
        v.widgets.open.corner_radius = 5.into();
        v.selection.bg_fill = if v.dark_mode {
            egui::Color32::from_rgb(38, 98, 115)
        } else {
            egui::Color32::from_rgb(166, 206, 227)
        };
        v.selection.stroke.width = 1.0;
        v.hyperlink_color = if v.dark_mode {
            egui::Color32::from_rgb(97, 175, 239)
        } else {
            egui::Color32::from_rgb(0, 94, 190)
        };
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.4 };
        v.collapsing_header_frame = true;
    });
    ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new("tp_theme"), true));
}
use crate::i18n::t;
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
    /// Ancla de la selección por rango (Shift): el intervalo va de aquí a
    /// la fila bajo el cursor, en orden visual del panel.
    selection_anchor: Option<PathBuf>,
    /// Cursor de teclado en la lista de sprites: fila sobre la que actúan
    /// las flechas. Se separa del ancla solo mientras Shift está pulsado.
    list_cursor: Option<PathBuf>,
    /// El panel de sprites tiene el foco de teclado: las flechas y Supr
    /// actúan en la lista aunque el puntero esté en otra parte de la
    /// ventana (hasta que un clic en el lienzo o el filtro lo devuelven).
    tree_kb_focus: bool,
    /// Rutas entradas por la última acción (suelta del SO o diálogo): el
    /// árbol y el lienzo las señalan para responder «¿dónde acaba de ir mi
    /// archivo?» mientras el atlas se calcula.
    just_added: Vec<PathBuf>,
    /// Instante de la última adición: el resaltado se apaga solo.
    just_added_at: Option<std::time::Instant>,
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
    /// El último resultado aplicado salió de una publicación (ficheros en
    /// disco) o de la vista previa (solo nombres predichos): manda en cómo
    /// etiqueta la pestaña «Archivos».
    files_written: bool,
    /// Preferencias de interfaz (idioma y tema), guardadas en `ui.toml`.
    prefs: crate::ui_prefs::UiPrefs,
    /// Dónde se guardan esas preferencias. Las pruebas usan una ruta
    /// temporal para no tocar el fichero real del usuario.
    prefs_path: PathBuf,
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
        Self::build_with_prefs(egui_ctx, initial_project, crate::ui_prefs::UiPrefs::load())
    }

    /// Igual que [`build`] pero con las preferencias que se le den: así los
    /// tests no leen el `ui.toml` real ni exponen la suite a la elección de
    /// idioma de la máquina que la lanza.
    fn build_with_prefs(
        egui_ctx: egui::Context,
        initial_project: Option<PathBuf>,
        prefs: crate::ui_prefs::UiPrefs,
    ) -> Self {
        let cc = eframe::CreationContext::_new_kittest(egui_ctx.clone());
        crate::i18n::set_choice(prefs.lang_choice());
        let prefs_path = crate::ui_prefs::UiPrefs::path();
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
            selection_anchor: None,
            list_cursor: None,
            tree_kb_focus: false,
            just_added: Vec::new(),
            just_added_at: None,
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
            watcher: None,
            egui_ctx: cc.egui_ctx.clone(),
            last_snapshot_poll: std::time::Instant::now(),
            preview_stale: false,
            bottom_collapsed: false,
            last_title: String::new(),
            preview_retry_used: false,
            files_written: false,
            bottom_height: BOTTOM_OPEN_HEIGHT,
            canvas_rect: None,
            preview_zoom: 1.0,
            sprite_rows: Vec::new(),
            prefs,
            prefs_path,
        };
        apply_theme(&egui_ctx, app.prefs.theme());
        app.log(
            LogKind::Info,
            t!("Bienvenido a TexturePacker-RS. Añade sprites y pulsa «Publicar».").into(),
        );
        app.sync_paths();
        app.sync_variants();
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
        // Preferencias propias con el idioma ya fijado a español: los tests no
        // deben cambiar según el locale de quien lance la suite ni según el
        // ui.toml de su máquina, y todas las hebras del proceso ven siempre el
        // mismo idioma (si no, una construcción en paralelo reabriría la
        // ventana del sistema en medio de otra prueba).
        let prefs = crate::ui_prefs::UiPrefs {
            lang: crate::i18n::LangChoice::Es.id().to_string(),
            ..Default::default()
        };
        let mut app = Self::build_with_prefs(egui_ctx, initial_project, prefs);
        static TEST_FILE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = TEST_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        app.prefs_path =
            std::env::temp_dir().join(format!("tp-app-ui-{}-{n}.toml", std::process::id()));
        crate::i18n::set_choice(crate::i18n::LangChoice::Es);
        app
    }

    /// Preferencias de interfaz vigentes (para pintar los selectores).
    pub fn prefs(&self) -> &crate::ui_prefs::UiPrefs {
        &self.prefs
    }

    /// Cambia el idioma de la interfaz, lo aplica al vuelo y lo guarda.
    pub fn set_lang_choice(&mut self, choice: crate::i18n::LangChoice) {
        self.prefs.lang = choice.id().to_string();
        crate::i18n::set_choice(choice);
        self.persist_prefs();
        self.egui_ctx.request_repaint();
    }

    /// Cambia el tema de la ventana, lo aplica al vuelo y lo guarda.
    pub fn set_theme(&mut self, theme: crate::ui_prefs::Theme) {
        self.prefs.theme = theme.id().to_string();
        theme.apply(&self.egui_ctx);
        self.persist_prefs();
    }

    /// Guarda `ui.toml`; si falla se avisa en el log en lugar de romper.
    fn persist_prefs(&mut self) {
        if let Err(err) = self.prefs.save_to(&self.prefs_path) {
            self.log(
                LogKind::Warning,
                t!("No se pudieron guardar los ajustes de interfaz: {}", err),
            );
        }
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

    /// Rutas seleccionadas en el panel izquierdo, en el orden visual de la
    /// lista (para pruebas y para operar sobre el lote completo).
    pub fn selected_paths(&self) -> &BTreeSet<PathBuf> {
        &self.selected_paths
    }

    /// Ruta bajo el cursor de teclado del panel de sprites.
    pub fn list_cursor(&self) -> Option<&Path> {
        self.list_cursor.as_deref()
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
        // Punto de paso único de lo que llega del motor: los mensajes de
        // tp-core son españoles y aquí se pasan al idioma de la interfaz.
        let text = crate::i18n::tr(&text);
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

    /// Apply a pipeline result: refresh textures and keep the selection
    /// stable. Pivots and 9-patch borders already come from the project
    /// (`pivot_overrides` / `border_overrides`), so nothing is reapplied here.
    fn apply_output(&mut self, ctx: &egui::Context, out: PipelineOutput, files_written: bool) {
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
    fn auto_repaint(&self, ctx: &egui::Context) {
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
                t!("Espera a que la vista se calcule antes de colocar sprites.").into(),
            );
            return 0;
        }

        // Activar Manual si no lo está: el usuario está componiendo a mano.
        if self.config.effective_algorithm() != tp_core::config::PackingAlgorithm::Manual {
            self.config.algorithm = tp_core::config::PackingAlgorithm::Manual;
            self.log(
                LogKind::Info,
                t!("Algoritmo cambiado a Manual: los sprites soltados fijan su posición.").into(),
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
                t!(
                    "{} sprite(s) colocados en ({}, {}): se reempaqueta al instante.",
                    n,
                    px,
                    py
                ),
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
        if self.variants_text.trim() != variants_joined && self.parse_variants() {
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

    fn parse_variants(&mut self) -> bool {
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
            true
        }
    }

    /// Publica la hoja.
    ///
    /// `force` reescribe los ficheros aunque nada haya cambiado desde la
    /// última publicación (la entrada «Forzar publicación» del menú del
    /// original); sin ella, una hoja ya al día se deja como está.
    fn start_pack(&mut self, force: bool) {
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
    fn remove_selected(&mut self, order: &[PathBuf]) {
        let selected: Vec<PathBuf> = self.selected_paths.iter().cloned().collect();
        if selected.is_empty() {
            return;
        }
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
        self.log(LogKind::Info, t!("{} sprite(s) quitado(s).", removed));
        self.change_seq += 1;
        self.request_preview(true);
    }

    /// Sincroniza `selected_sprite` (id dentro del pack) con `selected_paths`:
    /// solo hay id cuando la selección es exactamente un sprite.
    fn sync_selected_sprite(&mut self) {
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
        self.log(
            LogKind::Info,
            t!("Carpeta inteligente quitada: {}", dir.display()),
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
        self.log(LogKind::Info, t!("{} sprite(s) restaurado(s).", count));
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
                self.selected_paths.insert(path.clone());
                self.list_cursor = Some(path.clone());
                self.selection_anchor = Some(path);
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

    fn save_pivots(&mut self) {
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
                Ok(_) => self.log(LogKind::Info, t!("Pivots guardados en {}", path.display())),
                Err(e) => self.log(LogKind::Error, t!("No se pudo guardar: {}", e)),
            },
            Err(e) => self.log(LogKind::Error, t!("No se pudo serializar: {}", e)),
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
                    t!(
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

    fn load_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(t!("Proyecto"), &["tpproj", "tps"])
            .pick_file()
        else {
            return;
        };
        self.open_project(path);
    }

    fn open_project(&mut self, path: PathBuf) {
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

    fn reset_defaults(&mut self) {
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

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_theme(ctx, self.prefs.theme());

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
                        t!("El hilo de empaquetado terminó inesperadamente.").into(),
                    );
                }
            }
        }

        self.poll_pending(ctx);
        self.poll_changes(ctx);
        handle_shortcuts(self, ctx);

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
                                egui::RichText::new(t!(
                                    "Página {} · {}×{} px · relleno {}%",
                                    p.index + 1,
                                    p.width,
                                    p.height,
                                    format!("{:.0}", fill * 100.0)
                                ))
                                .weak(),
                            );
                            if pages > 1 {
                                ui.label(
                                    egui::RichText::new(t!(
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
                            egui::RichText::new(t!(
                                "{} sprites · {} aliases",
                                out.result.total_sprites,
                                out.result.alias_count
                            ))
                            .weak(),
                        );
                    } else {
                        ui.label(egui::RichText::new(t!("Sin atlas — añade sprites")).weak());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.project_path.is_none() {
                            ui.label(
                                egui::RichText::new(t!("proyecto sin guardar — Ctrl+S"))
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
/// Un fichero de proyecto que la ventana sabe abrir al soltarlo. Solo las
/// extensiones de proyecto: un `Cargo.toml` o un `ui.toml` sueltos encima
/// no deben reemplazar la configuración cargada.
fn is_project_file(path: &Path) -> bool {
    path.is_file()
        && matches!(
            path.extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("tpproj" | "tps")
        )
}

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
                2.0_f32,
                egui::Color32::from_rgba_unmultiplied(120, 200, 255, 200),
            ),
            egui::StrokeKind::Outside,
        );
        let text = if hovered.len() == 1 {
            t!("Suelta para añadir al workspace").to_string()
        } else {
            t!("Suelta para añadir {} elementos", hovered.len())
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
            t!("(sprites, carpetas o proyectos; los sprites se empaquetan al instante)"),
            egui::FontId::proportional(13.0),
            egui::Color32::from_rgba_unmultiplied(180, 210, 235, 220),
        );
        // El overlay solo existe si hay frames: el hover no pasa por
        // `poll_changes`, así que el siguiente frame se pide aquí.
        ctx.request_repaint();
    }
    if dropped.is_empty() {
        return;
    }
    let total = dropped.len();
    let mut added = 0;
    for file in dropped {
        if let Some(path) = file.path {
            // Un proyecto soltado (`.tpproj` o `.tps`) se abre en la ventana;
            // `add_input` lo descartaría por no ser una imagen.
            if is_project_file(&path) {
                app.open_project(path.clone());
                if app.project_path.as_deref() == Some(path.as_path()) {
                    // El drop corre después de `poll_changes`: se programa el
                    // repack y el frame siguiente para que el atlas salga ya
                    // con el proyecto cargado y no a la siguiente interacción.
                    app.after_workspace_change();
                    ctx.request_repaint();
                }
                continue;
            }
            if app.add_input(path) {
                added += 1;
            }
        }
    }
    if added > 0 {
        app.log(LogKind::Info, t!("{} sprite(s) añadido(s).", added));
        // Empaqueta ya, sin esperar al debounce: el lienzo arranca a
        // calcular en el mismo gesto y no en el siguiente round-trip.
        app.request_preview(false);
        // Programa el frame que apagará el resaltado «recién añadido».
        ctx.request_repaint_after(JUST_ADDED_HL);
    } else {
        app.log(
            LogKind::Warning,
            format!(
                "Nada nuevo en el workspace ({total} fichero(s) soltado(s): \
                 ya estaban o no son imágenes)."
            ),
        );
    }
    // `poll_changes` (que programa el repack) ya corrió AL INICIO de este
    // frame, antes de procesar el drop: sin este repaint la cadena
    // debounce → hilo → resultado moriría aquí y el atlas no se actualizaría
    // hasta la próxima interacción (p. ej. pulsar Publicar).
    ctx.request_repaint();
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
        app.start_pack(false);
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

    // Esc cierra la ventana flotante activa (convención estándar) y, de
    // paso, devuelve el foco de teclado de la lista de sprites al ratón.
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if esc {
        app.tree_kb_focus = false;
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
        let mut app = App::new_for_testing(ctx.clone(), None);
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
        let mut app = App::new_for_testing(ctx.clone(), None);
        begin_sprite_drag(&app, &ctx, vec!["a".into()]);
        let _ = app.run_frame(&ctx, egui::RawInput::default());
        assert!(
            SpriteDrag::payload(&ctx).is_some(),
            "payload debe sobrevivir a un frame sin release"
        );
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
