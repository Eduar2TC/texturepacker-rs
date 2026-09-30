//! Subsystem 5: Exportación & Cifrado.
//!
//! - Texture encoding: PNG con paleta automática de 8 bits (*Png Opt
//!   Level*), PNG-8 con cuantización y dithering (*Dithering* PngQuant),
//!   JPG y WebP lossy (*Image quality*), WebP sin pérdidas, ASTC 4x4 (via
//!   ARM astcenc when the `gpu-formats` feature is enabled), ETC2 RGBA8
//!   and PVRTC1 4bpp (built-in encoders)
//! - Conversión de formato de píxel (*Pixel format*) y volteo vertical
//!   (*flip-y*, solo formatos de hardware)
//! - AES-256-GCM symmetric encryption of generated image files
//! - Image scaling for @2x/@1x style variants

use crate::config::{GpuFormat, PixelFormat, PngDither, ProjectConfig};
use crate::error::{Result, TpError};
use crate::etc2;
use crate::pvrtc;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::ImageEncoder;
use std::collections::HashMap;

/// Opciones de codificación de textura (nivel de optimización PNG, dithering,
/// calidad, formato de píxel).
#[derive(Debug, Clone)]
pub struct EncodeOptions {
    pub format: GpuFormat,
    pub png_opt_level: u8,
    pub png8_dither: PngDither,
    pub jpg_quality: u8,
    pub webp_quality: u16,
    pub pixel_format: PixelFormat,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            format: GpuFormat::Png,
            png_opt_level: 1,
            png8_dither: PngDither::default(),
            jpg_quality: 80,
            webp_quality: 101,
            pixel_format: PixelFormat::default(),
        }
    }
}

impl EncodeOptions {
    /// Opciones derivadas de la configuración del proyecto.
    pub fn from_config(config: &ProjectConfig) -> Self {
        Self {
            format: config.gpu_format,
            png_opt_level: config.png_opt_level,
            png8_dither: config.png8_dither,
            jpg_quality: config.jpg_quality,
            webp_quality: config.webp_quality,
            pixel_format: config.pixel_format,
        }
    }
}

/// Encode an RGBA8 image into the *file* bytes of the requested format.
pub fn encode_to_bytes(
    rgba: &[u8],
    width: usize,
    height: usize,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    match opts.format {
        GpuFormat::Png => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_png(&data, width, height, color, opts)
        }
        GpuFormat::Png8 => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_png8(&data, width, height, color, opts)
        }
        GpuFormat::Jpg => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_jpg(&data, width, height, color, opts)
        }
        GpuFormat::WebP => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_webp(&data, width, height, color, opts)
        }
        // Formatos de hardware: comprimen RGBA directamente (ignoran el
        // formato de píxel, como indica la documentación).
        GpuFormat::Astc4x4 => encode_astc(rgba, width, height),
        GpuFormat::Etc2Rgba => Ok(encode_etc2_ktx(rgba, width, height)),
        GpuFormat::Pvrtc4Bpp => encode_pvrtc_pvr(rgba, width, height),
        GpuFormat::Pvr3Gz => encode_pvrtc_pvr(rgba, width, height).map(gzip_bytes),
        GpuFormat::Pvr3Ccz => encode_pvrtc_pvr(rgba, width, height).map(ccz_bytes),
        GpuFormat::Etc1 => Ok(encode_etc1_pkm(rgba, width, height)),
        GpuFormat::Etc1Ktx => Ok(encode_etc1_ktx(rgba, width, height)),
        // Formatos de software: pasan por la conversión de pixel format.
        GpuFormat::Bmp | GpuFormat::Tga | GpuFormat::Tiff => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_image_format(&data, width, height, color, opts.format)
        }
        GpuFormat::Dds => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_dds(&data, width, height, color)
        }
        GpuFormat::Zktx => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            let ktx = ktx1_rgba8(width, height, color, &data);
            Ok(zlib_bytes(&ktx))
        }
    }
}

/// Convierte RGBA8 al formato de píxel de salida (pixel format).
/// `Rgb888` compone la transparencia sobre negro; los formatos de hardware
/// no pasan por aquí.
fn apply_pixel_format(rgba: &[u8], format: PixelFormat) -> (Vec<u8>, image::ExtendedColorType) {
    use image::ExtendedColorType as Ct;
    match format {
        PixelFormat::Rgba8888 => (rgba.to_vec(), Ct::Rgba8),
        PixelFormat::Rgb888 => {
            let mut out = Vec::with_capacity(rgba.len() / 4 * 3);
            for px in rgba.chunks_exact(4) {
                out.extend_from_slice(&composite_black(px));
            }
            (out, Ct::Rgb8)
        }
        PixelFormat::Alpha8 => (rgba.chunks_exact(4).map(|px| px[3]).collect(), Ct::L8),
        PixelFormat::Intensity8 => (rgba.chunks_exact(4).map(luma).collect(), Ct::L8),
        PixelFormat::AlphaIntensity8 => {
            let mut out = Vec::with_capacity(rgba.len() / 2);
            for px in rgba.chunks_exact(4) {
                out.push(luma(px));
                out.push(px[3]);
            }
            (out, Ct::La8)
        }
        // RGBA5551: reduce cada canal a su rejilla
        // R5/G5/B5/A1 con replicación de bits (el PNG sigue siendo RGBA8, pero
        // los colores solo toman los 32 niveles / 2 niveles de alfa del
        // formato de 16 bits). Cuantiza R y B a 5 bits, G a 5 bits, A a 1 bit.
        PixelFormat::Rgba5551 => {
            let mut out = Vec::with_capacity(rgba.len());
            for px in rgba.chunks_exact(4) {
                // Cuantización 8→5 bits (redondeo) y expansión 5→8 por
                // replicación de bits: (v << 3) | (v >> 2).
                let q = |c: u8| ((c as u16 * 31 + 127) / 255) as u8; // 0..=31
                let expand5 = |v: u8| (v << 3) | (v >> 2);
                let g = expand5(q(px[1]));
                let b = expand5(q(px[2]));
                let a: u8 = if px[3] >= 128 { 255 } else { 0 };
                out.extend_from_slice(&[expand5(q(px[0])), g, b, a]);
            }
            (out, Ct::Rgba8)
        }
        // RGBA5555 (20 bits): igual que RGBA5551 pero el
        // alfa también se cuantiza a 5 bits (32 niveles) en vez de 0/255.
        PixelFormat::Rgba5555 => {
            let mut out = Vec::with_capacity(rgba.len());
            for px in rgba.chunks_exact(4) {
                let q = |c: u8| ((c as u16 * 31 + 127) / 255) as u8;
                let expand5 = |v: u8| (v << 3) | (v >> 2);
                out.extend_from_slice(&[
                    expand5(q(px[0])),
                    expand5(q(px[1])),
                    expand5(q(px[2])),
                    expand5(q(px[3])),
                ]);
            }
            (out, Ct::Rgba8)
        }
        // BGRA8888: intercambia R y B en el archivo.
        PixelFormat::Bgra8888 => {
            let mut out = Vec::with_capacity(rgba.len());
            for px in rgba.chunks_exact(4) {
                out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
            (out, Ct::Rgba8)
        }
    }
}

/// Compone un píxel RGBA sobre fondo negro (para formatos sin alfa).
fn composite_black(px: &[u8]) -> [u8; 3] {
    let alpha = px[3] as u16;
    let mix = |c: u8| ((c as u16 * alpha + 127) / 255) as u8;
    [mix(px[0]), mix(px[1]), mix(px[2])]
}

/// Luminancia BT.601 en enteros: 0.299 R + 0.587 G + 0.114 B.
fn luma(px: &[u8]) -> u8 {
    ((77u16 * px[0] as u16 + 150 * px[1] as u16 + 29 * px[2] as u16 + 128) >> 8) as u8
}

/// Muestra por píxel de cada formato de píxel de software.
fn color_channels(color: image::ExtendedColorType) -> usize {
    use image::ExtendedColorType as Ct;
    match color {
        Ct::Rgba8 => 4,
        Ct::Rgb8 => 3,
        Ct::La8 => 2,
        _ => 1,
    }
}

/// Mapea *Png Opt Level* (0-7) a compresión de deflate de `image`.
fn png_compression(level: u8) -> CompressionType {
    match level {
        0 => CompressionType::Fast,
        1 => CompressionType::Default,
        // 2..=7 → niveles de deflate crecientes (4..=9).
        _ => CompressionType::Level((level + 2).min(9)),
    }
}

/// *Png Opt Level* 0 = sin filtros (más rápido, archivo mayor).
fn png_filter(level: u8) -> FilterType {
    if level == 0 {
        FilterType::NoFilter
    } else {
        FilterType::Adaptive
    }
}

/// Paleta exacta (lossless) si la imagen tiene ≤256 colores únicos.
/// Devuelve `None` cuando no cabe en una paleta de 8 bits.
fn try_indexed_exact(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    level: u8,
) -> Result<Option<Vec<u8>>> {
    use image::ExtendedColorType as Ct;
    let channels = match color {
        Ct::Rgba8 | Ct::Rgb8 => color_channels(color),
        // Grises ya son compactos: no merece la pasada extra.
        _ => return Ok(None),
    };
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut lookup: HashMap<[u8; 4], u8> = HashMap::new();
    let mut indices = Vec::with_capacity(width * height);
    for px in data.chunks_exact(channels) {
        let key = if channels == 4 {
            [px[0], px[1], px[2], px[3]]
        } else {
            [px[0], px[1], px[2], 255]
        };
        let idx = match lookup.get(&key) {
            Some(&i) => i,
            None => {
                if palette.len() >= 256 {
                    return Ok(None);
                }
                let i = palette.len() as u8;
                palette.push(key);
                lookup.insert(key, i);
                i
            }
        };
        indices.push(idx);
    }
    write_indexed_png(width, height, &palette, &indices, level).map(Some)
}

/// PNG con paleta indexada de 8 bits (PLTE + tRNS cuando hay alfa).
fn write_indexed_png(
    width: usize,
    height: usize,
    palette: &[[u8; 4]],
    indices: &[u8],
    level: u8,
) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, width as u32, height as u32);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        let mut plte = Vec::with_capacity(palette.len() * 3);
        let mut trns = Vec::with_capacity(palette.len());
        let mut has_alpha = false;
        for c in palette {
            plte.extend_from_slice(&c[..3]);
            trns.push(c[3]);
            has_alpha |= c[3] != 255;
        }
        enc.set_palette(plte);
        if has_alpha {
            enc.set_trns(trns);
        }
        set_png_effort(&mut enc, level);
        let mut writer = enc
            .write_header()
            .map_err(|e| TpError::Other(format!("Error en cabecera PNG indexado: {e}")))?;
        writer
            .write_image_data(indices)
            .map_err(|e| TpError::Other(format!("Error en datos PNG indexado: {e}")))?;
    }
    Ok(buf)
}

/// Mapea *Png Opt Level* (0-7) a esfuerzo de compresión del crate `png`.
fn set_png_effort<W: std::io::Write>(enc: &mut png::Encoder<'_, W>, level: u8) {
    match level {
        0 => {
            enc.set_compression(png::Compression::Fastest);
            enc.set_filter(png::Filter::NoFilter);
        }
        1 => {
            enc.set_compression(png::Compression::Balanced);
            enc.set_filter(png::Filter::Adaptive);
        }
        _ => {
            enc.set_compression(png::Compression::Balanced);
            enc.set_deflate_compression(png::DeflateCompression::Level((level + 2).min(9)));
            enc.set_filter(png::Filter::Adaptive);
        }
    }
}

fn encode_png(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    if data.len() != width * height * color_channels(color) {
        return Err(TpError::Other(
            "Dimensiones de imagen inválidas para PNG".to_string(),
        ));
    }
    // Png Opt Level ≥ 1: PNG indexado de 8 bits (sin pérdida) cuando la
    // imagen tiene 256 colores o menos.
    if opts.png_opt_level >= 1 {
        if let Some(bytes) = try_indexed_exact(data, width, height, color, opts.png_opt_level)? {
            return Ok(bytes);
        }
    }
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(
        &mut buf,
        png_compression(opts.png_opt_level),
        png_filter(opts.png_opt_level),
    )
    .write_image(data, width as u32, height as u32, color)
    .map_err(|e| TpError::Other(format!("Error codificando PNG: {e}")))?;
    Ok(buf)
}

/// PNG-8: paleta de ≤256 colores con cuantización y dithering
/// (PngQuant Low/Medium/High).
fn encode_png8(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    if data.len() != width * height * color_channels(color) {
        return Err(TpError::Other(
            "Dimensiones de imagen inválidas para PNG".to_string(),
        ));
    }
    let rgba = expand_to_rgba(data, color);
    let (palette, indices) = quantize_rgba(&rgba, width, height, opts.png8_dither);
    write_indexed_png(width, height, &palette, &indices, opts.png_opt_level)
}

/// Expande cualquier formato de píxel de software a RGBA8.
fn expand_to_rgba(data: &[u8], color: image::ExtendedColorType) -> Vec<u8> {
    use image::ExtendedColorType as Ct;
    match color {
        Ct::Rgba8 => data.to_vec(),
        Ct::Rgb8 => {
            let mut out = Vec::with_capacity(data.len() / 3 * 4);
            for px in data.chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            out
        }
        Ct::La8 => {
            let mut out = Vec::with_capacity(data.len() / 2 * 4);
            for px in data.chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            out
        }
        _ => {
            let mut out = Vec::with_capacity(data.len() * 4);
            for &v in data {
                out.extend_from_slice(&[v, v, v, 255]);
            }
            out
        }
    }
}

/// Cuantiza RGBA8 a ≤256 colores (media-corte + dithering Floyd–Steinberg).
fn quantize_rgba(
    rgba: &[u8],
    width: usize,
    height: usize,
    dither: PngDither,
) -> (Vec<[u8; 4]>, Vec<u8>) {
    let mut hist: HashMap<[u8; 4], u32> = HashMap::new();
    for px in rgba.chunks_exact(4) {
        *hist.entry([px[0], px[1], px[2], px[3]]).or_default() += 1;
    }

    // ≤256 colores: paleta exacta (sin pérdida), sin importar el dithering.
    if hist.len() <= 256 {
        let mut palette: Vec<[u8; 4]> = hist.keys().copied().collect();
        palette.sort_unstable();
        let lookup: HashMap<[u8; 4], u8> = palette
            .iter()
            .enumerate()
            .map(|(i, c)| (*c, i as u8))
            .collect();
        let indices = rgba
            .chunks_exact(4)
            .map(|px| lookup[&[px[0], px[1], px[2], px[3]]])
            .collect();
        return (palette, indices);
    }

    let mut palette = median_cut(&hist, 256);
    if dither == PngDither::High {
        palette = refine_palette(&palette, rgba);
    }
    let indices = if dither == PngDither::Low {
        nearest_indices(rgba, &palette)
    } else {
        floyd_steinberg(rgba, width, height, &palette)
    };
    (palette, indices)
}

/// Median-cut de Heckbert: parte las cajas por su canal de mayor rango
/// hasta obtener `max_boxes` colores (medias ponderadas por población).
fn median_cut(hist: &HashMap<[u8; 4], u32>, max_boxes: usize) -> Vec<[u8; 4]> {
    let points: Vec<([u8; 4], u32)> = hist.iter().map(|(&c, &n)| (c, n)).collect();
    let mut boxes: Vec<Vec<([u8; 4], u32)>> = vec![points];

    while boxes.len() < max_boxes {
        // Caja que se pueda partir con mayor rango en algún canal.
        let mut best: Option<(usize, usize)> = None;
        let mut best_range = 0u32;
        for (i, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            for ch in 0..4 {
                let (mut lo, mut hi) = (255u32, 0u32);
                for (c, _) in b {
                    lo = lo.min(c[ch] as u32);
                    hi = hi.max(c[ch] as u32);
                }
                let range = hi - lo;
                if range > best_range {
                    best_range = range;
                    best = Some((i, ch));
                }
            }
        }
        let Some((i, ch)) = best else { break };

        let mut b = std::mem::take(&mut boxes[i]);
        b.sort_unstable_by_key(|(c, _)| c[ch]);
        let total: u64 = b.iter().map(|(_, n)| *n as u64).sum();
        let half = total.div_ceil(2);
        let mut acc = 0u64;
        let mut cut = b.len() / 2;
        for (idx, (_, n)) in b.iter().enumerate() {
            acc += *n as u64;
            if acc >= half {
                cut = (idx + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let right = b.split_off(cut);
        boxes[i] = b;
        boxes.push(right);
    }

    boxes.iter().map(|b| weighted_mean(b)).collect()
}

/// Media ponderada RGBA de una caja de median-cut.
fn weighted_mean(points: &[([u8; 4], u32)]) -> [u8; 4] {
    let total: u64 = points.iter().map(|(_, n)| *n as u64).sum::<u64>().max(1);
    let mut sums = [0u64; 4];
    for (c, n) in points {
        for ch in 0..4 {
            sums[ch] += c[ch] as u64 * *n as u64;
        }
    }
    let mut out = [0u8; 4];
    for ch in 0..4 {
        out[ch] = ((sums[ch] + total / 2) / total) as u8;
    }
    out
}

/// Índices de paleta por distancia mínima, con caché por color único.
fn nearest_indices(rgba: &[u8], palette: &[[u8; 4]]) -> Vec<u8> {
    let mut cache: HashMap<[u8; 4], u8> = HashMap::new();
    rgba.chunks_exact(4)
        .map(|px| {
            let key = [px[0], px[1], px[2], px[3]];
            if let Some(&i) = cache.get(&key) {
                return i;
            }
            let i = nearest_palette(&key, palette);
            cache.insert(key, i);
            i
        })
        .collect()
}

/// Color de paleta más próximo (distancia euclídea en RGBA).
fn nearest_palette(key: &[u8; 4], palette: &[[u8; 4]]) -> u8 {
    let mut best = 0usize;
    let mut best_dist = u32::MAX;
    for (i, p) in palette.iter().enumerate() {
        let mut dist = 0u32;
        for ch in 0..4 {
            let d = key[ch] as i32 - p[ch] as i32;
            dist += (d * d) as u32;
        }
        if dist < best_dist {
            best_dist = dist;
            best = i;
            if dist == 0 {
                break;
            }
        }
    }
    best as u8
}

/// Floyd–Steinberg con barrido serpentina (error de RGB difundido; el alfa
/// se elige sin difundirlo).
fn floyd_steinberg(rgba: &[u8], width: usize, height: usize, palette: &[[u8; 4]]) -> Vec<u8> {
    fn add_err(
        work: &mut [[f32; 4]],
        width: usize,
        height: usize,
        x: isize,
        y: usize,
        err: &[f32; 3],
        weight: f32,
    ) {
        if x < 0 || x as usize >= width || y >= height {
            return;
        }
        let i = y * width + x as usize;
        for c in 0..3 {
            work[i][c] += err[c] * weight;
        }
    }

    let mut work: Vec<[f32; 4]> = rgba
        .chunks_exact(4)
        .map(|px| [px[0] as f32, px[1] as f32, px[2] as f32, px[3] as f32])
        .collect();
    let mut out = vec![0u8; width * height];
    let mut cache: HashMap<[u8; 4], u8> = HashMap::new();
    let clamp = |v: f32| v.round().clamp(0.0, 255.0) as u8;

    for y in 0..height {
        let left_to_right = y % 2 == 0;
        let dir: isize = if left_to_right { 1 } else { -1 };
        for i in 0..width {
            let x = if left_to_right { i } else { width - 1 - i };
            let idx = y * width + x;
            let cur = [
                clamp(work[idx][0]),
                clamp(work[idx][1]),
                clamp(work[idx][2]),
                clamp(work[idx][3]),
            ];
            let pi = match cache.get(&cur) {
                Some(&v) => v,
                None => {
                    let v = nearest_palette(&cur, palette);
                    cache.insert(cur, v);
                    v
                }
            };
            out[idx] = pi;
            let p = palette[pi as usize];
            let err = [
                cur[0] as f32 - p[0] as f32,
                cur[1] as f32 - p[1] as f32,
                cur[2] as f32 - p[2] as f32,
            ];
            let xi = x as isize;
            add_err(&mut work, width, height, xi + dir, y, &err, 7.0 / 16.0);
            add_err(&mut work, width, height, xi - dir, y + 1, &err, 3.0 / 16.0);
            add_err(&mut work, width, height, xi, y + 1, &err, 5.0 / 16.0);
            add_err(&mut work, width, height, xi + dir, y + 1, &err, 1.0 / 16.0);
        }
    }
    out
}

/// Refinamiento de paleta (PngQuant High): reasigna los píxeles con
/// *nearest* y reemplaza cada color por la media de su clúster.
fn refine_palette(palette: &[[u8; 4]], rgba: &[u8]) -> Vec<[u8; 4]> {
    let mut sums = vec![[0u64; 4]; palette.len()];
    let mut counts = vec![0u64; palette.len()];
    let mut cache: HashMap<[u8; 4], u8> = HashMap::new();
    for px in rgba.chunks_exact(4) {
        let key = [px[0], px[1], px[2], px[3]];
        let i = match cache.get(&key) {
            Some(&v) => v,
            None => {
                let v = nearest_palette(&key, palette);
                cache.insert(key, v);
                v
            }
        };
        let slot = &mut sums[i as usize];
        for ch in 0..4 {
            slot[ch] += key[ch] as u64;
        }
        counts[i as usize] += 1;
    }
    palette
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if counts[i] == 0 {
                return *p;
            }
            let total = counts[i];
            let mut out = [0u8; 4];
            for ch in 0..4 {
                out[ch] = ((sums[i][ch] + total / 2) / total) as u8;
            }
            out
        })
        .collect()
}

/// JPG lossy (`--jpg-quality`, 0-100). Sin canal
/// alfa: compone sobre negro.
fn encode_jpg(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    use image::ExtendedColorType as Ct;
    if data.len() != width * height * color_channels(color) {
        return Err(TpError::Other(
            "Dimensiones de imagen inválidas para JPG".to_string(),
        ));
    }
    let mut buf = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, opts.jpg_quality);
    let result: std::result::Result<(), image::ImageError> = match color {
        Ct::L8 => enc.encode(data, width as u32, height as u32, Ct::L8),
        Ct::Rgb8 => enc.encode(data, width as u32, height as u32, Ct::Rgb8),
        Ct::La8 => {
            let gray: Vec<u8> = data.chunks_exact(2).map(|p| p[0]).collect();
            enc.encode(&gray, width as u32, height as u32, Ct::L8)
        }
        Ct::Rgba8 => {
            let mut rgb = Vec::with_capacity(data.len() / 4 * 3);
            for px in data.chunks_exact(4) {
                rgb.extend_from_slice(&composite_black(px));
            }
            enc.encode(&rgb, width as u32, height as u32, Ct::Rgb8)
        }
        _ => {
            return Err(TpError::Other(
                "Formato de píxel no compatible con JPG".to_string(),
            ))
        }
    };
    result.map_err(|e| TpError::Other(format!("Error codificando JPG: {e}")))?;
    Ok(buf)
}

/// WebP: sin pérdidas por defecto (calidad ≥ 101) o lossy 0-100
/// (`--webp-quality`).
fn encode_webp(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    use image::ExtendedColorType as Ct;
    if data.len() != width * height * color_channels(color) {
        return Err(TpError::Other(
            "Dimensiones de imagen inválidas para WebP".to_string(),
        ));
    }
    if opts.webp_quality >= 101 {
        return encode_webp_lossless(&expand_to_rgba(data, color), width, height);
    }
    // WebP lossy solo admite RGB/RGBA (libwebp).
    let rgba = match color {
        Ct::Rgba8 => data.to_vec(),
        _ => expand_to_rgba(data, color),
    };
    let encoder = webp::Encoder::from_rgba(&rgba, width as u32, height as u32);
    let memory = encoder.encode(opts.webp_quality as f32);
    Ok(memory.to_vec())
}

/// Voltea verticalmente un buffer RGBA (`--flip-y`; solo formatos de
/// hardware — las coordenadas de los frames no cambian).
pub fn flip_vertical_rgba(rgba: &mut [u8], width: usize, height: usize) {
    let stride = width * 4;
    if stride == 0 || rgba.len() < stride * height {
        return;
    }
    for y in 0..height / 2 {
        let top = y * stride;
        let bottom = (height - 1 - y) * stride;
        for i in 0..stride {
            rgba.swap(top + i, bottom + i);
        }
    }
}

fn encode_webp_lossless(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut buf)
        .encode(
            rgba,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| TpError::Other(format!("Error codificando WebP: {e}")))?;
    Ok(buf)
}

/// ASTC 4x4: ARM astcenc + `.astc` container.
fn encode_astc(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    #[cfg(feature = "gpu-formats")]
    {
        use astcenc_rs::{ConfigBuilder, Context, Extents, Image, Profile, Swizzle, PRESET_MEDIUM};
        let cfg = ConfigBuilder::new()
            .with_block_size(Extents::new(4, 4))
            .with_preset(PRESET_MEDIUM)
            .with_profile(Profile::LdrRgba)
            .build()
            .map_err(|e| TpError::Other(format!("Config ASTC inválida: {e:?}")))?;
        let mut ctx =
            Context::new(cfg).map_err(|e| TpError::Other(format!("Contexto ASTC: {e:?}")))?;
        let img = Image {
            extents: Extents::new(width as u32, height as u32),
            data: &[rgba][..],
        };
        let blocks = ctx
            .compress(&img, Swizzle::rgba())
            .map_err(|e| TpError::Other(format!("Error comprimiendo ASTC: {e:?}")))?;

        // .astc header (16 bytes): magic, block dims, x/y/z size (24-bit LE).
        let mut out = Vec::with_capacity(16 + blocks.len());
        out.extend_from_slice(&[0x13, 0xAB, 0xA1, 0x5C]); // magic 0x5CA1AB13
        out.push(4); // block_x
        out.push(4); // block_y
        out.push(1); // block_z
        out.extend_from_slice(&(width as u32).to_le_bytes()[..3]);
        out.extend_from_slice(&(height as u32).to_le_bytes()[..3]);
        out.extend_from_slice(&1u32.to_le_bytes()[..3]); // zsize
        out.extend_from_slice(&blocks);
        Ok(out)
    }
    #[cfg(not(feature = "gpu-formats"))]
    {
        let _ = (rgba, width, height);
        Err(TpError::Other(
            "ASTC_4x4 requiere compilar con la feature `gpu-formats` \
             (cargo build --features gpu-formats)"
                .to_string(),
        ))
    }
}

/// PVRTC1 4bpp in a PVR v3 container (`.pvr`).
fn encode_pvrtc_pvr(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let blocks = pvrtc::encode_pvrtc_4bpp(rgba, width, height)?;
    let mut out = Vec::with_capacity(52 + blocks.len());
    // PVR v3 header (52 bytes), all little-endian.
    out.extend_from_slice(b"PVR\x03"); // version
    out.extend_from_slice(&0u32.to_le_bytes()); // flags
    out.extend_from_slice(&0u64.to_le_bytes()); // pixel_format: PVRTC1 4bpp RGBA
    out.extend_from_slice(&0u32.to_le_bytes()); // colour_space: linearRGB
    out.extend_from_slice(&0u32.to_le_bytes()); // channel_type: unsigned byte
    out.extend_from_slice(&(height as u32).to_le_bytes());
    out.extend_from_slice(&(width as u32).to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // depth
    out.extend_from_slice(&1u32.to_le_bytes()); // num_surfaces
    out.extend_from_slice(&1u32.to_le_bytes()); // num_faces
    out.extend_from_slice(&1u32.to_le_bytes()); // mip_map_count
    out.extend_from_slice(&0u32.to_le_bytes()); // meta_data_size
    out.extend_from_slice(&blocks);
    Ok(out)
}

/// ETC2 RGBA8 in a KTX container.
fn encode_etc2_ktx(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let blocks = etc2::encode_etc2_rgba8(rgba, width, height);
    // GL_COMPRESSED_RGBA8_ETC2_EAC sobre GL_RGBA.
    ktx1_container(0, 0, 0x9278, 0x1908, width, height, &blocks)
}

/// ETC1 RGB in a PKM container (`.pkm`), the format PowerVR/GLES tools expect.
fn encode_etc1_pkm(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let blocks = etc2::encode_etc1_rgb(rgba, width, height);
    let ext_w = width.next_multiple_of(4);
    let ext_h = height.next_multiple_of(4);
    let mut out = Vec::with_capacity(16 + blocks.len());
    out.extend_from_slice(b"PKM 10"); // magic + version "10"
    out.extend_from_slice(&0u16.to_be_bytes()); // dataFormat: ETC1
    out.extend_from_slice(&(ext_w as u16).to_be_bytes());
    out.extend_from_slice(&(ext_h as u16).to_be_bytes());
    out.extend_from_slice(&(width as u16).to_be_bytes());
    out.extend_from_slice(&(height as u16).to_be_bytes());
    out.extend_from_slice(&blocks);
    out
}

/// ETC1 RGB in a KTX container (`.ktx`).
fn encode_etc1_ktx(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let blocks = etc2::encode_etc1_rgb(rgba, width, height);
    // GL_ETC1_RGB8_OES sobre GL_RGB.
    ktx1_container(0, 0, 0x8D60, 0x1907, width, height, &blocks)
}

/// KTX v1.1 container with a single mip level. `gl_type`/`gl_format` are 0
/// for compressed payloads, as the spec requires.
fn ktx1_container(
    gl_type: u32,
    gl_format: u32,
    internal_format: u32,
    base_internal_format: u32,
    width: usize,
    height: usize,
    payload: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + 4 + payload.len());
    out.extend_from_slice(b"\xABKTX 11\xBB\r\n\x1A\n");
    out.extend_from_slice(&0x04030201u32.to_le_bytes()); // endianness marker
    out.extend_from_slice(&gl_type.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // glTypeSize
    out.extend_from_slice(&gl_format.to_le_bytes());
    out.extend_from_slice(&internal_format.to_le_bytes());
    out.extend_from_slice(&base_internal_format.to_le_bytes());
    out.extend_from_slice(&(width as u32).to_le_bytes()); // pixelWidth
    out.extend_from_slice(&(height as u32).to_le_bytes()); // pixelHeight
    out.extend_from_slice(&0u32.to_le_bytes()); // pixelDepth
    out.extend_from_slice(&0u32.to_le_bytes()); // numberOfArrayElements
    out.extend_from_slice(&1u32.to_le_bytes()); // numberOfFaces
    out.extend_from_slice(&1u32.to_le_bytes()); // numberOfMipmapLevels
    out.extend_from_slice(&0u32.to_le_bytes()); // bytesOfKeyValueData
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // imageSize
    out.extend_from_slice(payload);
    // KTX padi cada nivel a un múltiplo de 4 bytes.
    let pad = (4 - (payload.len() % 4)) % 4;
    out.extend(std::iter::repeat_n(0u8, pad));
    out
}

/// Uncompressed KTX used as the payload of `.zktx`.
fn ktx1_rgba8(
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    data: &[u8],
) -> Vec<u8> {
    use image::ExtendedColorType as Ct;
    let (gl_format, internal, base) = match color {
        Ct::Rgb8 => (0x1907, 0x8051, 0x1907), // GL_RGB8
        Ct::L8 => (0x1909, 0x1909, 0x1909),   // GL_LUMINANCE
        Ct::La8 => (0x190A, 0x190A, 0x190A),  // GL_LUMINANCE_ALPHA
        _ => (0x1908, 0x8058, 0x1908),        // GL_RGBA8
    };
    ktx1_container(0x1401, gl_format, internal, base, width, height, data)
}

/// BMP/TGA/TIFF sin comprimir, con la conversión de pixel format ya aplicada.
fn encode_image_format(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    format: GpuFormat,
) -> Result<Vec<u8>> {
    use image::ImageEncoder;
    // Cursor: TIFF exige Write + Seek.
    let mut buf = std::io::Cursor::new(Vec::new());
    let name = format.as_str();
    let res = match format {
        GpuFormat::Bmp => image::codecs::bmp::BmpEncoder::new(&mut buf).write_image(
            data,
            width as u32,
            height as u32,
            color,
        ),
        GpuFormat::Tga => image::codecs::tga::TgaEncoder::new(&mut buf).write_image(
            data,
            width as u32,
            height as u32,
            color,
        ),
        GpuFormat::Tiff => image::codecs::tiff::TiffEncoder::new(&mut buf).write_image(
            data,
            width as u32,
            height as u32,
            color,
        ),
        _ => return Err(TpError::Other(format!("{name} no es un formato de imagen"))),
    };
    res.map_err(|e| TpError::Other(format!("Error codificando {name}: {e}")))?;
    Ok(buf.into_inner())
}

/// DDS sin comprimir: cabecera legacy de 124 bytes + píxeles. Las máscaras
/// de canal describen el orden real de los bytes, así que `RGBA8888` viaja
/// como R,G,B,A en memoria y `RGB888` como 24 bits con la misma disposición.
fn encode_dds(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
) -> Result<Vec<u8>> {
    use image::ExtendedColorType as Ct;
    // La8 no tiene equivalente directo: se expande a RGBA conservando el alfa.
    let (pixels, bit_count, flags, masks): (Vec<u8>, u32, u32, [u32; 4]) = match color {
        Ct::Rgba8 => (
            data.to_vec(),
            32u32,
            0x41u32,
            [0x0000_00ff, 0x0000_ff00, 0x00ff_0000, 0xff00_0000],
        ),
        Ct::Rgb8 => (
            data.to_vec(),
            24,
            0x40,
            [0x0000_00ff, 0x0000_ff00, 0x00ff_0000, 0],
        ),
        Ct::L8 => (data.to_vec(), 8, 0x2_0000, [0x0000_00ff, 0, 0, 0]),
        Ct::La8 => {
            let mut out = Vec::with_capacity(data.len() / 2 * 4);
            for px in data.chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            (
                out,
                32,
                0x41,
                [0x0000_00ff, 0x0000_ff00, 0x00ff_0000, 0xff00_0000],
            )
        }
        _ => {
            return Err(TpError::Other(format!(
                "DDS no admite el formato de píxel resultante ({color:?})"
            )))
        }
    };
    let stride = (bit_count / 8) as usize;
    if data.len() != width * height * stride {
        return Err(TpError::Other(
            "Tamaño de imagen inconsistente al codificar DDS".to_string(),
        ));
    }

    let mut out = Vec::with_capacity(128 + pixels.len());
    out.extend_from_slice(b"DDS ");
    out.extend_from_slice(&124u32.to_le_bytes()); // dwSize
                                                  // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
    out.extend_from_slice(&0x0000_100Fu32.to_le_bytes());
    out.extend_from_slice(&(height as u32).to_le_bytes());
    out.extend_from_slice(&(width as u32).to_le_bytes());
    out.extend_from_slice(&((width * stride) as u32).to_le_bytes()); // pitch
    out.extend_from_slice(&0u32.to_le_bytes()); // dwDepth
    out.extend_from_slice(&0u32.to_le_bytes()); // dwMipMapCount
    for _ in 0..11 {
        out.extend_from_slice(&0u32.to_le_bytes()); // dwReserved1
    }
    // DDS_PIXELFORMAT (32 bytes)
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // dwFourCC
    out.extend_from_slice(&bit_count.to_le_bytes());
    for m in masks {
        out.extend_from_slice(&m.to_le_bytes());
    }
    out.extend_from_slice(&0x0000_1000u32.to_le_bytes()); // dwCaps: TEXTURE
    out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps2
    out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps3
    out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps4
    out.extend_from_slice(&0u32.to_le_bytes()); // dwReserved2
    out.extend_from_slice(&pixels);
    Ok(out)
}

/// gzip del fichero completo (`.pvr.gz`).
fn gzip_bytes(data: Vec<u8>) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(&data);
    enc.finish().unwrap_or_default()
}

/// zlib del contenido (`.zktx`).
fn zlib_bytes(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

/// Contenedor CCZ de Cocos2D: cabecera de 16 bytes (campos en big-endian)
/// + el PVR v3 en zlib. `compression_type` 0 = zlib.
fn ccz_bytes(data: Vec<u8>) -> Vec<u8> {
    let payload = zlib_bytes(&data);
    let mut out = Vec::with_capacity(16 + payload.len());
    out.extend_from_slice(b"CCZ!");
    out.extend_from_slice(&0u16.to_be_bytes()); // compression_type: zlib
    out.extend_from_slice(&0u16.to_be_bytes()); // version
    out.extend_from_slice(&0u32.to_be_bytes()); // reserved
    out.extend_from_slice(&(data.len() as u32).to_be_bytes()); // len
    out.extend_from_slice(&payload);
    out
}

// ---------------------------------------------------------------------------
// Encryption (AES-256-GCM)
// ---------------------------------------------------------------------------

/// Header magic for encrypted texture files.
pub const ENC_MAGIC: &[u8; 6] = b"TPENC1";

/// Derive a 32-byte AES key from a passphrase (SHA-256).
pub fn derive_key(passphrase: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(passphrase.as_bytes());
    hasher.finalize().into()
}

/// Encrypt `plaintext` with AES-256-GCM. Output layout:
/// `TPENC1` (6) + nonce (12) + ciphertext+tag.
pub fn encrypt_bytes(plaintext: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use rand::RngCore;

    let key = derive_key(passphrase);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| TpError::Other(format!("Clave inválida: {e}")))?;
    let mut nonce = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce);

    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|e| TpError::Other(format!("Error cifrando: {e}")))?;

    let mut out = Vec::with_capacity(6 + 12 + ct.len());
    out.extend_from_slice(ENC_MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt a file produced by [`encrypt_bytes`]. Returns the plaintext.
pub fn decrypt_bytes(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    if data.len() < 6 + 12 {
        return Err(TpError::Other(
            "Archivo cifrado demasiado corto".to_string(),
        ));
    }
    if &data[..6] != ENC_MAGIC {
        return Err(TpError::Other(
            "No es un archivo cifrado TexturePacker-RS (falta cabecera TPENC1)".to_string(),
        ));
    }
    let key = derive_key(passphrase);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| TpError::Other(format!("Clave inválida: {e}")))?;
    let nonce = &data[6..18];
    let ct = &data[18..];
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| TpError::Other("Error descifrando (¿clave incorrecta?)".to_string()))
}

/// Decode a decrypted texture file into a viewable PNG for preview.
///
/// The atlas files generated with a `pixel_format` other than `RGBA8888`
/// store the channels in that same order (p. ej. `BGRA8888` swaps R and B),
/// so a normal viewer would show the colors wrong. This helper decodes the
/// image, re-applies [`apply_pixel_format`] — which restores the natural RGBA8
/// order for the involutive formats (channel swaps and bit-replicated
/// quantizations) — and re-encodes a regular RGBA8 PNG.
///
/// The grayscale formats (`Alpha8`, `Intensity8`, `AlphaIntensity8`) are the
/// exception: their file already stores exactly the visible channel, and the
/// conversion is not involutive (re-applying it would alter the values), so
/// the decoded image is re-encoded as-is.
pub fn decode_texture_preview_png(data: &[u8], pixel_format: PixelFormat) -> Result<Vec<u8>> {
    let img = image::load_from_memory(data)?;
    let (width, height) = (img.width() as usize, img.height() as usize);

    let needs_conversion = !matches!(
        pixel_format,
        PixelFormat::Alpha8 | PixelFormat::Intensity8 | PixelFormat::AlphaIntensity8
    );
    let rgba = if needs_conversion {
        apply_pixel_format(img.to_rgba8().as_raw(), pixel_format).0
    } else {
        img.to_rgba8().into_raw()
    };

    encode_png(
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
        &EncodeOptions::default(),
    )
}

// ---------------------------------------------------------------------------
// Scaling (variants)
// ---------------------------------------------------------------------------

/// Scale an RGBA8 image by `factor`.
///
/// `ScaleMode::Smooth` blends neighbouring pixels (bilinear);
/// `ScaleMode::Fast` picks the nearest source pixel (keeps hard edges).
///
/// The pixel-art modes (`Scale2x`, `Scale3x`, `Scale4x`, `Eagle`) only run at
/// their exact integer factor and never invent colors; for any other factor
/// they fall back to [`ScaleMode::Smooth`].
pub fn scale_rgba(
    rgba: &[u8],
    width: usize,
    height: usize,
    factor: f32,
    mode: crate::config::ScaleMode,
) -> (Vec<u8>, usize, usize) {
    use crate::config::ScaleMode;
    if let Some(scaled) = scale_pixel_art(rgba, width, height, factor, mode) {
        return scaled;
    }
    let nw = ((width as f32) * factor).round().max(1.0) as usize;
    let nh = ((height as f32) * factor).round().max(1.0) as usize;
    let mut out = vec![0u8; nw * nh * 4];

    let sample = |sx: f32, sy: f32, out: &mut [u8]| match mode {
        ScaleMode::Fast => {
            let x0 = sx.round().clamp(0.0, width as f32 - 1.0) as usize;
            let y0 = sy.round().clamp(0.0, height as f32 - 1.0) as usize;
            out.copy_from_slice(&rgba[(y0 * width + x0) * 4..(y0 * width + x0) * 4 + 4]);
        }
        // Smooth, and every pixel-art mode whose factor does not match: a
        // pixel-art filter only makes sense at its own integer scale.
        ScaleMode::Smooth
        | ScaleMode::Scale2x
        | ScaleMode::Scale3x
        | ScaleMode::Scale4x
        | ScaleMode::Eagle => {
            let (x0, y0) = (sx.floor().max(0.0) as usize, sy.floor().max(0.0) as usize);
            let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;
            for c in 0..4 {
                let p00 = rgba[(y0 * width + x0) * 4 + c] as f32;
                let p10 = rgba[(y0 * width + x1) * 4 + c] as f32;
                let p01 = rgba[(y1 * width + x0) * 4 + c] as f32;
                let p11 = rgba[(y1 * width + x1) * 4 + c] as f32;
                let top = p00 * (1.0 - fx) + p10 * fx;
                let bot = p01 * (1.0 - fx) + p11 * fx;
                out[c] = (top * (1.0 - fy) + bot * fy).round() as u8;
            }
        }
    };

    for y in 0..nh {
        let sy = (y as f32 + 0.5) / factor - 0.5;
        for x in 0..nw {
            let sx = (x as f32 + 0.5) / factor - 0.5;
            let dst = (y * nw + x) * 4;
            sample(sx, sy, &mut out[dst..dst + 4]);
        }
    }
    (out, nw, nh)
}

/// Read one source pixel, clamping outside coordinates to the border
/// (the scalers need a full 3x3 neighbourhood at the image edges).
fn sample_px(rgba: &[u8], width: usize, height: usize, x: i32, y: i32) -> [u8; 4] {
    let x = x.clamp(0, width as i32 - 1) as usize;
    let y = y.clamp(0, height as i32 - 1) as usize;
    let i = (y * width + x) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

fn write_px(out: &mut [u8], out_width: usize, x: usize, y: usize, c: [u8; 4]) {
    let i = (y * out_width + x) * 4;
    out[i..i + 4].copy_from_slice(&c);
}

/// Pixel-art upscalers, dispatched on the mode's exact integer factor.
/// Returns `None` when the mode is generic (`Smooth`/`Fast`) or `factor` is
/// not the factor the mode requires, so the caller falls back to Smooth.
fn scale_pixel_art(
    rgba: &[u8],
    width: usize,
    height: usize,
    factor: f32,
    mode: crate::config::ScaleMode,
) -> Option<(Vec<u8>, usize, usize)> {
    use crate::config::ScaleMode;
    if width == 0 || height == 0 {
        return None;
    }
    let wanted = mode.required_factor()?;
    if (factor - wanted as f32).abs() > 1e-6 {
        return None;
    }
    Some(match mode {
        ScaleMode::Scale2x => scale2x_rgba(rgba, width, height),
        ScaleMode::Scale3x => scale3x_rgba(rgba, width, height),
        // AdvMAME4x/Scale4x is Scale2x applied twice.
        ScaleMode::Scale4x => {
            let (mid, mid_w, mid_h) = scale2x_rgba(rgba, width, height);
            scale2x_rgba(&mid, mid_w, mid_h)
        }
        ScaleMode::Eagle => eagle2x_rgba(rgba, width, height),
        ScaleMode::Smooth | ScaleMode::Fast => return None,
    })
}

/// Scale2x / AdvMAME2x: each source pixel becomes a 2x2 block whose corners
/// borrow the colour of a diagonal neighbour when two edges line up.
///
/// ```text
/// A B C      1 2     1 = A if C==A && C!=D && A!=B
/// D E F  ->  3 4     2 = B if A==B && A!=C && B!=D
/// G H I            3 = C if D==C && D!=B && C!=A
///                  4 = D if B==D && B!=A && D!=C
/// ```
fn scale2x_rgba(rgba: &[u8], width: usize, height: usize) -> (Vec<u8>, usize, usize) {
    let out_w = width * 2;
    let out_h = height * 2;
    let mut out = vec![0u8; out_w * out_h * 4];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let a = sample_px(rgba, width, height, x - 1, y - 1);
            let b = sample_px(rgba, width, height, x, y - 1);
            let c = sample_px(rgba, width, height, x + 1, y - 1);
            let d = sample_px(rgba, width, height, x - 1, y);
            let e = sample_px(rgba, width, height, x, y);

            let p1 = if c == a && c != d && a != b { a } else { e };
            let p2 = if a == b && a != c && b != d { b } else { e };
            let p3 = if d == c && d != b && c != a { c } else { e };
            let p4 = if b == d && b != a && d != c { d } else { e };

            let (ox, oy) = (x as usize * 2, y as usize * 2);
            write_px(&mut out, out_w, ox, oy, p1);
            write_px(&mut out, out_w, ox + 1, oy, p2);
            write_px(&mut out, out_w, ox, oy + 1, p3);
            write_px(&mut out, out_w, ox + 1, oy + 1, p4);
        }
    }
    (out, out_w, out_h)
}

/// Scale3x / AdvMAME3x: generalisation of Scale2x to a 3x3 output block,
/// corners exactly as in Scale2x and edge pixels decided by the two
/// neighbouring corners of the same edge.
fn scale3x_rgba(rgba: &[u8], width: usize, height: usize) -> (Vec<u8>, usize, usize) {
    let out_w = width * 3;
    let out_h = height * 3;
    let mut out = vec![0u8; out_w * out_h * 4];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let a = sample_px(rgba, width, height, x - 1, y - 1);
            let b = sample_px(rgba, width, height, x, y - 1);
            let c = sample_px(rgba, width, height, x + 1, y - 1);
            let d = sample_px(rgba, width, height, x - 1, y);
            let e = sample_px(rgba, width, height, x, y);
            let f = sample_px(rgba, width, height, x + 1, y);
            let g = sample_px(rgba, width, height, x - 1, y + 1);
            let h = sample_px(rgba, width, height, x, y + 1);
            let i = sample_px(rgba, width, height, x + 1, y + 1);

            let db = d == b && d != h && b != f;
            let bf = b == f && b != d && f != h;
            let hd = h == d && h != f && d != b;
            let fh = f == h && f != b && h != d;

            let o1 = if db { d } else { e };
            let o2 = if (db && e != c) || (bf && e != a) {
                b
            } else {
                e
            };
            let o3 = if bf { f } else { e };
            let o4 = if (hd && e != a) || (db && e != g) {
                d
            } else {
                e
            };
            let o5 = e;
            let o6 = if (bf && e != i) || (fh && e != c) {
                f
            } else {
                e
            };
            let o7 = if hd { d } else { e };
            let o8 = if (fh && e != g) || (hd && e != i) {
                h
            } else {
                e
            };
            let o9 = if fh { f } else { e };

            let (ox, oy) = (x as usize * 3, y as usize * 3);
            let block = [o1, o2, o3, o4, o5, o6, o7, o8, o9];
            for (n, color) in block.into_iter().enumerate() {
                write_px(&mut out, out_w, ox + n % 3, oy + n / 3, color);
            }
        }
    }
    (out, out_w, out_h)
}

/// Eagle: each corner of the 2x2 block takes the colour shared by the three
/// source pixels touching that corner (they must all be equal).
///
/// ```text
/// S T U      1 2     1 = S if S==T==V
/// V E W  ->  3 4     2 = U if T==U==W
/// X Y Z              3 = X if V==X==Y
///                    4 = Z if W==Z==Y
/// ```
fn eagle2x_rgba(rgba: &[u8], width: usize, height: usize) -> (Vec<u8>, usize, usize) {
    let out_w = width * 2;
    let out_h = height * 2;
    let mut out = vec![0u8; out_w * out_h * 4];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let s = sample_px(rgba, width, height, x - 1, y - 1);
            let t = sample_px(rgba, width, height, x, y - 1);
            let u = sample_px(rgba, width, height, x + 1, y - 1);
            let v = sample_px(rgba, width, height, x - 1, y);
            let e = sample_px(rgba, width, height, x, y);
            let w = sample_px(rgba, width, height, x + 1, y);
            let xx = sample_px(rgba, width, height, x - 1, y + 1);
            let yy = sample_px(rgba, width, height, x, y + 1);
            let z = sample_px(rgba, width, height, x + 1, y + 1);

            let p1 = if v == s && s == t { s } else { e };
            let p2 = if t == u && u == w { u } else { e };
            let p3 = if v == xx && xx == yy { xx } else { e };
            let p4 = if w == z && z == yy { z } else { e };

            let (ox, oy) = (x as usize * 2, y as usize * 2);
            write_px(&mut out, out_w, ox, oy, p1);
            write_px(&mut out, out_w, ox + 1, oy, p2);
            write_px(&mut out, out_w, ox, oy + 1, p3);
            write_px(&mut out, out_w, ox + 1, oy + 1, p4);
        }
    }
    (out, out_w, out_h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PixelFormat;

    fn opts(format: GpuFormat) -> EncodeOptions {
        EncodeOptions {
            format,
            ..EncodeOptions::default()
        }
    }

    #[test]
    fn bgra8888_swaps_r_and_b() {
        let rgba = [10u8, 20, 30, 40, 200, 150, 100, 255];
        let (data, color) = apply_pixel_format(&rgba, PixelFormat::Bgra8888);
        assert_eq!(color, image::ExtendedColorType::Rgba8);
        assert_eq!(data, [30u8, 20, 10, 40, 100, 150, 200, 255]);
        // Roundtrip: re-intercambiar devuelve el original.
        let (back, _) = apply_pixel_format(&data, PixelFormat::Bgra8888);
        assert_eq!(back, rgba);
    }

    #[test]
    fn rgba5551_quantizes_to_5_bit_grid() {
        // 255 → 31 → expandido 255; 128 → 16 (≈132); 8 → 1 (≈8); 0 → 0.
        let rgba = [255u8, 128, 8, 200, 0, 0, 0, 100];
        let (data, color) = apply_pixel_format(&rgba, PixelFormat::Rgba5551);
        assert_eq!(color, image::ExtendedColorType::Rgba8);
        let expand5 = |v: u8| (v << 3) | (v >> 2);
        let q = |c: u8| ((c as u16 * 31 + 127) / 255) as u8;
        for (px, out) in rgba.chunks_exact(4).zip(data.chunks_exact(4)) {
            assert_eq!(out[0], expand5(q(px[0])));
            assert_eq!(out[1], expand5(q(px[1])));
            assert_eq!(out[2], expand5(q(px[2])));
            assert_eq!(out[3], if px[3] >= 128 { 255 } else { 0 });
        }
        // Casos exactos: blanco puro y alfa alto → opaco; alfa bajo → 0.
        assert_eq!(&data[..4], &[255, 132, 8, 255]);
        assert_eq!(&data[4..], &[0, 0, 0, 0]);
    }

    #[test]
    fn rgba5555_keeps_5_bit_alpha() {
        // A diferencia de RGBA5551, el alfa NO colapsa a 0/255: 200 →
        // q(200)=25 → expand5(25)=198; 100 → q(100)=12 → expand5(12)=99.
        let rgba = [255u8, 0, 0, 200, 0, 255, 0, 100];
        let (data, color) = apply_pixel_format(&rgba, PixelFormat::Rgba5555);
        assert_eq!(color, image::ExtendedColorType::Rgba8);
        let expand5 = |v: u8| (v << 3) | (v >> 2);
        let q = |c: u8| ((c as u16 * 31 + 127) / 255) as u8;
        assert_eq!(data[..4], [255, 0, 0, expand5(q(200))]);
        assert_eq!(data[4..], [0, 255, 0, expand5(q(100))]);
        assert_eq!(expand5(q(200)), 198);
        assert_eq!(expand5(q(100)), 99);
    }

    #[test]
    fn new_formats_roundtrip_through_png_encoder() {
        for format in [
            PixelFormat::Rgba5551,
            PixelFormat::Rgba5555,
            PixelFormat::Bgra8888,
        ] {
            let rgba: Vec<u8> = (0..16u32)
                .flat_map(|i| [(i * 16) as u8, (i * 8) as u8, 255 - i as u8, (i * 17) as u8])
                .collect();
            let (data, color) = apply_pixel_format(&rgba, format);
            let bytes = encode_png(&data, 4, 4, color, &opts(GpuFormat::Png)).unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
            assert_eq!(decoded.into_raw(), data, "{format:?} sobrevive al PNG");
        }
    }

    /// Byte del campo `color type` del IHDR de un PNG (offset 25).
    fn png_color_type(bytes: &[u8]) -> u8 {
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "no es un PNG");
        assert_eq!(&bytes[12..16], b"IHDR");
        bytes[25]
    }

    /// Imagen de prueba con 4 colores distintos (uno transparente).
    fn four_color_image() -> Vec<u8> {
        let colors = [
            [255, 0, 0, 255],
            [0, 255, 0, 128],
            [0, 0, 255, 255],
            [10, 20, 30, 0],
        ];
        let mut rgba = Vec::new();
        for y in 0..8 {
            for x in 0..8 {
                rgba.extend_from_slice(&colors[(y / 4) * 2 + x / 4]);
            }
        }
        rgba
    }

    #[test]
    fn png_roundtrip() {
        let w = 8;
        let h = 8;
        let mut rgba = vec![0u8; w * h * 4];
        for i in 0..(w * h) {
            rgba[i * 4] = (i % 256) as u8;
            rgba[i * 4 + 3] = 255;
        }
        let bytes = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Png)).unwrap();
        assert!(bytes.len() > 8);
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!(img.width(), 8);
        assert_eq!(img.height(), 8);
    }

    #[test]
    fn png_auto_index_and_opt_level_zero() {
        let rgba = four_color_image();
        // Nivel 1 (por defecto): 4 colores → PNG indexado (color type 3).
        let indexed = encode_to_bytes(&rgba, 8, 8, &opts(GpuFormat::Png)).unwrap();
        assert_eq!(png_color_type(&indexed), 3);
        // La paleta es exacta: la decodificación debe ser idéntica.
        let decoded = image::load_from_memory(&indexed).unwrap().to_rgba8();
        assert_eq!(decoded.as_raw(), &rgba);

        // Nivel 0: siempre RGBA de 32 bits sin optimizar (color type 6).
        let raw_opts = EncodeOptions {
            png_opt_level: 0,
            ..opts(GpuFormat::Png)
        };
        let raw = encode_to_bytes(&rgba, 8, 8, &raw_opts).unwrap();
        assert_eq!(png_color_type(&raw), 6);
        let decoded = image::load_from_memory(&raw).unwrap().to_rgba8();
        assert_eq!(decoded.as_raw(), &rgba);
    }

    #[test]
    fn png_opt_level7_bigger_than_level0() {
        // Gradiente (datos poco repetibles): más esfuerzo = archivo menor.
        let mut rgba = Vec::with_capacity(64 * 64 * 4);
        for y in 0..64 {
            for x in 0..64 {
                rgba.extend_from_slice(&[(x * 4) as u8, (y * 4) as u8, ((x + y) * 2) as u8, 255]);
            }
        }
        let fast = EncodeOptions {
            png_opt_level: 0,
            ..opts(GpuFormat::Png)
        };
        let best = EncodeOptions {
            png_opt_level: 7,
            ..opts(GpuFormat::Png)
        };
        let fast_bytes = encode_to_bytes(&rgba, 64, 64, &fast).unwrap();
        let best_bytes = encode_to_bytes(&rgba, 64, 64, &best).unwrap();
        assert!(
            best_bytes.len() < fast_bytes.len(),
            "nivel 7 ({}) debe comprimir mejor que nivel 0 ({})",
            best_bytes.len(),
            fast_bytes.len()
        );
        let decoded = image::load_from_memory(&best_bytes).unwrap().to_rgba8();
        assert_eq!(decoded.as_raw(), &rgba);
    }

    #[test]
    fn png8_quantizes_gradient_to_at_most_256_colors() {
        let mut rgba = Vec::with_capacity(64 * 64 * 4);
        for y in 0..64 {
            for x in 0..64 {
                rgba.extend_from_slice(&[
                    (x * 4) as u8,
                    (y * 4) as u8,
                    ((x * 3 + y) % 256) as u8,
                    255,
                ]);
            }
        }
        for dither in [PngDither::Low, PngDither::Medium, PngDither::High] {
            let o = EncodeOptions {
                png8_dither: dither,
                ..opts(GpuFormat::Png8)
            };
            let bytes = encode_to_bytes(&rgba, 64, 64, &o).unwrap();
            assert_eq!(png_color_type(&bytes), 3, "dither {dither:?}");
            let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
            assert_eq!((decoded.width(), decoded.height()), (64, 64));
            let unique: std::collections::HashSet<[u8; 4]> =
                decoded.pixels().map(|p| p.0).collect();
            assert!(
                unique.len() <= 256,
                "PNG-8 ({dither:?}) produjo {} colores",
                unique.len()
            );
        }
    }

    #[test]
    fn png8_small_image_is_exact() {
        let rgba = four_color_image();
        for dither in [PngDither::Low, PngDither::Medium, PngDither::High] {
            let o = EncodeOptions {
                png8_dither: dither,
                ..opts(GpuFormat::Png8)
            };
            let bytes = encode_to_bytes(&rgba, 8, 8, &o).unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
            assert_eq!(decoded.as_raw(), &rgba, "dither {dither:?}");
        }
    }

    #[test]
    fn jpg_solid_color_and_transparency_composited() {
        // Rojo sólido opaco.
        let red = [255u8, 0, 0, 255].repeat(16 * 16);
        let bytes = encode_to_bytes(&red, 16, 16, &opts(GpuFormat::Jpg)).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        for p in decoded.pixels() {
            let [r, g, b] = p.0;
            assert!(r.abs_diff(255) <= 10 && g <= 8 && b <= 8, "{p:?}");
        }

        // Totalmente transparente: se compone sobre negro.
        let clear = [200u8, 100, 50, 0].repeat(16 * 16);
        let bytes = encode_to_bytes(&clear, 16, 16, &opts(GpuFormat::Jpg)).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        for p in decoded.pixels() {
            let [r, g, b] = p.0;
            assert!(r <= 8 && g <= 8 && b <= 8, "{p:?}");
        }
    }

    #[test]
    fn webp_lossless_default_and_lossy_smaller() {
        // Ruido pseudoaleatorio (xorshift32): casi incompresible sin
        // pérdidas, así el lossy queda claramente por debajo.
        let mut state = 0x2545_F491u32;
        let mut noise = Vec::with_capacity(64 * 64 * 4);
        for _ in 0..64 * 64 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let r = state as u8;
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let g = state as u8;
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let b = state as u8;
            noise.extend_from_slice(&[r, g, b, 255]);
        }
        // Por defecto (≥101): sin pérdidas, decodificación exacta.
        let lossless = encode_to_bytes(&noise, 64, 64, &opts(GpuFormat::WebP)).unwrap();
        let decoded = image::load_from_memory(&lossless).unwrap().to_rgba8();
        assert_eq!(decoded.as_raw(), &noise);

        // Lossy q=10: decodificable y más pequeño que el lossless.
        let lossy_opts = EncodeOptions {
            webp_quality: 10,
            ..opts(GpuFormat::WebP)
        };
        let lossy = encode_to_bytes(&noise, 64, 64, &lossy_opts).unwrap();
        assert!(
            lossy.len() < lossless.len(),
            "{} >= {}",
            lossy.len(),
            lossless.len()
        );
        let decoded = image::load_from_memory(&lossy).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (64, 64));
    }

    #[test]
    fn pixel_formats_produce_expected_channels() {
        let rgba = [
            255, 128, 0, 255, // píxel opaco
            10, 20, 30, 0, // transparente
        ]
        .repeat(4); // 8 píxeles (4x2)
        let w = 4;
        let h = 2;

        // RGB888: sin alfa; el transparente queda en negro.
        let o = EncodeOptions {
            pixel_format: PixelFormat::Rgb888,
            png_opt_level: 0, // sin auto-indexar para mirar el color type
            ..opts(GpuFormat::Png)
        };
        let bytes = encode_to_bytes(&rgba, w, h, &o).unwrap();
        assert_eq!(png_color_type(&bytes), 2);
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(&decoded.as_raw()[..3], &[255, 128, 0]);
        // El píxel transparente (índice 1) queda en negro.
        assert_eq!(&decoded.as_raw()[3..6], &[0, 0, 0]);

        // ALPHA8: escala de grises con el valor del alfa.
        let o = EncodeOptions {
            pixel_format: PixelFormat::Alpha8,
            ..opts(GpuFormat::Png)
        };
        let bytes = encode_to_bytes(&rgba, w, h, &o).unwrap();
        assert_eq!(png_color_type(&bytes), 0);
        let decoded = image::load_from_memory(&bytes).unwrap().to_luma8();
        assert_eq!(decoded.as_raw(), &vec![255, 0, 255, 0, 255, 0, 255, 0]);

        // INTENSITY8: luminancia BT.601.
        let o = EncodeOptions {
            pixel_format: PixelFormat::Intensity8,
            ..opts(GpuFormat::Png)
        };
        let bytes = encode_to_bytes(&rgba, w, h, &o).unwrap();
        assert_eq!(png_color_type(&bytes), 0);
        let expected_luma = (77u16 * 255 + 150 * 128 + 128) >> 8;
        let decoded = image::load_from_memory(&bytes).unwrap().to_luma8();
        assert_eq!(decoded.as_raw()[0], expected_luma as u8);

        // ALPHA_INTENSITY8: grises + alfa (color type 4).
        let o = EncodeOptions {
            pixel_format: PixelFormat::AlphaIntensity8,
            ..opts(GpuFormat::Png)
        };
        let bytes = encode_to_bytes(&rgba, w, h, &o).unwrap();
        assert_eq!(png_color_type(&bytes), 4);
        let decoded = image::load_from_memory(&bytes).unwrap().to_luma_alpha8();
        assert_eq!(&decoded.as_raw()[..2], &[expected_luma as u8, 255]);
        assert_eq!(
            &decoded.as_raw()[2..4],
            &[
                /* luma de (10,20,30) */
                { ((77u16 * 10 + 150 * 20 + 29 * 30 + 128) >> 8) as u8 },
                0
            ]
        );
    }

    #[test]
    fn flip_vertical_helper_reverses_rows() {
        // 2x2 RGBA: fila 0 rojo, fila 1 azul.
        let mut rgba = vec![
            255, 0, 0, 255, 255, 0, 0, 255, // fila 0
            0, 0, 255, 255, 0, 0, 255, 255, // fila 1
        ];
        flip_vertical_rgba(&mut rgba, 2, 2);
        assert_eq!(&rgba[..8], &[0, 0, 255, 255, 0, 0, 255, 255]);
        assert_eq!(&rgba[8..], &[255, 0, 0, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn etc2_ktx_header() {
        let bytes = encode_etc2_ktx(&vec![0u8; 8 * 8 * 4], 8, 8);
        assert_eq!(&bytes[..12], b"\xABKTX 11\xBB\r\n\x1A\n");
        // internal format at offset 28 (12 magic + 4*4 header fields)
        let internal = u32::from_le_bytes(bytes[28..32].try_into().unwrap());
        assert_eq!(internal, 0x9278);
        assert_eq!(bytes.len(), 64 + 4 + 4 * 16);
    }

    #[test]
    fn pvrtc_pvr_file() {
        let rgba = vec![255u8; 8 * 8 * 4];
        let bytes = encode_to_bytes(&rgba, 8, 8, &opts(GpuFormat::Pvrtc4Bpp)).unwrap();
        assert_eq!(&bytes[..4], b"PVR\x03");
        assert_eq!(bytes.len(), 52 + 8 * 8 / 2);
        let payload = &bytes[52..];
        let mut buf = vec![0u32; 8 * 8];
        texture2ddecoder::decode_pvrtc_4bpp(payload, 8, 8, &mut buf).unwrap();
        // Non-power-of-two sizes error clearly (e.g. odd scale variants).
        assert!(encode_to_bytes(&rgba, 96, 96, &opts(GpuFormat::Pvrtc4Bpp)).is_err());
    }

    #[test]
    fn encryption_roundtrip() {
        let plain = b"hello texture world".to_vec();
        let enc = encrypt_bytes(&plain, "secret123").unwrap();
        assert_eq!(&enc[..6], ENC_MAGIC);
        let dec = decrypt_bytes(&enc, "secret123").unwrap();
        assert_eq!(dec, plain);
        assert!(decrypt_bytes(&enc, "wrong").is_err());
        assert!(decrypt_bytes(b"garbage", "x").is_err());
    }

    #[test]
    fn decrypt_preview_restores_bgra_channels() {
        // Atlas publicado con pixel_format BGRA8888: el archivo lleva R y B
        // intercambiados; el preview debe devolver el orden natural RGBA.
        let bgra = vec![255u8, 0, 0, 255, 0, 0, 255, 255]; // 2 px: azul, rojo
        let png = encode_to_bytes(&bgra, 2, 1, &opts(GpuFormat::Png)).unwrap();
        let enc = encrypt_bytes(&png, "clave").unwrap();
        let plain = decrypt_bytes(&enc, "clave").unwrap();

        let preview = decode_texture_preview_png(&plain, PixelFormat::Bgra8888).unwrap();
        let rgba = image::load_from_memory(&preview).unwrap().to_rgba8();
        assert_eq!(rgba.as_raw(), &[0, 0, 255, 255, 255, 0, 0, 255]);

        // Sin la conversión, los canales seguirían intercambiados.
        let sin_conversion = image::load_from_memory(&plain).unwrap().to_rgba8();
        assert_ne!(rgba.as_raw(), sin_conversion.as_raw());
    }

    #[test]
    fn decrypt_preview_keeps_alpha8_as_grayscale() {
        // Alpha8 no es involutivo: el archivo ya guarda el canal visible (el
        // nivel de alfa en escala de grises) y el preview lo respeta tal cual.
        let rgba = vec![10u8, 20, 30, 77, 40, 50, 60, 200]; // 2 px
        let png = encode_to_bytes(
            &rgba,
            2,
            1,
            &EncodeOptions {
                pixel_format: PixelFormat::Alpha8,
                ..opts(GpuFormat::Png)
            },
        )
        .unwrap();
        let preview = decode_texture_preview_png(&png, PixelFormat::Alpha8).unwrap();
        let img = image::load_from_memory(&preview).unwrap().to_rgba8();
        assert_eq!(img.as_raw(), &[77, 77, 77, 255, 200, 200, 200, 255]);
    }

    #[test]
    fn scaling_halves_size() {
        let rgba = vec![255u8; 16 * 16 * 4];
        let (out, w, h) = scale_rgba(&rgba, 16, 16, 0.5, crate::config::ScaleMode::Smooth);
        assert_eq!((w, h), (8, 8));
        assert!(out.iter().all(|&v| v == 255));
    }

    #[test]
    fn scale_mode_fast_keeps_hard_edges_and_smooth_blends() {
        // 2x2: mitad izquierda rojo, mitad derecha azul.
        let rgba = vec![
            255, 0, 0, 255, 0, 0, 255, 255, // fila 0
            255, 0, 0, 255, 0, 0, 255, 255, // fila 1
        ];
        let (fast, _, _) = scale_rgba(&rgba, 2, 2, 0.5, crate::config::ScaleMode::Fast);
        // 1x1 con nearest: elige un píxel fuente tal cual (sin mezclar).
        let is_source_color = fast[0..3] == [255, 0, 0] || fast[0..3] == [0, 0, 255];
        assert!(
            is_source_color,
            "nearest debe copiar un píxel exacto: {:?}",
            &fast[0..3]
        );
        let (smooth, _, _) = scale_rgba(&rgba, 2, 2, 0.5, crate::config::ScaleMode::Smooth);
        assert_ne!(&smooth[0..3], &[255, 0, 0]);
        assert_ne!(&smooth[0..3], &[0, 0, 255]);

        // Ampliar a 4x4 con cada modo y comprobar el píxel de transición.
        let (smooth4, w, h) = scale_rgba(&rgba, 2, 2, 2.0, crate::config::ScaleMode::Smooth);
        assert_eq!((w, h), (4, 4));
        let (fast4, _, _) = scale_rgba(&rgba, 2, 2, 2.0, crate::config::ScaleMode::Fast);
        let at = |buf: &[u8], x: usize, y: usize| -> Vec<u8> {
            buf[(y * 4 + x) * 4..(y * 4 + x) * 4 + 3].to_vec()
        };
        // Nearest conserva el corte seco entre rojo y azul…
        assert_eq!(at(&fast4, 1, 0), vec![255, 0, 0]);
        assert_eq!(at(&fast4, 2, 0), vec![0, 0, 255]);
        // …mientras Smooth interpola (no es rojo ni azul puro).
        let mid = at(&smooth4, 2, 0);
        assert_ne!(mid, vec![255, 0, 0]);
        assert_ne!(mid, vec![0, 0, 255]);
        assert!(mid[0] > 0 && mid[2] > 0, "se esperaba una mezcla: {mid:?}");
    }

    // --- Modos de escalado de pixel art (Scale2x/Scale3x/Scale4x/Eagle) ---

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn img(rows: &[[[u8; 4]; 3]]) -> Vec<u8> {
        rows.iter()
            .flatten()
            .flat_map(|px| px.iter().copied())
            .collect()
    }

    fn at(buf: &[u8], stride: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * stride + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn scale2x_corner_takes_the_diagonal_colour() {
        // A(NW)=rojo B(N)=azul C(NE)=rojo | D(W)=azul E=verde F=blanco |
        // G=blanco H(S)=blanco I=blanco
        let src = img(&[
            [RED, BLUE, RED],
            [BLUE, GREEN, WHITE],
            [WHITE, WHITE, WHITE],
        ]);
        let (out, w, h) = scale_rgba(&src, 3, 3, 2.0, crate::config::ScaleMode::Scale2x);
        assert_eq!((w, h), (6, 6));
        // Bloque 2x2 de E=(1,1) -> dest (2..3, 2..3).
        assert_eq!(at(&out, 6, 2, 2), RED, "1 toma A (diagonal superior-izq.)");
        assert_eq!(at(&out, 6, 3, 2), GREEN, "2 conserva E");
        assert_eq!(at(&out, 6, 2, 3), GREEN, "3 conserva E");
        assert_eq!(at(&out, 6, 3, 3), BLUE, "4 toma D (diagonal inferior-izq.)");
        // Nearest habría puesto el verde de E en las cuatro esquinas.
        let (fast, _, _) = scale_rgba(&src, 3, 3, 2.0, crate::config::ScaleMode::Fast);
        assert_eq!(at(&fast, 6, 2, 2), GREEN);
    }

    #[test]
    fn scale3x_edges_follow_the_corner_rules() {
        // B(N)=rojo D(W)=rojo H(S)=azul F(E)=azul, centro verde.
        let src = img(&[
            [WHITE, RED, WHITE],
            [RED, GREEN, BLUE],
            [WHITE, BLUE, WHITE],
        ]);
        let (out, w, h) = scale_rgba(&src, 3, 3, 3.0, crate::config::ScaleMode::Scale3x);
        assert_eq!((w, h), (9, 9));
        // Bloque 3x3 de E -> dest (3..5, 3..5).
        assert_eq!(at(&out, 9, 3, 3), RED, "1 toma D (esquina superior-izq.)");
        assert_eq!(at(&out, 9, 4, 4), GREEN, "5 sigue siendo E");
        // El borde 2 (arriba) toma B cuando D==B (y E != C): aquí sí se cumple.
        assert_eq!(at(&out, 9, 4, 3), RED, "2 toma B (D==B con E!=C)");
    }

    #[test]
    fn eagle_corner_takes_the_three_equal_neighbours() {
        // S=T=V=rojo: la esquina superior-izquierda pasa a rojo.
        let src = img(&[
            [RED, RED, WHITE],
            [RED, GREEN, WHITE],
            [WHITE, WHITE, WHITE],
        ]);
        let (out, w, h) = scale_rgba(&src, 3, 3, 2.0, crate::config::ScaleMode::Eagle);
        assert_eq!((w, h), (6, 6));
        assert_eq!(at(&out, 6, 2, 2), RED, "1 toma S (S==T==V)");
        assert_eq!(at(&out, 6, 3, 2), GREEN, "2 conserva E");
    }

    #[test]
    fn pixel_art_scalers_keep_the_palette_and_the_right_size() {
        const PALETTE: [[u8; 4]; 4] = [RED, GREEN, BLUE, WHITE];
        let mut src = Vec::new();
        for y in 0..5 {
            for x in 0..5 {
                src.extend_from_slice(&PALETTE[(x * 7 + y * 3) % 4]);
            }
        }
        for (mode, factor, mult) in [
            (crate::config::ScaleMode::Scale2x, 2.0, 2),
            (crate::config::ScaleMode::Scale3x, 3.0, 3),
            (crate::config::ScaleMode::Scale4x, 4.0, 4),
            (crate::config::ScaleMode::Eagle, 2.0, 2),
        ] {
            let (out, w, h) = scale_rgba(&src, 5, 5, factor, mode);
            assert_eq!((w, h), (5 * mult, 5 * mult), "{mode:?}");
            for px in out.chunks_exact(4) {
                assert!(
                    PALETTE.iter().any(|p| p.as_slice() == px),
                    "{mode:?} inventó el color {px:?}"
                );
            }
        }
    }

    #[test]
    fn pixel_art_modes_fall_back_to_smooth_off_their_factor() {
        let mut src = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                src.extend_from_slice(&[(x * 60) as u8, (y * 60) as u8, 128, 255]);
            }
        }
        for (mode, factor) in [
            (crate::config::ScaleMode::Scale2x, 0.5),
            (crate::config::ScaleMode::Scale2x, 1.5),
            (crate::config::ScaleMode::Scale3x, 2.0),
            (crate::config::ScaleMode::Scale4x, 2.0),
            (crate::config::ScaleMode::Eagle, 3.0),
        ] {
            let (got, gw, gh) = scale_rgba(&src, 4, 4, factor, mode);
            let (smooth, sw, sh) = scale_rgba(&src, 4, 4, factor, crate::config::ScaleMode::Smooth);
            assert_eq!((gw, gh), (sw, sh), "{mode:?} a {factor}x");
            assert_eq!(got, smooth, "{mode:?} a {factor}x debe caer en Smooth");
        }
    }

    /// Imagen de prueba determinista (bordes no transparentes alfa 128/255).
    fn gradient(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&[
                    (x * 7 % 256) as u8,
                    (y * 11 % 256) as u8,
                    ((x + y) * 5 % 256) as u8,
                    if (x + y) % 3 == 0 { 255 } else { 128 },
                ]);
            }
        }
        v
    }

    fn le32(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    fn be16(bytes: &[u8], at: usize) -> u16 {
        u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap())
    }

    /// Diferencia media por canal entre RGBA de origen y un buffer `u32` del
    /// decodificador (empaquetado `[b, g, r, a]` en little-endian).
    fn mean_channel_error(rgba: &[u8], decoded: &[u32]) -> f64 {
        let mut total = 0u64;
        for (px, out) in rgba.chunks_exact(4).zip(decoded) {
            let bytes = out.to_le_bytes();
            let (b, g, r) = (
                u64::from(bytes[0]),
                u64::from(bytes[1]),
                u64::from(bytes[2]),
            );
            total += (u64::from(px[0])).abs_diff(r);
            total += (u64::from(px[1])).abs_diff(g);
            total += (u64::from(px[2])).abs_diff(b);
        }
        total as f64 / (rgba.len() / 4 * 3) as f64
    }

    #[test]
    fn bmp_tga_tiff_roundtrip_the_rgba_pixels() {
        let (w, h) = (7, 5);
        let rgba = gradient(w, h);
        for format in [GpuFormat::Bmp, GpuFormat::Tga, GpuFormat::Tiff] {
            let bytes = encode_to_bytes(&rgba, w, h, &opts(format)).unwrap();
            // El TGA no lleva magia: hay que indicar el formato al decodificar.
            let img_format = match format {
                GpuFormat::Bmp => image::ImageFormat::Bmp,
                GpuFormat::Tga => image::ImageFormat::Tga,
                _ => image::ImageFormat::Tiff,
            };
            let decoded = image::load_from_memory_with_format(&bytes, img_format)
                .unwrap_or_else(|e| panic!("{} no decodifica: {e}", format.as_str()))
                .to_rgba8();
            assert_eq!(decoded.as_raw(), &rgba, "{}", format.as_str());
        }
    }

    #[test]
    fn software_image_formats_honour_the_pixel_format() {
        let (w, h) = (4, 2);
        let rgba = gradient(w, h);
        let o = EncodeOptions {
            pixel_format: PixelFormat::Rgb888,
            ..opts(GpuFormat::Tga)
        };
        let bytes = encode_to_bytes(&rgba, w, h, &o).unwrap();
        // TGA sin canal alfa: el decodificador devuelve opacos.
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga)
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (w as u32, h as u32));
        assert!(decoded.pixels().all(|p| p.0[3] == 255), "alfa no compuesto");
    }

    #[test]
    fn dds_writes_a_valid_legacy_header_and_raw_pixels() {
        let (w, h) = (4, 3);
        let rgba = gradient(w, h);
        let bytes = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Dds)).unwrap();
        assert_eq!(&bytes[..4], b"DDS ");
        assert_eq!(le32(&bytes, 4), 124, "dwSize");
        assert_eq!(le32(&bytes, 12), h as u32, "dwHeight");
        assert_eq!(le32(&bytes, 16), w as u32, "dwWidth");
        assert_eq!(le32(&bytes, 20), (w * 4) as u32, "pitch");
        // DDS_PIXELFORMAT en el offset 76 de la cabecera.
        assert_eq!(le32(&bytes, 76), 32, "pixelformat dwSize");
        assert_eq!(le32(&bytes, 80), 0x41, "DDPF_RGB | DDPF_ALPHAPIXELS");
        assert_eq!(le32(&bytes, 88), 32, "dwRGBBitCount");
        assert_eq!(le32(&bytes, 92), 0x0000_00FF, "máscara R");
        assert_eq!(le32(&bytes, 104), 0xFF00_0000, "máscara A");
        assert_eq!(le32(&bytes, 108), 0x1000, "DDSCAPS_TEXTURE");
        assert_eq!(&bytes[128..], &rgba[..], "payload en orden R,G,B,A");
    }

    #[test]
    fn dds_supports_the_rgb_and_gray_pixel_formats() {
        let (w, h) = (4, 2);
        let rgba = gradient(w, h);
        let rgb = EncodeOptions {
            pixel_format: PixelFormat::Rgb888,
            ..opts(GpuFormat::Dds)
        };
        let bytes = encode_to_bytes(&rgba, w, h, &rgb).unwrap();
        assert_eq!(le32(&bytes, 88), 24, "dwRGBBitCount");
        assert_eq!(le32(&bytes, 80), 0x40, "solo DDPF_RGB");
        assert_eq!(bytes.len(), 128 + w * h * 3);

        let alpha = EncodeOptions {
            pixel_format: PixelFormat::Alpha8,
            ..opts(GpuFormat::Dds)
        };
        let bytes = encode_to_bytes(&rgba, w, h, &alpha).unwrap();
        assert_eq!(le32(&bytes, 88), 8, "dwRGBBitCount");
        assert_eq!(le32(&bytes, 80), 0x2_0000, "DDPF_LUMINANCE");
        assert_eq!(bytes.len(), 128 + w * h);
    }

    #[test]
    fn zktx_is_a_zlib_compressed_ktx() {
        use std::io::Read;
        let (w, h) = (4, 4);
        let rgba = gradient(w, h);
        let bytes = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Zktx)).unwrap();
        let mut ktx = Vec::new();
        flate2::read::ZlibDecoder::new(&bytes[..])
            .read_to_end(&mut ktx)
            .expect("zktx es zlib");
        assert_eq!(&ktx[..12], b"\xABKTX 11\xBB\r\n\x1A\n");
        assert_eq!(le32(&ktx, 16), 0x1401, "glType = GL_UNSIGNED_BYTE");
        assert_eq!(le32(&ktx, 24), 0x1908, "glFormat = GL_RGBA");
        assert_eq!(le32(&ktx, 28), 0x8058, "glInternalFormat = GL_RGBA8");
        assert_eq!(le32(&ktx, 36), w as u32);
        assert_eq!(le32(&ktx, 40), h as u32);
        let image_size = le32(&ktx, 64) as usize;
        assert_eq!(image_size, rgba.len());
        assert_eq!(&ktx[68..68 + image_size], &rgba[..]);
    }

    #[test]
    fn pvr3_gz_and_ccz_wrap_the_pvr3_file() {
        use std::io::Read;
        let rgba = gradient(8, 8);
        let base = encode_to_bytes(&rgba, 8, 8, &opts(GpuFormat::Pvrtc4Bpp)).unwrap();
        assert_eq!(&base[..4], b"PVR\x03");

        let gz = encode_to_bytes(&rgba, 8, 8, &opts(GpuFormat::Pvr3Gz)).unwrap();
        assert_eq!(&gz[..2], &[0x1F, 0x8B], "cabecera gzip");
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&gz[..])
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, base, "pvr.gz debe descomprimir al PVR3 original");

        let ccz = encode_to_bytes(&rgba, 8, 8, &opts(GpuFormat::Pvr3Ccz)).unwrap();
        assert_eq!(&ccz[..4], b"CCZ!");
        assert_eq!(be16(&ccz, 4), 0, "compression_type = zlib");
        assert_eq!(be16(&ccz, 6), 0, "version");
        assert_eq!(
            u32::from_be_bytes(ccz[12..16].try_into().unwrap()) as usize,
            base.len()
        );
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&ccz[16..])
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, base, "pvr.ccz debe descomprimir al PVR3 original");
    }

    #[test]
    fn etc1_pkm_and_ktx_decode_close_to_the_source() {
        let (w, h) = (16, 16);
        let rgba = gradient(w, h);

        let pkm = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Etc1)).unwrap();
        assert_eq!(&pkm[..6], b"PKM 10");
        assert_eq!(be16(&pkm, 6), 0, "dataFormat ETC1");
        assert_eq!(be16(&pkm, 8), w as u16, "extended width");
        assert_eq!(be16(&pkm, 10), h as u16, "extended height");
        assert_eq!(be16(&pkm, 12), w as u16, "original width");
        assert_eq!(be16(&pkm, 14), h as u16, "original height");
        let mut buf = vec![0u32; w * h];
        texture2ddecoder::decode_etc1(&pkm[16..], w, h, &mut buf).unwrap();
        let err = mean_channel_error(&rgba, &buf);
        assert!(err < 6.0, "error medio ETC1 demasiado alto: {err:.2}");

        let ktx = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Etc1Ktx)).unwrap();
        assert_eq!(&ktx[..12], b"\xABKTX 11\xBB\r\n\x1A\n");
        assert_eq!(le32(&ktx, 16), 0, "glType = 0 (comprimido)");
        assert_eq!(le32(&ktx, 24), 0, "glFormat = 0 (comprimido)");
        assert_eq!(le32(&ktx, 28), 0x8D60, "GL_ETC1_RGB8_OES");
        assert_eq!(le32(&ktx, 32), 0x1907, "GL_RGB");
        let image_size = le32(&ktx, 64) as usize;
        assert_eq!(image_size, w * h / 2, "ETC1 son 8 bytes por bloque 4x4");
        let mut buf = vec![0u32; w * h];
        texture2ddecoder::decode_etc1(&ktx[68..68 + image_size], w, h, &mut buf).unwrap();
        let err = mean_channel_error(&rgba, &buf);
        assert!(err < 6.0, "error medio ETC1 (ktx) demasiado alto: {err:.2}");
    }

    #[test]
    fn etc1_encoder_stays_inside_the_etc1_mode_set() {
        // Un bloque que en ETC2 caería en modo T/H/planar (desbordamiento del
        // diferencial) debe seguir siendo decodificable como ETC1.
        let mut rgba = vec![0u8; 4 * 4 * 4];
        for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
            let v = ((i as u16 * 16) % 256) as u8;
            px.copy_from_slice(&[v, 255 - v, ((u16::from(v) * 3) % 256) as u8, 255]);
        }
        let blocks = crate::etc2::encode_etc1_rgb(&rgba, 4, 4);
        assert_eq!(blocks.len(), 8);
        // Ningún bloque puede salirse del conjunto de modos ETC1: con el bit
        // de diferencial activo, base + delta debe caber en 5 bits (si no,
        // ETC2 lo reinterpretaría como modo T/H/planar y ETC1 lo leería mal).
        if (blocks[3] >> 1) & 1 == 1 {
            for channel in blocks[..3].iter() {
                let base = i16::from(channel >> 3);
                let delta = i16::from(channel & 7);
                let delta = if delta >= 4 { delta - 8 } else { delta };
                let sum = base + delta;
                assert!(
                    (0..=31).contains(&sum),
                    "diferencial {sum} fuera del rango ETC1"
                );
            }
        }
        let mut buf = vec![0u32; 16];
        texture2ddecoder::decode_etc1(&blocks, 4, 4, &mut buf).unwrap();
        assert!(mean_channel_error(&rgba, &buf) < 48.0);
    }
}
