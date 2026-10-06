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
//!
//! El estado (`App`), los tipos con los que habla el resto de la app y el
//! bucle de frames (`eframe::App`) viven en este módulo; el resto del
//! comportamiento está repartido en submódulos hermanos (`theme`, `drag`,
//! `state`, `pipeline`, `sprites`, `sidecar`, `project`, `events`),
//! re-exportados abajo para que las rutas `crate::app::X` no cambien.

mod animation;
mod bottom;
mod drag;
mod events;
mod pipeline;
mod preview;
mod project;
mod settings;
mod shortcuts;
mod sidecar;
mod split_sheet;
mod sprite_settings;
mod sprites;
mod sprites_panel;
mod state;
mod theme;
mod toolbar;

pub use drag::begin_sprite_drag;
pub(crate) use drag::{SpriteDrag, DRAG_THRESHOLD_PX, GHOST_MIN_SIZE_PX};
use events::{handle_global_file_drop, handle_shortcuts};
pub(crate) use theme::{
    amber_color, apply_theme, drop_tint, info_color, just_added_color, muted_color,
    muted_hover_color,
};

use crate::i18n::t;
use eframe::egui;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use tp_core::config::ProjectConfig;
use tp_core::pipeline::PipelineOutput;

/// Retardo tras el último cambio antes de reempaquetar (ajuste de sliders).
const PREVIEW_DEBOUNCE_MS: u64 = 120;
/// Cadencia de sondeo del snapshot (mtimes de los sprites en disco).
const SNAPSHOT_POLL_MS: u64 = 150;
/// Cuánto dura el resaltado de «recién añadido» (árbol y lienzo).
const JUST_ADDED_HL: std::time::Duration = std::time::Duration::from_secs(8);
/// Umbral (px) a partir del cual el panel inferior se considera abierto.
const BOTTOM_OPEN_HEIGHT: f32 = 180.0;

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
    /// Encuadrar la hoja en cuanto haya resultado y lienzo medido. Se apaga
    /// solo al primer encuadre y en cuanto el usuario toca el zoom, para que
    /// la vista nunca se mueva por su cuenta dos veces seguidas.
    auto_fit: bool,
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
    /// Ayuda de atajos de teclado abierta (F1, «?» o el botón «?» de la
    /// barra; review UI/UX I2).
    show_shortcuts: bool,
    bottom_tab: BottomTab,
    logs: Vec<LogEntry>,
    /// Directorios que ya se avisó que no se pudieron leer: evita repetir el
    /// mismo aviso en cada repaso del snapshot (se olvidan al recuperarse).
    unreadable_dirs: Vec<PathBuf>,
    project_path: Option<PathBuf>,
    /// Huella TOML de la última versión del proyecto escrita (o cargada) en
    /// disco: mientras la configuración no se separe de ella no hay nada que
    /// guardar (review UI/UX C2).
    saved_config: String,
    /// El diálogo «hay cambios sin guardar» está abierto.
    exit_pending: bool,
    /// El usuario ya decidió salir sin guardar: el siguiente cierre pasa sin
    /// volver a preguntar.
    exit_confirmed: bool,
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
    /// Huella (`Debug`) de `config` vista por el último sondeo de
    /// `poll_changes`. Si un control muta la config sin avisar con
    /// `on_config_changed`, la huella no cuadra y el propio sondeo notifica
    /// en el frame siguiente: ningún widget del panel de Ajustes puede
    /// cablearse mal a medias. `None` = aún sin muestrear (primer frame).
    config_fingerprint: Option<String>,
    /// Árbol de entrada ya construido, con la clave que lo produce. Véase
    /// `App::take_tree` (en el módulo `sprites_panel`): `build_tree` hace
    /// `read_dir` recursivo con sort y allocations por nodo, y se llamaba
    /// dentro de `update()`, o sea en cada frame (≈60 repasos de disco por
    /// segundo). Ahora se rehace solo cuando cambia algo que lee (config y
    /// filtro) o cuando el snapshot de disco avisa (M14).
    tree_cache: Option<sprites_panel::TreeCache>,
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

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_theme(ctx, self.prefs.theme());

        // Cambios sin guardar: si el sistema pide cerrar la ventana con la
        // configuración tocada, el cierre se cancela y se abre el diálogo de
        // confirmación; si no hay nada pendiente, el cierre pasa solo (C2).
        let mut dirty = self.is_dirty();
        if ctx.input(|i| i.viewport().close_requested()) && dirty && !self.exit_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.exit_pending = true;
        }

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
                                "{} sprite(s) · {} alias(es)",
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
        shortcuts::shortcuts_window(self, ctx);

        // El diálogo de cierre puede haber guardado el proyecto, así que la
        // huella se vuelve a mirar justo antes de componer el título.
        self.exit_dialog(ctx);
        dirty = self.is_dirty();

        // La ruta del proyecto vive en el título de la ventana, no en la
        // barra de herramientas (evita truncamientos y ruido visual). El
        // punto delante avisa de que hay cambios sin guardar (C2).
        let base = match &self.project_path {
            Some(p) => format!("{} — TexturePacker-RS", p.display()),
            None => "TexturePacker-RS".to_string(),
        };
        let title = if dirty { format!("• {base}") } else { base };
        if self.last_title != title {
            self.last_title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
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
