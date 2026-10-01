//! Idioma de la interfaz.
//!
//! El español es el idioma de origen del proyecto, así que vive en el propio
//! código: el literal que pasa a [`t`] **es** la cadena en español. Si el
//! idioma activo es inglés, la traducción se busca en la tabla de
//! [`en`](super::i18n::en); si no está, se devuelve el original (mejor
//! español que un hueco).
//!
//! La elección del usuario («sistema», «español» o «inglés») se guarda en
//! [`crate::ui_prefs`] y se aplica aquí con [`set_choice`]; el resto de la UI
//! sólo necesita `use crate::i18n::t;` y `t!("…")`.

use std::sync::atomic::{AtomicU8, Ordering};

pub(crate) mod en;

/// Idioma efectivo: el que se pinta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
}

/// Elección persistida del usuario, que puede ser «seguir al sistema».
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LangChoice {
    /// Idioma del equipo (variables `LC_ALL`/`LC_MESSAGES`/`LANG`).
    #[default]
    System,
    Es,
    En,
}

impl LangChoice {
    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Es => "es",
            Self::En => "en",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "es" => Self::Es,
            "en" => Self::En,
            _ => Self::System,
        }
    }
}

impl Lang {
    const fn code(self) -> u8 {
        match self {
            Self::Es => 0,
            Self::En => 1,
        }
    }

    /// Traduce `es` a este idioma (independiente del estado global).
    pub fn translate(self, es: &str) -> &str {
        match self {
            Self::Es => es,
            Self::En => en::lookup(es).unwrap_or(es),
        }
    }
}

/// Elección vigente (lo que pintó el selector de Ajustes).
static CHOICE: AtomicU8 = AtomicU8::new(LangChoice::System as u8);
/// Idioma efectivo, ya resuelto: lo leen [`t`] en cada cadena.
static LANG: AtomicU8 = AtomicU8::new(0);

/// Idioma en el que se pinta la UI ahora mismo.
pub fn lang() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::Es,
    }
}

/// Cambia el idioma y (si toca) resuelve el del sistema.
pub fn set_choice(choice: LangChoice) {
    let code = match choice {
        LangChoice::Es => LangChoice::Es as u8,
        LangChoice::En => LangChoice::En as u8,
        LangChoice::System => LangChoice::System as u8,
    };
    CHOICE.store(code, Ordering::Relaxed);
    let effective = match choice {
        LangChoice::System => detect(|key| std::env::var(key)),
        LangChoice::Es => Lang::Es,
        LangChoice::En => Lang::En,
    };
    LANG.store(effective.code(), Ordering::Relaxed);
}

/// Idioma del equipo a partir de las variables de locale.
///
/// `es*` → español, `en*` → inglés, cualquier otro locale ya instalado
/// (fr, de, pt…) → inglés, que es el idioma internacional por el que se
/// traduce el software; sin locale (`C`, `POSIX`, vacío) → español, que es
/// la lengua de origen del proyecto y no hay información que mande sobre
/// ella.
pub fn detect(var: impl Fn(&str) -> Result<String, std::env::VarError>) -> Lang {
    let value: String = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| var(k).ok())
        .unwrap_or_default();
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() || value == "c" || value.starts_with("posix") {
        return Lang::Es;
    }
    if value.starts_with("es") {
        Lang::Es
    } else {
        Lang::En
    }
}

/// Traduce `es` al idioma activo.
pub fn translate(es: &str) -> &str {
    lang().translate(es)
}

/// Traduce `es` a `lang`, sin tocar el estado global (para tests).
#[cfg(test)]
pub fn translate_in(lang: Lang, es: &str) -> &str {
    lang.translate(es)
}

/// `t!("Ajustes")` → «Ajustes» en español, «Settings» en inglés.
macro_rules! t {
    ($es:literal) => {
        $crate::i18n::translate($es)
    };
}
pub(crate) use t;

/// Claves pasadas a `t!` en un fichero de origen.
#[cfg(test)]
fn t_keys_in(src: &str) -> Vec<String> {
    let chars: Vec<char> = src.chars().collect();
    let mut keys = Vec::new();
    let mut i = 0;
    while i + 2 < chars.len() {
        // «t!(» con `t` suelto: no vale dentro de `format!(`, `split!(`, …
        let prev = if i > 0 { chars[i - 1] } else { '\0' };
        let es_t_macro = prev != '_' && !prev.is_alphanumeric() && prev != '!';
        if es_t_macro && chars[i] == 't' && chars[i + 1] == '!' && chars[i + 2] == '(' {
            let mut j = i + 3;
            while j < chars.len() && (chars[j] == ' ' || chars[j] == '\n') {
                j += 1;
            }
            if chars.get(j) != Some(&'"') {
                i += 3;
                continue;
            }
            while j < chars.len() && chars[j] != '"' {
                j += 1;
            }
            let mut key = String::new();
            let mut closed = false;
            j += 1;
            while j < chars.len() {
                match chars[j] {
                    '\\' => {
                        if let Some(n) = chars.get(j + 1) {
                            key.push(*n);
                            j += 2;
                            continue;
                        }
                        j += 1;
                    }
                    '"' => {
                        closed = true;
                        j += 1;
                        break;
                    }
                    c => {
                        key.push(c);
                        j += 1;
                    }
                }
            }
            if closed {
                keys.push(key);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    keys
}

/// Todas las claves `t!("…")` de `tp-app/src` (los binarios no pinta la UI).
#[cfg(test)]
fn used_keys() -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    crate::glyph_guard::walk(&root, &mut files);
    let mut keys = Vec::new();
    for file in files {
        if file.components().any(|c| c.as_os_str() == "bin") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&file) else {
            continue;
        };
        // Los comentarios (y los doc-comentarios) pueden citar `t!("…")` como
        // ejemplo: no son claves reales.
        let codigo: String = src
            .lines()
            .filter(|linea| !linea.trim_start().starts_with("//"))
            .map(|linea| format!("{linea}\n"))
            .collect();
        keys.extend(t_keys_in(&codigo));
    }
    keys
}

/// Placeholders `{…}` de una cadena, para comprobar que la traducción no se
/// come argumentos de formato.
#[cfg(test)]
fn placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            break;
        };
        out.push(after[..end].to_string());
        rest = &after[end + 1..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_clave_en_uso_tiene_traduccion() {
        let mut sin_traducir = Vec::new();
        for key in used_keys() {
            if en::lookup(&key).is_none() {
                sin_traducir.push(key);
            }
        }
        sin_traducir.sort();
        sin_traducir.dedup();
        assert!(
            sin_traducir.is_empty(),
            "claves de t!(…) sin entrada en i18n/en.rs:\n  {}",
            sin_traducir.join("\n  ")
        );
    }

    #[test]
    fn la_tabla_no_tiene_claves_sobrantes_ni_duplicadas() {
        let usadas: std::collections::HashSet<String> = used_keys().into_iter().collect();
        let mut vistas = std::collections::HashSet::new();
        let mut sobrantes = Vec::new();
        for (es, en) in en::EN {
            assert!(!es.is_empty() && !en.is_empty(), "entrada vacía: {es:?}");
            assert!(vistas.insert(*es), "clave duplicada: {es}");
            if !usadas.contains(*es) {
                sobrantes.push(*es);
            }
        }
        sobrantes.sort();
        assert!(
            sobrantes.is_empty(),
            "claves en en.rs que ya no se usan:\n  {}",
            sobrantes.join("\n  ")
        );
    }

    #[test]
    fn la_traduccion_conserve_los_placeholders() {
        let mut malas = Vec::new();
        for (es, en) in en::EN {
            if placeholders(es) != placeholders(en) {
                malas.push(format!("{es} / {en}"));
            }
        }
        assert!(
            malas.is_empty(),
            "las traducciones pierden o añaden {{…}}:\n  {}",
            malas.join("\n  ")
        );
    }

    #[test]
    fn lo_que_no_esta_traducido_vuelve_al_espanol() {
        assert_eq!(
            translate_in(Lang::En, "clave sin traducir"),
            "clave sin traducir"
        );
        assert_eq!(translate_in(Lang::Es, "whatever"), "whatever");
    }

    #[test]
    fn el_idioma_del_sistema_se_interpreta_asi() {
        let var = |value: &'static str| {
            move |_: &str| -> Result<String, std::env::VarError> { Ok(value.to_string()) }
        };
        assert_eq!(detect(var("es_MX.UTF-8")), Lang::Es);
        assert_eq!(detect(var("en_US.UTF-8")), Lang::En);
        assert_eq!(detect(var("fr_FR.UTF-8")), Lang::En);
        assert_eq!(detect(var("C")), Lang::Es);
        assert_eq!(detect(|_| Err(std::env::VarError::NotPresent)), Lang::Es);
    }

    #[test]
    fn la_eleccion_del_usuario_se_guarda_como_texto_corto() {
        for choice in [LangChoice::System, LangChoice::Es, LangChoice::En] {
            assert_eq!(LangChoice::from_id(choice.id()), choice);
        }
        assert_eq!(LangChoice::from_id("otra-cosa"), LangChoice::System);
    }
}
