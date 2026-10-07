//! Preferencias de interfaz (idioma, tema y tamaño de la letra).
//!
//! Son propias del **usuario**, no del proyecto: se guardan en
//! `~/.config/texturepacker-rs/ui.toml` (o en `XDG_CONFIG_HOME`) para que
//! acompañen a la app en cualquier `.tpproj`. El fichero se tolera: si no
//! existe o está escrito a mano y no cuadra, se arranca con los valores por
//! defecto (sistema, sistema, normal).

use crate::i18n::LangChoice;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Tema de la ventana.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    /// El que use el sistema operativo.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }

    /// Pinta la ventana con este tema (inmediato, sin reiniciar).
    pub fn apply(self, ctx: &eframe::egui::Context) {
        let preference = match self {
            Self::System => eframe::egui::ThemePreference::System,
            Self::Light => eframe::egui::ThemePreference::Light,
            Self::Dark => eframe::egui::ThemePreference::Dark,
        };
        ctx.set_theme(preference);
    }
}

/// Tamaño de la letra de la interfaz: un tanto por uno sobre los tamaños
/// que trae egui. Es preferencia del usuario —igual que el idioma y el
/// tema—, así que viaja en el mismo `ui.toml` y acompaña a la app en
/// cualquier `.tpproj`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontScale {
    /// El 90 %: para pantallas pequeñas o mirada cansada.
    Small,
    /// Los tamaños de egui, sin tocar.
    #[default]
    Normal,
    /// Un 20 % más.
    Large,
    /// Un 40 % más: la UI se aprieta, pero se lee.
    Larger,
}

impl FontScale {
    pub fn id(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Normal => "normal",
            Self::Large => "large",
            Self::Larger => "larger",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "small" => Self::Small,
            "large" => Self::Large,
            "larger" => Self::Larger,
            _ => Self::Normal,
        }
    }

    /// Tanto por uno sobre los tamaños de egui.
    pub fn factor(self) -> f32 {
        match self {
            Self::Small => 0.9,
            Self::Normal => 1.0,
            Self::Large => 1.2,
            Self::Larger => 1.4,
        }
    }

    /// Pinta la escala en los estilos de texto de egui —los dos temas a la
    /// vez— y la deja guardada para los tamaños escritos a mano. La base se
    /// recupera dividiendo por la escala anterior: repetir la misma
    /// elección no la acumula y volver a «Normal» deja exactamente los
    /// tamaños que trae egui.
    pub fn apply(self, ctx: &eframe::egui::Context) {
        let factor = self.factor();
        let anterior = escala_actual(ctx);
        ctx.all_styles_mut(|style| {
            for fuente in style.text_styles.values_mut() {
                fuente.size = (fuente.size / anterior * factor).max(6.0);
            }
        });
        ctx.memory_mut(|m| m.data.insert_temp(egui_id(), factor));
    }
}

/// Escala de letra vigente (1,0 mientras nadie haya tocado nada).
pub fn escala_actual(ctx: &eframe::egui::Context) -> f32 {
    ctx.memory(|m| m.data.get_temp::<f32>(egui_id()))
        .unwrap_or(1.0)
}

/// Tamaño de letra con la escala del usuario aplicada: los tamaños que el
/// código fija a mano pasan por aquí para crecer y menguar junto al resto
/// de la interfaz en vez de quedarse clavados.
pub fn font_id(base: f32, ctx: &eframe::egui::Context) -> eframe::egui::FontId {
    eframe::egui::FontId::proportional(base * escala_actual(ctx))
}

/// Identidad de la escala dentro de la memoria de egui.
fn egui_id() -> eframe::egui::Id {
    eframe::egui::Id::new("tp_font_scale")
}

/// Contenido del fichero `ui.toml`. Los campos son texto corto para que un
/// fichero editado a mano no pueda tumbar el arranque.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    /// `system` | `es` | `en`
    pub lang: String,
    /// `system` | `light` | `dark`
    pub theme: String,
    /// `small` | `normal` | `large` | `larger`
    pub font_scale: String,
}

impl UiPrefs {
    /// Elección de idioma guardada (o la del sistema, si no la hay).
    pub fn lang_choice(&self) -> LangChoice {
        LangChoice::from_id(&self.lang)
    }

    /// Tema guardado (o el del sistema, si no está).
    pub fn theme(&self) -> Theme {
        Theme::from_id(&self.theme)
    }

    /// Tamaño de letra guardado (o el normal, si no está).
    pub fn font_scale(&self) -> FontScale {
        FontScale::from_id(&self.font_scale)
    }

    /// Ruta del fichero de preferencias del usuario.
    pub fn path() -> PathBuf {
        tp_core::keys::config_dir().join("ui.toml")
    }

    /// Lee el fichero; cualquier problema devuelve las preferencias por
    /// defecto en lugar de fallar el arranque.
    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    /// Escribe el fichero creando el directorio si hace falta.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).unwrap_or_default();
        std::fs::write(path, text)
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        // Contador del proceso, no por llamada: si cada `temp()` empezaba
        // en 0, los tests que lo usan compartían ruta y se pisan (uno deja
        // el toml ilegible y el otro lee los valores por defecto).
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("tp-app-ui-prefs-{}-{n}.toml", std::process::id()))
    }

    #[test]
    fn sin_fichero_se_arranca_con_los_valores_por_defecto() {
        let prefs = UiPrefs::load_from(Path::new("/no/existe/ui.toml"));
        assert_eq!(prefs, UiPrefs::default());
        assert_eq!(prefs.lang_choice(), LangChoice::System);
        assert_eq!(prefs.theme(), Theme::System);
    }

    #[test]
    fn el_fichero_se_escribe_y_se_vuelve_a_leer() {
        let path = temp();
        let prefs = UiPrefs {
            lang: "en".to_string(),
            theme: "dark".to_string(),
            ..Default::default()
        };
        prefs.save_to(&path).expect("escribe ui.toml");
        let vuelta = UiPrefs::load_from(&path);
        assert_eq!(vuelta.lang_choice(), LangChoice::En);
        assert_eq!(vuelta.theme(), Theme::Dark);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn un_fichero_mal_escrito_no_tumba_la_app() {
        let path = temp();
        std::fs::write(&path, "esto no es toml ][ scarcity").ok();
        assert_eq!(UiPrefs::load_from(&path), UiPrefs::default());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn campos_desconocidos_caen_en_sistema() {
        assert_eq!(
            UiPrefs::from_for_test("???").lang_choice(),
            LangChoice::System
        );
        assert_eq!(Theme::from_id("system"), Theme::System);
        assert_eq!(Theme::from_id("oscuro"), Theme::System);
    }

    /// I6: el tamaño de letra viaja en el mismo `ui.toml` que el idioma y
    /// el tema; lo que no se entiende vuelve a la normal, que es lo que
    /// ocurre con un fichero escrito antes de que existiera el campo.
    #[test]
    fn el_tamano_de_letra_se_guarda_y_vuelve_a_leer() {
        let path = temp();
        let prefs = UiPrefs {
            font_scale: FontScale::Large.id().to_string(),
            ..Default::default()
        };
        prefs.save_to(&path).expect("escribe ui.toml");
        assert_eq!(UiPrefs::load_from(&path).font_scale(), FontScale::Large);
        std::fs::remove_file(&path).ok();

        assert_eq!(UiPrefs::default().font_scale(), FontScale::Normal);
        assert_eq!(FontScale::from_id("grande"), FontScale::Normal);
        assert_eq!(FontScale::from_id("larger"), FontScale::Larger);
        assert_eq!(FontScale::Larger.factor() / FontScale::Normal.factor(), 1.4);
    }

    /// Un `ui.toml` de antes —o uno escrito a mano sin el campo— sigue
    /// valiendo: la letra arranca normal y nada se rompe al leerlo.
    #[test]
    fn un_fichero_sin_tamano_de_letra_cae_en_normal() {
        let path = temp();
        std::fs::write(&path, "lang = \"en\"\ntheme = \"dark\"\n").ok();
        let prefs = UiPrefs::load_from(&path);
        assert_eq!(prefs.theme(), Theme::Dark);
        assert_eq!(prefs.font_scale(), FontScale::Normal);
        std::fs::remove_file(&path).ok();
    }

    /// I6: aplicar la escala no la acumula —si lo hiciera, volver a elegir
    /// el tamaño en Ajustes lo multiplicaría otra vez— y volver a
    /// «Normal» deja exactamente los tamaños que trae egui.
    #[test]
    fn la_escala_no_se_acumula_y_normal_vuelve_a_egui() {
        let ctx = eframe::egui::Context::default();
        let base = cuerpo(&ctx);
        assert!(base > 0.0, "egui trae un tamaño de letra por defecto");

        FontScale::Large.apply(&ctx);
        let grande = cuerpo(&ctx);
        assert!(
            (grande - base * FontScale::Large.factor()).abs() < 0.01,
            "«Grande» debía ser {base} × 1,2 y es {grande}"
        );

        FontScale::Large.apply(&ctx);
        assert_eq!(
            cuerpo(&ctx),
            grande,
            "aplicar dos veces la misma escala no debe acumular"
        );

        FontScale::Normal.apply(&ctx);
        assert!(
            (cuerpo(&ctx) - base).abs() < 0.01,
            "volver a «Normal» debe dejar los tamaños de egui"
        );
    }

    /// I6: elegir un tamaño en Ajustes cambia la escala al vuelo —también
    /// los tamaños escritos a mano— y lo deja escrito en las prefs.
    #[test]
    fn elegir_un_tamano_lo_aplica_al_vuelo() {
        let ctx = eframe::egui::Context::default();
        let mut app = crate::app::App::new_for_testing(ctx.clone(), None);
        let base = cuerpo(&ctx);

        app.set_font_scale(FontScale::Larger);

        assert_eq!(app.prefs().font_scale(), FontScale::Larger);
        assert_eq!(escala_actual(&ctx), FontScale::Larger.factor());
        assert!(
            (cuerpo(&ctx) - base * FontScale::Larger.factor()).abs() < 0.01,
            "el tamaño de la UI debe seguir a la escala elegida"
        );
    }

    /// I6: un `FontId` con el número escrito a mano no crece con la escala
    /// del usuario, que era justo el defecto: los 9 px del lienzo se
    /// quedaban en 9 px con la letra grande. Todo tamaño pasa por
    /// `font_id`, y esta prueba lo vigila en todas las fuentes.
    #[test]
    fn nadie_fija_un_tamano_de_letra_a_mano() {
        let raiz = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut culpables = Vec::new();
        for ruta in fuentes(&raiz) {
            let src = std::fs::read_to_string(&ruta).expect("fuente legible");
            for marca in ["FontId::proportional(", "FontId::new("] {
                let mut desde = 0;
                while let Some(p) = src[desde..].find(marca) {
                    let ini = desde + p + marca.len();
                    if src[ini..]
                        .trim_start()
                        .starts_with(|c: char| c.is_ascii_digit())
                    {
                        let linea = src[..ini].matches('\n').count() + 1;
                        let nombre = ruta.file_name().unwrap_or_default().to_string_lossy();
                        culpables.push(format!("{nombre}:{linea}"));
                    }
                    desde = ini;
                }
            }
        }
        assert!(
            culpables.is_empty(),
            "tamaños de letra fijados a mano (usa `ui_prefs::font_id`):\n  {}",
            culpables.join("\n  ")
        );
    }

    /// Tamaño de letra del cuerpo de texto, para comparar escalas.
    fn cuerpo(ctx: &eframe::egui::Context) -> f32 {
        ctx.style()
            .text_styles
            .get(&eframe::egui::TextStyle::Body)
            .map(|f| f.size)
            .unwrap_or_default()
    }

    /// Todas las fuentes `.rs` de la app, en cualquier subdirectorio.
    fn fuentes(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut fuera = Vec::new();
        let Ok(entradas) = std::fs::read_dir(dir) else {
            return fuera;
        };
        for entrada in entradas.flatten() {
            let ruta = entrada.path();
            if ruta.is_dir() {
                fuera.extend(fuentes(&ruta));
            } else if ruta.extension().is_some_and(|ext| ext == "rs") {
                fuera.push(ruta);
            }
        }
        fuera
    }

    impl UiPrefs {
        fn from_for_test(raw: &str) -> Self {
            Self {
                lang: raw.to_string(),
                ..Default::default()
            }
        }
    }
}
