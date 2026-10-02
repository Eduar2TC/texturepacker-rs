//! Chequeo de glifos.
//!
//! Las fuentes por defecto de egui (Ubuntu-Light, NotoEmoji y
//! emoji-icon-font) no cubren todos los caracteres: los que faltan se pintan
//! como un cuadrado hueco en pantalla. Este módulo recorre los literales de
//! cadena que la UI puede llegar a pintar y falla si alguno no tiene glifo,
//! para que la próxima vez que aparezca un símbolo raro el test lo diga en
//! lugar de verse en la captura.

use eframe::egui;
use std::path::{Path, PathBuf};

/// Un carácter no ASCII de un literal de cadena, con su origen.
struct Occurrence {
    file: PathBuf,
    line: usize,
    ch: char,
}

/// Literales de cadena de un fichero, con su línea de inicio.
///
/// Maneja cadenas multilínea y salta las de bytes (`b"…"`), que nunca se
/// pintan. Un `"` suelto (p. ej. `'"'`) se ignora si está entre comillas
/// simples.
pub(crate) fn literals(src: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c != '"' {
            i += 1;
            continue;
        }
        let prev = if i > 0 { chars[i - 1] } else { '\0' };
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        // '"' de un carácter literal: no abre cadena.
        if prev == '\'' && next == '\'' {
            i += 1;
            continue;
        }
        let bytes = prev == 'b';
        let start_line = line;
        let mut body = String::new();
        i += 1;
        while i < chars.len() {
            let c = chars[i];
            match c {
                '\\' => {
                    // Conserva el escape: lo descodificamos después. Un
                    // escape de continuación de línea lleva su salto de
                    // línea con él, así que hay que contarlo.
                    body.push(c);
                    i += 1;
                    if let Some(n) = chars.get(i) {
                        if *n == '\n' {
                            line += 1;
                        }
                        body.push(*n);
                        i += 1;
                    }
                }
                '"' => {
                    i += 1;
                    break;
                }
                '\n' => {
                    line += 1;
                    body.push(c);
                    i += 1;
                }
                _ => {
                    body.push(c);
                    i += 1;
                }
            }
        }
        if !bytes {
            out.push((start_line, body));
        }
    }
    out
}

/// Descodifica los escapes que usamos en el código (`\u{2192}`, `\n`, `\"`…).
///
/// La barra al final de línea hace continuación, como en Rust: se come el salto
/// **y** la sangría de la línea siguiente (`"hola \` + `     mundo"` es
/// `hola mundo`), que es justo lo que compila el compilador.
pub(crate) fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('\n') => {
                while matches!(it.peek(), Some(' ') | Some('\t')) {
                    it.next();
                }
            }
            Some('\r') => {
                if it.peek() == Some(&'\n') {
                    it.next();
                    while matches!(it.peek(), Some(' ') | Some('\t')) {
                        it.next();
                    }
                } else {
                    out.push('\r');
                }
            }
            Some('u') => {
                if it.peek() == Some(&'{') {
                    it.next();
                    let mut hex = String::new();
                    for c in it.by_ref() {
                        if c == '}' {
                            break;
                        }
                        hex.push(c);
                    }
                    if let Ok(n) = u32::from_str_radix(&hex, 16) {
                        if let Some(c) = char::from_u32(n) {
                            out.push(c);
                        }
                    }
                }
            }
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('0') => out.push('\0'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

pub(crate) fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Raíces con los literales que la UI pinta.
pub(crate) fn source_roots() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    vec![manifest.join("src"), manifest.join("../tp-core/src")]
}

/// Caracteres no ASCII de los literales que la UI pinta: `tp-app/src` (sin
/// los binarios, que no pinta egui) y `tp-core/src`, cuyos mensajes acaban en
/// el log de la app.
fn ui_chars() -> Vec<Occurrence> {
    let mut files = Vec::new();
    for root in source_roots() {
        walk(&root, &mut files);
    }
    let mut out = Vec::new();
    for file in files {
        if file.components().any(|c| c.as_os_str() == "bin") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (line, body) in literals(&src) {
            for ch in unescape(&body).chars() {
                if !ch.is_ascii() {
                    out.push(Occurrence {
                        file: file.clone(),
                        line,
                        ch,
                    });
                }
            }
        }
    }
    out
}

#[test]
fn toda_cadena_de_la_ui_tiene_glifo() {
    let mut fonts =
        egui::text::Fonts::new(1024, Default::default(), egui::FontDefinitions::default());
    let id = egui::FontId::default(); // familia proporcional: la que usa la UI
    let mut faltan = Vec::new();
    for occ in ui_chars() {
        if !fonts.has_glyph(&id, occ.ch) {
            faltan.push(format!(
                "U+{:04X} '{}' en {}:{}",
                occ.ch as u32,
                occ.ch,
                occ.file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                occ.line,
            ));
        }
    }
    assert!(
        faltan.is_empty(),
        "estos caracteres no tienen glifo en las fuentes por defecto de egui \
         (se verian como un cuadrado):\n  {}",
        faltan.join("\n  ")
    );
}
