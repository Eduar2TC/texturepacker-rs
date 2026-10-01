//! Traducciones al inglés: `(español del código, inglés)`.
//!
//! La clave es **literalmente** el literal que pasa a `t!(…)` en el código,
//! así que se lee en contexto y se puede buscar. El valor debe conservar los
//! mismos placeholders `{…}` (lo comprueba un test).
//!
//! El orden no importa: otro test se encarga de avisar de claves duplicadas,
//! claves que ya no se usan y claves en uso sin traducir.

use std::collections::HashMap;
use std::sync::OnceLock;

pub(crate) static EN: &[(&str, &str)] = &[
    // Sección «Interfaz» del panel de Ajustes.
    ("Interfaz", "Interface"),
    ("Idioma", "Language"),
    ("Sistema (idioma del equipo)", "System (OS language)"),
    ("Tema", "Theme"),
    ("Sistema", "System"),
    ("Claro", "Light"),
    ("Oscuro", "Dark"),
    (
        "Se guarda en tu equipo, no en el proyecto.",
        "Stored on this machine, not in the project.",
    ),
];

fn map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| EN.iter().copied().collect())
}

/// Traducción de `es`, si la hay.
pub(crate) fn lookup(es: &str) -> Option<&'static str> {
    map().get(es).copied()
}
