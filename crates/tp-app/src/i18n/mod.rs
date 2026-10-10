//! Idioma de la interfaz.
//!
//! La implementación vive en el crate compartido [`tp_i18n`]: el CLI usa el
//! mismo código y las mismas tablas, de modo que un mensaje se escribe una
//! vez y sale en los dos idiomas en los dos binarios. Aquí sólo se
//! re-exporta lo que la app necesita (`t`, `tr`, `Lang`, `set_choice`, …)
//! y se guardan los tests que escanean el fuente de **esta** app en busca
//! de literales que la UI pinta sin pasar por `t!(…)`.

pub use tp_i18n::*;

/// El motivo de un [`std::io::Error`] en el idioma de la interfaz.
///
/// El texto crudo del sistema («No such file or directory (os error 2)»)
/// salía en inglés en mitad de un mensaje español, justo en el error que
/// más ve el usuario al abrir o guardar mal una ruta (M7). Aquí se traduce
/// por lo que describe, que es lo único que da pie a corregirlo; para los
/// códigos que la interfaz no nombra queda el número del sistema, que es el
/// mismo en todos.
pub fn io_motivo(e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    let motivo: &str = match e.kind() {
        ErrorKind::NotFound => t!("el fichero o la carpeta no existe"),
        ErrorKind::PermissionDenied => t!("no hay permiso para leer ni escribir"),
        ErrorKind::AlreadyExists => t!("el destino ya existe"),
        ErrorKind::IsADirectory => t!("el destino es una carpeta"),
        ErrorKind::NotADirectory => t!("el destino no es una carpeta"),
        ErrorKind::InvalidData => t!("el contenido no es válido"),
        ErrorKind::TimedOut => t!("se agotó el tiempo de espera"),
        _ => {
            return match e.raw_os_error() {
                Some(codigo) => t!("error del sistema (código {})", codigo),
                None => t!("no se pudo completar la operación").to_string(),
            };
        }
    };
    motivo.to_string()
}

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

    /// M7: el motivo crudo del SO («No such file or directory (os error 2)»)
    /// salía en inglés en mitad de un mensaje español. Se traduce por el
    /// motivo que describe; lo que la interfaz no nombra queda por el número
    /// del sistema, que sí aparece igual en todos.
    #[test]
    fn el_motivo_de_un_error_de_entrada_no_va_en_ingles() {
        use std::io::ErrorKind;
        let casos = [
            (ErrorKind::NotFound, "el fichero o la carpeta no existe"),
            (
                ErrorKind::PermissionDenied,
                "no hay permiso para leer ni escribir",
            ),
            (ErrorKind::AlreadyExists, "el destino ya existe"),
            (ErrorKind::IsADirectory, "el destino es una carpeta"),
            (ErrorKind::NotADirectory, "el destino no es una carpeta"),
            (ErrorKind::InvalidData, "el contenido no es válido"),
            (ErrorKind::TimedOut, "se agotó el tiempo de espera"),
        ];
        for (kind, esperado) in casos {
            let e = std::io::Error::from(kind);
            assert_eq!(io_motivo(&e), esperado, "motivo de {kind:?}");
        }
        // Un código que la interfaz no nombra: queda el número, nunca el
        // texto del sistema.
        let crudo = std::io::Error::from_raw_os_error(424_242);
        let motivo = io_motivo(&crudo);
        assert!(
            motivo.contains("424242"),
            "debe quedar el número del sistema: {motivo}"
        );
        assert!(
            !motivo.contains("os error"),
            "no debe quedar el texto crudo del SO: {motivo}"
        );
    }

    /// El motivo de cada caso tiene su entrada en la tabla: sin ella la
    /// interfaz en inglés se quedaría con la clave española.
    #[test]
    fn los_motivos_de_los_errores_de_entrada_estan_traducidos() {
        for (es, en) in [
            (
                "el fichero o la carpeta no existe",
                "the file or folder does not exist",
            ),
            (
                "no hay permiso para leer ni escribir",
                "there is no permission to read or write",
            ),
            ("el destino ya existe", "the destination already exists"),
            ("el destino es una carpeta", "the destination is a folder"),
            (
                "el destino no es una carpeta",
                "the destination is not a folder",
            ),
            ("el contenido no es válido", "the content is not valid"),
            ("se agotó el tiempo de espera", "the operation timed out"),
            ("error del sistema (código {})", "system error (code {})"),
            (
                "no se pudo completar la operación",
                "the operation could not be completed",
            ),
        ] {
            assert_eq!(
                crate::i18n::en::lookup(es),
                Some(en),
                "traducción ausente o cambiada de {es:?}"
            );
        }
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
        "menu_button",
    ];

    /// Constructores que también pintan texto, sin el punto delante:
    /// `egui::Button::new("…")` pinta igual que `.button("…")` y se le
    /// escapó al barrido —«⚙ Sprite», el único texto de la barra de
    /// herramientas que seguía a pelo, vino por aquí—.
    const UI_CTORS: &[&str] = &["Button::new(", "SelectableLabel::new("];

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
    const UI_IGUALES: &[&str] = &[
        "FPS", "Zoom", "x", "y", "Ctrl + O", "Ctrl + S", "Ctrl + Z", "Ctrl + Q", "F1", "F",
    ];

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
            crudos.extend(crudos_de(&fichero.display().to_string(), &src));
        }
        crudos.sort();
        crudos.dedup();
        assert!(
            crudos.is_empty(),
            "literales que la UI pinta sin pasar por t!(…):\n  {}",
            crudos.join("\n  ")
        );
    }

    /// Los literales crudos que `src` pinta sin pasar por `t!(…)`, con
    /// su origen. Los dos patrones pintan igual: `.button("…")` y
    /// `egui::Button::new("…")` son dos maneras de ponerle texto a un
    /// widget, y al segundo se le escapó «⚙ Sprite», el único texto de
    /// la barra de herramientas que seguía a pelo.
    fn crudos_de(origen: &str, src: &str) -> Vec<String> {
        let patrones: Vec<String> = UI_TEXTO
            .iter()
            .map(|metodo| format!(".{metodo}("))
            .chain(UI_CTORS.iter().map(|ctor| (*ctor).to_string()))
            .collect();
        let propias: Vec<&str> = src.lines().collect();
        let lineas = solo_produccion(&propias);
        let mut crudos = Vec::new();
        for (n, linea) in lineas.iter().enumerate() {
            // Lo que va tras `//` es comentario (un ejemplo en un
            // doc-comentario no es un literal de la UI).
            let codigo = linea.split("//").next().unwrap_or_default();
            for patron in &patrones {
                let mut desde = 0;
                while let Some(p) = codigo[desde..].find(patron.as_str()) {
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
                                crudos.push(format!("{origen}:{}: {sig}", n + 2));
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
                                crudos.push(format!("{origen}:{}: {texto:?}", n + 1));
                            }
                        }
                        continue;
                    };
                    let fin = cadena.find('"').unwrap_or(cadena.len());
                    let texto = &cadena[..fin];
                    if texto.chars().any(|c| c.is_ascii_alphabetic())
                        && !UI_IGUALES.contains(&texto)
                    {
                        crudos.push(format!("{origen}:{}: {texto:?}", n + 1));
                    }
                }
            }
        }
        crudos
    }

    /// Sólo el código que se instala: el montaje de las pruebas pinta lo
    /// que le dé la gana —«Botón con ayuda», por ejemplo— y no es la UI.
    /// Sus líneas quedan vacías y no fuera, para que el número de línea
    /// de un crudo posterior siga cuadrando con el fichero.
    fn solo_produccion<'a>(lineas: &[&'a str]) -> Vec<&'a str> {
        let mut fuera: Vec<&str> = Vec::with_capacity(lineas.len());
        let mut i = 0;
        while i < lineas.len() {
            if lineas[i].trim() != "#[cfg(test)]" {
                fuera.push(lineas[i]);
                i += 1;
                continue;
            }
            // El atributo y los que vengan pegados debajo (`#[allow(…)]`).
            fuera.push("");
            i += 1;
            while i < lineas.len() {
                let linea = lineas[i].trim();
                if linea.is_empty() || linea.starts_with("#[") {
                    fuera.push("");
                    i += 1;
                    continue;
                }
                break;
            }
            let Some(item) = lineas.get(i) else { break };
            // Un `use` o un `mod x;` acaban en `;`; un `mod x {` hay que
            // contarle las llaves hasta cerrarlo.
            if item.trim().ends_with(';') {
                fuera.push("");
                i += 1;
                continue;
            }
            let mut llaves =
                item.matches('{').count() as isize - item.matches('}').count() as isize;
            fuera.push("");
            i += 1;
            while llaves > 0 && i < lineas.len() {
                let linea = lineas[i];
                llaves += linea.matches('{').count() as isize;
                llaves -= linea.matches('}').count() as isize;
                fuera.push("");
                i += 1;
            }
        }
        fuera
    }

    /// El barrido no mira sólo `.button("…")`: `egui::Button::new("…")`
    /// pinta igual y es por donde se coló «⚙ Sprite», el único texto de
    /// la barra de herramientas que seguía a pelo. Los glifos —«…»,
    /// «+»— no son cadenas a traducir y se quedan fuera. (Los literales
    /// de estas pruebas no hace falta esconderlos: el barrido se salta
    /// `mod tests`, que es justo lo que comprueba la prueba siguiente.)
    #[test]
    fn el_barrido_tambien_caza_los_constructores() {
        let crudos = crudos_de("prueba.rs", "egui::Button::new(\"⚙ Sprite\").selected(x)");
        assert_eq!(
            crudos.len(),
            1,
            "tenía que cazar el literal que pinta un constructor: {crudos:?}"
        );
        assert!(crudos[0].contains("⚙ Sprite"), "{crudos:?}");

        assert!(
            crudos_de("prueba.rs", "egui::Button::new(\"…\")").is_empty(),
            "un glifo no es una cadena a traducir"
        );

        // La macro va escrita con `concat!`: `used_keys` no se salta
        // `mod tests` y se comería «⚙ Sprite» como clave viva.
        let macro_t = concat!("t", "!");
        let con_t = format!("egui::Button::new({macro_t}(\"⚙ Sprite\"))");
        assert!(
            crudos_de("prueba.rs", &con_t).is_empty(),
            "lo que pasa por t! ya está traducido"
        );

        // `menu_button` es un método que pinta texto y tampoco se libraba.
        let crudos = crudos_de("prueba.rs", "ui.menu_button(\"Archivo\", |ui| {})");
        assert_eq!(crudos.len(), 1, "tenía que cazar el menú crudo: {crudos:?}");
    }

    /// El montaje de las pruebas no es la UI que se instala y el barrido
    /// no lo mira; el código de producción de al lado sí.
    #[test]
    fn el_barrido_salta_el_codigo_de_las_pruebas() {
        let crudo = "egui::Button::new(\"Botón con ayuda\")";
        assert_eq!(
            crudos_de("prueba.rs", crudo).len(),
            1,
            "el código de la app sí se mira"
        );

        let dentro = format!(
            "#[cfg(test)]\nmod tests {{\n    fn montaje() {{\n        let _ = {crudo};\n    }}\n}}\n"
        );
        assert!(
            crudos_de("prueba.rs", &dentro).is_empty(),
            "el montaje de las pruebas no es la UI"
        );

        // Las líneas de prueba se vacían y no se quitan: lo que viene
        // detrás sigue contando la línea que le corresponde.
        let linea = dentro.lines().count() + 1;
        let crudos = crudos_de(
            "prueba.rs",
            &format!("{dentro}let _ = egui::Button::new(\"Zona\");\n"),
        );
        assert_eq!(crudos.len(), 1, "{crudos:?}");
        assert!(
            crudos[0].ends_with(&format!(":{linea}: \"Zona\"")),
            "la línea se cuenta mal: {crudos:?}"
        );
    }
}
