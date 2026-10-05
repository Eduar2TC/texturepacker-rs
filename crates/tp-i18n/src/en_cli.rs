//! Traducciones al inglés de los mensajes de la CLI (`tp-cli`).
//!
//! El CLI construye sus mensajes en español —así los tests unitarios del
//! parser los ven siempre igual, venga el locale que venga— y sólo los
//! traduce al imprimirlos, con [`crate::tr`]. Las claves son por tanto la
//! forma **en tiempo de ejecución** del mensaje, ya formateado: los
//! argumentos aparecen como `{}` en el orden en que se imprimen, y las
//! llaves literales del texto como `{{` / `}}`.
//!
//! Aquí no entran ni la ayuda (`help_text`, que por ahora sólo existe en
//! español) ni los mensajes del motor: éstos viven en `en_core`.
//!
//! Un test comprueba que las claves no estén vacías ni repetidas y que
//! inglés y español tengan los mismos huecos, que es lo que evita que una
//! traducción se coma un argumento.

use std::collections::HashMap;
use std::sync::OnceLock;

/// `(español de la CLI, inglés)`; el orden importa porque el primer patrón
/// con huecos que encaje en un mensaje es el que lo traduce.
pub static EN_CLI: &[(&str, &str)] = &[
    // Errores de uso y de proceso (main.rs).
    ("tp-cli: falta un comando (usa tp-cli --help para ver la ayuda)",
     "tp-cli: missing command (run tp-cli --help to see the usage)"),
    ("Comando desconocido: {}", "Unknown command: {}"),
    ("aviso: {}", "warning: {}"),
    ("Falta --input DIR (o pasa los sprites en posiciónles o un .tpproj/.tps)",
     "Missing --input DIR (or pass the sprites as positionals, or a .tpproj/.tps)"),
    ("Falta --output DIR, --sheet/--data (o un .tpproj/.tps)",
     "Missing --output DIR, --sheet/--data (or a .tpproj/.tps)"),
    ("sólo se admite un proyecto en la línea de comandos",
     "only one project is accepted on the command line"),
    ("--sheet {} y --data {} deben compartir el nombre base (p. ej. atlas.png con atlas.json, o atlas-{{n}}.png con atlas.json)",
     "--sheet {} and --data {} must share the base name (e.g. atlas.png with atlas.json, or atlas-{{n}}.png with atlas.json)"),
    ("{} {}: no se puede derivar el nombre base", "{} {}: the base name cannot be derived"),
    ("no existe: {}", "does not exist: {}"),
    ("no es una imagen: {}", "is not an image: {}"),
    ("la hoja se pidió como «{}» pero el nombre base lo fija --data: se numerará con el sufijo implícito _N (p. ej. {}.png, {}_1.png)",
     "the sheet was requested as \"{}\" but --data sets the base name: it will get the implicit _N suffix (e.g. {}.png, {}_1.png)"),
    // Sin traducción, a propósito: el mensaje de `check_exporter_only_options`
    // reparte artículos y sustantivos por los huecos («no presentar **la**
    // como **opción desconocida**», con singular y plural dentro), y en
    // inglés van en otro orden; con la misma plantilla saldría una frase
    // rota. Al no estar en la tabla `tr` lo devuelve entero en español, que
    // es preferible a un error a medias en dos idiomas.
    ("No se pudo crear {}: {}", "Could not create {}: {}"),
    ("No se pudo serializar el proyecto: {}", "Could not serialize the project: {}"),
    ("No se pudo guardar la clave: {}", "Could not save the key: {}"),
    ("✔ Proyecto guardado en {}", "✔ Project saved to {}"),
    (" (aviso: la extensión no es .tpproj)", " (warning: the extension is not .tpproj)"),
    ("Proyecto inválido {}: {}", "Invalid project {}: {}"),
    ("--convert-texture: no existe {}", "--convert-texture: {} does not exist"),
    ("--convert-texture: {}", "--convert-texture: {}"),
    ("Exportadores propios de {}:", "Custom exporters in {}:"),
    ("⚠ --custom-exporters-directory: no hay <id>.hbs en {}",
     "⚠ --custom-exporters-directory: no <id>.hbs in {}"),
    ("--data {} y --sheet {} deben estar en la misma carpeta",
     "--data {} and --sheet {} must be in the same folder"),
    ("· entrada: {} · salida: {} · nombre base: {}",
     "· input: {} · output: {} · base name: {}"),
    ("· textura: {} · píxeles: {} · datos: {} · hasta {} px · padding {} + {}",
     "· texture: {} · pixels: {} · data: {} · up to {} px · padding {} + {}"),
    ("✔ Empaquetado en {} ms: {} sprite(s) ({} alias(es)), {} página(s)",
     "✔ Packed in {} ms: {} sprite(s) ({} alias(es)), {} sheet(s)"),
    // decrypt (commands.rs).
    ("decrypt necesita un archivo .tpenc", "decrypt needs a .tpenc file"),
    ("decrypt necesita --key CLAVE", "decrypt needs --key PASSPHRASE"),
    ("No se pudo leer {}: {}", "Could not read {}: {}"),
    ("Descifrado fallido: {}", "Decryption failed: {}"),
    ("No se pudo generar la vista previa: {}", "Could not generate the preview: {}"),
    ("No se pudo escribir {}: {}", "Could not write {}: {}"),
    ("✔ Descifrado + vista previa ({}): {}", "✔ Decrypted + preview ({}): {}"),
    ("✔ Descifrado: {}", "✔ Decrypted: {}"),
    // Parseo de opciones (args.rs).
    ("opción desconocida: {}", "unknown option: {}"),
    ("opción desconocida", "unknown option"),
    ("falta el valor de {}", "missing value for {}"),
    ("{} (usa --help para ver las opciones)", "{} (run --help to see the options)"),
    ("--variant inválido: {}", "invalid --variant: {}"),
    ("--variant inválido (escala fuera de rango): {}",
     "invalid --variant (scale out of range): {}"),
    ("--variant {} inválido: {}", "invalid --variant {}: {}"),
    ("--variant: cuarto campo desconocido (use allowfraction): {}",
     "--variant: unknown fourth field (use allowfraction): {}"),
    ("--variant: el tamaño máximo debe ser un cuadrado > 0: {}",
     "--variant: max size must be a square > 0: {}"),
    ("--variant: faltan el ancho o el alto del tamaño máximo: {}",
     "--variant: max size is missing width or height: {}"),
    ("--sheet {}: falta la extensión (p. ej. atlas.png)",
     "--sheet {}: missing the extension (e.g. atlas.png)"),
    ("--sheet {}: extensión desconocida «{}»; elige el formato con --texture-format",
     "--sheet {}: unknown extension \"{}\"; pick the format with --texture-format"),
    ("--data {}: falta la extensión .{} del formato de datos",
     "--data {}: missing the .{} extension of the data format"),
    ("--data {}: la extensión «{}» no corresponde al formato de datos «{}»; {}",
     "--data {}: extension \"{}\" does not match data format \"{}\"; {}"),
    ("--texture-format inválido: {} (los formatos de datos se piden con --format)",
     "invalid --texture-format: {} (data formats are asked with --format)"),
    ("--default-pivot-point inválido: {} (usa X,Y con valores de 0 a 1)",
     "invalid --default-pivot-point: {} (use X,Y with values from 0 to 1)"),
    ("--background-color inválido: {} (usa RRGGBB o RRGGBBAA en hexadecimal)",
     "invalid --background-color: {} (use hexadecimal RRGGBB or RRGGBBAA)"),
    ("--replace inválido: {} (usa PATRÓN=TEXTO)", "invalid --replace: {} (use PATTERN=TEXT)"),
    ("--replace inválido: {} (falta el patrón)", "invalid --replace: {} (missing the pattern)"),
    ("--replace: la expresión regular «{}» no es válida: {}",
     "--replace: regular expression \"{}\" is not valid: {}"),
    ("--dpi inválido: {} (entero en 1..1000000)", "invalid --dpi: {} (integer in 1..1000000)"),
    ("--dpi debe estar entre 1 y 1000000: {}", "--dpi must be between 1 and 1000000: {}"),
    ("--pixel-format inválido: {}", "invalid --pixel-format: {}"),
    ("pixel format {} no es soportado por --format {}",
     "pixel format {} is not supported by --format {}"),
    ("--pvr-quality fuera de rango (0-7): {}", "--pvr-quality out of range (0-7): {}"),
    ("--etc1-quality fuera de rango (0-100): {}", "--etc1-quality out of range (0-100): {}"),
    ("--etc2-quality fuera de rango (0-100): {}", "--etc2-quality out of range (0-100): {}"),
    ("--astc-quality fuera de rango (0-4): {}", "--astc-quality out of range (0-4): {}"),
    ("--basis-quality fuera de rango (0-100): {}", "--basis-quality out of range (0-100): {}"),
    // Validación de opciones de empaquetado (pack.rs).
    ("--max-size inválido", "invalid --max-size"),
    ("--shape-padding inválido", "invalid --shape-padding"),
    ("--extrude inválido", "invalid --extrude"),
    ("--border-padding inválido", "invalid --border-padding"),
    ("--common-divisor inválido", "invalid --common-divisor"),
    ("--align-to-grid inválido", "invalid --align-to-grid"),
    ("--alpha-handling inválido: {}", "invalid --alpha-handling: {}"),
    ("--scale-mode inválido: {}", "invalid --scale-mode: {}"),
    ("--trim-threshold inválido", "invalid --trim-threshold"),
    ("--trim-mode inválido: {}", "invalid --trim-mode: {}"),
    ("--trim-margin inválido", "invalid --trim-margin"),
    ("--tolerance inválido", "invalid --tolerance"),
    ("--color-depth inválido: {}", "invalid --color-depth: {}"),
    ("--dither inválido: {} (usa none | NearestNeighbour | Linear | FloydSteinberg | FloydSteinbergAlpha | Atkinson | AtkinsonAlpha; alias nn, floyd, atkinson…)",
     "invalid --dither: {} (use none | NearestNeighbour | Linear | FloydSteinberg | FloydSteinbergAlpha | Atkinson | AtkinsonAlpha; aliases nn, floyd, atkinson…)"),
    ("--format inválido: {} (formato de textura: png | jpg | webp | ktx | astc | …; formato de datos: phaser | libgdx | cocos2d | sparrow | json-hash | …)",
     "invalid --format: {} (texture formats: png | jpg | webp | ktx | astc | …; data formats: phaser | libgdx | cocos2d | sparrow | json-hash | …)"),
    ("--template-format inválido: {} (familias: json | xml | plist | cpp | tsv | text; exportadores: json-hash, libgdx, cocos2d, sparrow, spine, phaser, pixijs4, egret…; o un <id>.hbs de --custom-exporters-directory)",
     "invalid --template-format: {} (families: json | xml | plist | cpp | tsv | text; exporters: json-hash, libgdx, cocos2d, sparrow, spine, phaser, pixijs4, egret…; or an <id>.hbs from --custom-exporters-directory)"),
    ("--png-opt-level inválido", "invalid --png-opt-level"),
    ("--png8-dither inválido: {}", "invalid --png8-dither: {}"),
    ("--jpg-quality inválido", "invalid --jpg-quality"),
    ("--webp-quality inválido", "invalid --webp-quality"),
    ("--pvr-quality inválido", "invalid --pvr-quality"),
    ("--etc1-quality inválido", "invalid --etc1-quality"),
    ("--etc2-quality inválido", "invalid --etc2-quality"),
    ("--astc-quality inválido", "invalid --astc-quality"),
    ("--basis-quality inválido", "invalid --basis-quality"),
    ("--dxt-mode inválido: {} (DXT_LINEAR | DXT_PERCEPTUAL)",
     "invalid --dxt-mode: {} (DXT_LINEAR | DXT_PERCEPTUAL)"),
    ("--strategy inválido: {}", "invalid --strategy: {}"),
    ("--algorithm inválido: {}", "invalid --algorithm: {}"),
    ("--basic-sort-by inválido: {}", "invalid --basic-sort-by: {}"),
    ("--basic-order inválido: {}", "invalid --basic-order: {}"),
    ("--pack-mode inválido: {}", "invalid --pack-mode: {}"),
    ("--size-constraints inválido: {}", "invalid --size-constraints: {}"),
    ("--width inválido", "invalid --width"),
    ("--height inválido", "invalid --height"),
    ("--scale inválido: {} (número en (0, 8])", "invalid --scale: {} (number in (0, 8])"),
    ("--gdx-filter inválido: {} (linear | nearest)",
     "invalid --gdx-filter: {} (linear | nearest)"),
    // Al final, a propósito: `tr` traduce con el primer patrón que encaje,
    // así que las plantillas genéricas tienen que ir detrás de las que
    // llevan su propio texto (`--scale inválido: {} (número en (0, 8])`),
    // o la abarcan entera y dejan el resto del mensaje sin traducir.
    ("{} inválido: {} (true | false)", "invalid {}: {} (true | false)"),
    ("{} inválido: {} (entero en 0..16384)", "invalid {}: {} (integer in 0..16384)"),
    ("{} debe ser 1 o más: {}", "{} must be 1 or more: {}"),
    ("{} debe estar entre 0 y 16384: {}", "{} must be between 0 and 16384: {}"),
    ("{} inválido: {}", "invalid {}: {}"),
];

fn map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| EN_CLI.iter().copied().collect())
}

/// Traducción exacta de un mensaje de la CLI, si la hay.
pub fn lookup(es: &str) -> Option<&'static str> {
    map().get(es).copied()
}
