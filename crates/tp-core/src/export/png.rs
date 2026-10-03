use super::options::EncodeOptions;
use super::pixel::{color_channels, expand_to_rgba};
use crate::config::PngDither;
use crate::error::{Result, TpError};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::ImageEncoder;
use std::collections::HashMap;

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

pub(super) fn encode_png(
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
pub(super) fn encode_png8(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GpuFormat;
    use crate::export::encode_to_bytes;
    use crate::export::test_helpers::{four_color_image, opts, png_color_type};

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
}
