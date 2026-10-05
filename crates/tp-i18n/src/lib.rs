//! Textos de la interfaz en los dos idiomas.
//!
//! El español es la lengua de origen del proyecto, así que vive en el propio
//! código: el literal que pasa a [`t`] **es** la cadena en español. Si el
//! idioma activo es inglés, la traducción se busca en la tabla [`en`]; si no
//! está, se devuelve el original (mejor español que un hueco).
//!
//! Este crate es el que comparten la app de escritorio y el CLI: ambos
//! resuelven el idioma con [`detect`] (variables `LC_ALL`/`LC_MESSAGES`/`LANG`)
//! y sólo necesitan `use tp_i18n::t;` para escribir `t!("…")`. La app además
//! persiste la elección del usuario con [`set_choice`].
//!
//! El motor (`tp-core`) devuelve siempre español, así que los mensajes que
//! llegan del proceso de empaquetado se traducen en el momento de mostrarlos
//! con [`tr`], que admite tanto la cadena entera como su patrón con huecos
//! `{}` (para cuando el mensaje ya venía formateado con argumentos).

use std::sync::atomic::{AtomicU8, Ordering};

pub mod en;
pub mod en_core;

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
/// `es*` → español, que es la lengua de origen del proyecto. Todo lo demás
/// → inglés: cualquier otro locale ya instalado (fr, de, pt…) y también
/// `C`, `POSIX` y el vacío, que es el locale por defecto de Unix, el que
/// fijan los runners de CI y el que un script toma cuando escribe
/// `LC_ALL=C`. Es la misma resolución que GNU coreutils: un mensaje
/// siempre disponible en inglés es lo que deja greppear la salida sin
/// depender de si hay traducciones instaladas.
pub fn detect(var: impl Fn(&str) -> Result<String, std::env::VarError>) -> Lang {
    let value: String = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| var(k).ok())
        .unwrap_or_default();
    let value = value.trim().to_ascii_lowercase();
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

/// Traduce `es` y sustituye sus `{}` por los argumentos, en orden.
///
/// No usa `format!` porque la plantilla traducida no es un literal en tiempo
/// de compilación; los marcadores con relleno o con nombre (`{:.2}`, `{n}`)
/// se dejan tal cual: las claves con formato de verdad no se traducen.
pub fn translate_with(es: &str, args: &[&dyn std::fmt::Display]) -> String {
    let plantilla = translate(es);
    let mut out = String::with_capacity(plantilla.len() + 16 * args.len());
    let mut rest = plantilla;
    let mut usados = 0;
    while let Some(ini) = rest.find('{') {
        let cuerpo = &rest[ini + 1..];
        let Some(fin) = cuerpo.find('}') else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..ini]);
        let spec = &cuerpo[..fin];
        if spec.is_empty() {
            match args.get(usados) {
                Some(arg) => {
                    out.push_str(&arg.to_string());
                    usados += 1;
                }
                None => out.push_str("{}"),
            }
        } else {
            out.push('{');
            out.push_str(spec);
            out.push('}');
        }
        rest = &cuerpo[fin + 1..];
    }
    out.push_str(rest);
    out
}

/// Traduce `es` a `lang`, sin tocar el estado global (para tests).
#[cfg(test)]
pub fn translate_in(lang: Lang, es: &str) -> &str {
    lang.translate(es)
}

/// Traduce un mensaje del motor (`tp-core`) al idioma activo.
///
/// `tp-core` escribe siempre en español y a menudo el mensaje llega ya
/// formateado dentro de otro (`«Empaquetado fallido: {…}»`), así que además
/// de la búsqueda exacta se prueba cada patrón con huecos `{}`: si encaja,
/// los argumentos recogidos se traducen a su vez (recursión acotada) y se
/// vuelven a insertar en la traducción. Lo que no encaja vuelve tal cual.
pub fn tr(msg: &str) -> String {
    tr_in(lang(), msg)
}

/// [`tr`] para un idioma concreto, sin tocar el estado global (para tests).
pub(crate) fn tr_in(lang: Lang, msg: &str) -> String {
    match lang {
        Lang::Es => msg.to_string(),
        Lang::En => tr_en(msg, 3),
    }
}

/// Lado inglés de [`tr`]: primero exacta, después patrón, y si no, el original.
fn tr_en(msg: &str, profundidad: u8) -> String {
    if let Some(en) = en::lookup(msg) {
        return en.to_string();
    }
    if let Some(en) = en_core::lookup(msg) {
        return en.to_string();
    }
    if profundidad == 0 {
        return msg.to_string();
    }
    for (es, en) in en::EN.iter().chain(en_core::EN_CORE.iter()) {
        if !es.contains('{') {
            continue;
        }
        if let Some(vuetecos) = match_pattern(es, msg) {
            let vuetecos: Vec<String> =
                vuetecos.iter().map(|v| tr_en(v, profundidad - 1)).collect();
            return substitute(en, &vuetecos);
        }
    }
    msg.to_string()
}

/// Trozo de una plantilla: texto literal o hueco `{}`.
enum Seg {
    Lit(String),
    Hole,
}

/// Trocea `patron` respetando `{{`/`}}` (llaves literales) y distinguiendo el
/// hueco `{}` de los marcadores con nombre o formato (`{n}`, `{:.2}`), que se
/// tratan como literal: esos mensajes no llevan argumentos en el texto.
fn tokenize(patron: &str) -> Vec<Seg> {
    let chars: Vec<char> = patron.chars().collect();
    let mut segs = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => {
                lit.push('{');
                i += 2;
            }
            '{' if chars.get(i + 1) == Some(&'}') => {
                if !lit.is_empty() {
                    segs.push(Seg::Lit(std::mem::take(&mut lit)));
                }
                segs.push(Seg::Hole);
                i += 2;
            }
            '{' => {
                // Marcador con nombre o formato: se copia tal cual.
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '}' {
                    j += 1;
                }
                if j < chars.len() {
                    lit.extend(&chars[i..=j]);
                    i = j + 1;
                } else {
                    lit.push('{');
                    i += 1;
                }
            }
            '}' if chars.get(i + 1) == Some(&'}') => {
                lit.push('}');
                i += 2;
            }
            c => {
                lit.push(c);
                i += 1;
            }
        }
    }
    if !lit.is_empty() {
        segs.push(Seg::Lit(lit));
    }
    segs
}

/// ¿Encaja `patron` (con huecos) en `texto`? Devuelve los huecos recogidos.
///
/// El primer literal se ancla al principio y el último al final; los de en
/// medio se buscan con la primera aparición. Sin huecos no encaja: esos
/// mensajes se resuelven con la búsqueda exacta.
fn match_pattern(patron: &str, texto: &str) -> Option<Vec<String>> {
    let segs = tokenize(patron);
    if !segs.iter().any(|s| matches!(s, Seg::Hole)) {
        return None;
    }
    let mut vuetecos = Vec::new();
    let mut pos = 0usize;
    let mut i = 0;
    if let Some(Seg::Lit(l)) = segs.first() {
        if !texto.starts_with(l.as_str()) {
            return None;
        }
        pos += l.len();
        i = 1;
    }
    while i < segs.len() {
        if !matches!(segs[i], Seg::Hole) {
            return None;
        }
        match segs.get(i + 1) {
            None => {
                vuetecos.push(texto[pos..].to_string());
                pos = texto.len();
                i += 1;
            }
            Some(Seg::Lit(l)) => {
                let resto = &texto[pos..];
                let found = if i + 2 == segs.len() {
                    // Último trozo: el literal tiene que cerrar el texto, y
                    // el índice se saca con `strip_suffix` en vez de
                    // restando longitudes. Restar bytes sobre un texto con
                    // acentos puede caer en medio de un carácter, y partir
                    // ahí era un panic («byte index not a char boundary»)
                    // que un mensaje con una «á» bastaba para disparar.
                    resto.strip_suffix(l.as_str()).map(str::len)?
                } else {
                    resto.find(l.as_str())?
                };
                vuetecos.push(resto[..found].to_string());
                pos += found + l.len();
                i += 2;
            }
            Some(Seg::Hole) => return None,
        }
    }
    (pos == texto.len()).then_some(vuetecos)
}

/// Escribe `plantilla` (inglés) insertando los huecos ya traducidos; las
/// llaves escapadas salen como llaves literales.
fn substitute(plantilla: &str, vuetecos: &[String]) -> String {
    let chars: Vec<char> = plantilla.chars().collect();
    let mut out = String::with_capacity(plantilla.len());
    let mut usados = 0;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => {
                out.push('{');
                i += 2;
            }
            '{' if chars.get(i + 1) == Some(&'}') => match vuetecos.get(usados) {
                Some(v) => {
                    out.push_str(v);
                    usados += 1;
                    i += 2;
                }
                None => {
                    out.push_str("{}");
                    i += 2;
                }
            },
            '{' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '}' {
                    j += 1;
                }
                if j < chars.len() {
                    out.extend(&chars[i..=j]);
                    i = j + 1;
                } else {
                    out.push('{');
                    i += 1;
                }
            }
            '}' if chars.get(i + 1) == Some(&'}') => {
                out.push('}');
                i += 2;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// `t!("Ajustes")` → «Ajustes» en español, «Settings» en inglés.
/// `t!("Página {}/{}", i, n)` → lo mismo con los argumentos de formato
/// (la traducción debe conservar los mismos `{…}`, lo comprueba un test).
#[macro_export]
macro_rules! t {
    ($es:literal) => {
        $crate::translate($es)
    };
    ($es:literal, $($arg:expr),* $(,)?) => {
        $crate::translate_with($es, &[$(&($arg) as &dyn ::std::fmt::Display),*])
    };
}

#[cfg(test)]
fn placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if chars.get(i + 1) == Some(&'{') {
                i += 2;
                continue;
            }
            if chars.get(i + 1) == Some(&'}') {
                out.push(String::new());
                i += 2;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '}' {
                j += 1;
            }
            if j >= chars.len() {
                break;
            }
            out.push(chars[i + 1..j].iter().collect());
            i = j + 1;
            continue;
        }
        if chars[i] == '}' && chars.get(i + 1) == Some(&'}') {
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn translate_with_sustituye_los_llenos() {
        // Clave sin traducción: el literal es el mismo en los dos idiomas,
        // así el test no depende del idioma global (los tests corren en paralelo).
        let fuera = translate_with("clave-que-no-existe {} {}", &[&1, &2]);
        assert_eq!(fuera, "clave-que-no-existe 1 2");
        // Un marcador con nombre no se toca: esas claves no se traducen.
        assert_eq!(translate_with("otra {n} {}", &[&7]), "otra {n} 7");
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
        assert_eq!(detect(var("es")), Lang::Es);
        assert_eq!(detect(var("en_US.UTF-8")), Lang::En);
        assert_eq!(detect(var("fr_FR.UTF-8")), Lang::En);
        // `C`, `POSIX` y el vacío son el locale por defecto de Unix: el que
        // fijan los runners de CI y el que un script toma con `LC_ALL=C`, y
        // en todo el software del sistema sus mensajes salen en inglés.
        assert_eq!(detect(var("C")), Lang::En);
        assert_eq!(detect(var("C.UTF-8")), Lang::En);
        assert_eq!(detect(var("c.utf8")), Lang::En);
        assert_eq!(detect(var("POSIX")), Lang::En);
        assert_eq!(detect(|_| Err(std::env::VarError::NotPresent)), Lang::En);
    }

    #[test]
    fn la_tabla_del_motor_no_tiene_claves_vacias_ni_duplicadas() {
        let mut vistas = std::collections::HashSet::new();
        for (es, en) in en_core::EN_CORE {
            assert!(!es.is_empty() && !en.is_empty(), "entrada vacía: {es:?}");
            assert!(vistas.insert(*es), "clave de tp-core duplicada: {es}");
        }
    }

    #[test]
    fn la_traduccion_del_motor_conserve_los_placeholders() {
        let mut malas = Vec::new();
        for (es, en) in en_core::EN_CORE {
            if placeholders(es) != placeholders(en) {
                malas.push(format!("{es} / {en}"));
            }
        }
        assert!(
            malas.is_empty(),
            "los mensajes del motor pierden o añaden {{…}}:\n  {}",
            malas.join("\n  ")
        );
    }

    #[test]
    fn tr_del_motor_traduce_entero_por_patron_y_por_argumentos() {
        // Mensaje completo: búsqueda exacta.
        assert_eq!(tr_in(Lang::En, "ASTC demasiado corto"), "ASTC too short");
        // Mensaje ya formateado: patrón con huecos.
        assert_eq!(
            tr_in(
                Lang::En,
                "Config inválida: max_texture_size debe ser una potencia de dos positiva \
                 (se obtuvo 3000)"
            ),
            "Invalid config: max_texture_size must be a positive power of two (got 3000)"
        );
        // Los argumentos recogidos también se traducen.
        assert_eq!(
            tr_in(Lang::En, "Error de E/S: El fichero no existe"),
            "I/O error: El fichero no existe"
        );
        // Las llaves literales ({n}) no son huecos y salen tal cual.
        let multipack = tr_in(
            Lang::En,
            "Multipack: 2 hojas generadas y el nombre base \"sheet\" no contiene {n} o {n1}; \
             se nombran con el sufijo _N (p. ej. sheet_1). Añade {n1} al nombre base para \
             nombrar cada hoja (p. ej. sheet{n1}).",
        );
        assert_eq!(
            multipack,
            "Multipack: 2 sheets generated and the base name \"sheet\" contains neither {n} nor \
             {n1}; they are named with the _N suffix (e.g. sheet_1). Add {n1} to the base name to \
             name every sheet (e.g. sheet{n1})."
        );
        // En español no se traduce nada (y lo que no encaja vuelve igual).
        assert_eq!(
            tr_in(Lang::Es, "ASTC demasiado corto"),
            "ASTC demasiado corto"
        );
        assert_eq!(tr_in(Lang::En, "Texto ya en inglés"), "Texto ya en inglés");
    }

    /// El caso que hacía panic al traducir: para comprobar que el último
    /// literal cierra el texto se restaban longitudes de byte, y con
    /// «áé» el resultado (1) cae en medio de la «á». Un mensaje del motor
    /// con un acento bastaba para tumbar la app al mostrarlo.
    #[test]
    fn match_pattern_no_parte_un_caracter_por_mitad() {
        // «ban» mide 3 bytes, «áé» 4: 4−3 = 1, que no es límite de carácter.
        assert_eq!(match_pattern("{}ban", "áé"), None);
        assert_eq!(match_pattern("{}ban", "ábán"), None);
        // Y con un final que sí encaja sigue emparejando.
        assert_eq!(
            match_pattern("{}ban", "soloban"),
            Some(vec!["solo".to_string()])
        );
    }

    #[test]
    fn match_pattern_recoge_los_huecos() {
        assert_eq!(
            match_pattern("A {} B {}", "A uno B dos"),
            Some(vec!["uno".to_string(), "dos".to_string()])
        );
        assert_eq!(
            match_pattern("{}: gzip: {}", "pvr.ccz: gzip: dato ilegible"),
            Some(vec!["pvr.ccz".to_string(), "dato ilegible".to_string()])
        );
        assert_eq!(
            match_pattern("A {} B", "A uno C B"),
            Some(vec!["uno C".to_string()])
        );
        assert_eq!(match_pattern("A B", "A B"), None);
        assert_eq!(match_pattern("A {}", "B uno"), None);
        assert_eq!(match_pattern("A {} B", "A uno"), None);
    }

    #[test]
    fn la_eleccion_del_usuario_se_guarda_como_texto_corto() {
        for choice in [LangChoice::System, LangChoice::Es, LangChoice::En] {
            assert_eq!(LangChoice::from_id(choice.id()), choice);
        }
        assert_eq!(LangChoice::from_id("otra-cosa"), LangChoice::System);
    }
}
