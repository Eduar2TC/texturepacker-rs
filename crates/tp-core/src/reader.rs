//! Lectura de formatos de entrada que `image` no cubre: XBM, XPM, PSD y SVG,
//! más los contenedores GPU (`.astc`, `.ktx`, `.ktx2`).
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
    match ext.as_deref() {
        Some("xbm") | Some("xpm") | Some("astc") | Some("ktx") | Some("ktx2") | Some("psd")
        | Some("svg") => {
            let bytes = std::fs::read(path)
                .map_err(|e| TpError::Other(format!("{}: {e}", path.display())))?;
            let decoded = match ext.as_deref() {
                Some("xbm") => decode_xbm(&bytes),
                Some("xpm") => decode_xpm(&bytes),
                Some("astc") => decode_astc(&bytes),
                Some("psd") => decode_psd(&bytes),
                Some("svg") => decode_svg(&bytes),
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

    let mut palette: Vec<(String, [u8; 4])> = Vec::with_capacity(colors);
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
    let payload = &bytes[16..];
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
        let payload = crate::etc2::encode_etc2_rgb_blocks(&source, 8, 8, 70);

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
