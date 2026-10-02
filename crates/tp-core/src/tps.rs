//! Lectura y escritura de proyectos del original (`.tps`).
//!
//! Un `.tps` es XML serializado por `QSettings` de Qt: un `<data>` con un
//! `<struct type="Settings">` hecho de pares `<key>` + valor (`<int>`,
//! `<uint>`, `<double>`, `<string>`, `<filename>`, `<true/>`/`<false/>`,
//! `<enum>`, `<point_f>`, `<rect>`, `<QSize>`, `<array>`, `<map>`,
//! `<struct>`…). Este módulo traduce a [`ProjectConfig`] lo que aquí tiene
//! equivalente y avisa, en bloque, de lo que no (formato de hoja, bordes
//! scale9, exportadores propios del original…), para que abrir un proyecto
//! real no parezca que se ha perdido en silencio.

use crate::config::{
    AlphaHandling, GdxFilter, PackMode, PackingAlgorithm, PixelFormat, PngDither, ProjectConfig,
    ScaleMode, SizeConstraint, SortOrder, TrimMode,
};
use crate::error::{Result, TpError};
use crate::types::Point2D;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Proyecto `.tps` ya traducido, con los avisos de lo que no se ha podido
/// aplicar. Los ajustes señalados se ignoran: la configuración resultante
/// es el subconjunto que aquí sí se entiende.
#[derive(Debug, Clone)]
pub struct TpsProject {
    pub config: ProjectConfig,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Modelo tipado del XML
// ---------------------------------------------------------------------------

/// Valor del árbol `.tps`, ya tipado.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Bool(bool),
    Int(i64),
    Uint(u64),
    Double(f64),
    Str(String),
    /// `<point_f>x,y</point_f>`
    Point(f32, f32),
    /// `<rect>x,y,w,h</rect>`
    Rect(i64, i64, i64, i64),
    /// `<QSize><key>width</key>…<key>height</key>…</QSize>`
    Size(i64, i64),
    List(Vec<Value>),
    /// `<struct>` y `<map>`: pares clave→valor.
    Map(Vec<(String, Value)>),
}

fn parse_document(text: &str) -> Result<Vec<(String, Value)>> {
    let doc = roxmltree::Document::parse(text)
        .map_err(|e| TpError::Config(format!("XML de .tps inválido: {e}")))?;
    let root = doc.root_element();
    if root.tag_name().name() != "data" {
        return Err(TpError::Config(format!(
            "no es un proyecto .tps (raíz <{}>, se esperaba <data>)",
            root.tag_name().name()
        )));
    }
    let settings = root
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "struct")
        .ok_or_else(|| TpError::Config("falta el <struct type=\"Settings\"> del .tps".into()))?;
    parse_pairs(settings)
}

/// Pares clave→valor de un `<struct>`/`<map>`.
///
/// Varias `<key>` seguidas comparten el valor que viene después: así agrupa
/// el original los sprites con ajustes idénticos dentro de
/// `individualSpriteSettings`, y así se lee sin ambigüedad.
fn parse_pairs(node: roxmltree::Node<'_, '_>) -> Result<Vec<(String, Value)>> {
    let mut out: Vec<(String, Value)> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    for child in node.children().filter(|n| n.is_element()) {
        let name = child.tag_name().name();
        if name == "key" {
            pending.push(child.text().unwrap_or_default().trim().to_string());
            continue;
        }
        let value = parse_value(child, name);
        for key in pending.drain(..) {
            out.push((key, value.clone()));
        }
    }
    // Una clave sin valor al final se descarta: el fichero queda incompleto,
    // no ilegible.
    Ok(out)
}

fn parse_value(node: roxmltree::Node<'_, '_>, name: &str) -> Value {
    let text = node.text().unwrap_or_default().trim();
    match name {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "int" => Value::Int(text.parse().unwrap_or(0)),
        "uint" => Value::Uint(text.parse().unwrap_or(0)),
        "double" => Value::Double(text.parse().unwrap_or(0.0)),
        "string" | "filename" | "enum" => Value::Str(text.to_string()),
        "point_f" => {
            let (x, y) = text.split_once(',').unwrap_or((text, "0"));
            Value::Point(
                x.trim().parse().unwrap_or(0.0),
                y.trim().parse().unwrap_or(0.0),
            )
        }
        "rect" => {
            let nums: Vec<i64> = text
                .split(',')
                .map(|p| p.trim().parse().unwrap_or(0))
                .collect();
            Value::Rect(
                nums.first().copied().unwrap_or(0),
                nums.get(1).copied().unwrap_or(0),
                nums.get(2).copied().unwrap_or(0),
                nums.get(3).copied().unwrap_or(0),
            )
        }
        "QSize" => {
            let pairs = parse_pairs(node).unwrap_or_default();
            let pick = |k: &str| -> i64 { get(&pairs, k).and_then(as_int).unwrap_or(-1) };
            Value::Size(pick("width"), pick("height"))
        }
        "array" => Value::List(
            node.children()
                .filter(|n| n.is_element())
                .map(|n| parse_value(n, n.tag_name().name()))
                .collect(),
        ),
        "struct" | "map" => Value::Map(parse_pairs(node).unwrap_or_default()),
        // Elemento de una versión más nueva: se conserva como texto para que
        // la clave aparezca en el aviso de ajustes no soportados.
        _ => Value::Str(text.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Accesos al árbol
// ---------------------------------------------------------------------------

fn get<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Uint(u) => i64::try_from(*u).ok(),
        Value::Double(d) => Some(*d as i64),
        _ => None,
    }
}

fn as_uint(v: &Value) -> Option<u64> {
    match v {
        Value::Uint(u) => Some(*u),
        Value::Int(i) => u64::try_from(*i).ok(),
        Value::Double(d) if *d >= 0.0 => Some(*d as u64),
        _ => None,
    }
}

fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Double(d) => Some(*d),
        Value::Int(i) => Some(*i as f64),
        Value::Uint(u) => Some(*u as f64),
        _ => None,
    }
}

fn as_str(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s),
        _ => None,
    }
}

fn as_map(v: &Value) -> Option<&[(String, Value)]> {
    match v {
        Value::Map(pairs) => Some(pairs),
        _ => None,
    }
}

fn as_list(v: &Value) -> Option<&[Value]> {
    match v {
        Value::List(items) => Some(items),
        _ => None,
    }
}

/// Traduce un valor enumerado del original con las reglas de `serde`
/// (todos los enums relevantes ya llevan `#[serde(rename)]` con el nombre
/// literal que escribe el original).
fn parse_enum<T: serde::de::DeserializeOwned>(v: &Value) -> Option<T> {
    let s = as_str(v)?;
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

fn enum_name<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Traducción a ProjectConfig
// ---------------------------------------------------------------------------

/// Claves de metadatos que existen en todo `.tps` y no dicen nada de la
/// configuración: no se cuentan como «no soportadas».
const IGNORED_KEYS: &[&str] = &[
    "fileFormatVersion",
    "texturePackerVersion",
    "fileName",
    "ignoredWarnings",
];

/// Claves que este traductor sí aplica (arriba y dentro de las structs que
/// recorre). Cualquier otra clave del fichero se avisa una sola vez, en
/// bloque, al final.
const SUPPORTED_KEYS: &[&str] = &[
    // Superior
    "allowRotation",
    "shapeDebug",
    "dpi",
    "dataFormat",
    "flipPVR",
    "ditherType",
    "backgroundColor",
    "shapePadding",
    "jpgQuality",
    "pngOptimizationLevel",
    "webpQualityLevel",
    "textureSubPath",
    "borderPadding",
    "maxTextureSize",
    "fixedTextureSize",
    "algorithmSettings",
    "libGdx",
    "dataFileNames",
    "multiPack",
    "outputFormat",
    "alphaHandling",
    "contentProtection",
    "autoAliasEnabled",
    "trimSpriteNames",
    "prependSmartFolderName",
    "autodetectAnimations",
    "globalSpriteSettings",
    "individualSpriteSettings",
    "fileList",
    "commonDivisorX",
    "commonDivisorY",
    "packNormalMaps",
    "autodetectNormalMaps",
    "normalMapFilter",
    "normalMapSuffix",
    "normalMapSheetFileName",
    "autoSDSettings",
    // QSizes y structs anidadas
    "width",
    "height",
    "algorithm",
    "sizeConstraints",
    "forceSquared",
    "freeSizeMode",
    "basic",
    "sortBy",
    "order",
    "polygon",
    "alignToGrid",
    "filtering",
    "x",
    "y",
    "data",
    "name",
    "key",
    // Ajustes globales de sprite
    "scale",
    "scaleMode",
    "extrude",
    "trimThreshold",
    "trimMargin",
    "trimMode",
    "heuristicMask",
    "defaultPivotPoint",
    // Ajustes por sprite
    "pivotPoint",
    // autoSDSettings
    "extension",
    "spriteFilter",
    "acceptFractionalValues",
];

fn collect_keys(pairs: &[(String, Value)], out: &mut BTreeSet<String>) {
    for (key, value) in pairs {
        // Las claves de `individualSpriteSettings` son rutas de fichero: se
        // saltan para que un sprite no suelte un aviso por su propio nombre.
        let looks_like_path = key.contains('/') || Path::new(key).extension().is_some();
        if !looks_like_path {
            out.insert(key.clone());
        }
        match value {
            Value::Map(inner) => collect_keys(inner, out),
            Value::List(items) => {
                for item in items {
                    if let Value::Map(inner) = item {
                        collect_keys(inner, out);
                    }
                }
            }
            _ => {}
        }
    }
}

fn apply_settings(pairs: &[(String, Value)], cfg: &mut ProjectConfig, base: Option<&Path>) {
    // El formato de datos primero: `apply_data_format_defaults` recomienda
    // rotación/algoritmo y lo que diga el .tps debe ganar después.
    if let Some(id) = get(pairs, "dataFormat").and_then(as_str) {
        if !cfg.apply_data_format(id) {
            // Id desconocido: se deja el por defecto y se avisa aparte.
            cfg.data_format.clear();
        }
    }

    let bool_of = |key: &str, slot: &mut bool| {
        if let Some(v) = get(pairs, key).and_then(as_bool) {
            *slot = v;
        }
    };
    bool_of("allowRotation", &mut cfg.allow_rotation);
    bool_of("shapeDebug", &mut cfg.shape_debug);
    bool_of("multiPack", &mut cfg.multipack);
    bool_of("autoAliasEnabled", &mut cfg.enable_aliasing);
    bool_of("trimSpriteNames", &mut cfg.trim_sprite_names);
    bool_of("prependSmartFolderName", &mut cfg.prepend_folder_name);
    bool_of(
        "autodetectAnimations",
        &mut cfg.enable_auto_detect_animations,
    );
    bool_of("packNormalMaps", &mut cfg.enable_normal_maps);
    bool_of("autodetectNormalMaps", &mut cfg.normal_map_auto_detect);

    if let Some(v) = get(pairs, "dpi").and_then(as_uint) {
        cfg.dpi = (v > 0).then_some(v as u32);
    }
    if let Some(v) = get(pairs, "flipPVR").and_then(as_bool) {
        cfg.flip_vertical = v;
    }
    if let Some(v) = get(pairs, "ditherType") {
        if let Some(d) = parse_enum::<PngDither>(v) {
            cfg.png8_dither = d;
        }
    }
    if let Some(v) = get(pairs, "backgroundColor").and_then(as_uint) {
        cfg.background_color = if v == 0 {
            None
        } else {
            Some([
                ((v >> 16) & 0xff) as u8,
                ((v >> 8) & 0xff) as u8,
                (v & 0xff) as u8,
                ((v >> 24) & 0xff).max(1) as u8,
            ])
        };
    }
    if let Some(v) = get(pairs, "shapePadding").and_then(as_uint) {
        cfg.padding = v.min(i32::MAX as u64) as i32;
    }
    if let Some(v) = get(pairs, "borderPadding").and_then(as_uint) {
        cfg.border_padding = v.min(i32::MAX as u64) as i32;
    }
    if let Some(v) = get(pairs, "jpgQuality").and_then(as_uint) {
        cfg.jpg_quality = v.min(100) as u8;
    }
    if let Some(v) = get(pairs, "pngOptimizationLevel").and_then(as_uint) {
        cfg.png_opt_level = v.min(7) as u8;
    }
    if let Some(v) = get(pairs, "webpQualityLevel").and_then(as_uint) {
        cfg.webp_quality = v.min(u16::MAX as u64) as u16;
    }
    if let Some(v) = get(pairs, "textureSubPath").and_then(as_str) {
        cfg.texture_path = (!v.is_empty()).then(|| v.to_string());
    }
    if let Some(v) = get(pairs, "outputFormat") {
        if let Some(fmt) = parse_enum::<PixelFormat>(v) {
            cfg.pixel_format = fmt;
        }
    }
    if let Some(v) = get(pairs, "alphaHandling") {
        if let Some(a) = parse_enum::<AlphaHandling>(v) {
            cfg.alpha_handling = a;
        }
    }
    if let Some(v) = get(pairs, "commonDivisorX").and_then(as_uint) {
        cfg.common_divisor_x = v.min(i32::MAX as u64) as i32;
    }
    if let Some(v) = get(pairs, "commonDivisorY").and_then(as_uint) {
        cfg.common_divisor_y = v.min(i32::MAX as u64) as i32;
    }
    if let Some(v) = get(pairs, "normalMapFilter").and_then(as_str) {
        cfg.normal_map_filter = v.to_string();
    }
    if let Some(v) = get(pairs, "normalMapSuffix").and_then(as_str) {
        cfg.normal_map_suffix = v.to_string();
    }
    if let Some(v) = get(pairs, "normalMapSheetFileName").and_then(as_str) {
        cfg.normal_map_sheet = v.to_string();
    }

    // Tamaños: el original da un par por eje, aquí el tope es un cuadrado.
    if let Some(Value::Size(w, h)) = get(pairs, "maxTextureSize") {
        let w = *w;
        let h = *h;
        if w > 0 && h > 0 {
            cfg.max_texture_size = w.max(h).min(i32::MAX as i64) as i32;
            if w != h {
                cfg.max_width = w.min(i32::MAX as i64) as i32;
                cfg.max_height = h.min(i32::MAX as i64) as i32;
            }
        }
    }
    if let Some(Value::Size(w, h)) = get(pairs, "fixedTextureSize") {
        cfg.fixed_width = (*w).max(0).min(i32::MAX as i64) as i32;
        cfg.fixed_height = (*h).max(0).min(i32::MAX as i64) as i32;
    }

    if let Some(algo) = get(pairs, "algorithmSettings").and_then(as_map) {
        if let Some(v) = get(algo, "algorithm") {
            if let Some(a) = parse_enum::<PackingAlgorithm>(v) {
                cfg.algorithm = a;
            }
        }
        if let Some(v) = get(algo, "sizeConstraints") {
            if let Some(s) = parse_enum::<SizeConstraint>(v) {
                cfg.size_constraints = s;
            }
        }
        if let Some(v) = get(algo, "forceSquared").and_then(as_bool) {
            cfg.force_squared = v;
        }
        if let Some(v) = get(algo, "freeSizeMode") {
            if let Some(m) = parse_enum::<PackMode>(v) {
                cfg.pack_mode = m;
            }
        }
        if let Some(basic) = get(algo, "basic").and_then(as_map) {
            if let Some(v) = get(basic, "sortBy") {
                if let Some(s) = parse_enum::<crate::config::BasicSortBy>(v) {
                    cfg.basic_sort_by = s;
                }
            }
            if let Some(v) = get(basic, "order") {
                if let Some(o) = parse_enum::<SortOrder>(v) {
                    cfg.basic_order = o;
                }
            }
        }
        if let Some(poly) = get(algo, "polygon").and_then(as_map) {
            if let Some(v) = get(poly, "alignToGrid").and_then(as_uint) {
                cfg.align_to_grid = v.min(i32::MAX as u64) as i32;
            }
        }
    }

    if let Some(libgdx) = get(pairs, "libGdx").and_then(as_map) {
        if let Some(filtering) = get(libgdx, "filtering").and_then(as_map) {
            if let Some(v) = get(filtering, "x") {
                if let Some(f) = parse_enum::<GdxFilter>(v) {
                    cfg.gdx_filter = f;
                }
            }
        }
    }

    // Nombre base del fichero de datos: `UI_Assets-{n}.json` → `UI_Assets`.
    if let Some(data) = get(pairs, "dataFileNames").and_then(as_map) {
        if let Some(entry) = get(data, "data").and_then(as_map) {
            if let Some(name) = get(entry, "name").and_then(as_str) {
                let stem = name.rsplit_once('.').map(|(head, _)| head).unwrap_or(name);
                // `atlas-{n}.json` → `atlas`: el marcador y su separador.
                let stem = stem.split("{n}").next().unwrap_or(stem);
                let stem = stem.trim_end_matches(['-', '_', '.', ' ']);
                if !stem.is_empty() {
                    cfg.base_file_name = stem.to_string();
                }
            }
        }
    }

    if let Some(v) = get(pairs, "contentProtection").and_then(as_map) {
        if let Some(key) = get(v, "key").and_then(as_str) {
            cfg.encryption_key = (!key.is_empty()).then(|| key.to_string());
        }
    }

    if let Some(g) = get(pairs, "globalSpriteSettings").and_then(as_map) {
        if let Some(v) = get(g, "scaleMode") {
            if let Some(m) = parse_enum::<ScaleMode>(v) {
                cfg.scale_mode = m;
            }
        }
        if let Some(v) = get(g, "extrude").and_then(as_uint) {
            cfg.extrude = v.min(i32::MAX as u64) as i32;
        }
        if let Some(v) = get(g, "trimThreshold").and_then(as_uint) {
            cfg.trim_threshold = v.min(255) as i32;
        }
        if let Some(v) = get(g, "trimMargin").and_then(as_uint) {
            cfg.trim_margin = v.min(i32::MAX as u64) as i32;
        }
        if let Some(v) = get(g, "trimMode") {
            if let Some(mode) = parse_enum::<TrimMode>(v) {
                cfg.enable_trim = mode != TrimMode::None;
                cfg.trim_mode = if mode == TrimMode::None {
                    TrimMode::Trim
                } else {
                    mode
                };
            }
        }
        if let Some(v) = get(g, "heuristicMask").and_then(as_bool) {
            cfg.heuristic_mask = v;
        }
        if let Some(Value::Point(x, y)) = get(g, "defaultPivotPoint") {
            cfg.default_pivot_x = (*x).clamp(0.0, 1.0);
            cfg.default_pivot_y = (*y).clamp(0.0, 1.0);
        }
    }

    // Pivots por sprite: la clave es la ruta de la imagen, el valor sólo
    // aporta `pivotPoint` (los bordes scale9 no tienen aquí dónde medirse).
    if let Some(map) = get(pairs, "individualSpriteSettings").and_then(as_map) {
        for (file, value) in map {
            let Some(inner) = as_map(value) else { continue };
            let Some(Value::Point(x, y)) = get(inner, "pivotPoint") else {
                continue;
            };
            let pivot = Point2D::new((*x).clamp(0.0, 1.0), (*y).clamp(0.0, 1.0));
            cfg.pivot_overrides.insert(file.clone(), pivot);
            // El id de sprite aquí puede llevar la extensión o no, según
            // «trim sprite names»: se registran las dos formas.
            if let Some(stem) = Path::new(file.as_str())
                .file_stem()
                .and_then(|s| s.to_str())
            {
                let stem = if file.contains('/') {
                    format!(
                        "{}/{}",
                        file.rsplit_once('/').map(|(d, _)| d).unwrap_or(""),
                        stem
                    )
                } else {
                    stem.to_string()
                };
                if stem != *file {
                    cfg.pivot_overrides.entry(stem).or_insert(pivot);
                }
            }
        }
    }

    // Variantes de escala (autoSDSettings).
    if let Some(list) = get(pairs, "autoSDSettings").and_then(as_list) {
        let scales: Vec<f32> = list
            .iter()
            .filter_map(as_map)
            .filter_map(|entry| get(entry, "scale").and_then(as_f64))
            .map(|s| s as f32)
            .filter(|s| s.is_finite() && *s > 0.0)
            .collect();
        if !scales.is_empty() {
            cfg.scale_variants = scales;
        }
    }

    // Ficheros de entrada: rutas relativas al propio .tps.
    if let Some(list) = get(pairs, "fileList").and_then(as_list) {
        let mut inputs: Vec<PathBuf> = list.iter().filter_map(as_str).map(PathBuf::from).collect();
        if let Some(base) = base {
            for path in &mut inputs {
                if path.is_relative() {
                    *path = base.join(&*path);
                }
            }
        }
        if let Some(first) = inputs.first().cloned() {
            cfg.input_directory = first;
        }
        cfg.extra_inputs.extend(inputs.into_iter().skip(1));
    }
}

/// Ajustes del `.tps` sin equivalente aquí: una sola línea con el recuento
/// y los primeros nombres, para no inundar el registro.
fn unsupported_warning(unsupported: &BTreeSet<String>) -> Option<String> {
    if unsupported.is_empty() {
        return None;
    }
    const SHOW: usize = 8;
    let names: Vec<&str> = unsupported.iter().map(String::as_str).collect();
    let shown = names[..names.len().min(SHOW)].join(", ");
    let more = names.len().saturating_sub(SHOW);
    Some(if more > 0 {
        format!(
            "{} ajustes del .tps no tienen equivalente aquí y se ignoran: {shown}, +{more}",
            names.len()
        )
    } else {
        format!(
            "{} ajustes del .tps no tienen equivalente aquí y se ignoran: {shown}",
            names.len()
        )
    })
}

// ---------------------------------------------------------------------------
// API pública
// ---------------------------------------------------------------------------

/// Lee un `.tps` y lo traduce a [`ProjectConfig`]. Las rutas relativas del
/// fichero se resuelven contra `base`.
pub fn parse_tps(text: &str, base: Option<&Path>) -> Result<TpsProject> {
    let pairs = parse_document(text)?;
    let mut config = ProjectConfig::default();
    apply_settings(&pairs, &mut config, base);

    let mut keys = BTreeSet::new();
    collect_keys(&pairs, &mut keys);
    let unsupported: BTreeSet<String> = keys
        .into_iter()
        .filter(|k| !SUPPORTED_KEYS.contains(&k.as_str()) && !IGNORED_KEYS.contains(&k.as_str()))
        .collect();

    let mut warnings = Vec::new();
    if let Some(w) = unsupported_warning(&unsupported) {
        warnings.push(w);
    }
    if config.data_format.is_empty() && get(&pairs, "dataFormat").is_some() {
        warnings.push("formato de datos del .tps desconocido: se usa el por defecto".into());
    }
    if config.output_directory.as_os_str().is_empty() {
        warnings.push("el .tps no trae directorio de salida: elige dónde escribir la hoja".into());
    }
    Ok(TpsProject { config, warnings })
}

/// Lee un fichero `.tps` del disco; las rutas relativas se resuelven contra
/// la carpeta del propio fichero.
pub fn load_tps(path: &Path) -> Result<TpsProject> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| TpError::Other(format!("No se pudo leer {}: {e}", path.display())))?;
    parse_tps(&text, path.parent())
}

/// Escribe la configuración como `.tps` (el subconjunto que aquí se
/// entiende; el resto lo pone el original con sus valores por defecto).
pub fn write_tps(config: &ProjectConfig) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<data version=\"1.0\">\n    <struct type=\"Settings\">\n");
    let mut w = Writer {
        out: &mut s,
        depth: 2,
        stack: Vec::new(),
    };
    // La cabecera ya abrió `<struct type="Settings">`: entra en la pila para
    // que `finish()` la cierre antes que `<data>`.
    w.stack.push("struct".to_string());
    w.int("fileFormatVersion", 4);
    w.str_("texturePackerVersion", "TexturePacker-RS");
    w.bool("allowRotation", config.allow_rotation);
    w.bool("shapeDebug", config.shape_debug);
    w.uint("dpi", config.dpi.unwrap_or(72).into());
    if !config.data_format.is_empty() {
        w.str_("dataFormat", &config.data_format);
    }
    w.bool("flipPVR", config.flip_vertical);
    w.enum_("ditherType", &enum_name(&config.png8_dither));
    w.uint(
        "backgroundColor",
        match config.background_color {
            None => 0,
            Some([r, g, b, a]) => {
                ((a as u64) << 24) | ((r as u64) << 16) | ((g as u64) << 8) | (b as u64)
            }
        },
    );
    w.uint("shapePadding", config.padding.max(0) as u64);
    w.uint("jpgQuality", config.jpg_quality.into());
    w.uint("pngOptimizationLevel", config.png_opt_level.into());
    w.uint("webpQualityLevel", config.webp_quality.into());
    w.str_(
        "textureSubPath",
        config.texture_path.as_deref().unwrap_or(""),
    );
    w.uint("borderPadding", config.border_padding.max(0) as u64);
    w.qsize(
        "maxTextureSize",
        config.max_texture_size.max(1),
        config.max_texture_size.max(1),
    );
    w.qsize(
        "fixedTextureSize",
        if config.fixed_width > 0 {
            config.fixed_width
        } else {
            -1
        },
        if config.fixed_height > 0 {
            config.fixed_height
        } else {
            -1
        },
    );
    w.algorithm_settings(config);
    w.lib_gdx(config);
    w.data_file_names(config);
    w.bool("multiPack", config.multipack);
    w.enum_("outputFormat", &enum_name(&config.pixel_format));
    w.enum_("alphaHandling", &enum_name(&config.alpha_handling));
    w.content_protection(config);
    w.bool("autoAliasEnabled", config.enable_aliasing);
    w.bool("trimSpriteNames", config.trim_sprite_names);
    w.bool("prependSmartFolderName", config.prepend_folder_name);
    w.bool("autodetectAnimations", config.enable_auto_detect_animations);
    w.global_sprite_settings(config);
    w.individual_sprite_settings(config);
    w.file_list(config);
    w.uint("commonDivisorX", config.common_divisor_x.max(0) as u64);
    w.uint("commonDivisorY", config.common_divisor_y.max(0) as u64);
    w.bool("packNormalMaps", config.enable_normal_maps);
    w.bool("autodetectNormalMaps", config.normal_map_auto_detect);
    w.str_("normalMapFilter", &config.normal_map_filter);
    w.str_("normalMapSuffix", &config.normal_map_suffix);
    w.str_("normalMapSheetFileName", &config.normal_map_sheet);
    w.auto_sd_settings(config);
    w.finish();
    s
}

/// Escribe el `.tps` en disco.
pub fn save_tps(config: &ProjectConfig, path: &Path) -> Result<()> {
    std::fs::write(path, write_tps(config))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Escritor
// ---------------------------------------------------------------------------

/// Genera el XML con la misma forma que el original: pares `<key>` + valor,
/// con la pila de etiquetas abiertas para no perder ningún cierre.
struct Writer<'a> {
    out: &'a mut String,
    depth: usize,
    stack: Vec<String>,
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl Writer<'_> {
    fn pad(&self) -> String {
        "    ".repeat(self.depth)
    }

    fn line(&mut self, key: &str, value: &str) {
        let pad = self.pad();
        self.out
            .push_str(&format!("{pad}<key>{key}</key>\n{pad}{value}\n"));
    }

    fn str_(&mut self, key: &str, value: &str) {
        self.line(key, &format!("<string>{}</string>", esc(value)));
    }

    fn filename(&mut self, key: &str, value: &str) {
        self.line(key, &format!("<filename>{}</filename>", esc(value)));
    }

    fn enum_(&mut self, key: &str, value: &str) {
        self.line(key, &format!("<enum>{}</enum>", esc(value)));
    }

    fn int(&mut self, key: &str, value: i64) {
        self.line(key, &format!("<int>{value}</int>"));
    }

    fn uint(&mut self, key: &str, value: u64) {
        self.line(key, &format!("<uint>{value}</uint>"));
    }

    fn double(&mut self, key: &str, value: f64) {
        self.line(key, &format!("<double>{value}</double>"));
    }

    fn bool(&mut self, key: &str, value: bool) {
        let tag = if value { "true" } else { "false" };
        self.line(key, &format!("<{tag}/>"));
    }

    fn point(&mut self, key: &str, x: f32, y: f32) {
        self.line(key, &format!("<point_f>{x},{y}</point_f>"));
    }

    fn qsize(&mut self, key: &str, width: i32, height: i32) {
        self.line(key, "<QSize>");
        self.depth += 1;
        self.int("width", width.into());
        self.int("height", height.into());
        self.depth -= 1;
        let pad = self.pad();
        self.out.push_str(&pad);
        self.out.push_str("</QSize>\n");
    }

    fn open(&mut self, key: &str, tag: &str, attrs: &str) {
        self.line(key, &format!("<{tag}{attrs}>"));
        self.depth += 1;
        self.stack.push(tag.to_string());
    }

    fn close(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        let pad = self.pad();
        if let Some(tag) = self.stack.pop() {
            self.out.push_str(&format!("{pad}</{tag}>\n"));
        }
    }

    /// Cierra todo lo que quede abierto y el `<data>`.
    fn finish(mut self) {
        while !self.stack.is_empty() {
            self.close();
        }
        self.out.push_str("</data>\n");
    }
}

/// Bloques compuestos (structs, mapas y arrays) que escribe el `.tps`.
impl Writer<'_> {
    /// Escribe una clave de fichero: `<key type="filename">ruta</key>`.
    fn key_filename(&mut self, key: &str) {
        let pad = self.pad();
        self.out
            .push_str(&format!("{pad}<key type=\"filename\">{}</key>\n", esc(key)));
    }

    /// Abre una etiqueta después de haber escrito la clave a mano.
    fn open_after_key(&mut self, tag: &str, attrs: &str) {
        let pad = self.pad();
        self.out.push_str(&format!("{pad}<{tag}{attrs}>\n"));
        self.depth += 1;
        self.stack.push(tag.to_string());
    }

    fn filename_item(&mut self, path: &std::path::Path) {
        let pad = self.pad();
        let text = path.to_string_lossy().into_owned();
        self.out
            .push_str(&format!("{pad}<filename>{}</filename>\n", esc(&text)));
    }

    fn algorithm_settings(&mut self, config: &ProjectConfig) {
        self.open("algorithmSettings", "struct", " type=\"AlgorithmSettings\"");
        self.enum_("algorithm", &enum_name(&config.algorithm));
        self.enum_("freeSizeMode", &enum_name(&config.pack_mode));
        self.enum_("sizeConstraints", &enum_name(&config.size_constraints));
        self.bool("forceSquared", config.force_squared);
        self.open("basic", "struct", " type=\"AlgorithmBasicSettings\"");
        self.enum_("sortBy", &enum_name(&config.basic_sort_by));
        self.enum_("order", &enum_name(&config.basic_order));
        self.close();
        self.open("polygon", "struct", " type=\"AlgorithmPolygonSettings\"");
        self.uint("alignToGrid", config.align_to_grid.max(0) as u64);
        self.close();
        self.close();
    }

    fn lib_gdx(&mut self, config: &ProjectConfig) {
        self.open("libGdx", "struct", " type=\"LibGDX\"");
        self.open("filtering", "struct", " type=\"LibGDXFiltering\"");
        self.enum_("x", &enum_name(&config.gdx_filter));
        self.enum_("y", &enum_name(&config.gdx_filter));
        self.close();
        self.close();
    }

    fn data_file_names(&mut self, config: &ProjectConfig) {
        let extension = crate::dataformats::find_data_format(&config.data_format)
            .map(|p| p.extension)
            .filter(|e| !e.is_empty())
            .unwrap_or("json");
        let name = format!("{}-{{n}}.{extension}", config.base_file_name);
        self.open("dataFileNames", "map", " type=\"GFileNameMap\"");
        self.open("data", "struct", " type=\"DataFile\"");
        self.filename("name", &name);
        self.close();
        self.close();
    }

    fn content_protection(&mut self, config: &ProjectConfig) {
        self.open("contentProtection", "struct", " type=\"ContentProtection\"");
        self.str_("key", config.encryption_key.as_deref().unwrap_or(""));
        self.close();
    }

    fn global_sprite_settings(&mut self, config: &ProjectConfig) {
        self.open("globalSpriteSettings", "struct", " type=\"SpriteSettings\"");
        self.double("scale", 1.0);
        self.enum_("scaleMode", &enum_name(&config.scale_mode));
        self.uint("extrude", config.extrude.max(0) as u64);
        self.uint("trimThreshold", config.trim_threshold.clamp(0, 255) as u64);
        self.uint("trimMargin", config.trim_margin.max(0) as u64);
        let trim = if config.enable_trim {
            enum_name(&config.trim_mode)
        } else {
            "None".to_string()
        };
        self.enum_("trimMode", &trim);
        self.bool("heuristicMask", config.heuristic_mask);
        self.point(
            "defaultPivotPoint",
            config.default_pivot_x,
            config.default_pivot_y,
        );
        self.close();
    }

    fn individual_sprite_settings(&mut self, config: &ProjectConfig) {
        if config.pivot_overrides.is_empty() {
            return;
        }
        self.open(
            "individualSpriteSettings",
            "map",
            " type=\"IndividualSpriteSettingsMap\"",
        );
        let mut entries: Vec<(&String, &Point2D)> = config.pivot_overrides.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (file, pivot) in entries {
            self.key_filename(file);
            self.open_after_key("struct", " type=\"IndividualSpriteSettings\"");
            self.point("pivotPoint", pivot.x, pivot.y);
            self.close();
        }
        self.close();
    }

    fn file_list(&mut self, config: &ProjectConfig) {
        self.open("fileList", "array", "");
        if !config.input_directory.as_os_str().is_empty() {
            self.filename_item(&config.input_directory);
        }
        for extra in &config.extra_inputs {
            self.filename_item(extra);
        }
        self.close();
    }

    fn auto_sd_settings(&mut self, config: &ProjectConfig) {
        self.open("autoSDSettings", "array", "");
        for scale in &config.scale_variants {
            self.open_after_key("struct", " type=\"AutoSDSettings\"");
            self.double("scale", f64::from(*scale));
            self.str_("extension", "");
            self.str_("spriteFilter", "");
            self.bool("acceptFractionalValues", false);
            self.qsize("maxTextureSize", -1, -1);
            self.close();
        }
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AlphaHandling, BasicSortBy, GdxFilter, PackMode, PackingAlgorithm, PixelFormat, PngDither,
        ScaleMode, SizeConstraint, SortOrder, TrimMode,
    };

    /// Un `.tps` mínimo pero representativo: casi todas las formas del
    /// formato (QSize, enums, structs anidadas, mapas, arrays, punto,
    /// rect) y una clave agrupada de varias rutas.
    fn sample() -> &'static str {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<data version="1.0">
    <struct type="Settings">
        <key>fileFormatVersion</key>
        <int>4</int>
        <key>texturePackerVersion</key>
        <string>4.5.0</string>
        <key>fileName</key>
        <string>/ruta/al/proyecto.tps</string>
        <key>allowRotation</key>
        <true/>
        <key>shapeDebug</key>
        <false/>
        <key>dpi</key>
        <uint>144</uint>
        <key>dataFormat</key>
        <string>json</string>
        <key>flipPVR</key>
        <true/>
        <key>ditherType</key>
        <enum type="SettingsBase::DitherType">PngQuantMedium</enum>
        <key>backgroundColor</key>
        <uint>4278190080</uint>
        <key>shapePadding</key>
        <uint>3</uint>
        <key>jpgQuality</key>
        <uint>90</uint>
        <key>pngOptimizationLevel</key>
        <uint>6</uint>
        <key>webpQualityLevel</key>
        <uint>75</uint>
        <key>textureSubPath</key>
        <string>/assets</string>
        <key>borderPadding</key>
        <uint>4</uint>
        <key>maxTextureSize</key>
        <QSize>
            <key>width</key>
            <int>1024</int>
            <key>height</key>
            <int>512</int>
        </QSize>
        <key>fixedTextureSize</key>
        <QSize>
            <key>width</key>
            <int>-1</int>
            <key>height</key>
            <int>-1</int>
        </QSize>
        <key>algorithmSettings</key>
        <struct type="AlgorithmSettings">
            <key>algorithm</key>
            <enum>MaxRects</enum>
            <key>freeSizeMode</key>
            <enum>Best</enum>
            <key>sizeConstraints</key>
            <enum>POT</enum>
            <key>forceSquared</key>
            <true/>
            <key>basic</key>
            <struct type="AlgorithmBasicSettings">
                <key>sortBy</key>
                <enum>Area</enum>
                <key>order</key>
                <enum>Descending</enum>
            </struct>
            <key>polygon</key>
            <struct type="AlgorithmPolygonSettings">
                <key>alignToGrid</key>
                <uint>2</uint>
            </struct>
        </struct>
        <key>libGdx</key>
        <struct type="LibGDX">
            <key>filtering</key>
            <struct type="LibGDXFiltering">
                <key>x</key>
                <enum>Nearest</enum>
                <key>y</key>
                <enum>Nearest</enum>
            </struct>
        </struct>
        <key>dataFileNames</key>
        <map type="GFileNameMap">
            <key>data</key>
            <struct type="DataFile">
                <key>name</key>
                <filename>hoja-{n}.json</filename>
            </struct>
        </map>
        <key>multiPack</key>
        <false/>
        <key>outputFormat</key>
        <enum>RGB888</enum>
        <key>alphaHandling</key>
        <enum>PremultiplyAlpha</enum>
        <key>contentProtection</key>
        <struct type="ContentProtection">
            <key>key</key>
            <string>secreto</string>
        </struct>
        <key>autoAliasEnabled</key>
        <false/>
        <key>trimSpriteNames</key>
        <false/>
        <key>prependSmartFolderName</key>
        <true/>
        <key>autodetectAnimations</key>
        <false/>
        <key>globalSpriteSettings</key>
        <struct type="SpriteSettings">
            <key>scale</key>
            <double>1</double>
            <key>scaleMode</key>
            <enum>Fast</enum>
            <key>extrude</key>
            <uint>2</uint>
            <key>trimThreshold</key>
            <uint>7</uint>
            <key>trimMargin</key>
            <uint>5</uint>
            <key>trimMode</key>
            <enum>Crop</enum>
            <key>tracerTolerance</key>
            <int>200</int>
            <key>heuristicMask</key>
            <true/>
            <key>defaultPivotPoint</key>
            <point_f>0.25,0.75</point_f>
        </struct>
        <key>individualSpriteSettings</key>
        <map type="IndividualSpriteSettingsMap">
            <key type="filename">PNG/a.png</key>
            <key type="filename">PNG/b.png</key>
            <struct type="IndividualSpriteSettings">
                <key>pivotPoint</key>
                <point_f>1,0</point_f>
                <key>scale9Enabled</key>
                <false/>
                <key>scale9Borders</key>
                <rect>10,9,19,18</rect>
            </struct>
        </map>
        <key>fileList</key>
        <array>
            <filename>sprites</filename>
        </array>
        <key>commonDivisorX</key>
        <uint>4</uint>
        <key>commonDivisorY</key>
        <uint>8</uint>
        <key>packNormalMaps</key>
        <true/>
        <key>autodetectNormalMaps</key>
        <false/>
        <key>normalMapFilter</key>
        <string>normals/</string>
        <key>normalMapSuffix</key>
        <string>_n</string>
        <key>normalMapSheetFileName</key>
        <filename>hoja_normal</filename>
        <key>textureFormat</key>
        <enum>png8</enum>
        <key>autoSDSettings</key>
        <array>
            <struct type="AutoSDSettings">
                <key>scale</key>
                <double>1</double>
            </struct>
            <struct type="AutoSDSettings">
                <key>scale</key>
                <double>0.5</double>
            </struct>
        </array>
    </struct>
</data>
"#
    }

    #[test]
    fn lee_un_tps_completo() {
        let tps = parse_tps(sample(), None).expect("el .tps debe parsear");
        let cfg = &tps.config;
        assert!(cfg.allow_rotation);
        assert!(!cfg.shape_debug);
        assert_eq!(cfg.dpi, Some(144));
        assert_eq!(cfg.data_format, "json");
        assert!(cfg.flip_vertical);
        assert_eq!(cfg.png8_dither, PngDither::Medium);
        assert_eq!(
            cfg.background_color,
            Some([0, 0, 0, 255]),
            "4278190080 = alfa 0 sobre negro opaco"
        );
        assert_eq!(cfg.padding, 3);
        assert_eq!(cfg.border_padding, 4);
        assert_eq!(cfg.jpg_quality, 90);
        assert_eq!(cfg.png_opt_level, 6);
        assert_eq!(cfg.webp_quality, 75);
        assert_eq!(cfg.texture_path.as_deref(), Some("/assets"));
        assert_eq!(cfg.max_texture_size, 1024, "el tope es el mayor de los dos");
        assert_eq!(cfg.max_width, 1024);
        assert_eq!(cfg.max_height, 512);
        assert_eq!(cfg.fixed_width, 0, "-1 significa tamaño libre");
        assert_eq!(cfg.algorithm, PackingAlgorithm::MaxRects);
        assert_eq!(cfg.pack_mode, PackMode::Best);
        assert_eq!(cfg.size_constraints, SizeConstraint::Pot);
        assert!(cfg.force_squared);
        assert_eq!(cfg.basic_sort_by, BasicSortBy::Area);
        assert_eq!(cfg.basic_order, SortOrder::Descending);
        assert_eq!(cfg.align_to_grid, 2);
        assert_eq!(cfg.gdx_filter, GdxFilter::Nearest);
        assert_eq!(cfg.base_file_name, "hoja");
        assert!(!cfg.multipack);
        assert_eq!(cfg.pixel_format, PixelFormat::Rgb888);
        assert_eq!(cfg.alpha_handling, AlphaHandling::PremultiplyAlpha);
        assert_eq!(cfg.encryption_key.as_deref(), Some("secreto"));
        assert!(!cfg.enable_aliasing);
        assert!(!cfg.trim_sprite_names);
        assert!(cfg.prepend_folder_name);
        assert!(!cfg.enable_auto_detect_animations);
        assert_eq!(cfg.scale_mode, ScaleMode::Fast);
        assert_eq!(cfg.extrude, 2);
        assert_eq!(cfg.trim_threshold, 7);
        assert_eq!(cfg.trim_margin, 5);
        assert_eq!(cfg.trim_mode, TrimMode::Crop);
        assert!(cfg.enable_trim);
        assert!(cfg.heuristic_mask);
        assert_eq!(cfg.default_pivot_x, 0.25);
        assert_eq!(cfg.default_pivot_y, 0.75);
        assert_eq!(
            cfg.pivot_overrides.get("PNG/a.png"),
            Some(&Point2D::new(1.0, 0.0)),
            "una clave compartida por varias rutas se aplica a todas"
        );
        assert_eq!(
            cfg.pivot_overrides.get("PNG/b.png"),
            Some(&Point2D::new(1.0, 0.0))
        );
        assert_eq!(
            cfg.pivot_overrides.get("PNG/a"),
            Some(&Point2D::new(1.0, 0.0)),
            "también sin extensión (trim sprite names)"
        );
        assert_eq!(
            cfg.input_directory,
            PathBuf::from("sprites"),
            "sin base, la ruta relativa se queda como está"
        );
        assert_eq!(cfg.common_divisor_x, 4);
        assert_eq!(cfg.common_divisor_y, 8);
        assert!(cfg.enable_normal_maps);
        assert!(!cfg.normal_map_auto_detect);
        assert_eq!(cfg.normal_map_filter, "normals/");
        assert_eq!(cfg.normal_map_suffix, "_n");
        assert_eq!(cfg.normal_map_sheet, "hoja_normal");
        assert_eq!(cfg.scale_variants, vec![1.0, 0.5]);
    }

    #[test]
    fn las_claves_sin_equivalente_se_avisan_en_bloque() {
        let tps = parse_tps(sample(), None).expect("el .tps debe parsear");
        let warned = tps
            .warnings
            .iter()
            .find(|w| w.contains("no tienen equivalente"))
            .expect("debe avisar de los ajustes no soportados");
        assert!(warned.contains("textureFormat"), "aviso: {warned}");
        assert!(warned.contains("scale9Borders"), "aviso: {warned}");
        assert!(warned.contains("tracerTolerance"), "aviso: {warned}");
        assert!(
            !warned.contains("fileFormatVersion"),
            "los metadatos no cuentan: {warned}"
        );
        assert!(
            !warned.contains("allowRotation"),
            "los ajustes traducidos no cuentan: {warned}"
        );
        assert_eq!(
            tps.warnings
                .iter()
                .filter(|w| w.contains("no tienen"))
                .count(),
            1,
            "un solo aviso por fichero"
        );
    }

    #[test]
    fn las_rutas_relativas_se_resuelven_contra_la_carpeta_del_tps() {
        let base = Path::new("/proyectos/hoja");
        let tps = parse_tps(sample(), Some(base)).expect("el .tps debe parsear");
        assert_eq!(tps.config.input_directory, base.join("sprites"));
    }

    #[test]
    fn un_xml_roto_no_es_un_panic() {
        let err = parse_tps("<data><struct", None).expect_err("debe fallar");
        assert!(err.to_string().contains(".tps"), "error: {err}");
        let err = parse_tps("<otra-raiz/>", None).expect_err("debe fallar");
        assert!(err.to_string().contains("<data>"), "error: {err}");
    }

    #[test]
    fn lo_que_escribe_vuelve_a_leerse_igual() {
        let mut cfg = ProjectConfig {
            allow_rotation: false,
            output_directory: PathBuf::from("salida"),
            input_directory: PathBuf::from("sprites"),
            extra_inputs: vec![PathBuf::from("sprites2")],
            base_file_name: "hoja".into(),
            data_format: "json".into(),
            padding: 5,
            border_padding: 2,
            extrude: 3,
            enable_trim: true,
            trim_mode: TrimMode::CropKeepPos,
            trim_threshold: 11,
            trim_margin: 6,
            enable_aliasing: false,
            multipack: false,
            pixel_format: PixelFormat::Rgba4444,
            alpha_handling: AlphaHandling::ReduceBorderArtifacts,
            algorithm: PackingAlgorithm::Basic,
            pack_mode: PackMode::Fast,
            size_constraints: SizeConstraint::WordAligned,
            force_squared: true,
            basic_sort_by: BasicSortBy::Circumference,
            basic_order: SortOrder::Ascending,
            align_to_grid: 8,
            gdx_filter: GdxFilter::Linear,
            png8_dither: PngDither::High,
            scale_mode: ScaleMode::Scale2x,
            default_pivot_x: 0.1,
            default_pivot_y: 0.9,
            background_color: Some([10, 20, 30, 40]),
            dpi: Some(300),
            texture_path: Some("/tex".into()),
            encryption_key: Some("clave".into()),
            jpg_quality: 70,
            png_opt_level: 4,
            webp_quality: 60,
            common_divisor_x: 2,
            common_divisor_y: 3,
            enable_normal_maps: true,
            normal_map_auto_detect: true,
            normal_map_filter: "n/".into(),
            normal_map_suffix: "_nm".into(),
            normal_map_sheet: "hoja_nm".into(),
            max_texture_size: 4096,
            scale_variants: vec![1.0, 0.5],
            ..ProjectConfig::default()
        };
        cfg.pivot_overrides
            .insert("PNG/a.png".into(), Point2D::new(0.3, 0.4));

        let text = write_tps(&cfg);
        assert!(text.starts_with("<?xml version=\"1.0\""), "cabecera");
        assert!(text.contains("</data>\n"), "todo se cierra");

        let back = parse_tps(&text, None).expect("lo escrito vuelve a leerse");
        assert_eq!(
            back.warnings,
            vec!["el .tps no trae directorio de salida: elige dónde escribir la hoja".to_string()],
            "un .tps nuestro sólo avisa de lo que el formato no guarda"
        );
        let got = &back.config;
        // El directorio de salida no vive en el .tps (lo elige el usuario):
        // la única traducción posible es dejarlo como estaba, vacío.
        assert!(got.output_directory.as_os_str().is_empty());
        assert_eq!(got.allow_rotation, cfg.allow_rotation);
        assert_eq!(got.input_directory, cfg.input_directory);
        assert_eq!(got.extra_inputs, cfg.extra_inputs);
        assert_eq!(got.base_file_name, cfg.base_file_name);
        assert_eq!(got.data_format, cfg.data_format);
        assert_eq!(got.padding, cfg.padding);
        assert_eq!(got.border_padding, cfg.border_padding);
        assert_eq!(got.extrude, cfg.extrude);
        assert_eq!(got.enable_trim, cfg.enable_trim);
        assert_eq!(got.trim_mode, cfg.trim_mode);
        assert_eq!(got.trim_threshold, cfg.trim_threshold);
        assert_eq!(got.trim_margin, cfg.trim_margin);
        assert_eq!(got.enable_aliasing, cfg.enable_aliasing);
        assert_eq!(got.multipack, cfg.multipack);
        assert_eq!(got.pixel_format, cfg.pixel_format);
        assert_eq!(got.alpha_handling, cfg.alpha_handling);
        assert_eq!(got.algorithm, cfg.algorithm);
        assert_eq!(got.pack_mode, cfg.pack_mode);
        assert_eq!(got.size_constraints, cfg.size_constraints);
        assert_eq!(got.force_squared, cfg.force_squared);
        assert_eq!(got.basic_sort_by, cfg.basic_sort_by);
        assert_eq!(got.basic_order, cfg.basic_order);
        assert_eq!(got.align_to_grid, cfg.align_to_grid);
        assert_eq!(got.gdx_filter, cfg.gdx_filter);
        assert_eq!(got.png8_dither, cfg.png8_dither);
        assert_eq!(got.scale_mode, cfg.scale_mode);
        assert_eq!(got.default_pivot_x, cfg.default_pivot_x);
        assert_eq!(got.default_pivot_y, cfg.default_pivot_y);
        assert_eq!(got.background_color, cfg.background_color);
        assert_eq!(got.dpi, cfg.dpi);
        assert_eq!(got.texture_path, cfg.texture_path);
        assert_eq!(got.encryption_key, cfg.encryption_key);
        assert_eq!(got.jpg_quality, cfg.jpg_quality);
        assert_eq!(got.png_opt_level, cfg.png_opt_level);
        assert_eq!(got.webp_quality, cfg.webp_quality);
        assert_eq!(got.common_divisor_x, cfg.common_divisor_x);
        assert_eq!(got.common_divisor_y, cfg.common_divisor_y);
        assert_eq!(got.enable_normal_maps, cfg.enable_normal_maps);
        assert_eq!(got.normal_map_auto_detect, cfg.normal_map_auto_detect);
        assert_eq!(got.normal_map_filter, cfg.normal_map_filter);
        assert_eq!(got.normal_map_suffix, cfg.normal_map_suffix);
        assert_eq!(got.normal_map_sheet, cfg.normal_map_sheet);
        assert_eq!(got.max_texture_size, cfg.max_texture_size);
        assert_eq!(got.scale_variants, cfg.scale_variants);
        assert_eq!(
            got.pivot_overrides.get("PNG/a.png"),
            Some(&Point2D::new(0.3, 0.4))
        );
    }

    #[test]
    fn el_trim_desactivado_se_escribe_como_none() {
        let cfg = ProjectConfig {
            output_directory: PathBuf::from("out"),
            enable_trim: false,
            ..ProjectConfig::default()
        };
        let text = write_tps(&cfg);
        let back = parse_tps(&text, None).expect("legible");
        assert!(!back.config.enable_trim);
        assert_eq!(back.config.trim_mode, TrimMode::Trim);
    }

    #[test]
    fn save_tps_escribe_el_fichero() {
        let dir = std::env::temp_dir().join(format!("tp_tps_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp");
        let path = dir.join("proj.tps");
        let cfg = ProjectConfig {
            output_directory: PathBuf::from("out"),
            input_directory: PathBuf::from("sprites"),
            ..ProjectConfig::default()
        };
        save_tps(&cfg, &path).expect("escribir");
        let text = std::fs::read_to_string(&path).expect("leer");
        assert!(text.contains("<data version=\"1.0\">"));
        let back = load_tps(&path).expect("cargar");
        // Relativa al .tps, como en un proyecto real.
        assert_eq!(
            back.config.input_directory,
            path.parent().unwrap().join("sprites")
        );
        assert_eq!(back.config.base_file_name, "atlas");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
