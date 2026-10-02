//! Preferencias de interfaz (idioma y tema).
//!
//! Son propias del **usuario**, no del proyecto: se guardan en
//! `~/.config/texturepacker-rs/ui.toml` (o en `XDG_CONFIG_HOME`) para que
//! acompañen a la app en cualquier `.tpproj`. El fichero se tolera: si no
//! existe o está escrito a mano y no cuadra, se arranca con los valores por
//! defecto (sistema, sistema).

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

/// Contenido del fichero `ui.toml`. Los campos son texto corto para que un
/// fichero editado a mano no pueda tumbar el arranque.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    /// `system` | `es` | `en`
    pub lang: String,
    /// `system` | `light` | `dark`
    pub theme: String,
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

    impl UiPrefs {
        fn from_for_test(raw: &str) -> Self {
            Self {
                lang: raw.to_string(),
                theme: String::new(),
            }
        }
    }
}
