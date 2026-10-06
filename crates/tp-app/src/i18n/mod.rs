//! Idioma de la interfaz.
//!
//! La implementación vive en el crate compartido [`tp_i18n`]: el CLI usa el
//! mismo código y las mismas tablas, de modo que un mensaje se escribe una
//! vez y sale en los dos idiomas en los dos binarios. Aquí sólo se
//! re-exporta lo que la app necesita (`t`, `tr`, `Lang`, `set_choice`, …)
//! y se guardan los tests que escanean el fuente de **esta** app en busca
//! de literales que la UI pinta sin pasar por `t!(…)`.

pub use tp_i18n::*;

#[cfg(test)]
pub(crate) fn t_keys_in(src: &str) -> Vec<String> {
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
            let mut raw = String::new();
            let mut closed = false;
            j += 1;
            while j < chars.len() {
                match chars[j] {
                    '\\' => {
                        if let Some(n) = chars.get(j + 1) {
                            raw.push('\\');
                            raw.push(*n);
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
                        raw.push(c);
                        j += 1;
                    }
                }
            }
            if closed {
                // Igual que en el código: «\\n» es un salto de línea real.
                keys.push(crate::glyph_guard::unescape(&raw));
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
pub(crate) fn used_keys() -> Vec<String> {
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

    /// Métodos de egui que pintan texto (no identificadores: `Grid::new`,
    /// `id_salt` y compañía quedan fuera a propósito).
    const UI_TEXTO: &[&str] = &[
        "button",
        "small_button",
        "heading",
        "strong",
        "label",
        "text",
        "hint_text",
        "placeholder_text",
        "selected_text",
        "on_hover_text",
        "colored_label",
        "from_label",
        "title",
        "shortcut_text",
        "checkbox",
    ];

    /// Texto del argumento que abre en `ini` (índice dentro de la línea `n`),
    /// desde el paréntesis que lo abre hasta el que lo cierra, saltando los
    /// literales para no contar sus llaves ni sus paréntesis. Devuelve `None`
    /// si el cierre no llega antes de que se acabe el archivo.
    fn argumento(lineas: &[&str], n: usize, ini: usize) -> Option<String> {
        let mut out = String::new();
        let mut fondo = 1usize;
        let mut en_cadena = false;
        let mut escape = false;
        let mut primera = true;
        for linea in lineas.get(n..)? {
            let texto = if primera {
                primera = false;
                &linea[ini..]
            } else {
                linea
            };
            for c in texto.chars() {
                if en_cadena {
                    out.push(c);
                    if escape {
                        escape = false;
                    } else if c == '\\' {
                        escape = true;
                    } else if c == '"' {
                        en_cadena = false;
                    }
                    continue;
                }
                match c {
                    '"' => {
                        en_cadena = true;
                        out.push(c);
                    }
                    '(' => {
                        fondo += 1;
                        out.push(c);
                    }
                    ')' => {
                        fondo -= 1;
                        out.push(c);
                        if fondo == 0 {
                            return Some(out);
                        }
                    }
                    _ => out.push(c),
                }
            }
            out.push('\n');
        }
        None
    }

    /// Literales que son idénticos en los dos idiomas (préstamos como «FPS»
    /// o «Zoom» y los separadores «x»/«y» de las dimensiones): no merecen
    /// entrada en la tabla y el test de arriba los deja pasar.
    const UI_IGUALES: &[&str] = &["FPS", "Zoom", "x", "y", "Ctrl + O", "Ctrl + S", "F1"];

    /// El test `cada_clave_en_uso_tiene_traduccion` solo mira lo que ya pasa
    /// por `t!`, así que un literal escrito a pelo en la UI no lo ve nadie y
    /// la pantalla se queda en el idioma en que se programó. Este barrido va
    /// al revés: recorre las fuentes en busca de literales que un método de
    /// texto pinta sin traducir (M16 cazó diez de ellos).
    #[test]
    fn la_ui_no_pinta_literales_sin_t() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut ficheros = Vec::new();
        crate::glyph_guard::walk(&root, &mut ficheros);
        let mut crudos = Vec::new();
        for fichero in ficheros {
            if fichero.components().any(|c| c.as_os_str() == "bin") {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&fichero) else {
                continue;
            };
            let lineas: Vec<&str> = src.lines().collect();
            for (n, linea) in lineas.iter().enumerate() {
                // Lo que va tras `//` es comentario (un ejemplo en un
                // doc-comentario no es un literal de la UI).
                let codigo = linea.split("//").next().unwrap_or_default();
                for metodo in UI_TEXTO {
                    let patron = format!(".{metodo}(");
                    let mut desde = 0;
                    while let Some(p) = codigo[desde..].find(&patron) {
                        let ini = desde + p + patron.len();
                        desde = ini;
                        let resto = &codigo[ini..];
                        let tras = resto.trim_start();
                        // `RichText::new("…")` envuelve a la cadena: quitarle
                        // la capa deja el mismo caso que `.label("…")`.
                        let tras = tras
                            .strip_prefix("egui::RichText::new(")
                            .map(str::trim_start)
                            .unwrap_or(tras);
                        let Some(cadena) = tras.strip_prefix('"') else {
                            // Sin cadena delante puede seguir en la línea
                            // siguiente: ahí `t!(` ya cuenta como traducido.
                            if resto.trim().is_empty() {
                                let sig = lineas.get(n + 1).copied().unwrap_or_default();
                                if sig.trim_start().starts_with('"') {
                                    crudos.push(format!("{}:{}: {sig}", fichero.display(), n + 2));
                                }
                                continue;
                            }
                            // El argumento no empieza por cadena (un `if`, un
                            // `format!(`, una variable…): se recorre entero y,
                            // si su primer literal no lo pinta una macro, la UI
                            // lo pinta a pelo. Así se cazaron «Pivots» y
                            // «Pausar»/«Reproducir» (I8).
                            if let Some(arg) = argumento(&lineas, n, ini) {
                                let Some(comilla) = arg.find('"') else {
                                    continue;
                                };
                                let antes = &arg[..comilla];
                                // La aguja se arma con `concat!` para no
                                // escribir el patrón en el fuente: los tests
                                // de este propio módulo lo buscan a mano.
                                let pinta_macro = [concat!("t", "!("), "format!(", "concat!("]
                                    .iter()
                                    .any(|m| antes.contains(m));
                                if pinta_macro {
                                    continue;
                                }
                                let tras_comilla = &arg[comilla + 1..];
                                let fin = tras_comilla.find('"').unwrap_or(tras_comilla.len());
                                let texto = &tras_comilla[..fin];
                                if texto.chars().any(|c| c.is_ascii_alphabetic())
                                    && !UI_IGUALES.contains(&texto)
                                {
                                    crudos.push(format!(
                                        "{}:{}: {texto:?}",
                                        fichero.display(),
                                        n + 1
                                    ));
                                }
                            }
                            continue;
                        };
                        let fin = cadena.find('"').unwrap_or(cadena.len());
                        let texto = &cadena[..fin];
                        if texto.chars().any(|c| c.is_ascii_alphabetic())
                            && !UI_IGUALES.contains(&texto)
                        {
                            crudos.push(format!("{}:{}: {texto:?}", fichero.display(), n + 1));
                        }
                    }
                }
            }
        }
        crudos.sort();
        crudos.dedup();
        assert!(
            crudos.is_empty(),
            "literales que la UI pinta sin pasar por t!(…):\n  {}",
            crudos.join("\n  ")
        );
    }
}
