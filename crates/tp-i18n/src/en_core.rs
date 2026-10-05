//! Traducciones al inglés de los **mensajes del motor** (`tp-core`).
//!
//! `tp-core` devuelve siempre español (es la lengua del proyecto y también
//! la de su CLI), así que la tabla vive aquí, en el crate compartido: [`crate::tr`]
//! traduce en el momento de mostrar, ya sea la cadena entera o su patrón
//! con huecos `{}` cuando el mensaje ya venía formateado con argumentos.
//!
//! Las claves son la forma **en tiempo de ejecución** de los mensajes: los
//! argumentos de `format!` aparecen como `{}` y las llaves literales del
//! mensaje como `{{` / `}}`. Un test comprueba que inglés y español tienen
//! los mismos huecos, que no hay claves repetidas y que ninguna queda vacía.

use std::collections::HashMap;
use std::sync::OnceLock;

/// `(español del motor, inglés)`; el orden importa porque el primer patrón
/// con huecos que encaje en un mensaje es el que lo traduce.
pub static EN_CORE: &[(&str, &str)] = &[
    ("iPad + iPhone (documentación)", "iPad + iPhone (documentation)"),
    ("Descuentos 1/2, 1/3 y 1/4", "1/2, 1/3 and 1/4 discounts"),
    ("(hoja principal)", "(main sheet)"),
    (
        "max_texture_size debe ser una potencia de dos positiva (se obtuvo {})",
        "max_texture_size must be a positive power of two (got {})",
    ),
    (
        "padding, border_padding y extrude no pueden ser negativos",
        "padding, border_padding and extrude cannot be negative",
    ),
    (
        "border_padding ({}) deja el atlas interior vacío en un atlas de {}",
        "border_padding ({}) leaves the inner atlas empty in an atlas of {}",
    ),
    (
        "{} debe estar entre 0 (sin tope propio) y 16384 (se obtuvo {})",
        "{} must be between 0 (no cap of its own) and 16384 (got {})",
    ),
    (
        "{} debe ser una potencia de dos con size_constraints = POT (se obtuvo {})",
        "{} must be a power of two with size_constraints = POT (got {})",
    ),
    (
        "border_padding ({}) deja el atlas interior vacío con {} = {}",
        "border_padding ({}) leaves the inner atlas empty with {} = {}",
    ),
    ("dpi debe estar entre 1 y 1000000 (se obtuvo {})", "dpi must be between 1 and 1000000 (got {})"),
    (
        "name_replacements: la expresión regular «{}» no es válida: {}",
        "name_replacements: the regular expression «{}» is not valid: {}",
    ),
    ("{} debe estar entre 1 y 2048 (se obtuvo {})", "{} must be between 1 and 2048 (got {})"),
    (
        "align_to_grid debe estar entre 0 y 2048 (se obtuvo {})",
        "align_to_grid must be between 0 and 2048 (got {})",
    ),
    (
        "png_opt_level debe estar entre 0 y 7 (se obtuvo {})",
        "png_opt_level must be between 0 and 7 (got {})",
    ),
    (
        "jpg_quality debe estar entre 0 y 100 (se obtuvo {})",
        "jpg_quality must be between 0 and 100 (got {})",
    ),
    (
        "pvr_quality debe estar entre 0 y 7 (se obtuvo {})",
        "pvr_quality must be between 0 and 7 (got {})",
    ),
    ("{} debe estar entre 0 y 100 (se obtuvo {})", "{} must be between 0 and 100 (got {})"),
    (
        "astc_quality debe estar entre 0 y 4 (se obtuvo {})",
        "astc_quality must be between 0 and 4 (got {})",
    ),
    (
        "pixel_format {} no es compatible con el formato de textura {}",
        "pixel_format {} is not compatible with texture format {}",
    ),
    ("El common divisor supera 2048 ({}x{})", "The common divisor exceeds 2048 ({}x{})"),
    ("texture_path no puede ser una cadena vacía", "texture_path cannot be an empty string"),
    ("trim_threshold debe estar entre 1 y 255", "trim_threshold must be between 1 and 255"),
    ("trim_margin debe estar entre 0 y 256", "trim_margin must be between 0 and 256"),
    ("polygon_tolerance no puede ser negativa", "polygon_tolerance cannot be negative"),
    (
        "normal_map_sheet debe ser un nombre de fichero, no una ruta",
        "normal_map_sheet must be a file name, not a path",
    ),
    ("scale_variants no puede estar vacío", "scale_variants cannot be empty"),
    (
        "scale_variants debe estar en (0, 8] (se obtuvo {})",
        "scale_variants must be in (0, 8] (got {})",
    ),
    ("variant_names debe estar en (0, 8] (se obtuvo {})", "variant_names must be in (0, 8] (got {})"),
    ("variant_names no admite rutas (se obtuvo {})", "variant_names does not accept paths (got {})"),
    (
        "variant_options: la escala debe estar en (0, 8] (se obtuvo {})",
        "variant_options: the scale must be in (0, 8] (got {})",
    ),
    (
        "variant_options: la escala {} no está en scale_variants",
        "variant_options: scale {} is not in scale_variants",
    ),
    (
        "variant_options: la escala {} aparece más de una vez",
        "variant_options: scale {} appears more than once",
    ),
    (
        "variant_options: max_texture_size de la escala {} debe ser 0 (usar el del proyecto) o una potencia de dos hasta 16384 (se obtuvo {})",
        "variant_options: max_texture_size for scale {} must be 0 (use the project one) or a power of two up to 16384 (got {})",
    ),
    ("encryption_key no puede ser una cadena vacía", "encryption_key cannot be an empty string"),
    (
        "{} debe estar entre 0 (auto) y 8192 (se obtuvo {})",
        "{} must be between 0 (auto) and 8192 (got {})",
    ),
    (
        "border_padding ({}) deja el interior vacío en un atlas fijo de {}x{}",
        "border_padding ({}) leaves the interior empty in a fixed atlas of {}x{}",
    ),
    (
        "falta un campo obligatorio en el proyecto TOML: `{}`",
        "a required field is missing in the TOML project: `{}`",
    ),
    (
        "faltan {} campos obligatorios en el proyecto TOML: {}",
        "{} required fields are missing in the TOML project: {}",
    ),
    ("Genéricos", "Generic"),
    ("Atlas de texto", "Text atlas"),
    ("JSON de motores", "Engine JSON"),
    ("plist y XML", "plist and XML"),
    ("Extras del propio clon", "This clone's own extras"),
    ("XML genérico", "Generic XML"),
    ("Texto plano (exportador de ejemplo)", "Plain text (sample exporter)"),
    ("Solo la hoja, sin fichero de datos", "Sheet only, no data file"),
    ("Unity (JSON en .txt)", "Unity (JSON in .txt)"),
    ("Amethyst (ids en Rust con --spriteids-file)", "Amethyst (Rust ids with --spriteids-file)"),
    ("SpriteKit (plist + cabecera ObjC)", "SpriteKit (plist + ObjC header)"),
    ("Cabecera C++/ObjC", "C++/ObjC header"),
    ("Error de E/S: {}", "I/O error: {}"),
    ("Error de imagen: {}", "Image error: {}"),
    ("Error JSON: {}", "JSON error: {}"),
    ("Error TOML: {}", "TOML error: {}"),
    ("Error de plantilla: {}", "Template error: {}"),
    ("Error de cifrado: {}", "Encryption error: {}"),
    ("Configuración inválida: {}", "Invalid configuration: {}"),
    ("Error en cabecera PNG indexado: {}", "Error in indexed PNG header: {}"),
    ("Error en datos PNG indexado: {}", "Error in indexed PNG data: {}"),
    ("Dimensiones de imagen inválidas para PNG", "Invalid image dimensions for PNG"),
    ("Error codificando PNG: {}", "Error encoding PNG: {}"),
    ("Dimensiones de imagen inválidas para JPG", "Invalid image dimensions for JPG"),
    ("Formato de píxel no compatible con JPG", "Pixel format not supported by JPG"),
    ("Error codificando JPG: {}", "Error encoding JPG: {}"),
    ("Dimensiones de imagen inválidas para WebP", "Invalid image dimensions for WebP"),
    ("Error codificando WebP: {}", "Error encoding WebP: {}"),
    ("Config ASTC inválida: {}", "Invalid ASTC config: {}"),
    ("Contexto ASTC: {}", "ASTC context: {}"),
    ("Error comprimiendo ASTC: {}", "Error compressing ASTC: {}"),
    (
        "ASTC requiere compilar con la feature `gpu-formats` (cargo build --features gpu-formats)",
        "ASTC requires building with the `gpu-formats` feature (cargo build --features gpu-formats)",
    ),
    (
        "Basis requiere compilar con la feature `gpu-formats` (cargo build --features gpu-formats)",
        "Basis requires building with the `gpu-formats` feature (cargo build --features gpu-formats)",
    ),
    (
        "KTX2 no puede describir el pixel format {} (solo RGBA8, RGB8 y un canal de 8 bits)",
        "KTX2 cannot describe pixel format {} (only RGBA8, RGB8 and a single 8-bit channel)",
    ),
    ("{} no es un formato de imagen", "{} is not an image format"),
    ("Error codificando {}: {}", "Error encoding {}: {}"),
    (
        "DDS no admite el formato de píxel resultante ({})",
        "DDS does not support the resulting pixel format ({})",
    ),
    ("Tamaño de imagen inconsistente al codificar DDS", "Inconsistent image size while encoding DDS"),
    ("{} no es un formato DXT comprimible en DDS", "{} is not a DXT-compressible format in DDS"),
    ("Clave inválida: {}", "Invalid key: {}"),
    ("Error cifrando: {}", "Error encrypting: {}"),
    ("Archivo cifrado demasiado corto", "Encrypted file too short"),
    (
        "No es un archivo cifrado TexturePacker-RS (falta cabecera TPENC1 o TPENC2)",
        "Not a TexturePacker-RS encrypted file (TPENC1 or TPENC2 header missing)",
    ),
    ("Error descifrando (¿clave incorrecta?)", "Error decrypting (wrong key?)"),
    ("pivots.json: {}", "pivots.json: {}"),
    ("borders.json: {}", "borders.json: {}"),
    ("No se encontraron imágenes en {}", "No images found in {}"),
    (
        "{} imagen(es) clasificada(s) como mapa de normales por su color",
        "{} image(s) classified as normal maps by their color",
    ),
    (
        "{} sprite(s) totalmente transparentes omitidos (trim mode {})",
        "{} fully transparent sprite(s) skipped (trim mode {})",
    ),
    (
        "{} sprite(s) opacos pasaron por la máscara heurística (--heuristic-mask)",
        "{} opaque sprite(s) went through the heuristic mask (--heuristic-mask)",
    ),
    ("Mapa de normales sin difusa asociada: {}", "Normal map without an associated diffuse: {}"),
    ("No se pudo crear {}: {}", "Could not create {}: {}"),
    ("No se pudo serializar las claves: {}", "Could not serialize the keys: {}"),
    ("No se pudo escribir {}: {}", "Could not write {}: {}"),
    ("El nombre de la clave está vacío", "The key name is empty"),
    ("La clave a guardar está vacía; usa --key CLAVE", "The key to save is empty; use --key KEY"),
    (
        "No hay ninguna clave global llamada «{}» (guárdala en Ajustes o en la CLI con --key CLAVE --save-key {})",
        "There is no global key named «{}» (save it in Settings or from the CLI with --key KEY --save-key {})",
    ),
    ("No se pudo leer {}: {}", "Could not read {}: {}"),
    ("Proyecto inválido: {}", "Invalid project: {}"),
    (
        "border_padding ({}) deja el área interior vacía en un atlas de {}x{}",
        "border_padding ({}) leaves the inner area empty in an atlas of {}x{}",
    ),
    (
        "El sprite '{}' ({}x{}) no cabe en un atlas de {}x{} (padding {} + borde {})",
        "Sprite '{}' ({}x{}) does not fit in an atlas of {}x{} (padding {} + border {})",
    ),
    ("{} (página {})", "{} (page {})"),
    (
        "El sprite '{}' no cabe en un atlas de {}x{} (rejilla {}x{}, padding {} + borde {})",
        "Sprite '{}' does not fit in an atlas of {}x{} (grid {}x{}, padding {} + border {})",
    ),
    (
        "La rejilla de {} px no cabe en el área interior de {}x{} (celda {}x{}, borde {})",
        "The {} px grid does not fit in the inner area of {}x{} (cell {}x{}, border {})",
    ),
    (
        "El sprite '{}' no cabe alineado a la rejilla de {} px en un atlas de {}x{}",
        "Sprite '{}' does not fit aligned to the {} px grid in an atlas of {}x{}",
    ),
    (
        "Padding ajustado a {} px y borde a {} px para respetar la rejilla de {} px",
        "Padding adjusted to {} px and border to {} px to respect the {} px grid",
    ),
    (
        "Voltear verticalmente (flip Y) solo aplica a formatos de hardware (ASTC/ETC2/ETC1/PVRTC); se ignora con el formato actual",
        "Flipping vertically (flip Y) only applies to hardware formats (ASTC/ETC2/ETC1/PVRTC); it is ignored with the current format",
    ),
    (
        "El formato de píxel solo aplica a los formatos de software (PNG/PNG8/JPG/WebP/BMP/TGA/TIFF/DDS/ZKTX); los formatos de hardware comprimen RGBA y lo ignoran",
        "The pixel format only applies to software formats (PNG/PNG8/JPG/WebP/BMP/TGA/TIFF/DDS/ZKTX); hardware formats compress RGBA and ignore it",
    ),
    (
        "No se encontraron sprites válidos en el directorio de entrada",
        "No valid sprites found in the input directory",
    ),
    (
        "{} solo se aplica a la escala exacta {}x; las variantes {} se reescalarán con Smooth",
        "{} only applies to the exact scale {}x; the {} variants will be rescaled with Smooth",
    ),
    ("variante {}", "variant {}"),
    (
        "Los sprites no caben en un solo atlas de {}x{} (harían {} hojas); activa «Multipack» o aumenta el tamaño máximo",
        "The sprites do not fit in a single {}x{} atlas (they would make {} sheets); enable «Multipack» or raise the max size",
    ),
    (
        "Variante {}: la hoja escalada supera su tamaño máximo de {} px; con layout idéntico no se puede reducir (desactiva «layout idéntico» o sube el máximo).",
        "Variant {}: the scaled sheet exceeds its max size of {} px; with an identical layout it cannot be shrunk (turn off «identical layout» or raise the max).",
    ),
    (
        "Multipack: {} hojas generadas y el nombre base \"{}\" no contiene {{n}} o {{n1}}; se nombran con el sufijo _N (p. ej. {}_1). Añade {{n1}} al nombre base para nombrar cada hoja (p. ej. {}{{n1}}).",
        "Multipack: {} sheets generated and the base name \"{}\" contains neither {{n}} nor {{n1}}; they are named with the _N suffix (e.g. {}_1). Add {{n1}} to the base name to name every sheet (e.g. {}{{n1}}).",
    ),
    (
        "Variante {}: el filtro de sprites excluye los {} sprites; se omite.",
        "Variant {}: the sprite filter excludes the {} sprites; it is skipped.",
    ),
    (
        "Variante {}: hay posiciones manuales, así que se reutiliza la hoja base y ni el filtro ni el tamaño máximo se aplican.",
        "Variant {}: there are manual positions, so the base sheet is reused and neither the filter nor the max size applies.",
    ),
    (
        "Variante {}: «aceptar valores fraccionarios» la deja fuera del común divisor, así que su hoja idéntica se redondea al píxel.",
        "Variant {}: «accept fractional values» keeps it out of the common divisor, so its identical sheet rounds to the pixel.",
    ),
    (
        "Variante {}: su escala {} no encaja en el común divisor {} y su hoja idéntica se redondea al píxel (activa «aceptar valores fraccionarios» si quieres que se asuma).",
        "Variant {}: its scale {} does not fit the common divisor {} and its identical sheet rounds to the pixel (turn on «accept fractional values» if you want it assumed).",
    ),
    (
        "PVRTC requiere dimensiones potencia de dos ≥ 8x8 (se obtuvo {}x{})",
        "PVRTC requires power-of-two dimensions ≥ 8x8 (got {}x{})",
    ),
    ("PVRTC: buffer RGBA de tamaño incorrecto", "PVRTC: wrong-sized RGBA buffer"),
    ("No se ha seleccionado ninguna hoja.", "No sheet has been selected."),
    ("La rejilla no produce ninguna celda válida.", "The grid does not produce any valid cell."),
    ("No se pudo leer la plantilla {}: {}", "Could not read template {}: {}"),
    ("XBM con ancho o alto 0", "XBM with width or height 0"),
    ("XBM sin el array de bits", "XBM without the bits array"),
    ("XBM sin el cierre del array de bits", "XBM without the closing of the bits array"),
    ("XBM byte inválido «{}»", "XBM invalid byte «{}»"),
    (
        "XBM con {}x{} necesita {} bytes y solo hay {}",
        "XBM with {}x{} needs {} bytes but there are only {}",
    ),
    ("XBM sin valor para «{}»", "XBM without a value for «{}»"),
    ("XBM: valor inválido «{}» en {}", "XBM: invalid value «{}» in {}"),
    ("XBM sin «#define ..._{}»", "XBM without «#define ..._{}»"),
    ("XPM: color hexagonal inválido «{}»", "XPM: invalid hexadecimal color «{}»"),
    ("XPM: color nombrado no soportado «{}»", "XPM: named color not supported «{}»"),
    ("PSD: {}", "PSD: {}"),
    ("PSD con ancho o alto 0", "PSD with width or height 0"),
    ("PSD: se esperaban {} bytes y hay {}", "PSD: expected {} bytes but there are {}"),
    ("SVG: {}", "SVG: {}"),
    ("SVG sin tamaño", "SVG without a size"),
    ("SVG: no se pudo crear el lienzo", "SVG: could not create the canvas"),
    ("ASTC: cabecera inválida", "ASTC: invalid header"),
    ("ASTC con ancho o alto 0", "ASTC with width or height 0"),
    ("ASTC: {}", "ASTC: {}"),
    ("KTX1: cabecera inválida", "KTX1: invalid header"),
    (
        "KTX1: solo se admiten ficheros en little-endian",
        "KTX1: only little-endian files are supported",
    ),
    ("KTX1 con ancho o alto 0", "KTX1 with width or height 0"),
    ("KTX1: no se admiten cubemaps", "KTX1: cubemaps are not supported"),
    ("KTX1: sin niveles de mipmap", "KTX1: no mipmap levels"),
    (
        "KTX1: tipo de dato no soportado 0x{} (solo UNSIGNED_BYTE)",
        "KTX1: unsupported data type 0x{} (only UNSIGNED_BYTE)",
    ),
    ("KTX1: formato crudo no soportado 0x{}", "KTX1: unsupported raw format 0x{}"),
    ("KTX1: payload más corto que el lienzo", "KTX1: payload shorter than the canvas"),
    ("KTX1 (internal 0x{}): {}", "KTX1 (internal 0x{}): {}"),
    ("KTX1: formato interno no soportado 0x{}", "KTX1: unsupported internal format 0x{}"),
    ("KTX1: bloque ASTC no soportado 0x{}", "KTX1: unsupported ASTC block 0x{}"),
    ("KTX2: cabecera inválida", "KTX2: invalid header"),
    ("KTX2 con ancho o alto 0", "KTX2 with width or height 0"),
    (
        "KTX2: payload con Basis Universal (se necesita un transcoder); exporta el sprite sin comprimir o en KTX v1",
        "KTX2: payload with Basis Universal (a transcoder is needed); export the sprite uncompressed or as KTX v1",
    ),
    ("KTX2: supercompresión no soportada ({})", "KTX2: unsupported supercompression ({})"),
    ("KTX2: índice de niveles truncado", "KTX2: truncated level index"),
    (
        "KTX2: vkFormat = 0 (formato descrito por el DFD, p. ej. Basis); no se puede leer aquí",
        "KTX2: vkFormat = 0 (format described by the DFD, e.g. Basis); it cannot be read here",
    ),
    (
        "KTX2: vkFormat no soportado ({}); solo se leen RGBA8/BGRA8/RGB8/R8",
        "KTX2: unsupported vkFormat ({}); only RGBA8/BGRA8/RGB8/R8 can be read",
    ),
    ("{}: payload más corto que el lienzo", "{}: payload shorter than the canvas"),
    ("KTX2: zlib: {}", "KTX2: zlib: {}"),
    ("Basis: firma inválida (no es un .basis)", "Basis: invalid signature (not a .basis)"),
    ("Basis: {}", "Basis: {}"),
    ("Basis: transcodificación con tamaño incoherente", "Basis: transcode with inconsistent size"),
    ("Basis requiere la feature \"gpu-formats\"", "Basis requires the \"gpu-formats\" feature"),
    ("cabecera truncada", "truncated header"),
    ("{}: gzip: {}", "{}: gzip: {}"),
    ("PKM: cabecera inválida", "PKM: invalid header"),
    ("PKM: versión desconocida {}", "PKM: unknown version {}"),
    (
        "PKM: dataFormat {} no soportado (sólo ETC1 RGB, 0)",
        "PKM: unsupported dataFormat {} (only ETC1 RGB, 0)",
    ),
    (
        "PKM: dimensiones incoherentes {}x{} dentro de {}x{}",
        "PKM: inconsistent dimensions {}x{} inside {}x{}",
    ),
    ("PKM: ETC1: {}", "PKM: ETC1: {}"),
    (
        "PVR: cabecera desconocida (ni «PVR\\x03» v3 ni «PVR!» v2)",
        "PVR: unknown header (neither «PVR\\x03» v3 nor «PVR!» v2)",
    ),
    ("PVR v3: cabecera truncada", "PVR v3: truncated header"),
    (
        "PVR v3: pixelFormat {} no soportado (sólo PVRTC1 0-3)",
        "PVR v3: unsupported pixelFormat {} (only PVRTC1 0-3)",
    ),
    ("PVR v3: {}: {}", "PVR v3: {}: {}"),
    ("PVR v2: headerSize {} no soportado (44 o 52)", "PVR v2: unsupported headerSize {} (44 or 52)"),
    ("PVR v2: falta el tag «PVR!»", "PVR v2: the «PVR!» tag is missing"),
    (
        "PVR v2: bpp {} no soportado (2/4 = PVRTC1, 16/32 = píxeles crudos)",
        "PVR v2: unsupported bpp {} (2/4 = PVRTC1, 16/32 = raw pixels)",
    ),
    ("PVR v2: {}", "PVR v2: {}"),
    ("{}: lienzo de tamaño nulo", "{}: zero-sized canvas"),
    ("{}x{} no es múltiplo del bloque PVRTC", "{}x{} is not a multiple of the PVRTC block"),
    ("pvr.ccz: cabecera inválida (falta «CCZ!»)", "pvr.ccz: invalid header («CCZ!» missing)"),
    (
        "pvr.ccz: sólo se lee compression_type = zlib (0)",
        "pvr.ccz: only compression_type = zlib (0) can be read",
    ),
    (
        "pvr.ccz: la cabecera declara {} bytes y salieron {}",
        "pvr.ccz: the header declares {} bytes but got {}",
    ),
    ("Ninguna", "None"),
    ("data_format desconocido: {}", "unknown data_format: {}"),
    ("Motores minoritarios", "Niche engines"),
    ("KTX2: lienzo 0×0", "KTX2: 0×0 canvas"),
    ("ASTC demasiado corto", "ASTC too short"),
    ("KTX1: payload truncado", "KTX1: truncated payload"),
    ("KTX2: payload truncado", "KTX2: truncated payload"),
    ("CSS simple", "Simple CSS"),
    ("Cocos2D v2 (plist, obsoleto)", "Cocos2D v2 (plist, deprecated)"),
    ("SpriteKit (plist + clase Swift)", "SpriteKit (plist + Swift class)"),
];

/// Índice de [`EN_CORE`] para las búsquedas exactas.
fn map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| EN_CORE.iter().copied().collect())
}

/// Traducción exacta de un mensaje (o etiqueta) del motor, si la hay.
pub fn lookup(es: &str) -> Option<&'static str> {
    map().get(es).copied()
}
