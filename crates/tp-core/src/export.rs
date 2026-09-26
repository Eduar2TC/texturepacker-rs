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
    let mut out = Vec::with_capacity(64 + 4 + blocks.len());
    // KTX v1.1 header (64 bytes).
    out.extend_from_slice(b"\xABKTX 11\xBB\r\n\x1A\n");
    out.extend_from_slice(&0x04030201u32.to_le_bytes()); // endianness marker
    out.extend_from_slice(&0u32.to_le_bytes()); // glType
    out.extend_from_slice(&1u32.to_le_bytes()); // glTypeSize
    out.extend_from_slice(&0u32.to_le_bytes()); // glFormat
    out.extend_from_slice(&0x9278u32.to_le_bytes()); // GL_COMPRESSED_RGBA8_ETC2_EAC
    out.extend_from_slice(&0x1908u32.to_le_bytes()); // GL_RGBA
    out.extend_from_slice(&(width as u32).to_le_bytes()); // pixelWidth
    out.extend_from_slice(&(height as u32).to_le_bytes()); // pixelHeight
    out.extend_from_slice(&0u32.to_le_bytes()); // pixelDepth
    out.extend_from_slice(&0u32.to_le_bytes()); // numberOfArrayElements
    out.extend_from_slice(&1u32.to_le_bytes()); // numberOfFaces
    out.extend_from_slice(&1u32.to_le_bytes()); // numberOfMipmapLevels
    out.extend_from_slice(&0u32.to_le_bytes()); // bytesOfKeyValueData
    out.extend_from_slice(&(blocks.len() as u32).to_le_bytes()); // imageSize
    out.extend_from_slice(&blocks);
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

// ---------------------------------------------------------------------------
// Scaling (variants)
// ---------------------------------------------------------------------------

/// Scale an RGBA8 image by `factor` (0 < factor <= 1).
///
/// `ScaleMode::Smooth` blends neighbouring pixels (bilinear);
/// `ScaleMode::Fast` picks the nearest source pixel (keeps hard edges).
pub fn scale_rgba(
    rgba: &[u8],
    width: usize,
    height: usize,
    factor: f32,
    mode: crate::config::ScaleMode,
) -> (Vec<u8>, usize, usize) {
    use crate::config::ScaleMode;
    let nw = ((width as f32) * factor).round().max(1.0) as usize;
    let nh = ((height as f32) * factor).round().max(1.0) as usize;
    let mut out = vec![0u8; nw * nh * 4];

    let sample = |sx: f32, sy: f32, out: &mut [u8]| match mode {
        ScaleMode::Fast => {
            let x0 = sx.round().clamp(0.0, width as f32 - 1.0) as usize;
            let y0 = sy.round().clamp(0.0, height as f32 - 1.0) as usize;
            out.copy_from_slice(&rgba[(y0 * width + x0) * 4..(y0 * width + x0) * 4 + 4]);
        }
        ScaleMode::Smooth => {
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
}
