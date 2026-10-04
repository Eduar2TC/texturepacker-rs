//! Lectura de formatos de entrada que `image` no cubre: XBM, XPM, PSD y SVG,
//! más los contenedores GPU (`.astc`, `.ktx`, `.ktx2`, `.pkm`, `.pvr`,
//! `.pvr.gz`, `.pvr.ccz`, `.pvrtc`, `.svgz`) y `.basis`.
//!
//! El resto de formatos (PNG/JPG/WebP/… y ahora también PBM/PGM/PPM) sigue
//! resolviéndose con la biblioteca `image`. Aquí solo viven los decodificadores
//! que hay que escribir a mano y el despacho por extensión.

use crate::error::{Result, TpError};
use std::path::Path;

/// Internal results carry plain messages; the public entry points turn them
/// into [`TpError`] together with the offending path.
type DecodeResult<T> = std::result::Result<T, String>;

/// Reads any supported input file into `(width, height, RGBA8)`.
pub fn load_image_rgba(path: &Path) -> Result<(i32, i32, Vec<u8>)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    // `.pvr.gz` se reconoce por el nombre base, porque la extensión es `gz`.
    if ext.as_deref() == Some("gz") && is_pvr_gz(path) {
        let bytes =
            std::fs::read(path).map_err(|e| TpError::Other(format!("{}: {e}", path.display())))?;
        return decode_pvr_gz(&bytes)
            .map_err(|e| TpError::Other(format!("{}: {e}", path.display())));
    }
    match ext.as_deref() {
        Some("xbm") | Some("xpm") | Some("astc") | Some("ktx") | Some("ktx2") | Some("psd")
        | Some("svg") | Some("svgz") | Some("basis") | Some("pkm") | Some("pvr")
        | Some("pvrtc") | Some("ccz") => {
            let bytes = std::fs::read(path)
                .map_err(|e| TpError::Other(format!("{}: {e}", path.display())))?;
            let decoded = match ext.as_deref() {
                Some("xbm") => decode_xbm(&bytes),
                Some("xpm") => decode_xpm(&bytes),
                Some("astc") => decode_astc(&bytes),
                Some("psd") => decode_psd(&bytes),
                Some("svg") => decode_svg(&bytes),
                Some("svgz") => decode_svgz(&bytes),
                Some("basis") => decode_basis(&bytes),
                Some("pkm") => decode_pkm(&bytes),
                Some("pvr") | Some("pvrtc") => decode_pvr(&bytes),
                Some("ccz") => decode_pvr_ccz(&bytes),
                // KTX v1 y v2 comparten identificador: el byte 12 es la
                // endianness (`01 02 03 04`) en v1 y el vkFormat en v2.
                _ if bytes.get(12..16) == Some(&[1, 2, 3, 4]) => decode_ktx(&bytes),
                _ => decode_ktx2(&bytes),
            };
            decoded.map_err(|e| TpError::Other(format!("{}: {e}", path.display())))
        }
        _ => {
            let img = image::open(path)
                .map_err(|e| TpError::Other(format!("{}: {e}", path.display())))?;
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            Ok((w as i32, h as i32, rgba.into_raw()))
        }
    }
}

/// `true` for `something.pvr.gz`: the only `.gz` accepted as input.
fn is_pvr_gz(path: &Path) -> bool {
    path.file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.to_ascii_lowercase().ends_with(".pvr"))
}

/// `texture2ddecoder` packs pixels as `0xAARRGGBB`; the rest of the pipeline
/// wants RGBA bytes.
fn from_u32_pixels(pixels: &[u32], width: i32, height: i32) -> (i32, i32, Vec<u8>) {
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for px in pixels {
        let [b, g, r, a] = px.to_le_bytes();
        out.extend_from_slice(&[r, g, b, a]);
    }
    (width, height, out)
}

// ---------------------------------------------------------------------------
// XBM (X bitmap, cabecera de C de 1 bit)
// ---------------------------------------------------------------------------

/// Reads a `.xbm` file: `#define <n>_width W`, `#define <n>_height H` and a
/// byte array where every bit is one pixel (LSB first, rows padded to a
/// byte). Set bits are opaque black on a transparent background, the usual
/// interpretation of an icon mask.
fn decode_xbm(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    let text = String::from_utf8_lossy(bytes);
    let width = xbm_define(&text, "width")?;
    let height = xbm_define(&text, "height")?;
    if width == 0 || height == 0 {
        return Err("XBM con ancho o alto 0".to_string());
    }
    let open = text
        .find('{')
        .ok_or_else(|| "XBM sin el array de bits".to_string())?;
    let rest = &text[open + 1..];
    let close = rest
        .find('}')
        .ok_or_else(|| "XBM sin el cierre del array de bits".to_string())?;
    let mut bits: Vec<u8> = Vec::new();
    for token in rest[..close].split([',', ' ', '\n', '\r', '\t']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let digits = token
            .strip_prefix("0x")
            .or_else(|| token.strip_prefix("0X"))
            .unwrap_or(token);
        let value =
            u8::from_str_radix(digits, 16).map_err(|_| format!("XBM byte inválido «{token}»"))?;
        bits.push(value);
    }
    let row_bytes = width.div_ceil(8);
    if bits.len() < row_bytes * height {
        return Err(format!(
            "XBM con {width}x{height} necesita {} bytes y solo hay {}",
            row_bytes * height,
            bits.len()
        ));
    }
    let mut out = vec![0u8; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let byte = bits[y * row_bytes + x / 8];
            if (byte >> (x % 8)) & 1 == 1 {
                let i = (y * width + x) * 4;
                out[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    Ok((width as i32, height as i32, out))
}

/// Value of `#define <name>_<kind> <value>`.
fn xbm_define(text: &str, kind: &str) -> DecodeResult<usize> {
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("#define") else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let name = words.next().unwrap_or_default();
        if !name.ends_with(kind) {
            continue;
        }
        let value = words
            .next()
            .ok_or_else(|| format!("XBM sin valor para «{name}»"))?;
        return value
            .parse()
            .map_err(|_| format!("XBM: valor inválido «{value}» en {name}"));
    }
    Err(format!("XBM sin «#define ..._{kind}»"))
}

// ---------------------------------------------------------------------------
// XPM (X Pixmap)
// ---------------------------------------------------------------------------

/// Reads a `.xpm` (XPM1 or XPM2) file: quoted header with the palette in
/// between, then one quoted row of characters per scanline.
fn decode_xpm(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    let text = String::from_utf8_lossy(bytes);
    if text.trim_start().starts_with("!XPM2") && !text.contains('"') {
        return Err("XPM2 sin cadenas de datos".to_string());
    }
    let strings = xpm_quoted(&text);
    let mut fields = strings.iter();
    let header = fields
        .next()
        .ok_or_else(|| "XPM sin la cabecera".to_string())?;
    let mut head = header.split_whitespace();
    let parse = |raw: Option<&str>, what: &str| -> DecodeResult<usize> {
        raw.ok_or_else(|| format!("XPM sin {what}"))?
            .parse()
            .map_err(|_| format!("XPM: {what} inválido «{raw:?}»"))
    };
    let width = parse(head.next(), "el ancho")?;
    let height = parse(head.next(), "el alto")?;
    let colors = parse(head.next(), "el nº de colores")?;
    let cpp = parse(head.next(), "los caracteres por píxel")?;
    if width == 0 || height == 0 || cpp == 0 || cpp > 4 {
        return Err(format!(
            "XPM con geometría no soportada: {width}x{height}, {cpp} chars/pixel"
        ));
    }
    // Tope implícito del propio fichero: cada fila aporta al menos
    // `width` bytes, de modo que una geometría mayor que el fichero es un
    // header corrupto. Sin comprobarlo aquí se reservaba
    // `width * height * 4` bytes antes de descubrir que faltan filas: con
    // un header de 10^9 por lado, decenas de petabytes que abortaban el
    // proceso en lugar de devolver un error.
    if width.checked_mul(height).is_none_or(|px| px > bytes.len()) {
        return Err(format!(
            "XPM: geometría {width}x{height} no cabe en el fichero ({} bytes)",
            bytes.len()
        ));
    }

    // `colors` también viene del header: sin tope, un número enorme
    // reservaría cientos de GB antes de fallar al leer la paleta.
    let mut palette: Vec<(String, [u8; 4])> = Vec::with_capacity(colors.min(bytes.len()));
    for _ in 0..colors {
        let entry = fields
            .next()
            .ok_or_else(|| "XPM: paleta truncada".to_string())?;
        let (key, color) = xpm_palette_entry(entry, cpp)?;
        palette.push((key, color));
    }

    let mut out = vec![0u8; width * height * 4];
    for y in 0..height {
        let row = fields
            .next()
            .ok_or_else(|| format!("XPM: faltan filas (hay {y} de {height})"))?;
        if row.chars().count() < width * cpp {
            return Err(format!(
                "XPM: la fila {y} tiene {} caracteres y se esperaban {}",
                row.chars().count(),
                width * cpp
            ));
        }
        let chars: Vec<char> = row.chars().collect();
        for x in 0..width {
            let key: String = chars[x * cpp..(x + 1) * cpp].iter().collect();
            let rgba = palette
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, c)| *c)
                .ok_or_else(|| format!("XPM: color sin definir «{key}»"))?;
            let i = (y * width + x) * 4;
            out[i..i + 4].copy_from_slice(&rgba);
        }
    }
    Ok((width as i32, height as i32, out))
}

/// All `"…"` strings of an XPM file, in order.
fn xpm_quoted(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else {
            break;
        };
        out.push(after[..end].to_string());
        rest = &after[end + 1..];
    }
    out
}

/// `<chars> <key> <value> …` → the palette key and its RGBA value. The key
/// letters (`c`, `s`, `g`, `m`) come after the characters, so the marker is
/// looked up only in what is left of the entry.
fn xpm_palette_entry(entry: &str, cpp: usize) -> DecodeResult<(String, [u8; 4])> {
    let chars: Vec<char> = entry.chars().collect();
    if chars.len() < cpp {
        return Err(format!("XPM: entrada de paleta demasiado corta «{entry}»"));
    }
    let key: String = chars[..cpp].iter().collect();
    let tail: String = chars[cpp..].iter().collect();
    let words: Vec<&str> = tail.split_whitespace().collect();
    let color = words
        .iter()
        .position(|w| *w == "c")
        .and_then(|i| words.get(i + 1))
        .ok_or_else(|| format!("XPM: sin color en «{entry}»"))?;
    Ok((key, xpm_color(color)?))
}

/// `None`, `#rgb`, `#rrggbb`, `#rrrgggbbb`, `#rrrrggggbbbb` or a name from
/// the classic X11 palette.
fn xpm_color(spec: &str) -> DecodeResult<[u8; 4]> {
    let spec = spec.trim().trim_matches('"');
    if spec.eq_ignore_ascii_case("none") {
        return Ok([0, 0, 0, 0]);
    }
    if let Some(hex) = spec.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .as_bytes()
            .iter()
            .map(|c| (*c as char).to_digit(16).unwrap_or(0) as u8)
            .collect();
        if matches!(digits.len(), 3 | 6 | 9 | 12) {
            let per_channel = digits.len() / 3;
            let channel = |i: usize| -> u8 {
                let mut value: u32 = 0;
                for d in &digits[i * per_channel..(i + 1) * per_channel] {
                    value = value * 16 + u32::from(*d);
                }
                let max = (1u32 << (4 * per_channel)) - 1;
                (value * 255 / max) as u8
            };
            return Ok([channel(0), channel(1), channel(2), 255]);
        }
        return Err(format!("XPM: color hexagonal inválido «{spec}»"));
    }
    if let Some(rgba) = xpm_named_color(spec) {
        return Ok(rgba);
    }
    Err(format!("XPM: color nombrado no soportado «{spec}»"))
}

/// Small subset of the X11 `rgb.txt` names that show up in XPM files.
fn xpm_named_color(name: &str) -> Option<[u8; 4]> {
    let rgb = match name.to_ascii_lowercase().as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" | "lime" => [0, 255, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "gray" | "grey" | "lightgray" | "lightgrey" => [190, 190, 190],
        "darkgray" | "darkgrey" | "dimgray" | "dimgrey" => [105, 105, 105],
        "orange" => [255, 165, 0],
        "brown" => [165, 42, 42],
        "pink" => [255, 192, 203],
        "purple" => [160, 32, 240],
        "navy" => [0, 0, 128],
        "teal" => [0, 128, 128],
        "maroon" => [128, 0, 0],
        "olive" => [128, 128, 0],
        "silver" => [192, 192, 192],
        "gold" => [255, 215, 0],
        "khaki" => [240, 230, 140],
        "salmon" => [250, 128, 114],
        "tomato" => [255, 99, 71],
        "coral" => [255, 127, 80],
        "skyblue" => [135, 206, 235],
        "steelblue" => [70, 130, 180],
        "slateblue" => [106, 90, 205],
        "seagreen" => [46, 139, 87],
        "forestgreen" => [34, 139, 34],
        "limegreen" => [50, 205, 50],
        "dodgerblue" => [30, 144, 255],
        "crimson" => [220, 20, 60],
        "indigo" => [75, 0, 130],
        "violet" => [238, 130, 238],
        "turquoise" => [64, 224, 208],
        "orchid" => [218, 112, 214],
        "plum" => [221, 160, 221],
        "beige" => [245, 245, 220],
        "ivory" => [255, 255, 240],
        "azure" => [240, 255, 255],
        "lavender" => [230, 230, 250],
        "tan" => [210, 180, 140],
        "wheat" => [245, 222, 179],
        "bisque" => [255, 228, 196],
        "chocolate" => [210, 105, 30],
        "firebrick" => [178, 34, 34],
        "sienna" => [160, 82, 45],
        "peru" => [205, 133, 63],
        "slategray" | "slategrey" => [112, 128, 144],
        "midnightblue" => [25, 25, 112],
        "royalblue" => [65, 105, 225],
        "deepskyblue" => [0, 191, 255],
        "lawngreen" => [124, 252, 0],
        "darkgreen" => [0, 100, 0],
        "darkblue" => [0, 0, 139],
        "darkred" => [139, 0, 0],
        "darkorange" => [255, 140, 0],
        "hotpink" => [255, 105, 180],
        _ => return None,
    };
    Some([rgb[0], rgb[1], rgb[2], 255])
}

// ---------------------------------------------------------------------------
// PSD (Adobe Photoshop)
// ---------------------------------------------------------------------------

/// Reads the flattened composite of a `.psd` document with the `psd` crate.
/// Layers are merged the way Photoshop does when saving the preview.
fn decode_psd(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    let psd = psd::Psd::from_bytes(bytes).map_err(|e| format!("PSD: {e}"))?;
    let (width, height) = (psd.width(), psd.height());
    if width == 0 || height == 0 {
        return Err("PSD con ancho o alto 0".to_string());
    }
    let rgba = psd.rgba();
    if rgba.len() < (width * height * 4) as usize {
        return Err(format!(
            "PSD: se esperaban {} bytes y hay {}",
            width * height * 4,
            rgba.len()
        ));
    }
    Ok((width as i32, height as i32, rgba))
}

// ---------------------------------------------------------------------------
// SVG
// ---------------------------------------------------------------------------

/// System fonts, loaded once: text inside an SVG must not trigger a font scan
/// per sprite.
fn shared_fontdb() -> std::sync::Arc<resvg::usvg::fontdb::Database> {
    static FONTDB: std::sync::OnceLock<std::sync::Arc<resvg::usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    FONTDB
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            std::sync::Arc::new(db)
        })
        .clone()
}

/// Rasterizes an `.svg` at its intrinsic size (or the 100×100 default) with
/// `resvg` and un-premultiplies the result: the pipeline works with straight
/// alpha while tiny-skia renders premultiplied.
fn decode_svg(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    let text = String::from_utf8_lossy(bytes);
    let opt = resvg::usvg::Options {
        fontdb: shared_fontdb(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(&text, &opt).map_err(|e| format!("SVG: {e}"))?;
    let size = tree.size();
    if !(size.width() > 0.0 && size.height() > 0.0) {
        return Err("SVG sin tamaño".to_string());
    }
    let width = size.width().ceil() as i32;
    let height = size.height().ceil() as i32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width as u32, height as u32)
        .ok_or_else(|| "SVG: no se pudo crear el lienzo".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    Ok((width, height, unpremultiply(&pixmap.take())))
}

/// tiny-skia composites to premultiplied RGBA; divide each channel by alpha.
fn unpremultiply(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    for px in out.chunks_exact_mut(4) {
        let alpha = px[3];
        if alpha == 0 {
            px[..3].fill(0);
            continue;
        }
        for value in px.iter_mut().take(3) {
            *value = ((u32::from(*value) * 255) / u32::from(alpha)).min(255) as u8;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// ASTC
// ---------------------------------------------------------------------------

/// Reads an `.astc` file (16-byte header + 16-byte blocks) with the block
/// decoder from `texture2ddecoder`.
fn decode_astc(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    if bytes.len() < 16 {
        return Err("ASTC demasiado corto".to_string());
    }
    if bytes[..4] != [0x13, 0xAB, 0xA1, 0x5C] {
        return Err("ASTC: cabecera inválida".to_string());
    }
    let block_x = bytes[4] as usize;
    let block_y = bytes[5] as usize;
    let le24 = |off: usize| {
        usize::from(bytes[off])
            | (usize::from(bytes[off + 1]) << 8)
            | (usize::from(bytes[off + 2]) << 16)
    };
    let width = le24(7);
    let height = le24(10);
    if width == 0 || height == 0 {
        return Err("ASTC con ancho o alto 0".to_string());
    }
    if block_x == 0 || block_y == 0 {
        return Err(format!("ASTC con bloque de tamaño {block_x}x{block_y}"));
    }
    let payload = &bytes[16..];
    // El payload son 16 bytes por bloque, así que una geometría que no
    // cabría en estos bytes es un header corrupto. Se comprueba antes de
    // reservar: los ejes del header son de 24 bits (hasta 16 777 215), y
    // `vec![0u32; width * height]` llegaba a petabytes.
    let Some(necesarios) = width
        .div_ceil(block_x)
        .checked_mul(height.div_ceil(block_y))
        .and_then(|bloques| bloques.checked_mul(16))
    else {
        return Err(format!("ASTC: geometría {width}x{height} demasiado grande"));
    };
    if necesarios > payload.len() {
        return Err(format!(
            "ASTC: {width}x{height} necesita {necesarios} bytes de payload y el \
             fichero tiene {}",
            payload.len()
        ));
    }
    let mut pixels = vec![0u32; width * height];
    texture2ddecoder::decode_astc(payload, width, height, block_x, block_y, &mut pixels)
        .map_err(|e| format!("ASTC: {e}"))?;
    Ok(from_u32_pixels(&pixels, width as i32, height as i32))
}

// ---------------------------------------------------------------------------
// KTX v1
// ---------------------------------------------------------------------------

const KTX1_IDENTIFIER: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];

/// Reads a `.ktx` (Khronos Texture v1) file: uncompressed RGBA/RGB payloads
/// directly, compressed ones through `texture2ddecoder`. Only the first mip
/// level of a single 2D face is read (the input is a sprite, not a mip chain).
fn decode_ktx(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    if bytes.len() < 64 || bytes[..12] != KTX1_IDENTIFIER {
        return Err("KTX1: cabecera inválida".to_string());
    }
    let le = |off: usize| -> u32 {
        u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
    };
    if le(12) != 0x0403_0201 {
        return Err("KTX1: solo se admiten ficheros en little-endian".to_string());
    }
    let gl_type = le(16);
    let gl_format = le(24);
    let internal = le(28);
    let width = le(36);
    let height = le(40);
    let faces = le(52);
    let kvd = le(60);
    if width == 0 || height == 0 {
        return Err("KTX1 con ancho o alto 0".to_string());
    }
    if faces > 1 {
        return Err("KTX1: no se admiten cubemaps".to_string());
    }
    let data = 64 + kvd as usize;
    if bytes.len() < data + 4 {
        return Err("KTX1: sin niveles de mipmap".to_string());
    }
    let image_size = le(data) as usize;
    let payload_off = data + 4;
    if bytes.len() < payload_off + image_size {
        return Err("KTX1: payload truncado".to_string());
    }
    let payload = &bytes[payload_off..payload_off + image_size];
    let w = width as usize;
    let h = height as usize;

    // Sin compresión: RGBA8 o RGB8 crudos.
    if matches!(internal, 0x8058 | 0x8051 | 0x8059 | 0x8056) {
        if gl_type != 0x1401 {
            return Err(format!(
                "KTX1: tipo de dato no soportado 0x{gl_type:04x} (solo UNSIGNED_BYTE)"
            ));
        }
        let per_pixel = match gl_format {
            0x1908 => 4, // GL_RGBA
            0x1907 => 3, // GL_RGB
            _ => {
                return Err(format!(
                    "KTX1: formato crudo no soportado 0x{gl_format:04x}"
                ))
            }
        };
        if payload.len() < w * h * per_pixel {
            return Err("KTX1: payload más corto que el lienzo".to_string());
        }
        let mut out = Vec::with_capacity(w * h * 4);
        for px in payload[..w * h * per_pixel].chunks_exact(per_pixel) {
            out.extend_from_slice(&[
                px[0],
                px[1],
                px[2],
                if per_pixel == 4 { px[3] } else { 255 },
            ]);
        }
        return Ok((width as i32, height as i32, out));
    }

    let mut pixels = vec![0u32; w * h];
    let decode = |result: std::result::Result<(), &'static str>| {
        result.map_err(|e| format!("KTX1 (internal 0x{internal:04x}): {e}"))
    };
    match internal {
        0x8D60 => decode(texture2ddecoder::decode_etc1(payload, w, h, &mut pixels))?,
        0x9274 => decode(texture2ddecoder::decode_etc2_rgb(
            payload,
            w,
            h,
            &mut pixels,
        ))?,
        0x9276 => decode(texture2ddecoder::decode_etc2_rgba1(
            payload,
            w,
            h,
            &mut pixels,
        ))?,
        0x9278 => decode(texture2ddecoder::decode_etc2_rgba8(
            payload,
            w,
            h,
            &mut pixels,
        ))?,
        0x83F0 => decode(texture2ddecoder::decode_bc1(payload, w, h, &mut pixels))?,
        0x83F1 => decode(texture2ddecoder::decode_bc1a(payload, w, h, &mut pixels))?,
        0x83F2 => decode(texture2ddecoder::decode_bc2(payload, w, h, &mut pixels))?,
        0x83F3 => decode(texture2ddecoder::decode_bc3(payload, w, h, &mut pixels))?,
        0x93B0..=0x93BD | 0x93D0..=0x93DB => {
            let block = astc_block_of(internal)?;
            decode(texture2ddecoder::decode_astc(
                payload,
                w,
                h,
                block.0,
                block.1,
                &mut pixels,
            ))?;
        }
        _ => {
            return Err(format!(
                "KTX1: formato interno no soportado 0x{internal:04x}"
            ))
        }
    }
    Ok(from_u32_pixels(&pixels, width as i32, height as i32))
}

/// Block size of a `COMPRESSED_RGBA_ASTC_*_KHR` internal format.
fn astc_block_of(internal: u32) -> DecodeResult<(usize, usize)> {
    const BLOCKS: [(u32, (usize, usize)); 26] = [
        (0x93B0, (4, 4)),
        (0x93B1, (5, 4)),
        (0x93B2, (5, 5)),
        (0x93B3, (6, 5)),
        (0x93B4, (6, 6)),
        (0x93B5, (8, 5)),
        (0x93B6, (8, 6)),
        (0x93B7, (8, 8)),
        (0x93B8, (10, 5)),
        (0x93B9, (10, 6)),
        (0x93BA, (10, 8)),
        (0x93BB, (10, 10)),
        (0x93BC, (12, 10)),
        (0x93BD, (12, 12)),
        (0x93D0, (4, 4)),
        (0x93D1, (5, 4)),
        (0x93D2, (5, 5)),
        (0x93D3, (6, 5)),
        (0x93D4, (6, 6)),
        (0x93D5, (8, 5)),
        (0x93D6, (8, 6)),
        (0x93D7, (8, 8)),
        (0x93D8, (10, 5)),
        (0x93D9, (10, 6)),
        (0x93DA, (10, 8)),
        (0x93DB, (10, 10)),
    ];
    BLOCKS
        .iter()
        .find(|(format, _)| *format == internal)
        .map(|(_, block)| *block)
        .ok_or_else(|| format!("KTX1: bloque ASTC no soportado 0x{internal:04x}"))
}

// ---------------------------------------------------------------------------
// KTX v2
// ---------------------------------------------------------------------------

const KTX2_IDENTIFIER: [u8; 12] = KTX1_IDENTIFIER;

/// Reads a `.ktx` **v2** file. Uncompressed `vkFormat` payloads (RGBA8,
/// BGRA8, RGB8, R8) with optional zlib supercompression are supported; Basis
/// Universal payloads need a transcoder and are reported as such.
fn decode_ktx2(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    if bytes.len() < 80 || bytes[..12] != KTX2_IDENTIFIER {
        return Err("KTX2: cabecera inválida".to_string());
    }
    let le = |off: usize| -> u32 {
        u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
    };
    let le64 = |off: usize| -> u64 {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&bytes[off..off + 8]);
        u64::from_le_bytes(buf)
    };
    let vk_format = le(12);
    let width = le(20);
    let height = le(24);
    // levelCount = 0 significa «solo el nivel base»: se lee como 1.
    let levels = le(40).max(1);
    let supercompression = le(44);
    if width == 0 || height == 0 {
        return Err("KTX2 con ancho o alto 0".to_string());
    }
    if supercompression == 1 {
        return Err(
            "KTX2: payload con Basis Universal (se necesita un transcoder); \
             exporta el sprite sin comprimir o en KTX v1"
                .to_string(),
        );
    }
    if supercompression > 3 {
        return Err(format!(
            "KTX2: supercompresión no soportada ({supercompression})"
        ));
    }
    // La cabecera ocupa 80 bytes e inmediatamente después viene el índice de
    // niveles (levelCount × 3 × u64); solo se lee el nivel base.
    let index = 80usize;
    if bytes.len() < index + (levels as usize) * 24 {
        return Err("KTX2: índice de niveles truncado".to_string());
    }
    let level_offset = le64(index) as usize;
    let level_length = le64(index + 8) as usize;
    let level_uncompressed = le64(index + 16) as usize;
    if bytes.len() < level_offset + level_length {
        return Err("KTX2: payload truncado".to_string());
    }
    let payload = &bytes[level_offset..level_offset + level_length];
    let payload: Vec<u8> = match supercompression {
        3 => inflate(payload, level_uncompressed)?,
        _ => payload.to_vec(),
    };

    let w = width as usize;
    let h = height as usize;
    let mut out = Vec::with_capacity(w * h * 4);
    match vk_format {
        // R8G8B8A8_UNORM / _SRGB
        37 | 38 => {
            expect_len(&payload, w * h * 4, "KTX2 RGBA8")?;
            out.extend_from_slice(&payload[..w * h * 4]);
        }
        // B8G8R8A8_UNORM / _SRGB
        43 | 44 => {
            expect_len(&payload, w * h * 4, "KTX2 BGRA8")?;
            for px in payload[..w * h * 4].chunks_exact(4) {
                out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
        // R8G8B8_UNORM / _SRGB
        10 | 11 => {
            expect_len(&payload, w * h * 3, "KTX2 RGB8")?;
            for px in payload[..w * h * 3].chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        // R8_UNORM / _SRGB
        9 | 13 => {
            expect_len(&payload, w * h, "KTX2 R8")?;
            for v in &payload[..w * h] {
                out.extend_from_slice(&[*v, *v, *v, 255]);
            }
        }
        0 => {
            return Err(
                "KTX2: vkFormat = 0 (formato descrito por el DFD, p. ej. Basis); \
                 no se puede leer aquí"
                    .to_string(),
            )
        }
        _ => {
            return Err(format!(
                "KTX2: vkFormat no soportado ({vk_format}); solo se leen RGBA8/BGRA8/RGB8/R8"
            ))
        }
    }
    Ok((width as i32, height as i32, out))
}

fn expect_len(payload: &[u8], expected: usize, what: &str) -> DecodeResult<()> {
    if payload.len() < expected {
        return Err(format!("{what}: payload más corto que el lienzo"));
    }
    Ok(())
}

fn inflate(payload: &[u8], expected: usize) -> DecodeResult<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::with_capacity(expected);
    flate2::read::ZlibDecoder::new(payload)
        .read_to_end(&mut out)
        .map_err(|e| format!("KTX2: zlib: {e}"))?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Basis Universal (`.basis`)
// ---------------------------------------------------------------------------

/// Reads a `.basis` file back to RGBA8 with the transcoder of
/// `basis-universal` (the same crate that encodes them on export).
///
/// Only the first image of the first mip level is read, as with the rest of
/// the container formats: input here is a sprite, not a texture chain.
#[cfg(feature = "gpu-formats")]
fn decode_basis(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    if bytes.len() < 4 || &bytes[..2] != b"sB" {
        return Err("Basis: firma inválida (no es un .basis)".to_string());
    }
    let (width, height, rgba) =
        tp_basis::transcode_rgba(bytes).map_err(|e| format!("Basis: {e}"))?;
    if width == 0 || height == 0 || rgba.len() != (width * height * 4) as usize {
        return Err("Basis: transcodificación con tamaño incoherente".to_string());
    }
    Ok((width as i32, height as i32, rgba))
}

/// Without the `gpu-formats` feature the transcoder is not linked: report it
/// instead of silently ignoring the file, like the GPU formats do on export.
#[cfg(not(feature = "gpu-formats"))]
fn decode_basis(_bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    Err("Basis requiere la feature \"gpu-formats\"".to_string())
}

// ---------------------------------------------------------------------------
// PKM, PVR y SVGZ (contenedores de entrada que faltaban)
// ---------------------------------------------------------------------------

fn be16(bytes: &[u8], off: usize) -> DecodeResult<u16> {
    let b = bytes.get(off..off + 2).ok_or("cabecera truncada")?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(bytes: &[u8], off: usize) -> DecodeResult<u32> {
    let b = bytes.get(off..off + 4).ok_or("cabecera truncada")?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn le32(bytes: &[u8], off: usize) -> DecodeResult<u32> {
    let b = bytes.get(off..off + 4).ok_or("cabecera truncada")?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn le64(bytes: &[u8], off: usize) -> DecodeResult<u64> {
    let b = bytes.get(off..off + 8).ok_or("cabecera truncada")?;
    let mut v = [0u8; 8];
    v.copy_from_slice(b);
    Ok(u64::from_le_bytes(v))
}

/// Reads a gzip stream into memory (`.pvr.gz`, `.svgz`).
fn gunzip(payload: &[u8], what: &str) -> DecodeResult<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(payload)
        .read_to_end(&mut out)
        .map_err(|e| format!("{what}: gzip: {e}"))?;
    Ok(out)
}

/// Crops an RGBA buffer from `src_w` down to `w`×`h` (the padded part of a
/// GPU block layout never belongs to the sprite).
fn crop_rgba(data: Vec<u8>, src_w: usize, w: usize, h: usize) -> Vec<u8> {
    if src_w == w && data.len() == w * h * 4 {
        return data;
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for row in data.chunks_exact(src_w * 4).take(h) {
        out.extend_from_slice(&row[..w * 4]);
    }
    out
}

/// ETC1 in a PKM container (`.pkm`): 16-byte header + ETC1 blocks.
///
/// Header layout, big-endian: `PKM ` (4) + version `10`/`20` (2) +
/// `dataFormat` (2, 0 = ETC1 RGB) + extended width/height (2+2) + original
/// width/height (2+2). The blocks cover the *extended* (padded to 4) size;
/// the sprite is the original rectangle inside it.
fn decode_pkm(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    if bytes.len() < 16 || &bytes[..4] != b"PKM " {
        return Err("PKM: cabecera inválida".to_string());
    }
    let version = [bytes[4], bytes[5]];
    if version != *b"10" && version != *b"20" {
        return Err(format!(
            "PKM: versión desconocida {:?}",
            String::from_utf8_lossy(&version)
        ));
    }
    let format = be16(bytes, 6)?;
    if format != 0 {
        return Err(format!(
            "PKM: dataFormat {format} no soportado (sólo ETC1 RGB, 0)"
        ));
    }
    let (ext_w, ext_h) = (be16(bytes, 8)? as usize, be16(bytes, 10)? as usize);
    let (w, h) = (be16(bytes, 12)? as usize, be16(bytes, 14)? as usize);
    if w == 0
        || h == 0
        || w > ext_w
        || h > ext_h
        || !ext_w.is_multiple_of(4)
        || !ext_h.is_multiple_of(4)
    {
        return Err(format!(
            "PKM: dimensiones incoherentes {w}x{h} dentro de {ext_w}x{ext_h}"
        ));
    }
    let blocks = (ext_w / 4) * (ext_h / 4);
    expect_len(&bytes[16..], blocks * 8, "PKM")?;
    let mut pixels = vec![0u32; ext_w * ext_h];
    texture2ddecoder::decode_etc1(&bytes[16..], ext_w, ext_h, &mut pixels)
        .map_err(|e| format!("PKM: ETC1: {e}"))?;
    let (_, _, rgba) = from_u32_pixels(&pixels, ext_w as i32, ext_h as i32);
    Ok((w as i32, h as i32, crop_rgba(rgba, ext_w, w, h)))
}

/// Dispatches a PowerVR container: v3 has the `PVR\x03` magic, v2 the `PVR!`
/// tag. `.pvr`, `.pvrtc`, `.pvr.gz` and `.pvr.ccz` all end up here.
fn decode_pvr(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    if bytes.len() >= 52 && bytes[..4] == *b"PVR\x03" {
        decode_pvr_v3(bytes)
    } else if bytes.len() >= 44 && matches!(le32(bytes, 0)?, 44 | 52) {
        decode_pvr_v2(bytes)
    } else {
        Err("PVR: cabecera desconocida (ni «PVR\\x03» v3 ni «PVR!» v2)".to_string())
    }
}

/// PVR v3: 52-byte header, little-endian, `pixelFormat` at 8 as u64.
/// Only the PVRTC1 encodings (0-3) are decoded; anything else gets a message
/// that says which number was seen.
fn decode_pvr_v3(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    let height = le32(bytes, 24)? as usize;
    let width = le32(bytes, 28)? as usize;
    let meta = le32(bytes, 48)? as usize;
    let pixel_format = le64(bytes, 8)?;
    let payload = bytes.get(52 + meta..).ok_or("PVR v3: cabecera truncada")?;
    let (w, h) = checked_size(width, height, "PVR v3")?;
    let (is_2bpp, label) = match pixel_format {
        0 | 1 => (true, "PVRTC1 2bpp"),
        2 | 3 => (false, "PVRTC1 4bpp"),
        other => {
            return Err(format!(
                "PVR v3: pixelFormat {other} no soportado (sólo PVRTC1 0-3)"
            ))
        }
    };
    decode_pvrtc_payload(payload, w, h, is_2bpp).map_err(|e| format!("PVR v3: {label}: {e}"))
}

/// PVR v2: `headerSize` (44 o 52) + `height`/`width` + `bpp` + máscaras de
/// canales, con el tag `PVR!` al final de la versión de 52 bytes.
///
/// `bpp` decides the payload: 2/4 are PVRTC1, 16/32 are raw pixels laid out
/// according to the channel masks.
fn decode_pvr_v2(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    let header_size = le32(bytes, 0)? as usize;
    if !matches!(header_size, 44 | 52) || bytes.len() < header_size {
        return Err(format!(
            "PVR v2: headerSize {header_size} no soportado (44 o 52)"
        ));
    }
    if header_size == 52 && bytes[44..48] != *b"PVR!" {
        return Err("PVR v2: falta el tag «PVR!»".to_string());
    }
    let height = le32(bytes, 4)? as usize;
    let width = le32(bytes, 8)? as usize;
    let bpp = le32(bytes, 24)?;
    let payload = &bytes[header_size..];
    let (w, h) = checked_size(width, height, "PVR v2")?;
    match bpp {
        2 => decode_pvrtc_payload(payload, w, h, true),
        4 => decode_pvrtc_payload(payload, w, h, false),
        16 | 32 => {
            let masks = [
                le32(bytes, 28)?,
                le32(bytes, 32)?,
                le32(bytes, 36)?,
                le32(bytes, 40)?,
            ];
            decode_raw_payload(payload, w, h, bpp as usize, &masks)
        }
        other => Err(format!(
            "PVR v2: bpp {other} no soportado (2/4 = PVRTC1, 16/32 = píxeles crudos)"
        )),
    }
    .map_err(|e| format!("PVR v2: {e}"))
}

/// Sanity check shared by the PVR readers: non-empty and PVRTC-compatible
/// (4bpp needs width and height multiples of 4, 2bpp also width of 8).
fn checked_size(width: usize, height: usize, what: &str) -> DecodeResult<(usize, usize)> {
    if width == 0 || height == 0 {
        return Err(format!("{what}: lienzo de tamaño nulo"));
    }
    Ok((width, height))
}

fn checked_pvrtc_size(w: usize, h: usize, is_2bpp: bool) -> DecodeResult<()> {
    let block_ok = if is_2bpp {
        w.is_multiple_of(8)
    } else {
        w.is_multiple_of(4)
    };
    if !block_ok || !h.is_multiple_of(4) {
        return Err(format!("{w}x{h} no es múltiplo del bloque PVRTC"));
    }
    Ok(())
}

/// PVRTC1 blocks → RGBA8 (4 bytes per 4x4 block at 4bpp, per 8x4 at 2bpp).
fn decode_pvrtc_payload(
    payload: &[u8],
    w: usize,
    h: usize,
    is_2bpp: bool,
) -> DecodeResult<(i32, i32, Vec<u8>)> {
    checked_pvrtc_size(w, h, is_2bpp)?;
    let block_w = if is_2bpp { 8 } else { 4 };
    expect_len(payload, w.div_ceil(block_w) * h.div_ceil(4) * 8, "PVRTC")?;
    let mut pixels = vec![0u32; w * h];
    let result = if is_2bpp {
        texture2ddecoder::decode_pvrtc_2bpp(payload, w, h, &mut pixels)
    } else {
        texture2ddecoder::decode_pvrtc_4bpp(payload, w, h, &mut pixels)
    };
    result.map_err(|e| e.to_string())?;
    let (_, _, rgba) = from_u32_pixels(&pixels, w as i32, h as i32);
    Ok((w as i32, h as i32, rgba))
}

/// Raw 16/32-bit pixels unpacked through the four channel masks.
fn decode_raw_payload(
    payload: &[u8],
    w: usize,
    h: usize,
    bpp: usize,
    masks: &[u32; 4],
) -> DecodeResult<(i32, i32, Vec<u8>)> {
    let stride = bpp / 8;
    expect_len(payload, w * h * stride, "PVR v2")?;
    let mut out = Vec::with_capacity(w * h * 4);
    for px in payload.chunks_exact(stride) {
        let mut value = 0u32;
        for (i, b) in px.iter().enumerate() {
            value |= (*b as u32) << (8 * i);
        }
        for mask in masks {
            let shift = mask.trailing_zeros();
            let bits = (*mask >> shift).count_ones();
            let raw = (value & mask) >> shift;
            // Escalado a 8 bits conservando el extremo: 31 -> 255 con 5 bits,
            // igual que la extensión `(v << 3) | (v >> 2)`.
            let scaled = if bits == 0 || bits >= 32 {
                0
            } else {
                raw * 255 / ((1u32 << bits) - 1)
            };
            out.push(scaled as u8);
        }
    }
    Ok((w as i32, h as i32, out))
}

/// `.pvr.gz`: the same container behind a gzip stream.
fn decode_pvr_gz(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    let inner = gunzip(bytes, "pvr.gz")?;
    decode_pvr(&inner)
}

/// `.pvr.ccz`: Cocos2D container, `CCZ!` (4) + compression type (2 BE, 0 =
/// zlib) + version (2) + reserved (4) + uncompressed length (4 BE) + payload.
fn decode_pvr_ccz(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    if bytes.len() < 16 || &bytes[..4] != b"CCZ!" {
        return Err("pvr.ccz: cabecera inválida (falta «CCZ!»)".to_string());
    }
    if be16(bytes, 4)? != 0 {
        return Err("pvr.ccz: sólo se lee compression_type = zlib (0)".to_string());
    }
    let expected = be32(bytes, 12)? as usize;
    let inner = inflate(&bytes[16..], expected)?;
    if inner.len() != expected {
        return Err(format!(
            "pvr.ccz: la cabecera declara {expected} bytes y salieron {}",
            inner.len()
        ));
    }
    decode_pvr(&inner)
}

/// `.svgz`: an SVG document inside a gzip stream.
fn decode_svgz(bytes: &[u8]) -> DecodeResult<(i32, i32, Vec<u8>)> {
    let inner = gunzip(bytes, "svgz")?;
    decode_svg(&inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes `bytes` to a temp file whose name carries a real extension, so
    /// the dispatch by extension is exercised end to end.
    fn temp(file: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("tp_reader_{}_{}", std::process::id(), file));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Four-quadrant 8x8 image (same shape the export round-trip uses).
    fn quadrants() -> Vec<u8> {
        let colors = [
            [10u8, 20, 30, 255],
            [200, 40, 50, 255],
            [60, 210, 70, 255],
            [8, 9, 220, 255],
        ];
        let mut out = Vec::with_capacity(8 * 8 * 4);
        for y in 0..8 {
            for x in 0..8 {
                out.extend_from_slice(&colors[(y / 4) * 2 + x / 4]);
            }
        }
        out
    }

    #[cfg(feature = "gpu-formats")]
    #[test]
    fn basis_files_are_read_back_as_rgba() {
        let rgba = quadrants();
        let file = tp_basis::encode_etc1s(&rgba, 8, 8, 80).unwrap();
        assert_eq!(&file[..2], b"sB", "firma de un .basis");

        let path = temp("basis.basis", &file);
        let (w, h, out) = load_image_rgba(&path).unwrap();
        assert_eq!(
            (w, h),
            (8, 8),
            "el primer nivel se lee con su tamaño original"
        );
        assert_eq!(out.len(), rgba.len());

        // ETC1S es con pérdida: se comprueba que no se desvía demasiado.
        let err: f64 = rgba
            .iter()
            .zip(&out)
            .map(|(a, b)| (i16::from(*a) - i16::from(*b)).abs() as f64)
            .sum::<f64>()
            / rgba.len() as f64;
        assert!(err < 5.0, "error medio Basis al leer: {err:.2}");

        // La extensión entra en la lista de formatos de entrada.
        assert!(
            crate::ingest::is_image_file(Path::new("sprite.basis")),
            "basis dejó de ser imagen"
        );
        let _ = std::fs::remove_file(path);
    }

    #[cfg(feature = "gpu-formats")]
    #[test]
    fn basis_files_report_a_broken_file() {
        let path = temp("broken.basis", b"sB\x00\x01basiseeeeeee");
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("Basis"), "{err}");
        assert!(err.contains("broken.basis"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    #[cfg(not(feature = "gpu-formats"))]
    #[test]
    fn basis_input_requires_the_gpu_formats_feature() {
        let path = temp("basis.basis", b"sB\x00\x01basiseeeeeee");
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("gpu-formats"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    // -- PKM / PVR / CCZ / SVGZ ------------------------------------------------

    /// Mean absolute error per byte between two RGBA buffers.
    fn mean_err(a: &[u8], b: &[u8]) -> f64 {
        assert_eq!(a.len(), b.len());
        a.iter()
            .zip(b)
            .map(|(x, y)| (i16::from(*x) - i16::from(*y)).abs() as f64)
            .sum::<f64>()
            / a.len() as f64
    }

    /// The same bytes the reader must produce, decoded straight from the
    /// payload with `texture2ddecoder`: the tests check the container plumbing
    /// (header, payload slice, ARGB -> RGBA), not the encoder's quality.
    fn pvrtc_reference(payload: &[u8], w: usize, h: usize, is_2bpp: bool) -> Vec<u8> {
        let mut buf = vec![0u32; w * h];
        let r = if is_2bpp {
            texture2ddecoder::decode_pvrtc_2bpp(payload, w, h, &mut buf)
        } else {
            texture2ddecoder::decode_pvrtc_4bpp(payload, w, h, &mut buf)
        };
        r.unwrap();
        from_u32_pixels(&buf, w as i32, h as i32).2
    }

    fn pvr_bytes(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
        crate::export::encode_to_bytes(
            rgba,
            w,
            h,
            &crate::export::EncodeOptions {
                format: crate::GpuFormat::Pvrtc4Bpp,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn pkm_reads_etc1_and_crops_to_the_original_size() {
        // 5x3 se rellena a 8x4 dentro del contenedor; el sprite mide 5x3.
        let color = [40u8, 180, 60, 255];
        let rgba: Vec<u8> = (0..5 * 3).flat_map(|_| color).collect();
        let file = crate::export::encode_to_bytes(
            &rgba,
            5,
            3,
            &crate::export::EncodeOptions {
                format: crate::GpuFormat::Etc1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(&file[..4], b"PKM ");
        assert_eq!(&file[4..6], b"10");
        assert_eq!(
            be16(&file, 8).unwrap(),
            8,
            "ancho extendido a múltiplo de 4"
        );
        assert_eq!(be16(&file, 12).unwrap(), 5, "ancho original");

        let path = temp("sprite.pkm", &file);
        let (w, h, out) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (5, 3), "recorte al tamaño original");
        assert_eq!(out.len(), rgba.len());
        let err = mean_err(&rgba, &out);
        assert!(err < 4.0, "error medio PKM: {err:.2}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pvr_and_pvrtc_read_the_container_we_write() {
        let rgba = quadrants();
        let file = pvr_bytes(&rgba, 8, 8);
        assert_eq!(&file[..4], b"PVR\x03");

        let expected = pvrtc_reference(&file[52..], 8, 8, false);
        for name in ["sprite.pvr", "sprite.pvrtc"] {
            let path = temp(name, &file);
            let (w, h, out) = load_image_rgba(&path).unwrap();
            assert_eq!((w, h), (8, 8), "{name}");
            assert_eq!(
                out, expected,
                "{name}: el lienzo no es el del decodificador"
            );
            let _ = std::fs::remove_file(path);
        }
        // Y además se parece al original: PVRTC es con pérdida.
        let err = mean_err(&rgba, &expected);
        assert!(err < 35.0, "error medio PVRTC contra el original: {err:.2}");
    }

    #[test]
    fn pvr_v2_reads_raw_pixels_with_their_channel_masks() {
        // 2x2 crudos en 32 bits: bytes RGBA, máscaras little-endian.
        let rgba = [
            10u8, 20, 30, 255, 200, 40, 50, 255, 60, 210, 70, 255, 8, 9, 220, 255,
        ];
        let mut file = Vec::new();
        file.extend_from_slice(&52u32.to_le_bytes()); // headerSize
        file.extend_from_slice(&2u32.to_le_bytes()); // height
        file.extend_from_slice(&2u32.to_le_bytes()); // width
        file.extend_from_slice(&1u32.to_le_bytes()); // mipmaps
        file.extend_from_slice(&0u32.to_le_bytes()); // flags
        file.extend_from_slice(&(rgba.len() as u32).to_le_bytes()); // dataSize
        file.extend_from_slice(&32u32.to_le_bytes()); // bpp
        file.extend_from_slice(&0x0000_00FFu32.to_le_bytes()); // red
        file.extend_from_slice(&0x0000_FF00u32.to_le_bytes()); // green
        file.extend_from_slice(&0x00FF_0000u32.to_le_bytes()); // blue
        file.extend_from_slice(&0xFF00_0000u32.to_le_bytes()); // alpha
        file.extend_from_slice(b"PVR!"); // tag
        file.extend_from_slice(&1u32.to_le_bytes()); // numSurfs
        file.extend_from_slice(&rgba);

        let path = temp("sprite.pvr", &file);
        let (w, h, out) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(out, rgba, "máscaras de canal en orden RGBA");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pvr_v2_reads_pvrtc_blocks() {
        let rgba = quadrants();
        let payload = &pvr_bytes(&rgba, 8, 8)[52..]; // sólo los bloques
        let mut file = Vec::new();
        file.extend_from_slice(&52u32.to_le_bytes());
        file.extend_from_slice(&8u32.to_le_bytes()); // height
        file.extend_from_slice(&8u32.to_le_bytes()); // width
        file.extend_from_slice(&1u32.to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        file.extend_from_slice(&4u32.to_le_bytes()); // bpp: PVRTC1 4bpp
        file.extend_from_slice(&[0u8; 16]); // máscaras sin uso
        file.extend_from_slice(b"PVR!");
        file.extend_from_slice(&1u32.to_le_bytes());
        file.extend_from_slice(payload);

        let path = temp("sprite.pvrtc", &file);
        let (w, h, out) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (8, 8));
        assert_eq!(
            out,
            pvrtc_reference(payload, 8, 8, false),
            "v2 con los mismos bloques"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pvr_gz_and_ccz_unwrap_the_same_file() {
        use std::io::Write;
        let rgba = quadrants();
        let plain = pvr_bytes(&rgba, 8, 8);

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&plain).unwrap();
        let gz = gz.finish().unwrap();
        let path = temp("sprite.pvr.gz", &gz);
        let (_, _, from_gz) = load_image_rgba(&path).unwrap();
        assert_eq!(
            from_gz,
            pvrtc_reference(&plain[52..], 8, 8, false),
            ".pvr.gz"
        );
        let _ = std::fs::remove_file(path);

        let mut zl = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        zl.write_all(&plain).unwrap();
        let payload = zl.finish().unwrap();
        let mut ccz = Vec::with_capacity(16 + payload.len());
        ccz.extend_from_slice(b"CCZ!");
        ccz.extend_from_slice(&0u16.to_be_bytes()); // compression_type: zlib
        ccz.extend_from_slice(&0u16.to_be_bytes()); // version
        ccz.extend_from_slice(&0u32.to_be_bytes()); // reserved
        ccz.extend_from_slice(&(plain.len() as u32).to_be_bytes());
        ccz.extend_from_slice(&payload);
        let path = temp("sprite.ccz", &ccz);
        let (w, h, from_ccz) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (8, 8), ".pvr.ccz se descomprime y se lee");
        let err = mean_err(&from_gz, &from_ccz);
        assert!(err < 0.5, "gz y ccz deben dar el mismo lienzo: {err:.3}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn svgz_reads_the_compressed_svg() {
        use std::io::Write;
        let svg =
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" viewBox="0 0 4 4">
  <rect x="0" y="0" width="4" height="4" fill="#00ff00"/>
</svg>"##;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(svg).unwrap();
        let file = gz.finish().unwrap();

        let path = temp("logo.svgz", &file);
        let (w, h, rgba) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (4, 4));
        assert_eq!(&rgba[..4], &[0, 255, 0, 255], "rect verde opaco");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn gpu_containers_report_their_broken_files() {
        let cases: [(&str, &[u8], &str); 4] = [
            (
                "bad.pkm",
                b"NOPE10\x00\x00\x00\x04\x00\x04\x00\x04\x00\x04",
                "PKM",
            ),
            ("bad.pvr", b"UNKN", "PVR"),
            ("bad.ccz", b"NOPE", "CCZ"),
            ("bad.pvr.gz", b"not a gzip stream at all", "gzip"),
        ];
        for (name, bytes, needle) in cases {
            let path = temp(name, bytes);
            let err = load_image_rgba(&path).unwrap_err().to_string();
            assert!(err.contains(needle), "{name}: esperaba «{needle}» en {err}");
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn xbm_reads_bits_lsb_first() {
        // 9x2: cada fila ocupa 2 bytes (9 bits).
        let text = "\
#define test_width 9
#define test_height 2
static unsigned char test_bits[] = { 0x01, 0x01, 0x80, 0x40 };
";
        let path = temp("xbm.xbm", text.as_bytes());
        let (w, h, rgba) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (9, 2));
        let at = |x: usize, y: usize| &rgba[(y * 9 + x) * 4..(y * 9 + x) * 4 + 4];
        assert_eq!(at(0, 0), &[0, 0, 0, 255], "bit 0 del primer byte");
        assert_eq!(at(7, 0), &[0, 0, 0, 0], "los bits altos están a 0");
        assert_eq!(at(8, 0), &[0, 0, 0, 255], "bit 0 del 2º byte = x8");
        assert_eq!(at(7, 1), &[0, 0, 0, 255], "0x80 es el bit 7 de la fila 1");
        assert_eq!(at(0, 1), &[0, 0, 0, 0], "el bit 0 de esa fila está a 0");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn xbm_rejects_truncated_arrays() {
        let text = "#define a_width 16\n#define a_height 2\nstatic char a_bits[] = { 0x01 };\n";
        let path = temp("xbm_bad.xbm", text.as_bytes());
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("necesita"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn xpm_reads_palette_and_none_transparency() {
        let text = r#"/* XPM */
static char * test[] = {
"3 2 4 1",
"  c None",
". c #ff0000",
"X c #00ff00",
"o c white",
" .X",
"oX."
};
"#;
        let path = temp("xpm.xpm", text.as_bytes());
        let (w, h, rgba) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (3, 2));
        let at = |x: usize, y: usize| &rgba[(y * 3 + x) * 4..(y * 3 + x) * 4 + 4];
        assert_eq!(at(0, 0), &[0, 0, 0, 0], "None = transparente");
        assert_eq!(at(1, 0), &[255, 0, 0, 255], "#ff0000");
        assert_eq!(at(2, 0), &[0, 255, 0, 255], "#00ff00");
        assert_eq!(at(0, 1), &[255, 255, 255, 255], "white");
        assert_eq!(at(1, 1), &[0, 255, 0, 255], "repetido");
        assert_eq!(at(2, 1), &[255, 0, 0, 255], ".");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn xpm_reports_unknown_colors() {
        let text = r#"
"1 1 1 1",
"  c glitter",
" "
"#;
        let path = temp("xpm_bad.xpm", text.as_bytes());
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("glitter"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn xpm_rejects_geometry_bigger_than_the_file() {
        // Header corrupto: 10^9 por lado. Antes de mirar las filas se
        // reservaban width*height*4 bytes (≈40 PB) y el proceso abortaba
        // en vez de devolver un error.
        let text = r#"
"1000000000 1000000000 1 1",
"  c #000000",
" "
"#;
        let path = temp("xpm_huge.xpm", text.as_bytes());
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("no cabe en el fichero"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn xpm_does_not_reserve_an_absurd_palette() {
        // Geometría válida (4x4 cabe en el fichero) pero 4 000 000 000
        // colores en la paleta: el `with_capacity` reservaría cientos de
        // GB antes de descubrir que la paleta está truncada, que es el
        // error que sí se devuelve.
        let text = r#"
"4 4 4000000000 1",
"  c #000000"
"#;
        let path = temp("xpm_huge_palette.xpm", text.as_bytes());
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("paleta truncada"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pnm_files_are_read_through_image() {
        // P6 binario: 2x1, rojo y azul.
        let mut bytes = b"P6\n2 1\n255\n".to_vec();
        bytes.extend_from_slice(&[255, 0, 0, 0, 0, 255]);
        let path = temp("ppm.ppm", &bytes);
        let (w, h, rgba) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        let _ = std::fs::remove_file(path);

        // P1 (mapa de bits ASCII) también entra: en PBM el 1 es negro y el 0
        // blanco (convención de netpbm, la inversa de XBM).
        let path = temp("pbm.pbm", b"P1\n2 2\n1 0\n0 1\n");
        let (w, h, rgba) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(&rgba[0..4], &[0, 0, 0, 255], "1 = negro");
        assert_eq!(&rgba[4..8], &[255, 255, 255, 255], "0 = blanco");
        assert_eq!(&rgba[8..12], &[255, 255, 255, 255]);
        assert_eq!(&rgba[12..16], &[0, 0, 0, 255]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ktx1_reads_uncompressed_rgba_and_etc2() {
        let rgba: Vec<u8> = (0..8 * 8 * 4).map(|i| (i % 251) as u8).collect();
        let mut ktx = Vec::new();
        ktx.extend_from_slice(&KTX1_IDENTIFIER);
        ktx.extend_from_slice(&0x0403_0201u32.to_le_bytes()); // endianness
        ktx.extend_from_slice(&0x1401u32.to_le_bytes()); // glType UNSIGNED_BYTE
        ktx.extend_from_slice(&1u32.to_le_bytes()); // glTypeSize
        ktx.extend_from_slice(&0x1908u32.to_le_bytes()); // GL_RGBA
        ktx.extend_from_slice(&0x8058u32.to_le_bytes()); // GL_RGBA8
        ktx.extend_from_slice(&0x1908u32.to_le_bytes()); // base internal
        ktx.extend_from_slice(&8u32.to_le_bytes()); // width
        ktx.extend_from_slice(&8u32.to_le_bytes()); // height
        ktx.extend_from_slice(&0u32.to_le_bytes()); // depth
        ktx.extend_from_slice(&0u32.to_le_bytes()); // array
        ktx.extend_from_slice(&1u32.to_le_bytes()); // faces
        ktx.extend_from_slice(&1u32.to_le_bytes()); // mips
        ktx.extend_from_slice(&0u32.to_le_bytes()); // kvd bytes
        ktx.extend_from_slice(&(rgba.len() as u32).to_le_bytes());
        ktx.extend_from_slice(&rgba);
        let path = temp("ktx1.ktx", &ktx);
        let (w, h, out) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (8, 8));
        assert_eq!(out, rgba, "payload RGBA8 crudo");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ktx1_reads_an_etc2_payload() {
        // Cuatro bloques 4x4 planos: cada uno un gris distinto, codificado con
        // nuestro ETC2 y leído con el decodificador independiente.
        let mut source = vec![0u8; 8 * 8 * 4];
        for (i, px) in source.chunks_exact_mut(4).enumerate() {
            let x = i % 8;
            let y = i / 8;
            let v = [255u8, 0, 128, 64][(y / 4) * 2 + x / 4];
            px.copy_from_slice(&[v, v, v, 255]);
        }
        let payload = crate::etc2::encode_etc2_rgb_blocks(&source, 8, 8, 70).unwrap();

        let mut ktx = Vec::new();
        ktx.extend_from_slice(&KTX1_IDENTIFIER);
        ktx.extend_from_slice(&0x0403_0201u32.to_le_bytes());
        ktx.extend_from_slice(&0u32.to_le_bytes()); // glType 0 = comprimido
        ktx.extend_from_slice(&1u32.to_le_bytes()); // glTypeSize
        ktx.extend_from_slice(&0u32.to_le_bytes()); // glFormat 0
        ktx.extend_from_slice(&0x9274u32.to_le_bytes()); // ETC2_RGB
        ktx.extend_from_slice(&0x1907u32.to_le_bytes()); // base GL_RGB
        ktx.extend_from_slice(&8u32.to_le_bytes()); // width
        ktx.extend_from_slice(&8u32.to_le_bytes()); // height
        ktx.extend_from_slice(&0u32.to_le_bytes()); // depth
        ktx.extend_from_slice(&0u32.to_le_bytes()); // array
        ktx.extend_from_slice(&1u32.to_le_bytes()); // faces
        ktx.extend_from_slice(&1u32.to_le_bytes()); // mips
        ktx.extend_from_slice(&0u32.to_le_bytes()); // kvd bytes
        ktx.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        ktx.extend_from_slice(&payload);

        let path = temp("ktx1_etc2.ktx", &ktx);
        let (w, h, out) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (8, 8));
        for (i, px) in out.chunks_exact(4).enumerate() {
            let x = i % 8;
            let y = i / 8;
            let expected = [255i32, 0, 128, 64][(y / 4) * 2 + x / 4];
            for (c, value) in px.iter().take(3).enumerate() {
                assert!(
                    (i32::from(*value) - expected).abs() <= 12,
                    "píxel {i} canal {c}: {value} vs {expected}"
                );
            }
            assert_eq!(px[3], 255, "RGB8 siempre opaco");
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ktx2_reads_uncompressed_and_zlib_levels() {
        let rgba: Vec<u8> = (0..4 * 2 * 4).map(|i| (i * 7) as u8).collect();
        let build = |scheme: u32, data: &[u8], uncompressed: u64| -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&KTX2_IDENTIFIER);
            out.extend_from_slice(&37u32.to_le_bytes()); // vkFormat R8G8B8A8
            out.extend_from_slice(&1u32.to_le_bytes()); // typeSize
            out.extend_from_slice(&4u32.to_le_bytes()); // width
            out.extend_from_slice(&2u32.to_le_bytes()); // height
            out.extend_from_slice(&0u32.to_le_bytes()); // depth
            out.extend_from_slice(&0u32.to_le_bytes()); // layers
            out.extend_from_slice(&1u32.to_le_bytes()); // faces
            out.extend_from_slice(&1u32.to_le_bytes()); // levels
            out.extend_from_slice(&scheme.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // dfd offset
            out.extend_from_slice(&0u32.to_le_bytes()); // dfd length
            out.extend_from_slice(&0u32.to_le_bytes()); // kvd offset
            out.extend_from_slice(&0u32.to_le_bytes()); // kvd length
            out.extend_from_slice(&0u64.to_le_bytes()); // sgd offset
            out.extend_from_slice(&0u64.to_le_bytes()); // sgd length
                                                        // Índice de niveles: byteOffset, byteLength, uncompressedByteLength.
            let level_offset = 80 + 24;
            out.extend_from_slice(&(level_offset as u64).to_le_bytes());
            out.extend_from_slice(&(data.len() as u64).to_le_bytes());
            out.extend_from_slice(&uncompressed.to_le_bytes());
            out.extend_from_slice(data);
            out
        };

        let plain = build(0, &rgba, rgba.len() as u64);
        let path = temp("ktx2.ktx2", &plain);
        let (w, h, out) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (4, 2));
        assert_eq!(out, rgba);
        let _ = std::fs::remove_file(path);

        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &rgba).unwrap();
        let compressed = encoder.finish().unwrap();
        let deflated = build(3, &compressed, rgba.len() as u64);
        let path = temp("ktx2_zlib.ktx2", &deflated);
        let (w, h, out) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (4, 2));
        assert_eq!(out, rgba, "zlib supercomprimido");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ktx2_reports_basis_payloads() {
        let mut out = Vec::new();
        out.extend_from_slice(&KTX2_IDENTIFIER);
        out.extend_from_slice(&0u32.to_le_bytes()); // vkFormat = 0 (DFD)
        out.extend_from_slice(&1u32.to_le_bytes()); // typeSize
        out.extend_from_slice(&4u32.to_le_bytes()); // width
        out.extend_from_slice(&4u32.to_le_bytes()); // height
        out.extend_from_slice(&0u32.to_le_bytes()); // depth
        out.extend_from_slice(&0u32.to_le_bytes()); // layers
        out.extend_from_slice(&1u32.to_le_bytes()); // faces
        out.extend_from_slice(&1u32.to_le_bytes()); // levels
        out.extend_from_slice(&1u32.to_le_bytes()); // BasisLZ
        out.extend_from_slice(&[0u8; 16]); // dfd + kvd (u32 × 4)
        out.extend_from_slice(&[0u8; 16]); // sgd (u64 × 2)
        out.extend_from_slice(&[0u8; 24]); // índice de niveles
        let path = temp("ktx2_basis.ktx2", &out);
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("Basis") || err.contains("transcoder"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn astc_reads_the_container_and_decodes_the_blocks() {
        // 8x8 en bloques de 4x4 = 4 bloques; payload fijo para que el test
        // sea determinista (decodificado por texture2ddecoder).
        let mut bytes = vec![0x13, 0xAB, 0xA1, 0x5C];
        bytes.extend_from_slice(&[4, 4, 1]); // bloque
        bytes.extend_from_slice(&8u32.to_le_bytes()[..3]); // ancho
        bytes.extend_from_slice(&8u32.to_le_bytes()[..3]); // alto
        bytes.extend_from_slice(&1u32.to_le_bytes()[..3]); // z
        bytes.extend_from_slice(&[0u8; 4 * 16]);
        let path = temp("astc.astc", &bytes);
        let (w, h, rgba) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (8, 8));
        assert_eq!(rgba.len(), 8 * 8 * 4);
        let _ = std::fs::remove_file(path);

        // Cabecera incorrecta.
        let path = temp("astc_bad.astc", b"NOPE");
        assert!(load_image_rgba(&path).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn astc_rejects_geometry_the_payload_cannot_hold() {
        // Ejes de 24 bits (16 777 215 por lado) y payload vacío: el
        // `vec![0u32; width * height]` de antes llegaba a ~1 PB. Los
        // 16 bytes por bloque que exige el formato no caben aquí.
        let mut bytes = vec![0x13, 0xAB, 0xA1, 0x5C];
        bytes.extend_from_slice(&[4, 4, 1]); // bloque
        bytes.extend_from_slice(&[0xFF, 0xFF, 0x0F]); // ancho = 16 777 215
        bytes.extend_from_slice(&[0xFF, 0xFF, 0x0F]); // alto = 16 777 215
        bytes.extend_from_slice(&1u32.to_le_bytes()[..3]); // z
        let path = temp("astc_huge.astc", &bytes);
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("payload"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn astc_rejects_zero_sized_blocks() {
        // Bloque 0x0: además de no ser un formato ASTC válido, partía la
        // comprobación del payload con una división por cero.
        let mut bytes = vec![0x13, 0xAB, 0xA1, 0x5C];
        bytes.extend_from_slice(&[0, 0, 1]); // bloque 0x0
        bytes.extend_from_slice(&8u32.to_le_bytes()[..3]);
        bytes.extend_from_slice(&8u32.to_le_bytes()[..3]);
        bytes.extend_from_slice(&1u32.to_le_bytes()[..3]);
        bytes.extend_from_slice(&[0u8; 4 * 16]);
        let path = temp("astc_zero_block.astc", &bytes);
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("bloque de tamaño 0x0"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn astc_block_table_covers_every_ldr_format() {
        for internal in 0x93B0..=0x93BD {
            assert!(astc_block_of(internal).is_ok(), "0x{internal:04x}");
        }
        for internal in 0x93D0..=0x93DB {
            assert!(astc_block_of(internal).is_ok(), "0x{internal:04x}");
        }
        assert!(astc_block_of(0x8058).is_err());
    }

    #[test]
    fn astc_decodes_a_real_atlas_exported_by_our_encoder() {
        // `tp-cli pack --format astc --pixel-format astc-4x4 --astc-quality 4`
        // de un PNG 8x8 con cuatro cuadrantes (rojo, verde, azul y amarillo
        // con alfa 200), ajustado a una hoja de 12x12. El payload se embebe
        // en hexadecimal para que el test verifique de verdad el decodificador
        // frente al codificador de astcenc.
        const HEX: &str = "\
            13aba15c0404010c00000c0000010000428001fe0100000000fe01003f3f3f0022c8090d00000000ff3f000000f0f0f0\
            4280010000fe010000fe0100fcfcfc002288651500f8ffff000000f0873f3f3f4288830504000140840020260f0ff0f0\
            42888203840880c300000080fcfc000042800100000000fe01fe0100003f3f3f43c842148000104200001300a4949492\
            428001fe01fe01000090010000fcfcfc";
        let bytes: Vec<u8> = (0..HEX.len() / 2)
            .map(|i| u8::from_str_radix(&HEX[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        let path = temp("golden.astc", &bytes);
        let (w, h, rgba) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (12, 12), "la hoja astc es 12x12");

        let at = |x: usize, y: usize| -> [i32; 4] {
            let i = (y * 12 + x) * 4;
            [
                rgba[i] as i32,
                rgba[i + 1] as i32,
                rgba[i + 2] as i32,
                rgba[i + 3] as i32,
            ]
        };
        let close = |got: [i32; 4], want: [i32; 4], label: &str| {
            for c in 0..4 {
                assert!(
                    (got[c] - want[c]).abs() <= 32,
                    "{label}: canal {c} = {} y se esperaba {want:?} en {got:?}",
                    got[c]
                );
            }
        };
        close(at(1, 1), [255, 0, 0, 255], "cuadrante rojo");
        close(at(6, 1), [0, 255, 0, 255], "cuadrante verde");
        close(at(1, 6), [0, 0, 255, 255], "cuadrante azul");
        close(at(6, 6), [255, 255, 0, 200], "cuadrante amarillo con alfa");
        assert!(
            at(11, 11)[3] < 40,
            "el relleno fuera del sprite queda transparente: {:?}",
            at(11, 11)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unknown_ktx_internal_formats_are_reported() {
        let mut ktx = Vec::new();
        ktx.extend_from_slice(&KTX1_IDENTIFIER);
        ktx.extend_from_slice(&0x0403_0201u32.to_le_bytes());
        ktx.extend_from_slice(&[0u8; 8]); // glType + typeSize
        ktx.extend_from_slice(&0u32.to_le_bytes()); // glFormat
        ktx.extend_from_slice(&0xDEADu32.to_le_bytes()); // interno
        ktx.extend_from_slice(&[0u8; 4]); // base
        ktx.extend_from_slice(&4u32.to_le_bytes());
        ktx.extend_from_slice(&4u32.to_le_bytes());
        ktx.extend_from_slice(&[0u8; 12]);
        ktx.extend_from_slice(&1u32.to_le_bytes()); // faces
        ktx.extend_from_slice(&1u32.to_le_bytes()); // mips
        ktx.extend_from_slice(&0u32.to_le_bytes()); // kvd
        ktx.extend_from_slice(&0u32.to_le_bytes()); // imageSize
        let path = temp("ktx_unknown.ktx", &ktx);
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("0xdead"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    /// Minimal flattened RGB PSD: header + empty sections + raw planar data.
    fn build_psd(width: u32, height: u32, color_at: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"8BPS");
        out.extend_from_slice(&1u16.to_be_bytes()); // versión
        out.extend_from_slice(&[0u8; 6]); // reservado
        out.extend_from_slice(&3u16.to_be_bytes()); // canales RGB
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&8u16.to_be_bytes()); // profundidad
        out.extend_from_slice(&3u16.to_be_bytes()); // modo RGB
        out.extend_from_slice(&0u32.to_be_bytes()); // color mode data
        out.extend_from_slice(&0u32.to_be_bytes()); // image resources
        out.extend_from_slice(&0u32.to_be_bytes()); // layer & mask
        out.extend_from_slice(&0u16.to_be_bytes()); // compresión raw
        for channel in 0..3usize {
            for y in 0..height {
                for x in 0..width {
                    out.push(color_at(x, y)[channel]);
                }
            }
        }
        out
    }

    #[test]
    fn psd_reads_the_flattened_rgb_composite() {
        let bytes = build_psd(4, 3, |x, y| {
            if x < 2 {
                [255, 0, 0]
            } else if y == 0 {
                [0, 0, 255]
            } else {
                [0, 128, 0]
            }
        });
        let path = temp("doc.psd", &bytes);
        let (w, h, rgba) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (4, 3));
        let at = |x: usize, y: usize| -> [u8; 4] {
            let i = (y * 4 + x) * 4;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        assert_eq!(at(0, 0), [255, 0, 0, 255]);
        assert_eq!(at(3, 0), [0, 0, 255, 255]);
        assert_eq!(at(3, 2), [0, 128, 0, 255]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn psd_reports_broken_documents() {
        let path = temp("broken.psd", b"8BPS");
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("PSD"), "{err}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn svg_rasterizes_shapes_with_straight_alpha() {
        let svg =
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" viewBox="0 0 4 4">
  <rect x="0" y="0" width="2" height="4" fill="#ff0000"/>
  <rect x="2" y="0" width="2" height="4" fill="#0000ff" fill-opacity="0.5"/>
</svg>"##;
        let path = temp("sprite.svg", svg);
        let (w, h, rgba) = load_image_rgba(&path).unwrap();
        assert_eq!((w, h), (4, 4));
        let at = |x: usize, y: usize| -> [u8; 4] {
            let i = (y * 4 + x) * 4;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        assert_eq!(at(1, 1), [255, 0, 0, 255], "rect rojo opaco");
        let half = at(3, 1);
        assert_eq!(half[0], 0, "azul sin rojo: {half:?}");
        assert!(
            (i32::from(half[2]) - 255).abs() <= 8,
            "azul casi puro: {half:?}"
        );
        assert!(
            (i32::from(half[3]) - 128).abs() <= 4,
            "alfa al 50% sin premultiplicar: {half:?}"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn svg_reports_malformed_markup() {
        let path = temp("broken.svg", b"<svg><rect");
        let err = load_image_rgba(&path).unwrap_err().to_string();
        assert!(err.contains("SVG"), "{err}");
        let _ = std::fs::remove_file(path);
    }
}
