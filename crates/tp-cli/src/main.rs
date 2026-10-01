//! TexturePacker-RS command-line interface.
//!
//! ```
//! tp-cli pack project.tpproj
//! tp-cli pack --input sprites/ --output build/ --max-size 4096 --format astc
//! tp-cli decrypt atlas_0.png.tpenc --key secreto -o atlas.png
//! ```

use std::path::{Path, PathBuf};
use tp_core::config::{
    AlphaHandling, BasicSortBy, ColorDepth, DitheringAlgorithm, DxtMode, GdxFilter, GpuFormat,
    PackMode, PackingAlgorithm, PackingStrategy, PixelFormat, PngDither, ProjectConfig, ScaleMode,
    SizeConstraint, SortOrder, TemplateFormat, TrimMode, VariantOptions,
};

/// Opciones de `pack` que reciben un valor. El parseador no consume el token
/// siguiente para las claves que no están aquí ni en [`PACK_FLAGS`], y
/// [`check_unknown_options`] rechaza cualquier clave al margen de ambas listas.
const PACK_VALUES: &[&str] = &[
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
const PACK_FLAGS: &[&str] = &[
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
const DECRYPT_VALUES: &[&str] = &["key", "o", "out", "pixel-format"];
const DECRYPT_FLAGS: &[&str] = &["help", "quiet", "verbose"];

/// Rechaza lo que no esté en los registros: hoy la CLI ignoraba en silencio
/// cualquier opción desconocida (un `--scale 0.5` del original «funcionaba»
/// sin efecto).
fn check_unknown_options(
    values: &[(String, String)],
    flags: &[String],
    known_values: &[&str],
    known_flags: &[&str],
) -> Result<(), String> {
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

/// Opciones que en el original sólo escriben propiedades de un exportador
/// concreto y que este clon todavía no plantea. Van registradas (para que no
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
fn check_exporter_only_options(
    values: &[(String, String)],
    flags: &[String],
) -> Result<(), String> {
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

/// `<bool>` del original: `true`/`false`, `1`/`0`, `yes`/`no`, `on`/`off`.
fn parse_bool(value: &str, flag: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!("{flag} inválido: {value} (true | false)")),
    }
}

/// Línea de `--version`, con el mismo patrón del original.
fn version_line() -> String {
    format!("TexturePacker-RS {}", env!("CARGO_PKG_VERSION"))
}

/// Ids de los `<id>.hbs` de un directorio de exportadores propios
/// (`--custom-exporters-directory`), ordenados para una salida estable.
fn custom_exporter_ids(dir: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_suffix(".hbs") {
                if !id.is_empty() && !id.contains(['/', '\\']) {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

/// Ids de data formats que aceptan `--format`/`--template-format`, uno por
/// línea, como el `--exporter-list` del original.
fn exporter_list_text() -> String {
    let mut ids: Vec<&str> = tp_core::dataformats::data_format_ids().collect();
    ids.sort_unstable();
    ids.dedup();
    let mut text = ids.join("\n");
    text.push('\n');
    text
}

/// Texto de la ayuda. Está separado de [`usage`] para poder testear que toda
/// opción documentada exista realmente en el parser.
fn help_text() -> String {
    format!(
        "TexturePacker-RS {}\n\
         \n\
         USOS:\n\
         \x20 tp-cli pack <proyecto.tpproj>\n\
         \x20 tp-cli pack --input DIR --output DIR [opciones]\n\
         \x20 tp-cli pack [opciones] <carpeta|imagen>…   (como el original: los sprites\n\
         \x20                       pueden ir en posiciónles, mezclados con las opciones)\n\
         \x20 tp-cli decrypt <archivo.tpenc> --key CLAVE -o salida.png [--pixel-format F]\n\
         \n\
         ENTRADA Y SALIDA:\n\
         \x20 --input DIR           Carpeta o fichero con los sprites (también en posiciónles)\n\
         \x20 --output DIR          Carpeta de salida de la hoja y de los metadatos\n\
         \x20 --sheet FICHERO       Hoja de salida: fija --output, el nombre base y el\n\
         \x20                        formato por su extensión (sobrescribe --output)\n\
         \x20 --data FICHERO        Metadatos de salida: fija --output y el nombre base;\n\
         \x20                        la extensión debe coincidir con el formato de datos\n\
         \x20 --format T            Doble acepción, como en el original:\n\
         \x20                          textura: png | png8 | jpg | webp | bmp | tga | tiff |\n\
         \x20                            dds | zktx | pvr3gz | pvr3ccz | pkm | ktx | astc |\n\
         \x20                            etc2 | pvrtc | ktx2 | basis\n\
         \x20                          datos: json | xml | plist | cpp | tsv | text o un\n\
         \x20                            exportador (json-hash, libgdx, cocos2d, phaser,\n\
         \x20                            pixijs4, sparrow, spine…)\n\
         \x20 --texture-format T    Formato de la hoja si no se deduce de la extensión de\n\
         \x20                        --sheet (tiene prioridad sobre --format de textura)\n\
         \x20 --base-name NOMBRE    Nombre base de la salida (admite {{n}} {{n1}} {{v}})\n\
         \x20 --template-format T   Sólo la acepción de datos de --format (familias y\n\
         \x20                        exportadores; json-hash es el hash del original)\n\
         \x20 --prepend-folder-name Antepone el nombre de la carpeta inteligente\n\
         \x20 --trim-sprite-names   Nombres de sprite sin extensión (defecto)\n\
         \x20 --keep-extension      Los nombres de sprite conservan la extensión\n\
         \x20 --no-recursive        No buscar en subdirectorios\n\
         \x20 --ignore-files PATRÓN Excluye los ficheros que coincidan (repetible; el *\n\
         \x20                        de glob_match también incluye /)\n\
         \x20 --replace PATRÓN=TEXTO Renombra sprites con una expresión regular (repetible,\n\
         \x20                        en orden; p.ej. ^old=new)\n\
         \x20 --heuristic-mask      Borra el color de fondo de un sprite totalmente opaco\n\
         \x20                        (color más frecuente del borde; antes de recortar)\n\
         \x20 --auto-folders        pack por carpetas automático: cada subcarpeta de entrada\n\
         \x20                        produce su hoja en la subcarpeta de salida\n\
         \x20 --multipack           Permitir varias hojas (anula --no-multipack)\n\
         \x20 --no-multipack        No emitir varias hojas (falla si no caben en una)\n\
         \x20 --cache-busting       Añade ?v=<hash> a la textura citada en los metadatos\n\
         \x20 --enable-cache-busting Alias del original de --cache-busting\n\
         \x20 --texture-path RUTA   Prefijo de la textura en los metadatos (alias: --texturepath)\n\
         \x20 --print-json          Imprimir los metadatos JSON en stdout\n\
         \x20 --force-publish       Reescribir la salida aunque los bytes no hayan cambiado\n\
         \x20 --save FICHERO        Guardar la configuración como .tpproj y terminar sin\n\
         \x20                        empaquetar (avisa si la extensión no es .tpproj)\n\
         \n\
         ATLAS:\n\
         \x20 --max-size N          Tamaño máximo del atlas (512..8192, potencia de 2)\n\
         \x20 --max-width N         Tope propio del ancho del atlas (0 = sin tope, 1..16384)\n\
         \x20 --max-height N        Tope propio del alto del atlas (0 = sin tope, 1..16384)\n\
         \x20 --background-color HEX  Color con el que se rellena la hoja (RGB/RRGGBB/RRGGBBAA)\n\
         \x20 --width N             Ancho fijo del atlas (0 = automático)\n\
         \x20 --height N            Alto fijo del atlas (0 = automático)\n\
         \x20 --shape-padding N     Espacio entre sprites (px)\n\
         \x20 --padding N           Alias corto de --shape-padding\n\
         \x20 --border-padding N    Margen entre los sprites y el borde (px)\n\
         \x20 --align-to-grid N     Alinea las esquinas de los sprites a N px (0 = off);\n\
         \x20                        alias: --align\n\
         \x20 --extrude N           Extrude de bordes (px)\n\
         \x20 --common-divisor N    Ancho y alto de los sprites múltiplos de N (los dos ejes)\n\
         \x20 --common-divisor-x N  Igual, sólo el eje horizontal\n\
         \x20 --common-divisor-y N  Igual, sólo el eje vertical\n\
         \x20 --default-pivot-point X,Y  Pivot por defecto de todos los sprites, normalizado\n\
         \x20                        (0,0 = esquina superior izquierda; defecto 0.5,0.5)\n\
         \x20 --force-identical-layout  Mismo layout en todas las variantes (requiere\n\
         \x20                        allowfraction en las variantes afectadas)\n\
         \x20 --algorithm T         maxrects | polygon | guillotine | grid | basic | manual\n\
         \x20 --strategy T          bssf (ShortSideFit) | baf (AreaFit) | blsf (LongSideFit) |\n\
         \x20                        best | bottom-left | contact-point | guillotine\n\
         \x20                        (alias: --maxrects-heuristics)\n\
         \x20 --pack-mode T         fast | good | best\n\
         \x20 --basic-sort-by T     best | name | width | height | area | circumference\n\
         \x20 --basic-order T       ascending | descending\n\
         \x20 --size-constraints T  any | pot | multiple-of-4 | word-aligned\n\
         \x20 --force-squared       Atlas cuadrado\n\
         \x20 --no-rotation         Desactivar rotación 90°\n\
         \x20 --disable-rotation    Alias del original de --no-rotation\n\
         \x20 --enable-rotation     Activar la rotación 90° (defecto)\n\
         \x20 --polygon             Modo polígono (mallas + empaquetado por contorno)\n\
         \x20 --tolerance N         Tolerancia de la aproximación poligonal en px (defecto\n\
         \x20                        1,5); el original la pide con tracer-tolerance en otras\n\
         \x20                        unidades, así que no se acepta ese nombre\n\
         \x20 --trim-mode T         none | trim | crop | cropkeeppos | polygon (defecto trim)\n\
         \x20 --trim-threshold N    Umbral de alpha para recorte (1-255, por defecto 1)\n\
         \x20 --trim-margin N       Margen transparente tras el recorte (px)\n\
         \x20 --no-trim             No recortar los sprites (equivale a --trim-mode none)\n\
         \x20 --no-aliasing         Desactivar deduplicación por hash (alias: --disable-auto-alias)\n\
         \x20 --shape-debug         Dibuja los contornos de los sprites sobre la hoja\n\
         \x20 --scale-mode T        smooth | fast | scale2x | scale3x | scale4x | eagle\n\
         \x20 --variant E[:N[:F[:allowfraction[:W:H]]]]  Variante (repetible o por comas),\n\
         \x20                        p.ej. 0.5:-hd, 1.0:-ipadhd::*, 0.25:::allowfraction:1024:1024\n\
         \x20 --variants LIST       Escalas, p.ej. 2,0.5 (sufijos @2x, -hd)\n\
         \x20 --scale F             Escala todas las variantes por F (0 < F <= 8); con una\n\
         \x20                        única variante el sufijo de fichero queda vacío\n\
         \n\
         CALIDAD:\n\
         \x20 --color-depth T       RGBA8888 | RGBA4444 | RGB565\n\
         \x20 --dither T            none | nn | linear | floyd | floyd-alpha | atkinson |\n\
         \x20                        atkinson-alpha (alias: --dither-type)\n\
         \x20 --alpha-handling T    keep | clear | bleed | premultiply\n\
         \x20 --png-opt-level N     Optimización PNG sin pérdida, 0-7 (1 = indexa si ≤256 colores)\n\
         \x20 --png8-dither T       Dithering PNG-8: low | medium | high\n\
         \x20 --jpg-quality N       Calidad JPG (0-100)\n\
         \x20 --webp-quality N      Calidad WebP (0-100; por defecto sin pérdida)\n\
         \x20 --pixel-format T      rgba8888 | rgb888 | alpha8 | intensity8 | alpha-intensity8 | rgba5551 | rgba5555 | bgra8888\n\
         \x20                        rgba4444 | rgb565 | pvrtc2bpp-rgba | pvrtc4bpp-rgba | pvrtc2bpp-rgb | pvrtc4bpp-rgb\n\
         \x20                        etc1 | etc2 | etc2-rgb | dxt1 | dxt5 | astc-4x4 | astc-8x8 | astc-12x12\n\
         \x20                        (alias: --opt)\n\
         \x20 --pvr-quality N       Calidad PVRTC 0-7 (defecto 3)\n\
         \x20 --etc1-quality N      Calidad ETC1 0-100 (defecto 70)\n\
         \x20 --etc2-quality N      Calidad ETC2 0-100 (defecto 70)\n\
         \x20 --astc-quality N      Calidad ASTC 0-4: 0=fastest .. 4=exhaustive (defecto 2)\n\
         \x20 --basis-quality N     Calidad Basis ETC1S 0-100 (defecto 50; alias: --basisu-quality)\n\
         \x20 --dxt-mode T          DXT_LINEAR (error uniforme) | DXT_PERCEPTUAL (ponderado)\n\
         \x20 --flip-y              Voltea la textura (alias: --flip-pvr)\n\
         \x20 --dpi N               Resolución de la hoja en ppp (1..1000000); sólo PNG\n\
         \n\
         METADATOS Y EXPORTADORES:\n\
         \x20 --class-file F        Fichero de clase Swift extra (spritekit-swift)\n\
         \x20 --header-file F       Cabecera C++/ObjC extra (cocos2d-x)\n\
         \x20 --source-file F       Código fuente C++ extra (cocos2d-x)\n\
         \x20 --spriteids-file F    Lista de ids de sprites extra (amethyst)\n\
         \x20 --template F          Plantilla de metadatos propia\n\
         \x20 --gdx-filter T        Filtro del data format de LibGDX: linear | nearest\n\
         \x20 --key CLAVE           Cifrar texturas con AES-256-GCM\n\
         \x20 --key-name NOMBRE     Usar la clave global guardada con ese nombre\n\
         \x20 --save-key NOMBRE     Guardar --key en el almacén global y usarla\n\
         \x20 --custom-exporters-directory DIR  Carga <id>.hbs propios como formatos de\n\
         \x20                        datos, válidos en --format y --template-format\n\
         \x20 --convert-texture FICHERO  Convierte una imagen a --texture-format y termina\n\
         \x20                        (aplica también --pixel-format, --dpi y --scale)\n\
         \x20 --css-sprite-prefix P  Prefijo de las clases CSS (p.ej. icon-)\n\
         \x20 --css-media-query-2x Q  Envuelve en esta media query la CSS de variantes >1\n\
         \x20 --plain-string-property TEXTO\n\
         \x20                        exporterProperties.string_property de la plantilla\n\
         \x20 --plain-bool-property BOOL  exporterProperties.bool_property (true|false)\n\
         \x20 Del original, registradas pero rechazadas con su motivo porque no escribimos\n\
         \x20 esa salida: easeljs-framerate, zim-framerate,\n\
         \x20 gamemaker-texturegroup-frame-speed, classfile-file, spine-legacy-output,\n\
         \x20 libgdx-legacy-output, spritestudio-writePivots, orx-includeComments,\n\
         \x20 orx-keepInCache, orx-keyDuration, orx-optimizeSectionNames, orx-pixelSnap\n\
         \n\
         MAPAS DE NORMALES:\n\
         \x20 --pack-normalmaps     Empaquetar los mapas de normales (defecto)\n\
         \x20 --no-normals          No empaquetar mapas de normales\n\
         \x20 --normalmap-suffix T  Sufijo del mapa de normales (defecto _normal)\n\
         \x20 --normalmap-filter T  Ficheros con esta subcadena en la ruta son normales\n\
         \x20 --normalmap-sheet N   Nombre base de la hoja de normales (defecto <imagen>_normal)\n\
         \x20 --normalmap-detect    Detectar mapas de normales por su color\n\
         \n\
         ANIMACIONES:\n\
         \x20 --no-auto-animations  No agrupar sprites walk_001..N como animaciones\n\
         \n\
         INFORMACIÓN:\n\
         \x20 --help                Esta ayuda (alias: -h)\n\
         \x20 --version             Versión del programa\n\
         \x20 --exporter-list       Ids de data formats aceptados por --format de datos\n\
         \x20 --verbose             Detalle extra de la entrada y la configuración usada\n\
         \x20 --quiet               Sólo errores (calla el resumen de empaquetado)",
        env!("CARGO_PKG_VERSION")
    )
}

fn usage() -> ! {
    println!("{}", help_text());
    std::process::exit(0);
}

fn fail(msg: String) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        usage();
    }
    match args[0].as_str() {
        "--version" | "-V" => println!("{}", version_line()),
        "--exporter-list" => print!("{}", exporter_list_text()),
        "pack" => cmd_pack(&args[1..]),
        "decrypt" => cmd_decrypt(&args[1..]),
        other => fail(format!("Comando desconocido: {other}")),
    }
}

/// Divide los argumentos en posicionales, `--clave valor` y `--flag`. Las
/// claves de [`PACK_FLAGS`] nunca consumen el token siguiente, así que
/// `pack sprites --no-rotation` no se come la carpeta.
fn parse_args(args: &[String]) -> (Vec<String>, Vec<(String, String)>, Vec<String>) {
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
/// como en el original: se acepta repetida o separada por comas. Los trozos
/// que no empiezan por una escala vuelven al filtro anterior, así que un
/// filtro con comas escribe bien. Fija también `scale_variants`.
fn apply_variant_flags(values: &[(String, String)], cfg: &mut ProjectConfig) {
    let variant_values: Vec<String> = values
        .iter()
        .filter(|(k, _)| k == "variant")
        .map(|(_, v)| v.clone())
        .collect();
    if variant_values.is_empty() {
        return;
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
            .unwrap_or_else(|_| fail(format!("--variant inválido: {segment}")));
        if !(0.0..=8.0).contains(&scale) {
            fail(format!(
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
            fail(format!(
                "--variant: cuarto campo desconocido (use allowfraction): {segment}"
            ));
        }
        let accept_fractional = fourth.eq_ignore_ascii_case("allowfraction");
        let max_texture_size = match (fields.get(4), fields.get(5)) {
            (Some(w), Some(h)) => {
                let number = |raw: &str, what: &str| -> i32 {
                    raw.trim()
                        .parse()
                        .unwrap_or_else(|_| fail(format!("--variant {what} inválido: {segment}")))
                };
                let (w, h) = (number(w, "ancho"), number(h, "alto"));
                if w <= 0 || h <= 0 || w != h {
                    fail(format!(
                        "--variant: el tamaño máximo debe ser un cuadrado > 0: {segment}"
                    ));
                }
                Some(w)
            }
            (None, None) => None,
            _ => fail(format!(
                "--variant: faltan el ancho o el alto del tamaño máximo: {segment}"
            )),
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
}

/// Mapea `--template-format` sobre la config.
///
/// Primero los tokens legados del original (cada familia con su semántica
/// concreta) y después los ids de exportador que el propio original enumera
/// para sus data formats (`libgdx`, `cocos2d`, `phaser`…), que aplican además
/// los valores recomendados del preset. `json` legado es el array, así que el
/// hash del original se pide como `json-hash`.
fn apply_template_format(cfg: &mut ProjectConfig, value: &str) -> Result<(), String> {
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
            if apply_custom_exporter(cfg, id) {
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

/// `--custom-exporters-directory DIR` + `--format <id>`: si existe
/// `DIR/<id>.hbs`, ese exportador es válido y su plantilla es la salida. La
/// extensión la sigue decidiendo la familia (`--template-format`, json por
/// defecto), igual que en el original, donde el exportador propio sólo
/// aporta el texto.
fn apply_custom_exporter(cfg: &mut ProjectConfig, id: &str) -> bool {
    let Some(dir) = cfg.custom_exporters_directory.clone() else {
        return false;
    };
    if id.is_empty() || id.contains(['/', '\\']) {
        return false;
    }
    let path = dir.join(format!("{id}.hbs"));
    if !path.is_file() {
        return false;
    }
    cfg.export_template = Some(path);
    true
}

/// Qué es un argumento posicional de la línea de comandos.
enum Positional {
    /// Un `.tpproj` (el `.tps` del original ocupa ese sitio).
    Project,
    /// Una carpeta o imagen con sprites.
    Input,
    /// Algo que no podemos leer, como un `.tps` del original.
    Unsupported,
}

fn classify_positional(path: &Path) -> Positional {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("tpproj") | Some("toml") => Positional::Project,
        Some("tps") => Positional::Unsupported,
        _ => Positional::Input,
    }
}

fn stem_of(path: &Path, flag: &str) -> Result<String, String> {
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

/// Rutas del original: `--sheet` fija carpeta, nombre base y formato de
/// textura por la extensión; `--data` fija carpeta y nombre base. Nuestro
/// modelo sólo admite un nombre base para los dos, así que se comprueba que
/// convivan. Devuelve la ruta de `--data` (para validar la extensión cuando
/// ya se conozca el formato) y un aviso si la hoja pedía numeración `{n}`.
fn apply_output_paths(
    cfg: &mut ProjectConfig,
    values: &[(String, String)],
) -> Result<(Option<PathBuf>, Option<String>), String> {
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
fn check_data_extension(cfg: &ProjectConfig, path: &Path) -> Result<(), String> {
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
enum FormatTarget {
    Texture,
    Data,
}

/// `--format` del original: formato de textura para unos (`png`, `ktx`…) y
/// formato de datos para otros (`phaser`, `libgdx`…). Los dos conjuntos son
/// disjuntos, así que el orden de la comprobación no cambia nada.
fn apply_format(cfg: &mut ProjectConfig, value: &str) -> Result<FormatTarget, String> {
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
fn apply_texture_format(cfg: &mut ProjectConfig, value: &str) -> Result<(), String> {
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

fn parse_divisor(value: &str, flag: &str) -> Result<i32, String> {
    let d: i32 = value
        .parse()
        .map_err(|_| format!("{flag} inválido: {value}"))?;
    if d < 1 {
        return Err(format!("{flag} debe ser 1 o más: {value}"));
    }
    Ok(d)
}

/// `--default-pivot-point X,Y` en unidades normalizadas como el original.
fn parse_pivot_point(value: &str) -> Result<(f32, f32), String> {
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
fn parse_atlas_limit(value: &str, flag: &str) -> Result<i32, String> {
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
fn parse_background_color(value: &str) -> Result<[u8; 4], String> {
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
fn parse_replacement(value: &str) -> Result<(String, String), String> {
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
fn apply_phase_c_options(
    cfg: &mut ProjectConfig,
    values: &[(String, String)],
) -> Result<(), String> {
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
fn apply_scale_factor(cfg: &mut ProjectConfig, factor: f32) {
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

/// Opciones de layout que el original expone y la GUI ya sabe aplicar: ejes
/// sueltos del common divisor y pivot por defecto. Va después de
/// `--common-divisor` para que el eje gane sobre el valor conjunto.
fn apply_parity_layout_options(
    cfg: &mut ProjectConfig,
    values: &[(String, String)],
) -> Result<(), String> {
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
fn apply_flag_options(cfg: &mut ProjectConfig, flags: &[String]) {
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
        // Alias del original de nuestro `--no-rotation`.
        cfg.allow_rotation = false;
    }
    if has("enable-cache-busting") {
        // Alias del original de nuestro `--cache-busting`.
        cfg.cache_busting = true;
    }
    if has("heuristic-mask") {
        cfg.heuristic_mask = true;
    }
    if has("force-publish") {
        cfg.force_publish = true;
    }
}

/// Escribe el proyecto como TOML y devuelve el mensaje de resumen. La
/// extensión distinta de `.tpproj` se avisa pero no se rechaza, como en el
/// original, que guarda el archivo que se le pida.
fn save_project(cfg: &ProjectConfig, path: &Path) -> Result<String, String> {
    let text = cfg
        .to_toml()
        .map_err(|e| format!("No se pudo serializar el proyecto: {e}"))?;
    std::fs::write(path, text)
        .map_err(|e| format!("No se pudo escribir {}: {e}", path.display()))?;
    let mut msg = format!("✔ Proyecto guardado en {}", path.display());
    if path.extension().and_then(|e| e.to_str()) != Some("tpproj") {
        msg.push_str(" (aviso: la extensión no es .tpproj)");
    }
    Ok(msg)
}

/// `--convert-texture FICHERO`: convierte una sola imagen al formato
/// pedido, sin empaquetar. Aplican el formato y sus calidades (`--format`,
/// `--texture-format`, `--pixel-format`), el `--dpi` de los PNG y el `--scale`
/// de la primera variante; el recorte no tiene sentido sin hoja, así que no
/// se aplica. Escribe junto a la entrada, o en `--output` si se dio.
fn convert_texture(cfg: &ProjectConfig, input: &Path) -> Result<String, String> {
    if !input.is_file() {
        return Err(format!("--convert-texture: no existe {}", input.display()));
    }
    let (w, h, rgba) =
        tp_core::reader::load_image_rgba(input).map_err(|e| format!("--convert-texture: {e}"))?;
    let factor = cfg.scale_variants.first().copied().unwrap_or(1.0);
    let (pixels, width, height) = if (factor - 1.0).abs() < 1e-6 {
        (rgba, w as usize, h as usize)
    } else {
        tp_core::export::scale_rgba(&rgba, w as usize, h as usize, factor, cfg.scale_mode)
    };
    let opts = tp_core::export::EncodeOptions::from_config(cfg);
    let mut bytes = tp_core::export::encode_to_bytes(&pixels, width, height, &opts)
        .map_err(|e| format!("--convert-texture: {e}"))?;
    bytes = tp_core::export::apply_dpi(bytes, cfg.gpu_format, cfg.dpi);

    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let file_name = format!("{stem}.{}", cfg.gpu_format.file_extension());
    let dir = if cfg.output_directory.as_os_str().is_empty() {
        input
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf()
    } else {
        cfg.output_directory.clone()
    };
    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    }
    let out = dir.join(file_name);
    std::fs::write(&out, &bytes)
        .map_err(|e| format!("No se pudo escribir {}: {e}", out.display()))?;
    Ok(format!(
        "✔ Convertido {} → {} ({width}×{height}, {})",
        input.display(),
        out.display(),
        cfg.gpu_format.as_str()
    ))
}

fn cmd_pack(args: &[String]) {
    let (positionals, values, flags) = parse_args(args);
    check_unknown_options(&values, &flags, PACK_VALUES, PACK_FLAGS).unwrap_or_else(|e| fail(e));
    check_exporter_only_options(&values, &flags).unwrap_or_else(|e| fail(e));
    let has = |k: &str| flags.iter().any(|f| f == k);
    let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
    let quiet = has("quiet");

    if has("help") {
        usage();
    }
    if has("version") {
        println!("{}", version_line());
        return;
    }
    if has("exporter-list") {
        print!("{}", exporter_list_text());
        // Los `<id>.hbs` de --custom-exporters-directory también son formatos.
        if let Some(dir) = val("custom-exporters-directory") {
            let ids = custom_exporter_ids(Path::new(&dir));
            if ids.is_empty() {
                eprintln!("⚠ --custom-exporters-directory: no hay <id>.hbs en {dir}");
            } else {
                println!("Exportadores propios de {dir}:");
                for id in ids {
                    println!("  {id}");
                }
            }
        }
        return;
    }

    let mut cfg = ProjectConfig::default();

    // Posicionales: un `.tpproj` (como el `.tps` del original) y el resto
    // son sprites, tal y como se pasan en la línea de comandos del original.
    let mut project: Option<PathBuf> = None;
    let mut inputs: Vec<PathBuf> = Vec::new();
    for raw in &positionals {
        match classify_positional(Path::new(raw)) {
            Positional::Project => {
                if project.is_some() {
                    fail("sólo se admite un proyecto en la línea de comandos".into());
                }
                project = Some(PathBuf::from(raw));
            }
            Positional::Unsupported => fail(format!(
                "{raw}: los proyectos del original (.tps) no son legibles; usa un .tpproj"
            )),
            Positional::Input => {
                if !Path::new(raw).exists() {
                    fail(format!("no existe: {raw}"));
                }
                if Path::new(raw).is_file() && !tp_core::ingest::is_image_file(Path::new(raw)) {
                    fail(format!("no es una imagen: {raw}"));
                }
                inputs.push(PathBuf::from(raw));
            }
        }
    }

    if let Some(p) = &project {
        let text = std::fs::read_to_string(p)
            .unwrap_or_else(|e| fail(format!("No se pudo leer {}: {e}", p.display())));
        cfg = ProjectConfig::from_toml(&text)
            .unwrap_or_else(|e| fail(format!("Proyecto inválido {}: {e}", p.display())));
    }
    if let Some(v) = val("input") {
        cfg.input_directory = PathBuf::from(v);
    }
    if let Some(v) = val("output") {
        cfg.output_directory = PathBuf::from(v);
    }
    for path in inputs {
        if cfg.input_directory.as_os_str().is_empty() {
            cfg.input_directory = path;
        } else {
            cfg.extra_inputs.push(path);
        }
    }
    // `--sheet`/`--data` del original: fijan carpeta, nombre base y formato.
    let (data_path, path_warning) =
        apply_output_paths(&mut cfg, &values).unwrap_or_else(|e| fail(e));
    if let Some(v) = val("base-name") {
        cfg.base_file_name = v;
    }
    if let Some(v) = val("max-size") {
        cfg.max_texture_size = v
            .parse()
            .unwrap_or_else(|_| fail("--max-size inválido".into()));
    }
    // `--shape-padding` es el nombre canónico; `--padding` queda
    // como alias corto.
    if let Some(v) = val("shape-padding").or_else(|| val("padding")) {
        cfg.padding = v
            .parse()
            .unwrap_or_else(|_| fail("--shape-padding inválido".into()));
    }
    if let Some(v) = val("extrude") {
        cfg.extrude = v
            .parse()
            .unwrap_or_else(|_| fail("--extrude inválido".into()));
    }
    if let Some(v) = val("border-padding") {
        cfg.border_padding = v
            .parse()
            .unwrap_or_else(|_| fail("--border-padding inválido".into()));
    }
    if let Some(v) = val("common-divisor") {
        let d: i32 = v
            .parse()
            .unwrap_or_else(|_| fail("--common-divisor inválido".into()));
        cfg.common_divisor_x = d;
        cfg.common_divisor_y = d;
    }
    // Ejes sueltos y pivot por defecto: van después de `--common-divisor` para
    // que el eje gane sobre el valor conjunto.
    apply_parity_layout_options(&mut cfg, &values).unwrap_or_else(|e| fail(e));
    // Fase C: topes por eje, fondo, dpi, filtros de entrada y renombrado.
    apply_phase_c_options(&mut cfg, &values).unwrap_or_else(|e| fail(e));
    if let Some(v) = val("align-to-grid").or_else(|| val("align")) {
        cfg.align_to_grid = v
            .parse()
            .unwrap_or_else(|_| fail("--align-to-grid inválido".into()));
    }
    if let Some(v) = val("alpha-handling") {
        cfg.alpha_handling = AlphaHandling::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--alpha-handling inválido: {v}")));
    }
    if let Some(v) = val("scale-mode") {
        cfg.scale_mode = ScaleMode::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--scale-mode inválido: {v}")));
    }
    if let Some(v) = val("texture-path").or_else(|| val("texturepath")) {
        cfg.texture_path = Some(v);
    }
    if let Some(v) = val("trim-threshold") {
        cfg.trim_threshold = v
            .parse()
            .unwrap_or_else(|_| fail("--trim-threshold inválido".into()));
    }
    if let Some(v) = val("trim-mode") {
        cfg.trim_mode = TrimMode::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--trim-mode inválido: {v}")));
        cfg.enable_trim = cfg.trim_mode != TrimMode::None;
    }
    if let Some(v) = val("trim-margin") {
        cfg.trim_margin = v
            .parse()
            .unwrap_or_else(|_| fail("--trim-margin inválido".into()));
    }
    if let Some(v) = val("tolerance") {
        cfg.polygon_tolerance = v
            .parse()
            .unwrap_or_else(|_| fail("--tolerance inválido".into()));
    }

    if let Some(v) = val("color-depth") {
        cfg.color_depth = match v.to_ascii_lowercase().as_str() {
            "rgba8888" => ColorDepth::Rgba8888,
            "rgba4444" => ColorDepth::Rgba4444,
            "rgb565" => ColorDepth::Rgb565,
            _ => fail(format!("--color-depth inválido: {v}")),
        };
    }
    if let Some(v) = val("dither").or_else(|| val("dither-type")) {
        cfg.dithering_algorithm = match v.to_ascii_lowercase().as_str() {
            "none" => DitheringAlgorithm::None,
            "floyd" | "floydsteinberg" => DitheringAlgorithm::FloydSteinberg,
            "floyd-alpha" | "floydsteinbergalpha" => DitheringAlgorithm::FloydSteinbergAlpha,
            "atkinson" => DitheringAlgorithm::Atkinson,
            "atkinson-alpha" | "atkinsonalpha" => DitheringAlgorithm::AtkinsonAlpha,
            "nn" | "nearestneighbour" | "nearest-neighbor" | "nearest" => {
                DitheringAlgorithm::NearestNeighbour
            }
            "linear" => DitheringAlgorithm::Linear,
            _ => fail(format!(
                "--dither inválido: {v} (usa none | NearestNeighbour | Linear | FloydSteinberg | FloydSteinbergAlpha | Atkinson | AtkinsonAlpha; alias nn, floyd, atkinson…)"
            )),
        };
    }
    if let Some(v) = val("format") {
        apply_format(&mut cfg, &v).unwrap_or_else(|e| fail(e));
    }
    if let Some(v) = val("texture-format") {
        apply_texture_format(&mut cfg, &v).unwrap_or_else(|e| fail(e));
    }
    if let Some(v) = val("png-opt-level") {
        cfg.png_opt_level = v
            .parse()
            .unwrap_or_else(|_| fail("--png-opt-level inválido".into()));
    }
    if let Some(v) = val("png8-dither") {
        cfg.png8_dither = match v.to_ascii_lowercase().as_str() {
            "low" => PngDither::Low,
            "medium" => PngDither::Medium,
            "high" => PngDither::High,
            _ => fail(format!("--png8-dither inválido: {v}")),
        };
    }
    if let Some(v) = val("jpg-quality") {
        cfg.jpg_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--jpg-quality inválido".into()));
    }
    if let Some(v) = val("webp-quality") {
        cfg.webp_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--webp-quality inválido".into()));
    }
    if let Some(v) = val("pixel-format").or_else(|| val("opt")) {
        cfg.pixel_format = parse_pixel_format(&v);
    }
    if let Some(v) = val("pvr-quality") {
        cfg.pvr_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--pvr-quality inválido".into()));
    }
    if let Some(v) = val("etc1-quality") {
        cfg.etc1_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--etc1-quality inválido".into()));
    }
    if let Some(v) = val("etc2-quality") {
        cfg.etc2_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--etc2-quality inválido".into()));
    }
    if let Some(v) = val("astc-quality") {
        cfg.astc_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--astc-quality inválido".into()));
    }
    if let Some(v) = val("basis-quality").or_else(|| val("basisu-quality")) {
        cfg.basis_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--basis-quality inválido".into()));
    }
    if let Some(v) = val("dxt-mode") {
        cfg.dxt_mode = DxtMode::parse(&v).unwrap_or_else(|| {
            fail(format!(
                "--dxt-mode inválido: {v} (DXT_LINEAR | DXT_PERCEPTUAL)"
            ))
        });
    }
    if let Err(e) = check_export_flags(&cfg) {
        fail(e);
    }
    if let Some(v) = val("strategy").or_else(|| val("maxrects-heuristics")) {
        cfg.packing_strategy = PackingStrategy::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--strategy inválido: {v}")));
    }
    if let Some(v) = val("algorithm") {
        cfg.algorithm = PackingAlgorithm::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--algorithm inválido: {v}")));
    }
    if let Some(v) = val("basic-sort-by") {
        cfg.basic_sort_by = BasicSortBy::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--basic-sort-by inválido: {v}")));
    }
    if let Some(v) = val("basic-order") {
        cfg.basic_order = SortOrder::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--basic-order inválido: {v}")));
    }
    if let Some(v) = val("pack-mode") {
        cfg.pack_mode = PackMode::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--pack-mode inválido: {v}")));
    }
    if let Some(v) = val("size-constraints") {
        cfg.size_constraints = SizeConstraint::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--size-constraints inválido: {v}")));
    }
    if let Some(v) = val("width") {
        cfg.fixed_width = v
            .parse()
            .unwrap_or_else(|_| fail("--width inválido".into()));
    }
    if let Some(v) = val("height") {
        cfg.fixed_height = v
            .parse()
            .unwrap_or_else(|_| fail("--height inválido".into()));
    }
    apply_variant_flags(&values, &mut cfg);

    if let Some(v) = val("variants") {
        let parsed: Vec<f32> = v
            .split([',', ';', ' '])
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| s.trim().parse().ok())
            .filter(|x| *x > 0.0 && *x <= 8.0)
            .collect();
        if !parsed.is_empty() {
            cfg.scale_variants = parsed;
        }
    }
    // `--scale` va después de --variants/--variant: multiplica lo que haya.
    if let Some(v) = val("scale") {
        let factor: f32 = v
            .parse()
            .unwrap_or_else(|_| fail(format!("--scale inválido: {v} (número en (0, 8])")));
        if !(factor > 0.0 && factor <= 8.0) {
            fail(format!("--scale inválido: {v} (número en (0, 8])"));
        }
        apply_scale_factor(&mut cfg, factor);
    }
    if let Some(v) = val("template-format") {
        if let Err(e) = apply_template_format(&mut cfg, &v) {
            fail(e);
        }
    }
    if let Some(v) = val("template") {
        cfg.export_template = Some(PathBuf::from(v));
    }
    if let Some(v) = val("class-file") {
        cfg.class_file = v;
    }
    if let Some(v) = val("header-file") {
        cfg.header_file = v;
    }
    if let Some(v) = val("source-file") {
        cfg.source_file = v;
    }
    if let Some(v) = val("spriteids-file") {
        cfg.spriteids_file = v;
    }
    if let Some(v) = val("gdx-filter") {
        cfg.gdx_filter = GdxFilter::parse(&v)
            .unwrap_or_else(|| fail(format!("--gdx-filter inválido: {v} (linear | nearest)")));
    }
    if let Some(v) = val("key") {
        cfg.encryption_key = if v.is_empty() { None } else { Some(v) };
    }
    if let Some(v) = val("key-name") {
        cfg.encryption_key_name = if v.trim().is_empty() { None } else { Some(v) };
    }
    if let Some(v) = val("save-key") {
        let key = cfg
            .encryption_key
            .clone()
            .unwrap_or_else(|| fail("--save-key requiere --key CLAVE".into()));
        tp_core::keys::put(&v, &key)
            .unwrap_or_else(|e| fail(format!("No se pudo guardar la clave: {e}")));
        cfg.encryption_key_name = Some(v);
    }
    if let Some(v) = val("normalmap-suffix") {
        cfg.normal_map_suffix = v;
    }
    if let Some(v) = val("normalmap-filter") {
        cfg.normal_map_filter = v;
    }
    if let Some(v) = val("normalmap-sheet") {
        cfg.normal_map_sheet = v;
    }

    // Opciones booleanas, incluidas las nuevas de paridad con el original.
    apply_flag_options(&mut cfg, &flags);

    if let Some(msg) = path_warning {
        if !quiet {
            println!("⚠ {msg}");
        }
    }
    // `--convert-texture` convierte una imagen y termina, sin empaquetar.
    if let Some(v) = val("convert-texture") {
        let msg = convert_texture(&cfg, Path::new(&v)).unwrap_or_else(|e| fail(e));
        if !quiet {
            println!("{msg}");
        }
        return;
    }
    // `--save FICHERO`: vuelca la configuración ya montada a un .tpproj y
    // termina, para poder guardar una línea de comandos y reutilizarla.
    if let Some(v) = val("save") {
        let msg = save_project(&cfg, Path::new(&v)).unwrap_or_else(|e| fail(e));
        if !quiet {
            println!("{msg}");
        }
        return;
    }
    if let Some(dp) = &data_path {
        check_data_extension(&cfg, dp).unwrap_or_else(|e| fail(e));
    }

    if cfg.input_directory.as_os_str().is_empty() {
        fail("Falta --input DIR (o pasa los sprites en posiciónles o un .tpproj)".into());
    }
    if cfg.output_directory.as_os_str().is_empty() {
        fail("Falta --output DIR, --sheet/--data (o un .tpproj)".into());
    }

    if has("verbose") && !quiet {
        let data_desc = if cfg.data_format.is_empty() {
            format!("{:?}", cfg.template_format)
        } else {
            cfg.data_format.clone()
        };
        println!(
            "· entrada: {} · salida: {} · nombre base: {}",
            cfg.input_directory.display(),
            cfg.output_directory.display(),
            cfg.base_file_name
        );
        println!(
            "· textura: {} · píxeles: {} · datos: {} · hasta {} px · padding {} + {}",
            cfg.gpu_format.as_str(),
            cfg.pixel_format.as_str(),
            data_desc,
            cfg.max_texture_size,
            cfg.padding,
            cfg.border_padding
        );
    }

    let started = std::time::Instant::now();
    // Pack por carpetas: si hay grupos con nombre y sprites, cada grupo
    // escribe su hoja en `<output>/<grupo>/`.
    let grouped = cfg
        .folder_groups
        .iter()
        .any(|g| !g.name.is_empty() && !g.sprites.is_empty());
    let out = if grouped {
        tp_core::pipeline::run_grouped(&cfg)
    } else {
        tp_core::pipeline::run(&cfg)
    }
    .unwrap_or_else(|e| fail(format!("Empaquetado fallido: {e}")));
    let result = &out.result;

    if !quiet {
        println!(
            "✔ Empaquetado en {} ms: {} sprites ({} aliases), {} página(s)",
            started.elapsed().as_millis(),
            result.total_sprites,
            result.alias_count,
            result.pages.len()
        );
        for w in &result.warnings {
            println!("⚠ {w}");
        }
        for f in &result.output_files {
            println!("  → {f}");
        }
        for (stage, ms) in &result.stage_times_ms {
            println!("  [{stage}] {ms} ms");
        }
    }
    if has("print-json") {
        // Render the base-scale JSON metadata and print it.
        let ctx = tp_core::templates::build_context(
            result,
            &result.pages,
            &result
                .pages
                .iter()
                .map(|p| p.file_name.clone())
                .collect::<Vec<_>>(),
            1.0,
            result.config.effective_trim_mode(),
        );
        println!("{}", serde_json::to_string_pretty(&ctx).unwrap());
    }
}

fn cmd_decrypt(args: &[String]) {
    let (positionals, values, flags) = parse_args(args);
    check_unknown_options(&values, &flags, DECRYPT_VALUES, DECRYPT_FLAGS)
        .unwrap_or_else(|e| fail(e));
    if flags.iter().any(|f| f == "help") {
        usage();
    }
    let Some(file) = positionals.first().map(PathBuf::from) else {
        fail("decrypt necesita un archivo .tpenc".into());
    };
    let Some(key) = values.iter().find(|(v, _)| v == "key").map(|(_, v)| v) else {
        fail("decrypt necesita --key CLAVE".into());
    };
    let out_path = values
        .iter()
        .find(|(v, _)| v == "o" || v == "out")
        .map(|(_, v)| PathBuf::from(v))
        .unwrap_or_else(|| {
            let mut p = file.clone();
            p.set_extension("dec.png");
            p
        });
    let data = std::fs::read(&file)
        .unwrap_or_else(|e| fail(format!("No se pudo leer {}: {e}", file.display())));
    let plain = tp_core::export::decrypt_bytes(&data, key)
        .unwrap_or_else(|e| fail(format!("Descifrado fallido: {e}")));
    let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.as_str());

    // Vista previa: con `--pixel-format` se re-aplica la conversión del atlas
    // (BGRA8888 y demás) para que la imagen se vea con los colores correctos.
    let preview = match val("pixel-format") {
        Some(pf) => tp_core::export::decode_texture_preview_png(&plain, parse_pixel_format(pf))
            .unwrap_or_else(|e| fail(format!("No se pudo generar la vista previa: {e}"))),
        None => plain,
    };

    std::fs::write(&out_path, &preview)
        .unwrap_or_else(|e| fail(format!("No se pudo escribir {}: {e}", out_path.display())));
    if flags.iter().any(|f| f == "quiet") {
        return;
    }
    match val("pixel-format") {
        Some(pf) => println!(
            "✔ Descifrado + vista previa ({}): {}",
            pf.to_ascii_uppercase(),
            out_path.display()
        ),
        None => println!("✔ Descifrado: {}", out_path.display()),
    }
}

/// Reglas de exportación: rangos de calidades y que el pixel format elegido
/// sea soportado por el formato de textura. Devuelve el mensaje de error.
fn check_export_flags(cfg: &ProjectConfig) -> Result<(), String> {
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
fn parse_pixel_format(v: &str) -> PixelFormat {
    let norm: String = v
        .to_ascii_lowercase()
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .collect();
    match norm.as_str() {
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
        _ => fail(format!("--pixel-format inválido: {v}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn pixel_format_flag_is_shared_between_commands() {
        assert!(matches!(
            parse_pixel_format("BGRA8888"),
            PixelFormat::Bgra8888
        ));
        assert!(matches!(
            parse_pixel_format("rgba5555"),
            PixelFormat::Rgba5555
        ));
        // Variantes con alias.
        assert!(matches!(parse_pixel_format("5551"), PixelFormat::Rgba5551));
        assert!(matches!(
            parse_pixel_format("Alpha-Intensity8"),
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
            parse_pixel_format("PVRTCI_2BPP_RGBA"),
            PixelFormat::Pvrtc2BppRgba
        ));
        assert!(matches!(
            parse_pixel_format("pvrtc-4bpp-rgb"),
            PixelFormat::Pvrtc4BppRgb
        ));
        assert!(matches!(parse_pixel_format("etc1"), PixelFormat::Etc1Rgb));
        assert!(matches!(
            parse_pixel_format("ETC2_RGB"),
            PixelFormat::Etc2Rgb
        ));
        assert!(matches!(parse_pixel_format("dxt5"), PixelFormat::Dxt5));
        assert!(matches!(
            parse_pixel_format("astc-12x12"),
            PixelFormat::Astc12x12
        ));
        assert!(matches!(
            parse_pixel_format("rgba4444"),
            PixelFormat::Rgba4444
        ));
        assert!(matches!(parse_pixel_format("rgb565"), PixelFormat::Rgb565));
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
        apply_variant_flags(&values, &mut cfg);
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
        apply_variant_flags(&values, &mut cfg);
        assert_eq!(cfg.variant_options_for(0.5).unwrap().sprite_filter, "a,b");
    }

    #[test]
    fn decrypt_writes_preview_png_with_bgra_restored() {
        use image::ImageEncoder;

        // Atlas "publicado" con BGRA8888 (R y B intercambiados) y cifrado.
        let bgra = vec![255u8, 0, 0, 255, 0, 0, 255, 255]; // azul, rojo
        let mut atlas = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(
            &mut atlas,
            image::codecs::png::CompressionType::Default,
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(&bgra, 2, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
        let enc = tp_core::export::encrypt_bytes(&atlas, "clave").unwrap();
        let f = std::env::temp_dir().join("tpcli_decrypt_preview.tpenc");
        let o = std::env::temp_dir().join("tpcli_decrypt_preview_out.png");
        std::fs::write(&f, &enc).unwrap();

        cmd_decrypt(&args(&[
            f.to_str().unwrap(),
            "--key",
            "clave",
            "-o",
            o.to_str().unwrap(),
            "--pixel-format",
            "bgra8888",
        ]));
        let out = std::fs::read(&o).unwrap();
        let img = image::load_from_memory(&out).unwrap().to_rgba8();
        // Con la conversión, R/B vuelven al orden natural RGBA.
        assert_eq!(img.as_raw(), &[0, 0, 255, 255, 255, 0, 0, 255]);
        let _ = std::fs::remove_file(&f);
        let _ = std::fs::remove_file(f.with_extension("dec.png"));
        let _ = std::fs::remove_file(&o);
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

        // El hash del original se pide como `json-hash` (json = array).
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

    /// Todas las `--opciones` que aparecen en el texto de la ayuda.
    fn help_options(text: &str) -> Vec<String> {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'-' && bytes[i + 1] == b'-' {
                let start = i + 2;
                let mut j = start;
                // Mayúsculas también: `--orx-keyDuration` lleva las suyas.
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-') {
                    j += 1;
                }
                if j > start {
                    out.push(text[start..j].to_string());
                }
                i = j;
            } else {
                i += 1;
            }
        }
        out
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
        // El caso del original: atlas-{n}.png junto a atlas.json.
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
            Positional::Unsupported
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
        // los sprites vayan mezclados con las opciones como en el original.
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
    fn exporter_list_and_version_are_stable() {
        let list = exporter_list_text();
        assert!(list.contains("phaser"), "{list}");
        assert!(list.contains("libgdx"), "{list}");
        assert!(list.ends_with('\n'));
        assert!(!list.lines().any(|line| line == "png"));
        assert!(version_line().starts_with("TexturePacker-RS "));
    }

    #[test]
    fn every_option_in_the_help_is_known() {
        // La ayuda prometió cosas que el parser no leía (--no-auto-animations
        // era una de ellas): ya no puede volver a pasar.
        for opt in help_options(&help_text()) {
            // Comparación exacta: la ayuda tiene que citar la grafía del
            // registro, que es la que el parser distingue (`--orx-keyDuration`).
            let known = PACK_VALUES.contains(&opt.as_str()) || PACK_FLAGS.contains(&opt.as_str());
            assert!(known, "la ayuda documenta --{opt} y el parser no la conoce");
        }
    }

    #[test]
    fn help_documents_the_packing_options() {
        let help = help_text();
        for opt in [
            "input",
            "output",
            "sheet",
            "data",
            "format",
            "texture-format",
            "trim-mode",
            "height",
            "webp-quality",
            "tolerance",
            "template",
            "no-trim",
            "no-auto-animations",
            "version",
            "quiet",
            "verbose",
            "exporter-list",
            "common-divisor-x",
            "default-pivot-point",
            "force-identical-layout",
            "trim-sprite-names",
            "enable-rotation",
            "pack-normalmaps",
            "max-width",
            "max-height",
            "background-color",
            "dpi",
            "ignore-files",
            "replace",
            "scale",
            "save",
            "heuristic-mask",
            "force-publish",
            "custom-exporters-directory",
            "convert-texture",
            "css-sprite-prefix",
            "css-media-query-2x",
            "plain-string-property",
            "plain-bool-property",
            "disable-rotation",
            "enable-cache-busting",
        ] {
            assert!(
                help.contains(&format!("--{opt}")),
                "falta --{opt} en la ayuda"
            );
        }
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
    fn scale_flag_multiplies_every_variant() {
        // Sin variantes declaradas: una única escala y sufijo vacío, así
        // que los nombres de los ficheros no cambian.
        let mut cfg = ProjectConfig::default();
        apply_scale_factor(&mut cfg, 0.5);
        assert_eq!(cfg.scale_variants, vec![0.5]);
        assert_eq!(cfg.variant_names, vec![(0.5, String::new())]);
        cfg.validate().unwrap();

        // Con variantes y nombres: las escalas se multiplican y los
        // nombres siguen pegados a su variante.
        let mut cfg = ProjectConfig {
            scale_variants: vec![1.0, 0.5],
            variant_names: vec![(1.0, "-ipadhd".to_string()), (0.5, "-hd".to_string())],
            ..ProjectConfig::default()
        };
        apply_scale_factor(&mut cfg, 2.0);
        assert_eq!(cfg.scale_variants, vec![2.0, 1.0]);
        assert_eq!(
            cfg.variant_names,
            vec![(2.0, "-ipadhd".to_string()), (1.0, "-hd".to_string())]
        );
        cfg.validate().unwrap();

        // El factor llega desde la línea de comandos: --scale sólo
        // multiplica, y la escala resultante sigue siendo válida.
        let out = std::env::temp_dir().join("tpcli_save_scale.tpproj");
        let _ = std::fs::remove_file(&out);
        cmd_pack(&args(&[
            "--input",
            "sprites",
            "--output",
            "build",
            "--variants",
            "1,0.5",
            "--scale",
            "0.5",
            "--save",
            out.to_str().unwrap(),
        ]));
        let cfg = ProjectConfig::from_toml(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(cfg.scale_variants, vec![0.5, 0.25]);
        cfg.validate().unwrap();
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn save_writes_a_project_and_skips_the_packing() {
        let out = std::env::temp_dir().join("tpcli_save_phase_c.tpproj");
        let _ = std::fs::remove_file(&out);
        cmd_pack(&args(&[
            "--input",
            "sprites",
            "--output",
            "build",
            "--max-width",
            "2048",
            "--background-color",
            "112233",
            "--ignore-files",
            "*.psd",
            "--save",
            out.to_str().unwrap(),
        ]));
        let text = std::fs::read_to_string(&out).unwrap();
        let cfg = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(cfg.max_width, 2048);
        assert_eq!(cfg.background_color, Some([0x11, 0x22, 0x33, 0xff]));
        assert_eq!(cfg.input_directory, PathBuf::from("sprites"));
        assert_eq!(cfg.output_directory, PathBuf::from("build"));
        assert_eq!(cfg.ignore_patterns, vec!["*.psd"]);
        let _ = std::fs::remove_file(&out);

        // La extensión distinta de .tpproj se guarda igual, con aviso.
        let odd = std::env::temp_dir().join("tpcli_save_phase_c.toml");
        let _ = std::fs::remove_file(&odd);
        cmd_pack(&args(&[
            "--input",
            "sprites",
            "--output",
            "build",
            "--save",
            odd.to_str().unwrap(),
        ]));
        assert!(odd.exists());
        let _ = std::fs::remove_file(&odd);
    }

    #[test]
    fn exporter_only_options_are_registered_and_rejected() {
        // Ninguna es «desconocida»: están registradas con la grafía del
        // original, y se rechazan diciendo de qué exportador hablan.
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

        // Con bools del original también se acepta la forma sin valor.
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
        assert!(apply_custom_exporter(&mut cfg, "miexportador"));
        assert_eq!(
            cfg.export_template.as_deref(),
            Some(dir.join("miexportador.hbs").as_path())
        );

        // Un id que no existe no vale, ni uno con ruta en la del directorio.
        let mut cfg = ProjectConfig {
            custom_exporters_directory: Some(dir.clone()),
            ..ProjectConfig::default()
        };
        assert!(!apply_custom_exporter(&mut cfg, "no-existe"));
        assert!(!apply_custom_exporter(&mut cfg, "../algo"));

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

        // Lo que --exporter-list lista junto a los del original.
        assert_eq!(
            custom_exporter_ids(&dir),
            vec!["miexportador".to_string(), "otro.json".to_string()]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn convert_texture_writes_the_requested_format() {
        use image::ImageEncoder;

        let src = std::env::temp_dir().join("tpcli_convert_src.png");
        let rgba = [255u8, 0, 0, 255, 0, 0, 255, 255];
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(
            &mut png,
            image::codecs::png::CompressionType::Default,
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(&rgba, 2, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
        std::fs::write(&src, &png).unwrap();

        // Mismo directorio que la entrada y otro formato.
        let cfg = ProjectConfig {
            gpu_format: GpuFormat::Jpg,
            ..ProjectConfig::default()
        };
        let msg = convert_texture(&cfg, &src).unwrap();
        assert!(msg.contains("Convertido"), "{msg}");
        let jpg = src.with_extension("jpg");
        let bytes = std::fs::read(&jpg).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "no es un JPEG");

        // Con --scale se amplía antes de codificar, y --output dirige dónde.
        let out_dir = std::env::temp_dir().join("tpcli_convert_out");
        let _ = std::fs::remove_dir_all(&out_dir);
        let cfg = ProjectConfig {
            gpu_format: GpuFormat::Png,
            scale_variants: vec![2.0],
            output_directory: out_dir.clone(),
            ..ProjectConfig::default()
        };
        convert_texture(&cfg, &src).unwrap();
        let scaled = out_dir.join("tpcli_convert_src.png");
        let img = image::load_from_memory(&std::fs::read(&scaled).unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (4, 2));

        // Un fichero que no existe se rechaza con el nombre de la opción.
        let err = convert_texture(&cfg, &std::env::temp_dir().join("no_esta.png")).unwrap_err();
        assert!(err.contains("--convert-texture"), "{err}");

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&jpg);
        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
