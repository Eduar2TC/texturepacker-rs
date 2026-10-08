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
//! comportamiento está repartido en submódulos hermanos (`aviso`, `theme`,
//! `drag`, `state`, `pipeline`, `sprites`, `sidecar`, `project`, `events`),
//! re-exportados abajo para que las rutas `crate::app::X` no cambien.

mod animation;
mod aviso;
mod bottom;
mod deshacer;
mod drag;
mod events;
mod menubar;
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

#[cfg(test)]
mod dock_ajustes;

#[cfg(test)]
mod ventana_minima;

pub use drag::begin_sprite_drag;
pub(crate) use drag::{SpriteDrag, DRAG_THRESHOLD_PX, GHOST_MIN_SIZE_PX};
use events::{handle_global_file_drop, handle_shortcuts};
pub(crate) use theme::{
    amber_color, apply_theme, drop_tint, info_color, just_added_color, muted_color,
    muted_hover_color, superficies,
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
///
/// Es también la altura con la que arranca: 180 dejaban 81 px de hueco por
/// debajo de tres líneas de log, más espacio muerto que lo escrito (M3). Con
/// 120 las tres líneas caben con un poco de aire y el resto sigue siendo del
/// atlas; quien necesite más lo estira con el ratón, y quien no lo pliega.
const BOTTOM_OPEN_HEIGHT: f32 = 120.0;
/// Ancho (px) por debajo del cual el dock de Ajustes se pliega solo.
///
/// Con los dos paneles laterales (250 + 330) por debajo de este umbral el
/// lienzo se queda en menos del 45 % de la ventana, que es justo lo que
/// pide el objetivo de área. Sigue a mano con `F9` o con el menú «Ver»:
/// el pliegue ocurre al *cruzar* el umbral, no en cada frame, para que
/// esa tecla pueda devolverlo con la ventana todavía estrecha.
const ANCHO_DOCK_AJUSTES: f32 = 1100.0;

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
    /// Texto del buscador de Ajustes: estrecha el panel a las secciones
    /// que coinciden (review UI/UX I3).
    settings_filter: String,
    /// Dock de Ajustes a la vista (estado de sesión, arranca abierto):
    /// lo giran la casilla del menú «Ver» y `F9`. Es la intención del
    /// usuario, no el estado pintado —ver `settings_estrecho_antes`—.
    show_settings: bool,
    /// Si en el frame anterior la ventana ya estaba por debajo del umbral
    /// de pliegue ([`ANCHO_DOCK_AJUSTES`]). El dock se pliega sólo al
    /// *cruzar* el umbral, para que `F9` lo pueda devolver aunque la
    /// ventana siga estrecha (Fase 4).
    settings_estrecho_antes: bool,
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
    /// Ventana «Acerca de» abierta (menú de app; review UI/UX M5).
    show_about: bool,
    bottom_tab: BottomTab,
    logs: Vec<LogEntry>,
    /// Aviso in-situ que se está enseñando junto al último gesto (review
    /// UI/UX I4): se apaga solo. Véase el módulo [`aviso`].
    aviso: Option<aviso::Aviso>,
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
    /// `Ctrl+F` pide el foco para el filtro; el panel lo consume al pintar
    /// su campo, que es donde ese widget existe (Fase 6 del rediseño).
    tree_filter_focus: bool,
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
    /// Motivo del último intento de vista previa que falló (`None` = sin
    /// fallo pendiente). El lienzo lo enseña con «Reintentar» para no
    /// quedarse en «Preparando…» con el spinner girando (C4).
    preview_error: Option<String>,
    /// Instantáneas de la configuración previa a cada operación
    /// deshacible —reset, posiciones manuales, sprites quitados— para
    /// `Ctrl+Z` (C6). El tope lo pone [`deshacer::MAXIMO`].
    deshacer: Vec<ProjectConfig>,
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

        menubar::menubar(self, ctx);
        toolbar::toolbar(self, ctx);

        // Barra de estado (abajo del todo, declarada primero): datos del
        // atlas actual. Responde a «¿dónde están mis datos?» sin robar
        // altura a la vista.
        egui::TopBottomPanel::bottom("status_bar")
            .resizable(false)
            .exact_height(22.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Los ficheros de entrada van delante: es el recuento del
                    // panel izquierdo, y con él se entiende por qué el de
                    // sprites de la derecha es menor (M2).
                    let ficheros = self.input_file_count();
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
                                "{} ficheros · {} sprite(s) · {} alias(es)",
                                ficheros,
                                out.result.total_sprites,
                                out.result.alias_count
                            ))
                            .weak(),
                        );
                    } else {
                        ui.label(egui::RichText::new(t!("Sin atlas — añade sprites")).weak());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match &self.project_path {
                            // El hueco de la derecha es donde va lo del
                            // título: la ruta del proyecto, con lo que quepa
                            // y el ratón encima para enseñarla entera (L5).
                            Some(ruta) => {
                                let texto = ruta.display().to_string();
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&texto).weak()).truncate(),
                                )
                                .on_hover_text(texto)
                            }
                            None => ui.label(
                                egui::RichText::new(t!("proyecto sin guardar — Ctrl+S"))
                                    .weak()
                                    .italics(),
                            ),
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

        // Un rol de superficie por zona (fase 2 del rediseño): los docks
        // con el color «panel», el lienzo con el suyo —más extremo, es la
        // zona de trabajo— y las barras con el «chrome» que les deja egui
        // en `panel_fill`. Antes la diferencia era de 6 unidades de gris,
        // invisible: ahora cada zona se reconoce sin necesidad de texto.
        let superficies = superficies(ctx.style().visuals.dark_mode);

        egui::SidePanel::left("sprites_panel")
            .resizable(true)
            .default_width(250.0)
            .min_width(180.0)
            .frame(egui::Frame::default().fill(superficies.panel))
            .show(ctx, |ui| sprites_panel::sprites_ui(self, ui));

        // El dock de Ajustes sólo se pinta si está pedido: la casilla del
        // menú «Ver» y `F9` lo giran, y por debajo del umbral se pliega
        // solo para que el lienzo no quede en una tira. El pliegue ocurre
        // al *cruzar* el umbral, así que `F9` lo devuelve aunque la
        // ventana siga estrecha (Fase 4 del rediseño).
        let estrecho = ctx.content_rect().width() < ANCHO_DOCK_AJUSTES;
        if estrecho && !self.settings_estrecho_antes && self.show_settings {
            self.show_settings = false;
        }
        self.settings_estrecho_antes = estrecho;

        if self.show_settings {
            egui::SidePanel::right("settings_panel")
                .resizable(true)
                .default_width(330.0)
                .min_width(260.0)
                .frame(egui::Frame::default().fill(superficies.panel))
                .show(ctx, |ui| settings::settings_ui(self, ui));
        }

        // El marco de `Frame::central_panel` con su margen de 8 px dejaba
        // una tira de lienzo entre la barra de zoom y el panel inferior: la
        // banda medía 39 px en vez de los 32 del diseño (Fase 5). Se quita
        // el margen; el relleno sigue siendo el token del lienzo, así que
        // el color de la zona no cambia, sólo dónde empieza su contenido.
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(superficies.lienzo))
            .show(ctx, |ui| preview::preview_ui(self, ui));

        sprite_settings::sprite_settings_window(self, ctx);
        animation::animation_window(self, ctx);
        split_sheet::split_window(self, ctx);
        shortcuts::shortcuts_window(self, ctx);
        menubar::about_window(self, ctx);

        // El diálogo de cierre puede haber guardado el proyecto, así que la
        // huella se vuelve a mirar justo antes de componer el título.
        self.exit_dialog(ctx);
        dirty = self.is_dirty();

        // El título dice qué documento está abierto: el nombre del fichero,
        // no la ruta entera. Con un proyecto real la ruta larga se ponía a
        // scroll en la barra de título y dejaba de decir nada (L5); la ruta
        // vive ahora en la tira de estado, con su tooltip. El punto delante
        // avisa de que hay cambios sin guardar (C2).
        let base = match &self.project_path {
            Some(p) => format!("{} — TexturePacker-RS", nombre_del_fichero(p)),
            None => "TexturePacker-RS".to_string(),
        };
        let title = if dirty { format!("• {base}") } else { base };
        if self.last_title != title {
            self.last_title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }

        // El aviso in-situ va el último: así queda por encima de todo lo
        // demás y ve el frame completo (reloj, puntero y repintado).
        aviso::pintar(ctx, &mut self.aviso);
    }
}

/// Nombre del fichero de un proyecto, para el título de la ventana: es lo
/// único que cabe y lo único que dice qué documento está abierto. Una ruta
/// sin nombre (un directorio recién creado) no puede dejar el título vacío,
/// así que en ese caso se enseña la ruta tal cual.
fn nombre_del_fichero(ruta: &std::path::Path) -> String {
    ruta.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| ruta.display().to_string())
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

/// Fase 6 (L5): el título dice qué documento está abierto —el nombre del
/// fichero— y la ruta entera vive en la tira de estado, donde cabe y donde
/// se puede mirar sin que la barra de título se ponga a scroll.
#[cfg(test)]
mod titulo_tests {
    use super::*;
    use crate::testing::{idle_input, texto_pintado};

    fn titulos(out: &egui::FullOutput) -> Vec<String> {
        out.viewport_output
            .values()
            .flat_map(|v| v.commands.iter())
            .filter_map(|c| match c {
                egui::ViewportCommand::Title(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn el_titulo_dice_el_fichero_y_la_ruta_se_queda_en_la_tira() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        app.project_path = Some(PathBuf::from("/home/edu/proyectos/evid.tpproj"));

        let salida = app.run_frame(&ctx, idle_input());
        let dados = titulos(&salida);
        assert!(
            dados.contains(&"evid.tpproj — TexturePacker-RS".to_string()),
            "el título enseña el fichero abierto: {dados:?}"
        );
        assert!(
            !dados.iter().any(|t| t.contains("/home/edu")),
            "la ruta entera no debe ir en el título: {dados:?}"
        );

        let texto = texto_pintado(&salida);
        assert!(
            texto.contains("/home/edu/proyectos/evid.tpproj"),
            "la ruta entera vive en la tira: {texto}"
        );

        // Con cambios sin guardar, el punto va delante del nombre: es lo
        // único que avisa en la barra de título (C2).
        app.config.padding = 5;
        let dados = titulos(&app.run_frame(&ctx, idle_input()));
        assert!(
            dados.contains(&"• evid.tpproj — TexturePacker-RS".to_string()),
            "el punto de cambios sin guardar va delante: {dados:?}"
        );
    }

    #[test]
    fn sin_proyecto_el_titulo_no_inventa_documento() {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);

        let salida = app.run_frame(&ctx, idle_input());
        let dados = titulos(&salida);
        assert!(
            dados.contains(&"TexturePacker-RS".to_string()),
            "sin proyecto sólo está el nombre del programa: {dados:?}"
        );
        let texto = texto_pintado(&salida);
        assert!(
            texto.contains("proyecto sin guardar — Ctrl+S"),
            "la tira recuerda que no hay dónde guardar: {texto}"
        );
    }
}
