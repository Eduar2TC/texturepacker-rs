use std::path::{Path, PathBuf};

use super::CmdResult;
use tp_core::config::{GpuFormat, PixelFormat, ProjectConfig, TemplateFormat, VariantOptions};

/// Opciones de `pack` que reciben un valor. El parseador no consume el token
/// siguiente para las claves que no están aquí ni en [`PACK_FLAGS`], y
/// [`check_unknown_options`] rechaza cualquier clave al margen de ambas listas.
pub(crate) const PACK_VALUES: &[&str] = &[
    "align",
    "align-to-grid",
    "algorithm",
    "alpha-handling",
    "astc-quality",
    "background-color",
    "base-name",
    "basic-order",
    "basic-sort-by",
    "basis-quality",
    "basisu-quality",
    "border-padding",
    "class-file",
    "classfile-file",
    "color-depth",
    "common-divisor",
    "common-divisor-x",
    "common-divisor-y",
    "convert-texture",
    "css-media-query-2x",
    "css-sprite-prefix",
    "custom-exporters-directory",
    "data",
    "default-pivot-point",
    "dither",
    "dither-type",
    "dpi",
    "dxt-mode",
    "easeljs-framerate",
    "etc1-quality",
    "etc2-quality",
    "extrude",
    "format",
    "gamemaker-texturegroup-frame-speed",
    "gdx-filter",
    "header-file",
    "height",
    "ignore-files",
    "input",
    "jpg-quality",
    "key",
    "key-name",
    "libgdx-legacy-output",
    "maxrects-heuristics",
    "max-size",
    "max-height",
    "max-width",
    "normalmap-filter",
    "normalmap-sheet",
    "normalmap-suffix",
    "opt",
    "orx-includeComments",
    "orx-keyDuration",
    "orx-keepInCache",
    "orx-optimizeSectionNames",
    "orx-pixelSnap",
    "output",
    "pack-mode",
    "padding",
    "pixel-format",
    "plain-bool-property",
    "plain-string-property",
    "png8-dither",
    "png-opt-level",
    "pvr-quality",
    "replace",
    "save",
    "save-key",
    "scale",
    "scale-mode",
    "sheet",
    "shape-padding",
    "size-constraints",
    "source-file",
    "spine-legacy-output",
    "spriteids-file",
    "spritestudio-writePivots",
    "strategy",
    "template",
    "template-format",
    "texture-format",
    "texture-path",
    "texturepath",
    "tolerance",
    "trim-margin",
    "trim-mode",
    "trim-threshold",
    "variant",
    "variants",
    "webp-quality",
    "width",
    "zim-framerate",
];

/// Opciones de `pack` sin valor. Se reconocen antes de mirar si les sigue un
/// token, para que `tp-cli pack sprites --no-rotation` no se coma la carpeta.
pub(crate) const PACK_FLAGS: &[&str] = &[
    "auto-folders",
    "cache-busting",
    "disable-auto-alias",
    "disable-rotation",
    "enable-cache-busting",
    "enable-rotation",
    "exporter-list",
    "flip-pvr",
    "flip-vertical",
    "flip-y",
    "force-identical-layout",
    "force-publish",
    "force-squared",
    "help",
    "heuristic-mask",
    "keep-extension",
    "libgdx-legacy-output",
    "multipack",
    "no-aliasing",
    "no-auto-animations",
    "no-multipack",
    "no-normals",
    "no-recursive",
    "no-rotation",
    "no-trim",
    "normalmap-detect",
    "orx-includeComments",
    "orx-keepInCache",
    "orx-optimizeSectionNames",
    "orx-pixelSnap",
    "pack-normalmaps",
    "polygon",
    "prepend-folder-name",
    "print-json",
    "quiet",
    "shape-debug",
    "spine-legacy-output",
    "spritestudio-writePivots",
    "trim-sprite-names",
    "verbose",
    "version",
];

/// Opciones de `decrypt` (más estrechas que las de `pack`).
pub(crate) const DECRYPT_VALUES: &[&str] = &["key", "o", "out", "pixel-format"];
pub(crate) const DECRYPT_FLAGS: &[&str] = &["help", "quiet", "verbose"];

/// Rechaza lo que no esté en los registros: hoy la CLI ignoraba en silencio
/// cualquier opción desconocida (un `--scale 0.5` sin registrar «funcionaba»
/// sin efecto).
pub(crate) fn check_unknown_options(
    values: &[(String, String)],
    flags: &[String],
    known_values: &[&str],
    known_flags: &[&str],
) -> CmdResult<()> {
    let mut unknown: Vec<String> = values
        .iter()
        .map(|(k, _)| k.as_str())
        .filter(|k| !known_values.contains(k))
        .map(|k| format!("--{k}"))
        .collect();
    let mut missing: Vec<String> = Vec::new();
    for key in flags {
        if known_flags.contains(&key.as_str()) {
            continue;
        }
        if known_values.contains(&key.as_str()) {
            missing.push(format!("--{key}"));
        } else {
            unknown.push(format!("--{key}"));
        }
    }
    if unknown.is_empty() && missing.is_empty() {
        return Ok(());
    }
    unknown.sort();
    unknown.dedup();
    missing.sort();
    missing.dedup();
    let mut problems = Vec::new();
    if !unknown.is_empty() {
        problems.push(format!("opción desconocida: {}", unknown.join(", ")));
    }
    if !missing.is_empty() {
        problems.push(format!("falta el valor de {}", missing.join(", ")));
    }
    Err(format!(
        "{} (usa --help para ver las opciones)",
        problems.join("; ")
    ))
}

/// Opciones que sólo escriben propiedades de un exportador concreto y que
/// este proyecto todavía no plantea. Van registradas (para que no
/// parezcan desconocidas) y se rechazan con su motivo: fingirlas sería peor
/// que no leerlas.
const EXPORTER_ONLY_OPTIONS: &[(&str, &str)] = &[
    ("classfile-file", "monogame"),
    ("easeljs-framerate", "easeljs"),
    (
        "gamemaker-texturegroup-frame-speed",
        "gamemaker-texturegroup",
    ),
    ("libgdx-legacy-output", "libgdx"),
    ("orx-includeComments", "orx"),
    ("orx-keepInCache", "orx"),
    ("orx-keyDuration", "orx"),
    ("orx-optimizeSectionNames", "orx"),
    ("orx-pixelSnap", "orx"),
    ("spine-legacy-output", "spine"),
    ("spritestudio-writePivots", "spritestudio"),
    ("zim-framerate", "zim"),
];

/// Rechaza las opciones de [`EXPORTER_ONLY_OPTIONS`] con un mensaje que
/// explica qué formatos existen y cómo conseguir una salida propia.
pub(crate) fn check_exporter_only_options(
    values: &[(String, String)],
    flags: &[String],
) -> CmdResult<()> {
    let used: Vec<&(&str, &str)> = EXPORTER_ONLY_OPTIONS
        .iter()
        .filter(|(name, _)| {
            values.iter().any(|(k, _)| k == name) || flags.iter().any(|f| f == name)
        })
        .collect();
    if used.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = used.iter().map(|(n, _)| format!("--{n}")).collect();
    let mut formats: Vec<&str> = used.iter().map(|(_, f)| *f).collect();
    formats.sort();
    formats.dedup();
    let formats = formats.join(", ");
    let one = names.len() == 1;
    Err(format!(
        "{}: {} que este clon todavía no escribe; {} aquí para no presentar{} como {}. \
         Usa --exporter-list para ver los formatos soportados o \
         --template/--custom-exporters-directory para una salida propia.",
        names.join(", "),
        if one {
            format!("es una propiedad del exportador {formats}")
        } else {
            format!("son propiedades de los exportadores {formats}")
        },
        if one { "se acepta" } else { "se aceptan" },
        if one { "la" } else { "las" },
        if one {
            "opción desconocida"
        } else {
            "opciones desconocidas"
        },
    ))
}

/// `<bool>`: `true`/`false`, `1`/`0`, `yes`/`no`, `on`/`off`.
fn parse_bool(value: &str, flag: &str) -> CmdResult<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!("{flag} inválido: {value} (true | false)")),
    }
}

/// Divide los argumentos en posicionales, `--clave valor` y `--flag`. Las
/// claves de [`PACK_FLAGS`] nunca consumen el token siguiente, así que
/// `pack sprites --no-rotation` no se come la carpeta.
pub(crate) fn parse_args(args: &[String]) -> (Vec<String>, Vec<(String, String)>, Vec<String>) {
    let mut flags = Vec::new();
    let mut values = Vec::new();
    let mut positional = Vec::new();
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        // `--flag valor` solo cuando el siguiente argumento no es otra opción
        // (así los booleanos como --no-rotation pueden encadenarse).
        if let Some(v) = a.strip_prefix("--") {
            if PACK_FLAGS.contains(&v) {
                flags.push(v.to_string());
                continue;
            }
            match it.peek() {
                Some(next) if !next.starts_with('-') => {
                    let val = it.next().unwrap().clone();
                    values.push((v.to_string(), val));
                }
                _ => flags.push(v.to_string()),
            }
        } else if let Some(v) = a.strip_prefix('-') {
            // single-dash option like -o
            match it.peek() {
                Some(next) if !next.starts_with('-') => {
                    let val = it.next().unwrap().clone();
                    values.push((v.to_string(), val));
                }
                _ => flags.push(v.to_string()),
            }
        } else {
            positional.push(a.clone());
        }
    }
    (positional, values, flags)
}

/// `--variant <escala>[:<nombre>[:<filtro>[:allowfraction[:<ancho>:<alto>]]]]`,
/// se acepta repetida o separada por comas. Los trozos
/// que no empiezan por una escala vuelven al filtro anterior, así que un
/// filtro con comas escribe bien. Fija también `scale_variants`.
pub(crate) fn apply_variant_flags(
    values: &[(String, String)],
    cfg: &mut ProjectConfig,
) -> CmdResult<()> {
    let variant_values: Vec<String> = values
        .iter()
        .filter(|(k, _)| k == "variant")
        .map(|(_, v)| v.clone())
        .collect();
    if variant_values.is_empty() {
        return Ok(());
    }
    let mut segments: Vec<String> = Vec::new();
    for value in &variant_values {
        for part in value.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let starts_with_scale = part
                .split(':')
                .next()
                .unwrap_or("")
                .trim()
                .parse::<f32>()
                .is_ok();
            if starts_with_scale {
                segments.push(part.to_string());
            } else if let Some(last) = segments.last_mut() {
                last.push(',');
                last.push_str(part);
            }
        }
    }
    let mut names: Vec<(f32, String)> = Vec::new();
    let mut options: Vec<VariantOptions> = Vec::new();
    for segment in &segments {
        let fields: Vec<&str> = segment.split(':').collect();
        let scale: f32 = fields[0]
            .trim()
            .parse()
            .map_err(|_| format!("--variant inválido: {segment}"))?;
        if !(0.0..=8.0).contains(&scale) {
            return Err(format!(
                "--variant inválido (escala fuera de rango): {segment}"
            ));
        }
        let name = fields
            .get(1)
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        let filter = fields
            .get(2)
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        let fourth = fields.get(3).map(|v| v.trim()).unwrap_or("");
        if !fourth.is_empty() && !fourth.eq_ignore_ascii_case("allowfraction") {
            return Err(format!(
                "--variant: cuarto campo desconocido (use allowfraction): {segment}"
            ));
        }
        let accept_fractional = fourth.eq_ignore_ascii_case("allowfraction");
        let max_texture_size = match (fields.get(4), fields.get(5)) {
            (Some(w), Some(h)) => {
                let number = |raw: &str, what: &str| -> CmdResult<i32> {
                    raw.trim()
                        .parse()
                        .map_err(|_| format!("--variant {what} inválido: {segment}"))
                };
                let (w, h) = (number(w, "ancho")?, number(h, "alto")?);
                if w <= 0 || h <= 0 || w != h {
                    return Err(format!(
                        "--variant: el tamaño máximo debe ser un cuadrado > 0: {segment}"
                    ));
                }
                Some(w)
            }
            (None, None) => None,
            _ => {
                return Err(format!(
                    "--variant: faltan el ancho o el alto del tamaño máximo: {segment}"
                ))
            }
        };
        names.push((scale, name));
        if !filter.is_empty() || accept_fractional || max_texture_size.is_some() {
            options.push(VariantOptions {
                scale,
                sprite_filter: filter,
                max_texture_size,
                accept_fractional,
                ..VariantOptions::default()
            });
        }
    }
    if !names.is_empty() {
        cfg.variant_names = names;
        cfg.scale_variants = cfg.variant_names.iter().map(|(s, _)| *s).collect();
        cfg.variant_options = options;
    }
    Ok(())
}

/// Mapea `--template-format` sobre la config.
///
/// Primero los tokens legados (cada familia con su semántica concreta) y
/// después los ids de exportador que documentan los data formats
/// (`libgdx`, `cocos2d`, `phaser`…), que aplican además los valores
/// recomendados del preset. `json` legado es el array, así que el hash se
/// pide como `json-hash`.
pub(crate) fn apply_template_format(cfg: &mut ProjectConfig, value: &str) -> CmdResult<()> {
    match value.to_ascii_lowercase().as_str() {
        "json" => cfg.template_format = TemplateFormat::Json,
        "xml" => cfg.template_format = TemplateFormat::Xml,
        "plist" => cfg.template_format = TemplateFormat::Plist,
        "cpp" => cfg.template_format = TemplateFormat::CppHeader,
        "tsv" => cfg.template_format = TemplateFormat::Tsv,
        "text" => cfg.template_format = TemplateFormat::PlainText,
        other => {
            let id = if other == "json-hash" { "json" } else { other };
            if cfg.apply_data_format(id) {
                return Ok(());
            }
            if cfg.select_custom_exporter(id) {
                return Ok(());
            }
            return Err(format!(
                "--template-format inválido: {value} (familias: json | xml | plist | cpp | \
                 tsv | text; exportadores: json-hash, libgdx, cocos2d, sparrow, spine, \
                 phaser, pixijs4, egret…; o un <id>.hbs de --custom-exporters-directory)"
            ));
        }
    }
    Ok(())
}

/// Qué es un argumento posicional de la línea de comandos.
pub(crate) enum Positional {
    /// Un `.tpproj` (TOML) o un `.tps` del original (XML).
    Project,
    /// Una carpeta o imagen con sprites.
    Input,
}

pub(crate) fn classify_positional(path: &Path) -> Positional {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("tpproj") | Some("toml") | Some("tps") => Positional::Project,
        _ => Positional::Input,
    }
}

fn stem_of(path: &Path, flag: &str) -> CmdResult<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "{flag} {}: no se puede derivar el nombre base",
                path.display()
            )
        })
}

/// `--sheet atlas-{n}.png` convive con `--data atlas.json`: la hoja sólo puede
/// añadir el placeholder de página al nombre base de los metadatos.
fn sheet_matches_data(sheet: &str, data: &str) -> bool {
    if sheet == data {
        return true;
    }
    sheet
        .strip_prefix(data)
        .is_some_and(|rest| matches!(rest.trim_start_matches(['-', '_']), "{n}" | "{n0}" | "{n1}"))
}

/// Rutas: `--sheet` fija carpeta, nombre base y formato de
/// textura por la extensión; `--data` fija carpeta y nombre base. Nuestro
/// modelo sólo admite un nombre base para los dos, así que se comprueba que
/// convivan. Devuelve la ruta de `--data` (para validar la extensión cuando
/// ya se conozca el formato) y un aviso si la hoja pedía numeración `{n}`.
pub(crate) fn apply_output_paths(
    cfg: &mut ProjectConfig,
    values: &[(String, String)],
) -> CmdResult<(Option<PathBuf>, Option<String>)> {
    let val =
        |k: &str| -> Option<String> { values.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone()) };
    let dir_of = |path: &Path| path.parent().map(Path::to_path_buf).unwrap_or_default();

    let Some(sheet) = val("sheet") else {
        let Some(data) = val("data") else {
            return Ok((None, None));
        };
        let path = PathBuf::from(&data);
        let dir = dir_of(&path);
        if !dir.as_os_str().is_empty() {
            cfg.output_directory = dir;
        }
        cfg.base_file_name = stem_of(&path, "--data")?;
        return Ok((Some(path), None));
    };

    let sheet_path = PathBuf::from(&sheet);
    let sheet_dir = dir_of(&sheet_path);
    let sheet_stem = stem_of(&sheet_path, "--sheet")?;
    let ext = sheet_path
        .extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| format!("--sheet {sheet}: falta la extensión (p. ej. atlas.png)"))?;
    let gpu = GpuFormat::parse(ext).ok_or_else(|| {
        format!(
            "--sheet {sheet}: extensión desconocida «{ext}»; elige el formato con --texture-format"
        )
    })?;
    cfg.gpu_format = gpu;

    let mut base = sheet_stem.clone();
    let mut warning = None;
    let mut data_path = None;
    if let Some(data) = val("data") {
        let data_path_buf = PathBuf::from(&data);
        let data_dir = dir_of(&data_path_buf);
        if data_dir != sheet_dir {
            return Err(format!(
                "--data {data} y --sheet {sheet} deben estar en la misma carpeta"
            ));
        }
        let data_stem = stem_of(&data_path_buf, "--data")?;
        if !sheet_matches_data(&sheet_stem, &data_stem) {
            return Err(format!(
                "--sheet {sheet} y --data {data} deben compartir el nombre base \
                 (p. ej. atlas.png con atlas.json, o atlas-{{n}}.png con atlas.json)"
            ));
        }
        if sheet_stem != data_stem {
            warning = Some(format!(
                "la hoja se pidió como «{sheet}» pero el nombre base lo fija --data: \
                 se numerará con el sufijo implícito _N (p. ej. {data_stem}.png, {data_stem}_1.png)"
            ));
        }
        base = data_stem;
        data_path = Some(data_path_buf);
    }
    if !sheet_dir.as_os_str().is_empty() {
        cfg.output_directory = sheet_dir;
    }
    cfg.base_file_name = base;
    Ok((data_path, warning))
}

/// La extensión de `--data` debe ser la del formato de datos elegido (o no
/// llevarla si el formato no escribe metadatos).
pub(crate) fn check_data_extension(cfg: &ProjectConfig, path: &Path) -> CmdResult<()> {
    let want = tp_core::templates::data_file_extension(cfg);
    if want.is_empty() {
        return Ok(());
    }
    let got = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    if got.eq_ignore_ascii_case(want) {
        return Ok(());
    }
    if got.is_empty() {
        return Err(format!(
            "--data {}: falta la extensión .{want} del formato de datos",
            path.display()
        ));
    }
    let hint = match tp_core::dataformats::find_data_format_by_extension(got) {
        Some(preset) => format!(
            "si esa es la salida que quieres, pide --format {}",
            preset.id
        ),
        None => "usa --format o --template-format".to_string(),
    };
    Err(format!(
        "--data {}: la extensión «{got}» no corresponde al formato de datos «{want}»; {hint}",
        path.display()
    ))
}

/// Qué lado de la doble acepción de `--format` ha casado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatTarget {
    Texture,
    Data,
}

/// `--format`: formato de textura para unos (`png`, `ktx`…) y
/// formato de datos para otros (`phaser`, `libgdx`…). Los dos conjuntos son
/// disjuntos, así que el orden de la comprobación no cambia nada.
pub(crate) fn apply_format(cfg: &mut ProjectConfig, value: &str) -> CmdResult<FormatTarget> {
    if let Some(gpu) = GpuFormat::parse(value) {
        cfg.gpu_format = gpu;
        return Ok(FormatTarget::Texture);
    }
    apply_template_format(cfg, value).map_err(|_| {
        format!(
            "--format inválido: {value} (formato de textura: png | jpg | webp | ktx | astc | …; \
             formato de datos: phaser | libgdx | cocos2d | sparrow | json-hash | …)"
        )
    })?;
    Ok(FormatTarget::Data)
}

/// `--texture-format`, que sólo admite formatos de textura y manda sobre la
/// extensión de `--sheet`.
pub(crate) fn apply_texture_format(cfg: &mut ProjectConfig, value: &str) -> CmdResult<()> {
    match GpuFormat::parse(value) {
        Some(gpu) => {
            cfg.gpu_format = gpu;
            Ok(())
        }
        None => Err(format!(
            "--texture-format inválido: {value} (los formatos de datos se piden con --format)"
        )),
    }
}

fn parse_divisor(value: &str, flag: &str) -> CmdResult<i32> {
    let d: i32 = value
        .parse()
        .map_err(|_| format!("{flag} inválido: {value}"))?;
    if d < 1 {
        return Err(format!("{flag} debe ser 1 o más: {value}"));
    }
    Ok(d)
}

/// `--default-pivot-point X,Y` en unidades normalizadas.
fn parse_pivot_point(value: &str) -> CmdResult<(f32, f32)> {
    let bad = || format!("--default-pivot-point inválido: {value} (usa X,Y con valores de 0 a 1)");
    let mut parts = value.split(',');
    let x = parts
        .next()
        .unwrap_or_default()
        .trim()
        .parse::<f32>()
        .map_err(|_| bad())?;
    let y = parts
        .next()
        .unwrap_or_default()
        .trim()
        .parse::<f32>()
        .map_err(|_| bad())?;
    if parts.next().is_some() || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        return Err(bad());
    }
    Ok((x, y))
}

/// `--max-width` / `--max-height`: tope propio de un eje del atlas
/// (0 = sin tope; el rango y la potencia de dos los valida el proyecto).
fn parse_atlas_limit(value: &str, flag: &str) -> CmdResult<i32> {
    let n: i32 = value
        .parse()
        .map_err(|_| format!("{flag} inválido: {value} (entero en 0..16384)"))?;
    if !(0..=16384).contains(&n) {
        return Err(format!("{flag} debe estar entre 0 y 16384: {value}"));
    }
    Ok(n)
}

/// `--background-color`: color con el que se rellena la hoja, en hexadecimal
/// (`RGB`, `RRGGBB` o `RRGGBBAA`, con o sin `#`; sin alfa = opaco).
fn parse_background_color(value: &str) -> CmdResult<[u8; 4]> {
    let bad =
        || format!("--background-color inválido: {value} (usa RRGGBB o RRGGBBAA en hexadecimal)");
    let hex = value.trim().trim_start_matches('#');
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let wide: String = match hex.len() {
        3 => format!("{}ff", hex.chars().flat_map(|c| [c, c]).collect::<String>()),
        6 => format!("{hex}ff"),
        8 => hex.to_string(),
        _ => return Err(bad()),
    };
    if wide.len() != 8 {
        return Err(bad());
    }
    let mut out = [0u8; 4];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&wide[i * 2..i * 2 + 2], 16).map_err(|_| bad())?;
    }
    Ok(out)
}

/// `--replace PATRÓN=TEXTO` (repetible): sustituciones regex sobre los ids
/// de sprite; la expresión se comprueba aquí, no al empaquetar.
fn parse_replacement(value: &str) -> CmdResult<(String, String)> {
    let Some((pattern, text)) = value.split_once('=') else {
        return Err(format!("--replace inválido: {value} (usa PATRÓN=TEXTO)"));
    };
    if pattern.is_empty() {
        return Err(format!("--replace inválido: {value} (falta el patrón)"));
    }
    regex::Regex::new(pattern)
        .map_err(|e| format!("--replace: la expresión regular «{pattern}» no es válida: {e}"))?;
    Ok((pattern.to_string(), text.to_string()))
}

/// Opciones de la Fase C del CLI: topes de atlas por eje, color de la hoja,
/// resolución de salida, filtros de entrada y renombrado de sprites.
pub(crate) fn apply_phase_c_options(
    cfg: &mut ProjectConfig,
    values: &[(String, String)],
) -> CmdResult<()> {
    let val = |k: &str| values.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
    if let Some(v) = val("max-width") {
        cfg.max_width = parse_atlas_limit(&v, "--max-width")?;
    }
    if let Some(v) = val("max-height") {
        cfg.max_height = parse_atlas_limit(&v, "--max-height")?;
    }
    if let Some(v) = val("background-color") {
        cfg.background_color = Some(parse_background_color(&v)?);
    }
    if let Some(v) = val("dpi") {
        let dpi: u32 = v
            .parse()
            .map_err(|_| format!("--dpi inválido: {v} (entero en 1..1000000)"))?;
        if !(1..=1_000_000).contains(&dpi) {
            return Err(format!("--dpi debe estar entre 1 y 1000000: {v}"));
        }
        cfg.dpi = Some(dpi);
    }
    // Repetibles: cada aparición añade un patrón / una sustitución.
    for (_, v) in values.iter().filter(|(k, _)| k == "ignore-files") {
        cfg.ignore_patterns.push(v.clone());
    }
    for (_, v) in values.iter().filter(|(k, _)| k == "replace") {
        let (pattern, text) = parse_replacement(v)?;
        cfg.name_replacements.push((pattern, text));
    }
    // Propiedades de los exportadores que sí escribimos (css y plain) y
    // carpeta de exportadores propios. Van antes de `--format`, que consulta
    // el directorio para saber si el id existe.
    if let Some(v) = val("css-media-query-2x") {
        cfg.css_media_query_2x = Some(v);
    }
    if let Some(v) = val("css-sprite-prefix") {
        cfg.css_sprite_prefix = Some(v);
    }
    if let Some(v) = val("plain-string-property") {
        cfg.plain_string_property = Some(v);
    }
    if let Some(v) = val("plain-bool-property") {
        cfg.plain_bool_property = Some(parse_bool(&v, "--plain-bool-property")?);
    }
    if let Some(v) = val("custom-exporters-directory") {
        cfg.custom_exporters_directory = Some(PathBuf::from(v));
    }
    Ok(())
}

/// `--scale F`: multiplica todas las variantes de escala declaradas
/// (`--variants`, `--variant`) y sus nombres. Con una única variante el
/// sufijo de fichero queda vacío, así que la salida conserva su nombre.
pub(crate) fn apply_scale_factor(cfg: &mut ProjectConfig, factor: f32) {
    cfg.scale_variants = cfg
        .scale_variants
        .iter()
        .map(|s| s * factor)
        .collect::<Vec<_>>();
    for (scale, _) in cfg.variant_names.iter_mut() {
        *scale *= factor;
    }
    for opts in cfg.variant_options.iter_mut() {
        opts.scale *= factor;
    }
    if cfg.scale_variants.len() == 1 && cfg.variant_names.is_empty() {
        cfg.variant_names
            .push((cfg.scale_variants[0], String::new()));
    }
}

/// Opciones de layout que la GUI ya sabe aplicar: ejes
/// sueltos del common divisor y pivot por defecto. Va después de
/// `--common-divisor` para que el eje gane sobre el valor conjunto.
pub(crate) fn apply_parity_layout_options(
    cfg: &mut ProjectConfig,
    values: &[(String, String)],
) -> CmdResult<()> {
    let val = |k: &str| values.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
    if let Some(v) = val("common-divisor-x") {
        cfg.common_divisor_x = parse_divisor(&v, "--common-divisor-x")?;
    }
    if let Some(v) = val("common-divisor-y") {
        cfg.common_divisor_y = parse_divisor(&v, "--common-divisor-y")?;
    }
    if let Some(v) = val("default-pivot-point") {
        let (x, y) = parse_pivot_point(&v)?;
        cfg.default_pivot_x = x;
        cfg.default_pivot_y = y;
    }
    Ok(())
}

/// Opciones de pack sin valor, agrupadas para poderlas testear y para fijar
/// la precedencia de los toggles nuevos respecto a los presets de `--format`
/// (que ya se han aplicado al llegar aquí).
pub(crate) fn apply_flag_options(cfg: &mut ProjectConfig, flags: &[String]) {
    let has = |k: &str| flags.iter().any(|f| f == k);
    if has("cache-busting") {
        cfg.cache_busting = true;
    }
    if has("shape-debug") {
        cfg.shape_debug = true;
    }
    if has("enable-rotation") {
        cfg.allow_rotation = true;
    }
    if has("no-rotation") {
        cfg.allow_rotation = false;
    }
    if has("no-trim") {
        cfg.enable_trim = false;
    }
    if has("polygon") {
        cfg.enable_polygon = true;
    }
    if has("no-aliasing") || has("disable-auto-alias") {
        cfg.enable_aliasing = false;
    }
    if has("pack-normalmaps") {
        cfg.enable_normal_maps = true;
    }
    if has("no-normals") {
        cfg.enable_normal_maps = false;
    }
    if has("normalmap-detect") {
        cfg.normal_map_auto_detect = true;
    }
    if has("trim-sprite-names") {
        cfg.trim_sprite_names = true;
    }
    if has("keep-extension") {
        cfg.trim_sprite_names = false;
    }
    if has("prepend-folder-name") {
        cfg.prepend_folder_name = true;
    }
    if has("no-recursive") {
        cfg.recursive = false;
    }
    if has("force-squared") {
        cfg.force_squared = true;
    }
    if has("force-identical-layout") {
        // La opción vive por variante (reutilizar la hoja base escalada);
        // sin variantes no hay nada que fijar.
        for opts in cfg.variant_options.iter_mut() {
            opts.force_identical_layout = true;
        }
    }
    if has("no-auto-animations") {
        cfg.enable_auto_detect_animations = false;
    }
    if has("no-multipack") {
        cfg.multipack = false;
    }
    if has("auto-folders") {
        cfg.auto_folder_groups = true;
    }
    if has("multipack") {
        cfg.multipack = true;
    }
    if has("flip-y") || has("flip-vertical") || has("flip-pvr") {
        cfg.flip_vertical = true;
    }
    if has("disable-rotation") {
        // Alias de `--no-rotation`.
        cfg.allow_rotation = false;
    }
    if has("enable-cache-busting") {
        // Alias de `--cache-busting`.
        cfg.cache_busting = true;
    }
    if has("heuristic-mask") {
        cfg.heuristic_mask = true;
    }
    if has("force-publish") {
        cfg.force_publish = true;
    }
}

/// Reglas de exportación: rangos de calidades y que el pixel format elegido
/// sea soportado por el formato de textura. Devuelve el mensaje de error.
pub(crate) fn check_export_flags(cfg: &ProjectConfig) -> CmdResult<()> {
    if cfg.pvr_quality > 7 {
        return Err(format!(
            "--pvr-quality fuera de rango (0-7): {}",
            cfg.pvr_quality
        ));
    }
    if cfg.etc1_quality > 100 {
        return Err(format!(
            "--etc1-quality fuera de rango (0-100): {}",
            cfg.etc1_quality
        ));
    }
    if cfg.etc2_quality > 100 {
        return Err(format!(
            "--etc2-quality fuera de rango (0-100): {}",
            cfg.etc2_quality
        ));
    }
    if cfg.astc_quality > 4 {
        return Err(format!(
            "--astc-quality fuera de rango (0-4): {}",
            cfg.astc_quality
        ));
    }
    if cfg.basis_quality > 100 {
        return Err(format!(
            "--basis-quality fuera de rango (0-100): {}",
            cfg.basis_quality
        ));
    }
    if !cfg.pixel_format.is_compatible_with(cfg.gpu_format) {
        return Err(format!(
            "pixel format {} no es soportado por --format {}",
            cfg.pixel_format.as_str(),
            cfg.gpu_format.as_str()
        ));
    }
    Ok(())
}

/// Parseo compartido del flag `--pixel-format` (pack y decrypt).
/// Mayúsculas/minúsculas y `-`/`_` indiferentes: se normaliza antes de casar.
pub(crate) fn parse_pixel_format(v: &str) -> CmdResult<PixelFormat> {
    let norm: String = v
        .to_ascii_lowercase()
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .collect();
    Ok(match norm.as_str() {
        "rgba8888" => PixelFormat::Rgba8888,
        "rgb888" => PixelFormat::Rgb888, // compone sobre negro al decodificar
        "alpha8" => PixelFormat::Alpha8, // nivel de alfa → gris
        "intensity8" => PixelFormat::Intensity8,
        "alphaintensity8" => PixelFormat::AlphaIntensity8,
        "rgba5551" | "5551" => PixelFormat::Rgba5551,
        "rgba5555" | "5555" => PixelFormat::Rgba5555,
        "bgra8888" => PixelFormat::Bgra8888,
        "rgba4444" | "4444" => PixelFormat::Rgba4444,
        "rgb565" | "565" => PixelFormat::Rgb565,
        "pvrtc2bpprgba" | "pvrtci2bpprgba" => PixelFormat::Pvrtc2BppRgba,
        "pvrtc4bpprgba" | "pvrtci4bpprgba" => PixelFormat::Pvrtc4BppRgba,
        "pvrtc2bpprgb" | "pvrtci2bpprgb" => PixelFormat::Pvrtc2BppRgb,
        "pvrtc4bpprgb" | "pvrtci4bpprgb" => PixelFormat::Pvrtc4BppRgb,
        "etc1" | "etc1rgb" => PixelFormat::Etc1Rgb,
        "etc2" | "etc2rgba" => PixelFormat::Etc2Rgba,
        "etc2rgb" => PixelFormat::Etc2Rgb,
        "dxt1" => PixelFormat::Dxt1,
        "dxt5" => PixelFormat::Dxt5,
        "astc4x4" => PixelFormat::Astc4x4,
        "astc5x4" => PixelFormat::Astc5x4,
        "astc5x5" => PixelFormat::Astc5x5,
        "astc6x5" => PixelFormat::Astc6x5,
        "astc6x6" => PixelFormat::Astc6x6,
        "astc8x5" => PixelFormat::Astc8x5,
        "astc8x6" => PixelFormat::Astc8x6,
        "astc8x8" => PixelFormat::Astc8x8,
        "astc10x5" => PixelFormat::Astc10x5,
        "astc10x6" => PixelFormat::Astc10x6,
        "astc10x8" => PixelFormat::Astc10x8,
        "astc10x10" => PixelFormat::Astc10x10,
        "astc12x10" => PixelFormat::Astc12x10,
        "astc12x12" => PixelFormat::Astc12x12,
        _ => return Err(format!("--pixel-format inválido: {v}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tp_core::config::{
        AlphaHandling, BasicSortBy, DxtMode, GdxFilter, PackMode, PackingAlgorithm,
        PackingStrategy, ScaleMode, SizeConstraint, SortOrder,
    };

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn pixel_format_flag_is_shared_between_commands() {
        assert!(matches!(
            parse_pixel_format("BGRA8888").unwrap(),
            PixelFormat::Bgra8888
        ));
        assert!(matches!(
            parse_pixel_format("rgba5555").unwrap(),
            PixelFormat::Rgba5555
        ));
        // Variantes con alias.
        assert!(matches!(
            parse_pixel_format("5551").unwrap(),
            PixelFormat::Rgba5551
        ));
        assert!(matches!(
            parse_pixel_format("Alpha-Intensity8").unwrap(),
            PixelFormat::AlphaIntensity8
        ));
    }

    #[test]
    fn data_format_extra_flags_parse() {
        let (positionals, values, flags) = parse_args(&args(&[
            "--cache-busting",
            "--shape-debug",
            "--gdx-filter",
            "Nearest",
        ]));
        assert!(positionals.is_empty());
        assert!(flags.iter().any(|f| f == "cache-busting"));
        assert!(flags.iter().any(|f| f == "shape-debug"));
        assert_eq!(
            values
                .iter()
                .find(|(k, _)| k == "gdx-filter")
                .map(|(_, v)| v.as_str()),
            Some("Nearest")
        );
        assert!(matches!(
            GdxFilter::parse("NEAREST"),
            Some(GdxFilter::Nearest)
        ));
        assert!(matches!(
            GdxFilter::parse("linear"),
            Some(GdxFilter::Linear)
        ));
        assert!(GdxFilter::parse("bilinear").is_none());
    }

    #[test]
    fn pixel_format_gpu_tokens_and_astc_blocks_parse() {
        assert!(matches!(
            parse_pixel_format("PVRTCI_2BPP_RGBA").unwrap(),
            PixelFormat::Pvrtc2BppRgba
        ));
        assert!(matches!(
            parse_pixel_format("pvrtc-4bpp-rgb").unwrap(),
            PixelFormat::Pvrtc4BppRgb
        ));
        assert!(matches!(
            parse_pixel_format("etc1").unwrap(),
            PixelFormat::Etc1Rgb
        ));
        assert!(matches!(
            parse_pixel_format("ETC2_RGB").unwrap(),
            PixelFormat::Etc2Rgb
        ));
        assert!(matches!(
            parse_pixel_format("dxt5").unwrap(),
            PixelFormat::Dxt5
        ));
        assert!(matches!(
            parse_pixel_format("astc-12x12").unwrap(),
            PixelFormat::Astc12x12
        ));
        assert!(matches!(
            parse_pixel_format("rgba4444").unwrap(),
            PixelFormat::Rgba4444
        ));
        assert!(matches!(
            parse_pixel_format("rgb565").unwrap(),
            PixelFormat::Rgb565
        ));
    }

    #[test]
    fn pixel_format_must_match_the_texture_format() {
        let mut cfg = ProjectConfig {
            pixel_format: PixelFormat::Dxt1,
            gpu_format: GpuFormat::Dds,
            ..Default::default()
        };
        assert!(check_export_flags(&cfg).is_ok());

        cfg.gpu_format = GpuFormat::Png;
        let err = check_export_flags(&cfg).unwrap_err();
        assert!(err.contains("DXT1") && err.contains("PNG"), "{err}");

        // Un pixel format de software vale con cualquier formato de textura.
        cfg.pixel_format = PixelFormat::Rgba8888;
        assert!(check_export_flags(&cfg).is_ok());
    }

    #[test]
    fn quality_flags_must_stay_inside_the_original_ranges() {
        assert!(check_export_flags(&ProjectConfig {
            pvr_quality: 8,
            ..Default::default()
        })
        .unwrap_err()
        .contains("--pvr-quality"));
        assert!(check_export_flags(&ProjectConfig {
            astc_quality: 5,
            ..Default::default()
        })
        .unwrap_err()
        .contains("--astc-quality"));
        assert!(check_export_flags(&ProjectConfig {
            etc1_quality: 101,
            ..Default::default()
        })
        .unwrap_err()
        .contains("--etc1-quality"));
        assert!(check_export_flags(&ProjectConfig {
            etc2_quality: 101,
            ..Default::default()
        })
        .unwrap_err()
        .contains("--etc2-quality"));
        assert!(check_export_flags(&ProjectConfig {
            basis_quality: 101,
            ..Default::default()
        })
        .unwrap_err()
        .contains("--basis-quality"));
        assert!(check_export_flags(&ProjectConfig {
            etc2_quality: 0,
            ..Default::default()
        })
        .is_ok());
    }

    #[test]
    fn quality_and_dxt_mode_flags_parse() {
        let (_, values, _) = parse_args(&args(&[
            "--pvr-quality",
            "7",
            "--etc1-quality",
            "25",
            "--etc2-quality",
            "100",
            "--astc-quality",
            "4",
            "--basis-quality",
            "60",
            "--dxt-mode",
            "DXT_PERCEPTUAL",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("pvr-quality").as_deref(), Some("7"));
        assert_eq!(val("etc1-quality").as_deref(), Some("25"));
        assert_eq!(val("etc2-quality").as_deref(), Some("100"));
        assert_eq!(val("astc-quality").as_deref(), Some("4"));
        assert_eq!(val("basis-quality").as_deref(), Some("60"));
        assert!(matches!(
            DxtMode::parse(val("dxt-mode").as_deref().unwrap()),
            Some(DxtMode::Perceptual)
        ));
    }

    #[test]
    fn global_key_flags_parse() {
        let (_, values, _) = parse_args(&args(&[
            "--key-name",
            "juego",
            "--save-key",
            "otra",
            "--key",
            "clave-secreta",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("key-name").as_deref(), Some("juego"));
        assert_eq!(val("save-key").as_deref(), Some("otra"));
        assert_eq!(val("key").as_deref(), Some("clave-secreta"));
    }

    #[test]
    fn variant_flag_parses_filter_allowfraction_and_size() {
        let (_, values, _) = parse_args(&args(&[
            "--variant",
            "1.0:-ipadhd",
            "--variant",
            "0.5:-hd:hero*,coin:allowfraction:1024:1024",
        ]));
        let mut cfg = ProjectConfig::default();
        apply_variant_flags(&values, &mut cfg).unwrap();
        assert_eq!(cfg.scale_variants, vec![1.0, 0.5]);
        assert_eq!(
            cfg.variant_names,
            vec![(1.0, "-ipadhd".to_string()), (0.5, "-hd".to_string())]
        );
        let opts = cfg.variant_options_for(0.5).unwrap();
        assert_eq!(opts.sprite_filter, "hero*,coin");
        assert!(opts.accept_fractional);
        assert_eq!(opts.max_texture_size, Some(1024));
        assert!(cfg.variant_options_for(1.0).is_none());
        assert!(cfg.validate().is_ok());

        // Un filtro con comas no se parte: los trozos que no empiezan por
        // escala vuelven al segmento anterior.
        let (_, values, _) = parse_args(&args(&["--variant", "0.5::a,b"]));
        let mut cfg = ProjectConfig::default();
        apply_variant_flags(&values, &mut cfg).unwrap();
        assert_eq!(cfg.variant_options_for(0.5).unwrap().sprite_filter, "a,b");
    }

    #[test]
    fn boolean_flags_can_be_chained() {
        let (_, values, flags) = parse_args(&args(&[
            "--input",
            "in",
            "--output",
            "out",
            "--no-rotation",
            "--keep-extension",
            "--prepend-folder-name",
        ]));
        assert_eq!(
            values,
            vec![
                ("input".to_string(), "in".to_string()),
                ("output".to_string(), "out".to_string())
            ]
        );
        assert!(flags.iter().any(|f| f == "no-rotation"));
        assert!(flags.iter().any(|f| f == "keep-extension"));
        assert!(flags.iter().any(|f| f == "prepend-folder-name"));
    }

    #[test]
    fn lote7_multipack_flags_parse() {
        let (_, _, flags) = parse_args(&args(&[
            "--input",
            "in",
            "--output",
            "out",
            "--no-multipack",
        ]));
        assert!(flags.iter().any(|f| f == "no-multipack"));

        let (_, _, flags) = parse_args(&args(&["--input", "in", "--output", "out", "--multipack"]));
        assert!(flags.iter().any(|f| f == "multipack"));
        assert!(!flags.iter().any(|f| f == "no-multipack"));
    }

    #[test]
    fn lote8_export_flags_parse() {
        let (_, values, flags) = parse_args(&args(&[
            "--input",
            "in",
            "--output",
            "out",
            "--format",
            "jpg",
            "--jpg-quality",
            "90",
            "--webp-quality",
            "80",
            "--png-opt-level",
            "4",
            "--png8-dither",
            "low",
            "--pixel-format",
            "rgb888",
            "--flip-y",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("format").as_deref(), Some("jpg"));
        assert_eq!(val("jpg-quality").as_deref(), Some("90"));
        assert_eq!(val("webp-quality").as_deref(), Some("80"));
        assert_eq!(val("png-opt-level").as_deref(), Some("4"));
        assert_eq!(val("png8-dither").as_deref(), Some("low"));
        assert_eq!(val("pixel-format").as_deref(), Some("rgb888"));
        assert!(flags.iter().any(|f| f == "flip-y"));
    }

    #[test]
    fn normalmap_flags_parse() {
        let (_, values, flags) = parse_args(&args(&[
            "--normalmap-suffix",
            "_n",
            "--normalmap-filter",
            "normals/",
            "--normalmap-sheet",
            "norms",
            "--normalmap-detect",
            "--no-normals",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("normalmap-suffix").as_deref(), Some("_n"));
        assert_eq!(val("normalmap-filter").as_deref(), Some("normals/"));
        assert_eq!(val("normalmap-sheet").as_deref(), Some("norms"));
        assert!(flags.iter().any(|f| f == "normalmap-detect"));
        assert!(flags.iter().any(|f| f == "no-normals"));
    }

    #[test]
    fn extra_data_file_flags_parse() {
        let (_, values, _) = parse_args(&args(&[
            "--class-file",
            "Sprites.swift",
            "--header-file",
            "Sprites.h",
            "--source-file",
            "Sprites.cpp",
            "--spriteids-file",
            "spriteids.txt",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("class-file").as_deref(), Some("Sprites.swift"));
        assert_eq!(val("header-file").as_deref(), Some("Sprites.h"));
        assert_eq!(val("source-file").as_deref(), Some("Sprites.cpp"));
        assert_eq!(val("spriteids-file").as_deref(), Some("spriteids.txt"));
    }

    #[test]
    fn lote5_value_flags_parse() {
        let (_, values, _) = parse_args(&args(&[
            "--border-padding",
            "8",
            "--common-divisor",
            "4",
            "--align",
            "4",
            "--alpha-handling",
            "premultiply",
            "--scale-mode",
            "fast",
            "--texture-path",
            "/assets",
            "--dither",
            "floyd-alpha",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("border-padding").as_deref(), Some("8"));
        assert_eq!(val("common-divisor").as_deref(), Some("4"));
        assert_eq!(val("align").as_deref(), Some("4"));
        assert_eq!(val("alpha-handling").as_deref(), Some("premultiply"));
        assert_eq!(val("scale-mode").as_deref(), Some("fast"));
        assert_eq!(val("texture-path").as_deref(), Some("/assets"));
        assert_eq!(val("dither").as_deref(), Some("floyd-alpha"));
    }

    #[test]
    fn lote5_enums_parse_from_cli_tokens() {
        assert_eq!(
            AlphaHandling::parse("bleed"),
            Some(AlphaHandling::ReduceBorderArtifacts)
        );
        assert_eq!(
            AlphaHandling::parse("premultiply"),
            Some(AlphaHandling::PremultiplyAlpha)
        );
        assert_eq!(ScaleMode::parse("fast"), Some(ScaleMode::Fast));
        assert_eq!(
            ScaleMode::parse("smooth"),
            Some(tp_core::config::ScaleMode::Smooth)
        );
    }

    #[test]
    fn canonical_flags_parse() {
        // Aliases con los nombres canónicos de las opciones.
        let (_, values, flags) = parse_args(&args(&[
            "--input",
            "in",
            "--output",
            "out",
            "--shape-padding",
            "3",
            "--align-to-grid",
            "8",
            "--maxrects-heuristics",
            "contact-point",
            "--disable-auto-alias",
            "--variant",
            "1.0:-ipadhd,0.5:-hd",
        ]));
        assert_eq!(val2(&values, "shape-padding"), Some("3"));
        assert_eq!(val2(&values, "align-to-grid"), Some("8"));
        assert_eq!(val2(&values, "maxrects-heuristics"), Some("contact-point"));
        assert!(flags.contains(&"disable-auto-alias".to_string()));
        assert_eq!(val2(&values, "variant"), Some("1.0:-ipadhd,0.5:-hd"));
    }

    fn val2<'a>(values: &'a [(String, String)], key: &str) -> Option<&'a str> {
        values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn lote6_pack_flags_parse() {
        let (_, values, flags) = parse_args(&args(&[
            "--algorithm",
            "grid",
            "--pack-mode",
            "best",
            "--size-constraints",
            "pot",
            "--width",
            "1024",
            "--height",
            "512",
            "--basic-sort-by",
            "name",
            "--basic-order",
            "desc",
            "--strategy",
            "contact-point",
            "--force-squared",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("algorithm").as_deref(), Some("grid"));
        assert_eq!(val("pack-mode").as_deref(), Some("best"));
        assert_eq!(val("size-constraints").as_deref(), Some("pot"));
        assert_eq!(val("width").as_deref(), Some("1024"));
        assert_eq!(val("height").as_deref(), Some("512"));
        assert_eq!(val("basic-sort-by").as_deref(), Some("name"));
        assert_eq!(val("basic-order").as_deref(), Some("desc"));
        assert_eq!(val("strategy").as_deref(), Some("contact-point"));
        assert!(flags.iter().any(|f| f == "force-squared"));
    }

    #[test]
    fn template_format_accepts_families_and_exporters() {
        // Tokens legados: sólo cambian la familia (sin preset asociado).
        let mut cfg = ProjectConfig::default();
        apply_template_format(&mut cfg, "json").unwrap();
        assert_eq!(cfg.template_format, TemplateFormat::Json);
        assert!(cfg.data_format.is_empty());

        // El hash se pide como `json-hash` (json = array).
        let mut cfg = ProjectConfig::default();
        apply_template_format(&mut cfg, "json-hash").unwrap();
        assert_eq!(cfg.template_format, TemplateFormat::JsonHash);
        assert_eq!(cfg.data_format, "json");

        // Exportador: familia + extensión + recomendados del preset.
        let mut cfg = ProjectConfig {
            enable_auto_detect_animations: false,
            ..ProjectConfig::default()
        };
        apply_template_format(&mut cfg, "libgdx").unwrap();
        assert_eq!(cfg.template_format, TemplateFormat::LibgdxAtlas);
        assert_eq!(cfg.data_format, "libgdx");
        assert!(cfg.enable_auto_detect_animations);

        let mut cfg = ProjectConfig {
            allow_rotation: true,
            ..ProjectConfig::default()
        };
        apply_template_format(&mut cfg, "css").unwrap();
        assert!(!cfg.allow_rotation);

        let err = apply_template_format(&mut ProjectConfig::default(), "no-existe").unwrap_err();
        assert!(err.contains("--template-format inválido"), "{err}");
    }

    #[test]
    fn lote6_enums_parse_from_cli_tokens() {
        assert_eq!(
            PackingAlgorithm::parse("grid"),
            Some(PackingAlgorithm::Grid)
        );
        assert_eq!(PackMode::parse("best"), Some(PackMode::Best));
        assert_eq!(SizeConstraint::parse("pot"), Some(SizeConstraint::Pot));
        assert_eq!(
            SizeConstraint::parse("multiple-of-4"),
            Some(SizeConstraint::MultipleOf4)
        );
        assert_eq!(
            PackingStrategy::parse("contact-point"),
            Some(PackingStrategy::ContactPoint)
        );
        assert_eq!(
            PackingStrategy::parse("bottom-left"),
            Some(PackingStrategy::BottomLeft)
        );
        assert_eq!(
            BasicSortBy::parse("circumference"),
            Some(BasicSortBy::Circumference)
        );
        assert_eq!(SortOrder::parse("desc"), Some(SortOrder::Descending));
    }

    #[test]
    fn unknown_options_are_rejected_with_a_hint() {
        let err = check_unknown_options(
            &[("multiplier".to_string(), "0.5".to_string())],
            &[],
            PACK_VALUES,
            PACK_FLAGS,
        )
        .unwrap_err();
        assert!(err.contains("--multiplier"), "{err}");
        assert!(err.contains("--help"), "{err}");

        let err = check_unknown_options(&[], &["bogus".to_string()], PACK_VALUES, PACK_FLAGS)
            .unwrap_err();
        assert!(err.contains("--bogus"), "{err}");

        // Una opción con valor escrita sin él se distingue del resto.
        let err = check_unknown_options(&[], &["input".to_string()], PACK_VALUES, PACK_FLAGS)
            .unwrap_err();
        assert!(err.contains("falta el valor de --input"), "{err}");

        // Alias y opciones nuevas sí son conocidas.
        assert!(check_unknown_options(
            &[
                ("dither-type".to_string(), "floyd".to_string()),
                ("texturepath".to_string(), "/a".to_string()),
                ("basisu-quality".to_string(), "60".to_string()),
                ("opt".to_string(), "rgb565".to_string()),
            ],
            &["flip-pvr".to_string(), "trim-sprite-names".to_string()],
            PACK_VALUES,
            PACK_FLAGS,
        )
        .is_ok());
    }

    #[test]
    fn format_accepts_texture_and_data_tokens() {
        let mut cfg = ProjectConfig::default();
        assert_eq!(
            apply_format(&mut cfg, "png").unwrap(),
            FormatTarget::Texture
        );
        assert_eq!(cfg.gpu_format, GpuFormat::Png);

        let mut cfg = ProjectConfig::default();
        assert_eq!(
            apply_format(&mut cfg, "phaser").unwrap(),
            FormatTarget::Data
        );
        assert_eq!(cfg.data_format, "phaser");

        let err = apply_format(&mut ProjectConfig::default(), "no-existe").unwrap_err();
        assert!(err.contains("textura") && err.contains("datos"), "{err}");
    }

    #[test]
    fn texture_format_beats_the_sheet_extension() {
        let mut cfg = ProjectConfig::default();
        let (data, warn) = apply_output_paths(
            &mut cfg,
            &[("sheet".to_string(), "build/atlas.webp".to_string())],
        )
        .unwrap();
        assert!(data.is_none() && warn.is_none());
        assert_eq!(cfg.gpu_format, GpuFormat::WebP);
        assert_eq!(cfg.output_directory, PathBuf::from("build"));
        assert_eq!(cfg.base_file_name, "atlas");

        apply_texture_format(&mut cfg, "ktx").unwrap();
        assert_eq!(cfg.gpu_format, GpuFormat::Etc1Ktx);

        let err = apply_texture_format(&mut ProjectConfig::default(), "phaser").unwrap_err();
        assert!(err.contains("--format"), "{err}");
    }

    #[test]
    fn sheet_and_data_share_one_base_name() {
        // Caso esperado: atlas-{n}.png junto a atlas.json.
        let mut cfg = ProjectConfig::default();
        let values = vec![
            ("sheet".to_string(), "out/atlas-{n}.png".to_string()),
            ("data".to_string(), "out/atlas.json".to_string()),
        ];
        let (data, warn) = apply_output_paths(&mut cfg, &values).unwrap();
        let data = data.expect("--data debe devolver su ruta");
        assert_eq!(data, PathBuf::from("out/atlas.json"));
        assert!(warn.is_some(), "la numeración {{n}} de la hoja se pierde");
        assert_eq!(cfg.base_file_name, "atlas");
        assert_eq!(cfg.output_directory, PathBuf::from("out"));
        assert!(check_data_extension(&cfg, &data).is_ok());

        // Un nombre base distinto no cabe en nuestro modelo.
        let mut cfg = ProjectConfig::default();
        let values = vec![
            ("sheet".to_string(), "sprites.png".to_string()),
            ("data".to_string(), "atlas.json".to_string()),
        ];
        let err = apply_output_paths(&mut cfg, &values).unwrap_err();
        assert!(err.contains("nombre base"), "{err}");

        // Ni dos carpetas distintas.
        let mut cfg = ProjectConfig::default();
        let values = vec![
            ("sheet".to_string(), "a/atlas.png".to_string()),
            ("data".to_string(), "b/atlas.json".to_string()),
        ];
        let err = apply_output_paths(&mut cfg, &values).unwrap_err();
        assert!(err.contains("misma carpeta"), "{err}");

        // Sólo --data: fija carpeta y nombre base, sin avisos.
        let mut cfg = ProjectConfig::default();
        let values = vec![("data".to_string(), "build/atlas.json".to_string())];
        let (data, warn) = apply_output_paths(&mut cfg, &values).unwrap();
        assert_eq!(data, Some(PathBuf::from("build/atlas.json")));
        assert!(warn.is_none());
        assert_eq!(cfg.output_directory, PathBuf::from("build"));
        assert_eq!(cfg.base_file_name, "atlas");
    }

    #[test]
    fn data_extension_must_match_the_data_format() {
        let mut cfg = ProjectConfig::default();
        assert!(check_data_extension(&cfg, Path::new("a.json")).is_ok());
        let err = check_data_extension(&cfg, Path::new("a.plist")).unwrap_err();
        assert!(err.contains("plist") && err.contains("json"), "{err}");
        let err = check_data_extension(&cfg, Path::new("a")).unwrap_err();
        assert!(err.contains("extensión"), "{err}");

        apply_template_format(&mut cfg, "plist").unwrap();
        assert!(check_data_extension(&cfg, Path::new("a.plist")).is_ok());
    }

    #[test]
    fn positionals_are_split_into_project_inputs_and_errors() {
        assert!(matches!(
            classify_positional(Path::new("game.tpproj")),
            Positional::Project
        ));
        assert!(matches!(
            classify_positional(Path::new("game.toml")),
            Positional::Project
        ));
        assert!(matches!(
            classify_positional(Path::new("game.tps")),
            Positional::Project
        ));
        assert!(matches!(
            classify_positional(Path::new("sprites")),
            Positional::Input
        ));
        assert!(matches!(
            classify_positional(Path::new("hero.png")),
            Positional::Input
        ));

        // El parser devuelve todos los posicionales, en orden, y acepta que
        // los sprites vayan mezclados con las opciones.
        let (positionals, values, flags) = parse_args(&args(&[
            "--format",
            "phaser",
            "sprites/",
            "hero.png",
            "game.tpproj",
        ]));
        assert_eq!(
            positionals,
            vec![
                "sprites/".to_string(),
                "hero.png".to_string(),
                "game.tpproj".to_string()
            ]
        );
        assert_eq!(values.len(), 1);
        assert!(flags.is_empty());
    }

    #[test]
    fn boolean_options_do_not_swallow_the_next_positional() {
        let (positionals, values, flags) =
            parse_args(&args(&["--multipack", "sprites/", "--no-rotation"]));
        assert_eq!(positionals, vec!["sprites/".to_string()]);
        assert!(values.is_empty());
        assert!(flags.contains(&"multipack".to_string()));
        assert!(flags.contains(&"no-rotation".to_string()));
    }

    #[test]
    fn parity_flags_reach_the_config() {
        let (_, values, flags) = parse_args(&args(&[
            "--common-divisor-x",
            "4",
            "--common-divisor-y",
            "2",
            "--default-pivot-point",
            "0.25,0.75",
            "--no-auto-animations",
            "--trim-sprite-names",
            "--enable-rotation",
            "--pack-normalmaps",
            "--force-identical-layout",
        ]));
        let mut cfg = ProjectConfig::default();
        apply_parity_layout_options(&mut cfg, &values).unwrap();
        assert_eq!(cfg.common_divisor_x, 4);
        assert_eq!(cfg.common_divisor_y, 2);
        assert_eq!(cfg.default_pivot_x, 0.25);
        assert_eq!(cfg.default_pivot_y, 0.75);

        cfg.enable_auto_detect_animations = true;
        cfg.trim_sprite_names = false;
        cfg.allow_rotation = false;
        cfg.enable_normal_maps = false;
        cfg.variant_options = vec![VariantOptions {
            force_identical_layout: false,
            ..VariantOptions::default()
        }];
        apply_flag_options(&mut cfg, &flags);
        assert!(!cfg.enable_auto_detect_animations);
        assert!(cfg.trim_sprite_names);
        assert!(cfg.allow_rotation);
        assert!(cfg.enable_normal_maps);
        assert!(cfg.variant_options[0].force_identical_layout);

        // Los valores inválidos se rechazan con el nombre de su opción.
        let err = apply_parity_layout_options(&mut cfg, &[("common-divisor-x".into(), "0".into())])
            .unwrap_err();
        assert!(err.contains("--common-divisor-x"), "{err}");
        let err =
            apply_parity_layout_options(&mut cfg, &[("default-pivot-point".into(), "2,0".into())])
                .unwrap_err();
        assert!(err.contains("--default-pivot-point"), "{err}");
    }

    #[test]
    fn phase_c_value_flags_reach_the_config() {
        let (_, values, flags) = parse_args(&args(&[
            "--max-width",
            "2048",
            "--max-height",
            "1024",
            "--background-color",
            "#11223380",
            "--dpi",
            "300",
            "--ignore-files",
            "*.psd",
            "--ignore-files",
            "build/*",
            "--replace",
            "^old=new",
            "--heuristic-mask",
            "--force-publish",
        ]));
        let mut cfg = ProjectConfig::default();
        apply_phase_c_options(&mut cfg, &values).unwrap();
        apply_flag_options(&mut cfg, &flags);

        assert_eq!(cfg.max_width, 2048);
        assert_eq!(cfg.max_height, 1024);
        assert_eq!(cfg.background_color, Some([0x11, 0x22, 0x33, 0x80]));
        assert_eq!(cfg.dpi, Some(300));
        assert_eq!(cfg.ignore_patterns, vec!["*.psd", "build/*"]);
        assert_eq!(
            cfg.name_replacements,
            vec![("^old".to_string(), "new".to_string())]
        );
        assert!(cfg.heuristic_mask);
        assert!(cfg.force_publish);
        cfg.validate().unwrap();

        // El color sin alfa queda opaco y también se admite sin #.
        assert_eq!(
            parse_background_color("112233").unwrap(),
            [0x11, 0x22, 0x33, 0xff]
        );
        assert_eq!(
            parse_background_color("#abc").unwrap(),
            [0xaa, 0xbb, 0xcc, 0xff]
        );

        // Valores inválidos se rechazan con el nombre de su opción.
        for bad in [
            vec![("background-color".into(), "nope".into())],
            vec![("dpi".into(), "0".into())],
            vec![("dpi".into(), "2000000".into())],
            vec![("max-width".into(), "20000".into())],
            vec![("replace".into(), "sin-igual".into())],
            vec![("replace".into(), "(=x".into())],
        ] {
            let err = apply_phase_c_options(&mut ProjectConfig::default(), &bad).unwrap_err();
            assert!(err.starts_with("--"), "sin nombre de opción: {err}");
        }
    }

    #[test]
    fn exporter_only_options_are_registered_and_rejected() {
        // Ninguna es «desconocida»: están registradas con su grafía, y se
        // rechazan diciendo de qué exportador hablan.
        for (name, format) in EXPORTER_ONLY_OPTIONS {
            let flag = format!("--{name}");
            let list = [flag.as_str(), "valor"];
            let (_, values, flags) = parse_args(&args(&list));
            assert!(
                check_unknown_options(&values, &flags, PACK_VALUES, PACK_FLAGS).is_ok(),
                "--{name} no está registrado"
            );
            let err = check_exporter_only_options(&values, &flags).unwrap_err();
            assert!(err.contains(&flag), "{err}");
            assert!(err.contains(*format), "{err}");
            assert!(err.contains("--exporter-list"), "{err}");
        }

        // Con bools también se acepta la forma sin valor.
        let (_, _, flags) = parse_args(&args(&["--spine-legacy-output"]));
        assert!(check_unknown_options(&[], &flags, PACK_VALUES, PACK_FLAGS).is_ok());
        assert!(check_exporter_only_options(&[], &flags).is_err());

        // Sin esas opciones no molesta, y el resto sigue funcionando.
        assert!(check_exporter_only_options(&[], &[]).is_ok());
        assert!(check_unknown_options(
            &[("css-sprite-prefix".into(), "icon-".into())],
            &[],
            PACK_VALUES,
            PACK_FLAGS
        )
        .is_ok());
    }

    #[test]
    fn css_and_plain_exporter_options_reach_the_config() {
        let (_, values, flags) = parse_args(&args(&[
            "--css-sprite-prefix",
            "icon-",
            "--css-media-query-2x",
            "(-webkit-min-device-pixel-ratio: 2)",
            "--plain-string-property",
            "hola",
            "--plain-bool-property",
            "false",
            "--disable-rotation",
            "--enable-cache-busting",
        ]));
        let mut cfg = ProjectConfig {
            allow_rotation: true,
            cache_busting: false,
            ..ProjectConfig::default()
        };
        apply_phase_c_options(&mut cfg, &values).unwrap();
        apply_flag_options(&mut cfg, &flags);
        assert_eq!(cfg.css_sprite_prefix.as_deref(), Some("icon-"));
        assert_eq!(
            cfg.css_media_query_2x.as_deref(),
            Some("(-webkit-min-device-pixel-ratio: 2)")
        );
        assert_eq!(cfg.plain_string_property.as_deref(), Some("hola"));
        assert_eq!(cfg.plain_bool_property, Some(false));
        assert!(!cfg.allow_rotation);
        assert!(cfg.cache_busting);
        cfg.validate().unwrap();

        let err = apply_phase_c_options(
            &mut ProjectConfig::default(),
            &[("plain-bool-property".into(), "quizas".into())],
        )
        .unwrap_err();
        assert!(err.contains("--plain-bool-property"), "{err}");
    }

    #[test]
    fn custom_exporters_directory_adds_an_id() {
        let dir = std::env::temp_dir().join("tpcli_custom_exporters");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("miexportador.hbs"),
            "{{#each frames}}{{this.filename}}\n{{/each}}",
        )
        .unwrap();
        std::fs::write(dir.join("otro.json.hbs"), "{}").unwrap();

        let mut cfg = ProjectConfig {
            custom_exporters_directory: Some(dir.clone()),
            ..ProjectConfig::default()
        };
        assert!(cfg.select_custom_exporter("miexportador"));
        assert_eq!(
            cfg.export_template.as_deref(),
            Some(dir.join("miexportador.hbs").as_path())
        );

        // Un id que no existe no vale, ni uno con ruta en la del directorio.
        let mut cfg = ProjectConfig {
            custom_exporters_directory: Some(dir.clone()),
            ..ProjectConfig::default()
        };
        assert!(!cfg.select_custom_exporter("no-existe"));
        assert!(!cfg.select_custom_exporter("../algo"));

        // --format con ese id pasa por apply_template_format.
        let mut cfg = ProjectConfig {
            custom_exporters_directory: Some(dir.clone()),
            ..ProjectConfig::default()
        };
        apply_template_format(&mut cfg, "miexportador").unwrap();
        assert!(cfg.export_template.is_some());
        assert!(apply_template_format(&mut cfg, "otro.json").is_ok());

        // Sin directorio configurado sigue fallando como antes.
        let mut cfg = ProjectConfig::default();
        assert!(apply_template_format(&mut cfg, "miexportador").is_err());

        // Lo que --exporter-list lista.
        assert_eq!(
            tp_core::dataformats::custom_exporter_ids(&dir),
            vec!["miexportador".to_string(), "otro.json".to_string()]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
