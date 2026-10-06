//! App state: boot, interface preferences, getters and the log mirror.

use super::{
    animation, apply_theme, split_sheet, App, BottomTab, LogEntry, LogKind, BOTTOM_OPEN_HEIGHT,
};
use crate::i18n::t;
use eframe::egui;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tp_core::config::ProjectConfig;
use tp_core::pipeline::PipelineOutput;

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
    pub(super) fn build(egui_ctx: egui::Context, initial_project: Option<PathBuf>) -> Self {
        Self::build_with_prefs(egui_ctx, initial_project, crate::ui_prefs::UiPrefs::load())
    }

    /// Igual que [`build`] pero con las preferencias que se le den: así los
    /// tests no leen el `ui.toml` real ni exponen la suite a la elección de
    /// idioma de la máquina que la lanza.
    pub(super) fn build_with_prefs(
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
            auto_fit: true,
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
            settings_filter: String::new(),
            show_sprite_settings: false,
            show_animation: false,
            anim: animation::AnimState::default(),
            show_split: false,
            split: split_sheet::SplitState::default(),
            show_shortcuts: false,
            show_about: false,
            bottom_tab: BottomTab::Log,
            logs: Vec::new(),
            aviso: None,
            unreadable_dirs: Vec::new(),
            project_path: None,
            saved_config: String::new(),
            exit_pending: false,
            exit_confirmed: false,
            tree_filter: String::new(),
            tree_filter_focused: false,
            tree_force_open: None,
            pending: None,
            change_seq: 0,
            pending_seq: None,
            packed_snapshot: None,
            pending_force: false,
            // `None` = sin muestrear: el primer frame de `poll_changes` toma
            // la huella sin notificar (ver `App::config_fingerprint`).
            config_fingerprint: None,
            tree_cache: None,
            watcher: None,
            egui_ctx: cc.egui_ctx.clone(),
            last_snapshot_poll: std::time::Instant::now(),
            preview_stale: false,
            preview_error: None,
            deshacer: Vec::new(),
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
        // Huella de «sin guardar»: al arrancar la configuración en memoria es
        // exactamente la que hay en disco (la por defecto, todavía sin tocar).
        app.saved_config = app.config.to_toml().unwrap_or_default();
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
    pub(super) fn persist_prefs(&mut self) {
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

    pub(super) fn log(&mut self, kind: LogKind, text: String) {
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

    pub(super) fn sync_paths(&mut self) {
        self.input_dir_text = self.config.input_directory.display().to_string();
        self.output_dir_text = self.config.output_directory.display().to_string();
    }

    pub(super) fn sync_variants(&mut self) {
        self.variants_text = self
            .config
            .scale_variants
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ");
    }

    /// Keep the text fields in sync with the config before running/saving.
    pub(super) fn commit_paths(&mut self) {
        self.config.input_directory = PathBuf::from(self.input_dir_text.trim());
        self.config.output_directory = PathBuf::from(self.output_dir_text.trim());
        // Escribe en `config`: repon la huella para que el sondeo por frame
        // no vuelva a notificar este mismo cambio (ver `config_fingerprint`).
        self.refresh_config_fingerprint();
    }

    /// Deja `config_fingerprint` reflejando el estado actual. Toda función
    /// que escriba en `config` sin pasar por `on_config_changed` debe
    /// llamarla al terminar (`commit_paths`, `parse_variants`).
    pub(super) fn refresh_config_fingerprint(&mut self) {
        self.config_fingerprint = Some(format!("{:?}", self.config));
    }
}
