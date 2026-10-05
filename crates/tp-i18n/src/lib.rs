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
pub mod en_cli;
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
///
/// El texto puede marcar el plural con la notación `palabra(s)` o
/// `palabra(es)`: el **último** hueco sustituido decide
/// ([`expandir_plural`]), de modo que un solo mensaje sirve para «1 página»
/// y «2 páginas» en los dos idiomas (review UI/UX I8: «1 page(s)»,
/// «1 aliases»…). Los marcadores que no van detrás de un número se quedan en
/// plural, que es lo que se espera de un recuento.
pub fn translate_with(es: &str, args: &[&dyn std::fmt::Display]) -> String {
    let plantilla = translate(es);
    let mut out = String::with_capacity(plantilla.len() + 16 * args.len());
    let mut rest = plantilla;
    let mut usados = 0;
    // ¿El hueco anterior valía 1? Eso decide los marcadores que siguen.
    let mut uno = false;
    while let Some(ini) = rest.find('{') {
        let cuerpo = &rest[ini + 1..];
        let Some(fin) = cuerpo.find('}') else {
            out.push_str(&expandir_plural(rest, uno));
            return out;
        };
        out.push_str(&expandir_plural(&rest[..ini], uno));
        let spec = &cuerpo[..fin];
        if spec.is_empty() {
            match args.get(usados) {
                Some(arg) => {
                    let texto = arg.to_string();
                    uno = texto == "1";
                    out.push_str(&texto);
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
    out.push_str(&expandir_plural(rest, uno));
    out
}

/// Abre o quita los marcadores de plural `palabra(s)` / `palabra(es)`.
///
/// `1 celda(s)` ⇒ «1 celda» y `2 celda(s)` ⇒ «2 celdas»; `posición(es)`
/// hace lo propio con «es». Sólo se mira el texto literal, así que un
/// paréntesis de verdad («(ejemplo)») no se toca.
fn expandir_plural(texto: &str, uno: bool) -> String {
    if !texto.contains("(s)") && !texto.contains("(es)") {
        return texto.to_string();
    }
    let mut out = String::with_capacity(texto.len() + 4);
    let mut rest = texto;
    loop {
        // El más a la izquierda de los dos marcadores: «(es)» no contiene
        // «(s)», pero ambos pueden convivir en la misma frase.
        let pos_es = rest.find("(es)");
        let pos_s = rest.find("(s)");
        let (ini, largo) = match (pos_es, pos_s) {
            (Some(a), Some(b)) if a <= b => (a, 4),
            (_, Some(b)) => (b, 3),
            (Some(a), None) => (a, 4),
            (None, None) => break,
        };
        out.push_str(&rest[..ini]);
        if !uno {
            // Español: los sustantivos en -ón/-ción/-sión pierden la tilde en
            // el plural («posición» ⇒ «posiciones», nunca «posiciónes»).
            if largo == 4 && out.ends_with("ón") {
                out.truncate(out.len() - "ón".len());
                out.push_str("on");
            }
            // «(s)» ⇒ «s», «(es)» ⇒ «es»: entre los paréntesis está la letra.
            out.push_str(&rest[ini + 1..ini + largo - 1]);
        }
        rest = &rest[ini + largo..];
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
        // Cinco niveles: un mensaje del CLI puede llevar el prefijo
        // («aviso: »), el envoltorio del comando («Empaquetado fallido: »),
        // el del subsistema («Configuración inválida: ») y aún así llegar
        // al detalle con huecos. Con tres se quedaba a medias.
        Lang::En => tr_en(msg, 5),
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
    if let Some(en) = en_cli::lookup(msg) {
        return en.to_string();
    }
    if profundidad == 0 {
        return msg.to_string();
    }
    for (es, en) in en::EN
        .iter()
        .chain(en_core::EN_CORE.iter())
        .chain(en_cli::EN_CLI.iter())
    {
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
    // Mismo criterio que `translate_with`: el último hueco relleno decide si
    // los marcadores `(s)`/`(es)` que vienen detrás se quedan en plural.
    let mut uno = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i..].starts_with(&['(', 'e', 's', ')']) {
            if !uno {
                out.push_str("es");
            }
            i += 4;
            continue;
        }
        if chars[i..].starts_with(&['(', 's', ')']) {
            if !uno {
                out.push('s');
            }
            i += 3;
            continue;
        }
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => {
                out.push('{');
                i += 2;
            }
            '{' if chars.get(i + 1) == Some(&'}') => match vuetecos.get(usados) {
                Some(v) => {
                    uno = v == "1";
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

    /// Un marcador de plural decide con el **último** lleno que se sustituyó:
    /// así «1 página(s)» sale «1 página» y «2 página(s)» sale «2 páginas»,
    /// sin tener que mantener dos claves por mensaje (review UI/UX I8).
    ///
    /// Claves inventadas: no están en la tabla, así que el literal vuelve
    /// igual en los dos idiomas y el test no toca el idioma global (los
    /// tests corren en paralelo).
    #[test]
    fn el_marcador_de_plural_sigue_al_ultimo_lleno() {
        assert_eq!(translate_with("{} página(s)", &[&1]), "1 página");
        assert_eq!(translate_with("{} página(s)", &[&2]), "2 páginas");
        // Todos los marcadores detrás del mismo recuento van a la par.
        assert_eq!(
            translate_with("{} fichero(s) soltado(s)", &[&1]),
            "1 fichero soltado"
        );
        assert_eq!(
            translate_with("{} fichero(s) soltado(s)", &[&3]),
            "3 ficheros soltados"
        );
        // «(es)» hace lo propio con la e.
        assert_eq!(translate_with("{} posición(es)", &[&1]), "1 posición");
        assert_eq!(translate_with("{} posición(es)", &[&2]), "2 posiciones");
        // Cada hueco manda sólo detrás de sí: el resto sigue al anterior.
        assert_eq!(
            translate_with("{} ms: {} sprite(s)", &[&45, &2]),
            "45 ms: 2 sprites"
        );
        assert_eq!(
            translate_with("{} ms: {} sprite(s)", &[&45, &1]),
            "45 ms: 1 sprite"
        );
        // Un lleno que no es recuento no da singular: el plural es el
        // valor por defecto de un mensaje que no sabemos contar.
        assert_eq!(
            translate_with("{} archivo(s)", &[&"pepe.png"]),
            "pepe.png archivos"
        );
        // Los paréntesis que no son marcador se quedan como están.
        assert_eq!(translate_with("(p. ej. {})", &[&1]), "(p. ej. 1)");
        assert_eq!(translate_with("sin lleno(s)", &[]), "sin llenos");
    }

    /// El CLI no traduce con `t!`: formatea en español y pasa el mensaje
    /// entero por `tr`, que lo encaja en la plantilla y rellena la
    /// inglesa. Ese camino (`substitute`) tiene que abrir los mismos
    /// marcadores o la CLI seguiría imprimiendo «1 sheet(s)».
    #[test]
    fn el_tr_del_cli_tambien_abre_los_marcadores() {
        assert_eq!(
            tr_in(
                Lang::En,
                "✔ Empaquetado en 45 ms: 1 sprite(s) (1 alias(es)), 1 página(s)"
            ),
            "✔ Packed in 45 ms: 1 sprite (1 alias), 1 sheet"
        );
        assert_eq!(
            tr_in(
                Lang::En,
                "✔ Empaquetado en 45 ms: 3 sprite(s) (2 alias(es)), 2 página(s)"
            ),
            "✔ Packed in 45 ms: 3 sprites (2 aliases), 2 sheets"
        );
        // En español el mensaje ya viene formateado y `tr` no lo toca.
        assert_eq!(
            tr_in(
                Lang::Es,
                "✔ Empaquetado en 45 ms: 1 sprite(s) (1 alias(es)), 1 página(s)"
            ),
            "✔ Empaquetado en 45 ms: 1 sprite(s) (1 alias(es)), 1 página(s)"
        );
    }

    /// Un marcador que no vaya detrás de un `{}` no se puede decidir: saldría
    /// impreso tal cual («palabra(s)»), que es justo lo que queremos evitar.
    #[test]
    fn ningún_marcador_de_plural_viene_antes_de_un_lleno() {
        let mut malas = Vec::new();
        for (es, en) in en::EN
            .iter()
            .chain(en_core::EN_CORE.iter())
            .chain(en_cli::EN_CLI.iter())
        {
            for (lado, texto) in [("es", *es), ("en", *en)] {
                if !texto.contains("(s)") && !texto.contains("(es)") {
                    continue;
                }
                let Some(lleno) = texto.find('{') else {
                    // Sin ningún lleno no hay recuento que lo mande: el
                    // marcador se imprimiría tal cual.
                    malas.push(format!("{lado}: {texto}"));
                    continue;
                };
                let antes = &texto[..lleno];
                if antes.contains("(s)") || antes.contains("(es)") {
                    malas.push(format!("{lado}: {texto}"));
                }
            }
        }
        assert!(
            malas.is_empty(),
            "marcador(s) de plural sin lleno delante que lo decida:\n  {}",
            malas.join("\n  ")
        );
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
    fn la_tabla_del_cli_no_tiene_claves_vacias_ni_duplicadas() {
        let mut vistas = std::collections::HashSet::new();
        for (es, en) in en_cli::EN_CLI {
            assert!(!es.is_empty() && !en.is_empty(), "entrada vacía: {es:?}");
            assert!(vistas.insert(*es), "clave de tp-cli duplicada: {es}");
        }
    }

    /// El CLI traduce al imprimir, sobre el mensaje ya formateado: si una
    /// traducción perdiera o añadiera un `{}`, `substitute` insertaría los
    /// argumentos en el hueco equivocado y saldría basura.
    #[test]
    fn la_traduccion_del_cli_conserve_los_placeholders() {
        let mut malas = Vec::new();
        for (es, en) in en_cli::EN_CLI {
            if placeholders(es) != placeholders(en) {
                malas.push(format!("{es} / {en}"));
            }
        }
        assert!(
            malas.is_empty(),
            "los mensajes de la CLI pierden o añaden {{…}}:\n  {}",
            malas.join("\n  ")
        );
    }

    /// Un mensaje de la CLI formateado se traduce entero y con sus
    /// argumentos en el sitio, que es justo lo que hace `fail()` al salir.
    #[test]
    fn tr_del_cli_traduce_el_mensaje_ya_formateado() {
        let en = tr_in(Lang::En, "--scale inválido: 9 (número en (0, 8])");
        assert_eq!(en, "invalid --scale: 9 (number in (0, 8])");
        // El hueco lleva dentro un mensaje del motor, que se traduce a su
        // vez: es lo que separa «Decryption failed:» de un error entero en
        // español metido en medio de una frase en inglés.
        assert_eq!(
            tr_in(
                Lang::En,
                "Descifrado fallido: Error descifrando (¿clave incorrecta?)"
            ),
            "Decryption failed: Error decrypting (wrong key?)"
        );
        // Un hueco sin clave propia se queda como está: el hueco no lo
        // llena un traductor, y en español sigue siendo legible —esa es la
        // regla del crate: mejor español que un hueco vacío—.
        assert_eq!(
            tr_in(Lang::En, "No se pudo leer a.png: El fichero no existe"),
            "Could not read a.png: El fichero no existe"
        );
        // El caso real que hizo panicar al CLI: un mensaje del motor largo y
        // con acentos, metido dentro de un mensaje del propio CLI.
        assert_eq!(
            tr_in(
                Lang::En,
                "Empaquetado fallido: No se encontraron sprites válidos en el directorio de entrada"
            ),
            "Pack failed: No valid sprites found in the input directory"
        );
        // En español no se traduce nada.
        assert_eq!(
            tr_in(Lang::Es, "--scale inválido: 9 (número en (0, 8])"),
            "--scale inválido: 9 (número en (0, 8])"
        );
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
    /// con un acento bastaba para tumbar el CLI al imprimirlo.
    /// Un mensaje real de la CLI anida cuatro plantillas: el prefijo de
    /// aviso, el envoltorio del comando, el del subsistema y el detalle con
    /// huecos. Con tres niveles de recursión el último salía en español, a
    /// medias dentro de una frase en inglés.
    #[test]
    fn tr_baja_hasta_el_ultimo_nivel_de_envoltorio() {
        assert_eq!(
            tr_in(
                Lang::En,
                "aviso: Empaquetado fallido: Configuración inválida: max_texture_size debe ser una potencia de dos positiva (se obtuvo 5)"
            ),
            "warning: Pack failed: Invalid configuration: max_texture_size must be a positive power of two (got 5)"
        );
    }

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
