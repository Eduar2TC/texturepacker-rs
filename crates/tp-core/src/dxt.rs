//! Codificadores DXT1 (BC1) y DXT5 (BC3) propios, con la cuantización
//! `--dxt-mode` (`DXT_LINEAR` / `DXT_PERCEPTUAL`).
//!
//! Cada bloque de 4×4 se comprime en 8 bytes (DXT1) o 16 bytes (DXT5: 8 de
//! alfa + 8 de color). El ajuste de extremos combina tres candidatos
//! iniciales (caja envolvente por canal, eje dominante desde la media y eje
//! principal por potencia sobre la matriz de covarianza) con pasadas de
//! mínimos cuadrados contra la paleta ya cuantizada; el error que se
//! minimiza está ponderado por luminancia en modo perceptual y es uniforme
//! en modo lineal.
//!
//! Reglas del formato que respeta el codificador:
//!
//! - DXT1 con `c0 > c1` (sin signo) usa los 4 colores interpolados; con
//!   `c0 <= c1` el cuarto color es transparente y se usa para los píxeles
//!   con alfa < 128.
//! - DXT5 guarda el color siempre en modo de 4 colores y el alfa en su
//!   propio bloque (8 valores interpolados si `a0 > a1`; 6 valores más 0 y
//!   255 si no).
//!
//! Los bloques incompletos del borde se rellenan con clamp de borde, como
//! hacen el resto de codificadores de hardware de este módulo.

use crate::config::DxtMode;

/// Ponderación BT.709 usada en `DxtMode::Perceptual`.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
/// Pasadas de refinamiento (mínimos cuadrados) por candidato.
const REFINE_ITERS: usize = 4;
/// Umbral de alfa: por debajo el píxel es transparente en DXT1.
const ALPHA_THRESHOLD: u8 = 128;

#[inline]
fn weights(mode: DxtMode) -> [f32; 3] {
    match mode {
        DxtMode::Linear => [1.0, 1.0, 1.0],
        DxtMode::Perceptual => LUMA,
    }
}

#[inline]
fn dist2(a: [u8; 3], b: [u8; 3], w: [f32; 3]) -> f32 {
    let d = [
        a[0] as f32 - b[0] as f32,
        a[1] as f32 - b[1] as f32,
        a[2] as f32 - b[2] as f32,
    ];
    w[0] * d[0] * d[0] + w[1] * d[1] * d[1] + w[2] * d[2] * d[2]
}

/// Cuantiza un canal al número de bits pedido eligiendo, entre los valores
/// vecinos del redondeo clásico, el que su expansión por replicación deja
/// más cerca del valor original (`v >> (8 - bits)` recortaría siempre hacia
/// abajo y aleja los extremos claros, p. ej. 240 → 247 en vez de 239).
#[inline]
fn quant_c(v: u8, bits: u32) -> u16 {
    let max = (1u32 << bits) - 1;
    let expand = |q: u32| -> u32 {
        match bits {
            5 => (q << 3) | (q >> 2),
            6 => (q << 2) | (q >> 4),
            _ => (q << (8 - bits)) | (q >> (2 * bits - 8)),
        }
    };
    let approx = ((v as u32 * max) as f64 / 255.0).round() as u32;
    let mut best_q = approx;
    let mut best_e = u32::MAX;
    for q in approx.saturating_sub(1)..=(approx + 1).min(max) {
        let e = expand(q).abs_diff(v as u32);
        if e < best_e {
            best_e = e;
            best_q = q;
        }
    }
    best_q as u16
}

#[inline]
fn quant565(c: [u8; 3]) -> u16 {
    (quant_c(c[0], 5) << 11) | (quant_c(c[1], 6) << 5) | quant_c(c[2], 5)
}

/// Cuantiza extremos aún sin recortar (salen del ajuste en coma flotante).
#[inline]
fn quant565f(c: [f32; 3]) -> u16 {
    quant565([
        c[0].clamp(0.0, 255.0) as u8,
        c[1].clamp(0.0, 255.0) as u8,
        c[2].clamp(0.0, 255.0) as u8,
    ])
}

/// Expansión de 5 bits con replicación de bits (igual que el decodificador).
#[inline]
fn expand5(v: u8) -> u8 {
    ((v & 0x1f) << 3) | (v >> 2)
}

#[inline]
fn expand6(v: u8) -> u8 {
    ((v & 0x3f) << 2) | (v >> 4)
}

#[inline]
fn from565(q: u16) -> [u8; 3] {
    [
        expand5((q >> 11) as u8),
        expand6(((q >> 5) & 0x3f) as u8),
        expand5((q & 0x1f) as u8),
    ]
}

/// Paleta de 4 colores que reconstruye exactamente el decodificador
/// (división entera, sin redondeo).
fn palette(q0: u16, q1: u16, transparent_mode: bool) -> [[u8; 3]; 4] {
    let c0 = from565(q0);
    let c1 = from565(q1);
    let mix = |k: u32, n: u32| -> [u8; 3] {
        [
            ((c0[0] as u32 * k + c1[0] as u32 * n) / (k + n)) as u8,
            ((c0[1] as u32 * k + c1[1] as u32 * n) / (k + n)) as u8,
            ((c0[2] as u32 * k + c1[2] as u32 * n) / (k + n)) as u8,
        ]
    };
    if q0 > q1 {
        [c0, c1, mix(2, 1), mix(1, 2)]
    } else {
        let half = [
            ((c0[0] as u32 + c1[0] as u32) / 2) as u8,
            ((c0[1] as u32 + c1[1] as u32) / 2) as u8,
            ((c0[2] as u32 + c1[2] as u32) / 2) as u8,
        ];
        // Cuarto color: transparente en DXT1; en DXT5 nunca se usa porque su
        // color siempre va con c0 > c1.
        let _ = transparent_mode;
        [c0, c1, half, [0, 0, 0]]
    }
}

/// Índice (0-3) de la paleta más cercana al píxel, con la métrica del modo.
fn best_index(p: [u8; 3], pal: &[[u8; 3]; 4], w: [f32; 3], limit: usize) -> u8 {
    let mut best = 0usize;
    let mut best_err = f32::INFINITY;
    for (i, c) in pal.iter().enumerate().take(limit) {
        let e = dist2(p, *c, w);
        if e < best_err {
            best_err = e;
            best = i;
        }
    }
    best as u8
}

/// Peso del extremo `c1` para el índice dado (modo de 4 colores o de 3
/// colores + transparente).
fn t_of(index: u8, transparent_mode: bool) -> Option<f32> {
    if transparent_mode {
        match index {
            0 => Some(0.0),
            1 => Some(1.0),
            2 => Some(0.5),
            _ => None, // transparente: no entra en el ajuste
        }
    } else {
        // La paleta del decodificador es [c0, c1, (2c0+c1)/3, (c0+2c1)/3].
        Some(match index {
            0 => 0.0,
            1 => 1.0,
            2 => 1.0 / 3.0,
            _ => 2.0 / 3.0,
        })
    }
}

/// Refinamiento por mínimos cuadrados: dado el reparto de índices, busca los
/// extremos ideales (sin cuantizar) que mejor aproximan cada canal.
fn least_squares(
    px: &[[u8; 3]],
    idx: &[u8; 16],
    transparent_mode: bool,
) -> Option<([f32; 3], [f32; 3])> {
    let mut n = 0.0f64;
    let mut sum_t = 0.0f64;
    let mut sum_p = [0.0f64; 3];
    let mut sum_tt = 0.0f64;
    let mut sum_tp = [0.0f64; 3];
    for (i, p) in px.iter().enumerate() {
        let Some(t) = t_of(idx[i], transparent_mode) else {
            continue;
        };
        let t = t as f64;
        n += 1.0;
        sum_t += t;
        sum_tt += t * t;
        for c in 0..3 {
            sum_p[c] += p[c] as f64;
            sum_tp[c] += t * p[c] as f64;
        }
    }
    if n < 2.0 {
        return None;
    }
    let denom = n * sum_tt - sum_t * sum_t;
    if denom.abs() < 1e-9 {
        return None;
    }
    let mut c0 = [0.0f32; 3];
    let mut c1 = [0.0f32; 3];
    for c in 0..3 {
        let b = (n * sum_tp[c] - sum_t * sum_p[c]) / denom;
        let a = (sum_p[c] - b * sum_t) / n;
        c0[c] = a as f32;
        c1[c] = (a + b) as f32;
    }
    Some((c0, c1))
}

/// Resultado de ajustar un bloque: extremos cuantizados, índices y error.
struct Fitted {
    q0: u16,
    q1: u16,
    indices: [u8; 16],
    error: f32,
}

/// Ordena los extremos para el modo pedido: el modo de 4 colores exige
/// `q0 > q1` (si no, el decodificador entraría en el modo de 3 colores y el
/// índice 3 sería transparente) y el modo transparente exige `q0 <= q1`.
/// Los índices se recalculan siempre después, así que basta devolver la
/// pareja ordenada.
fn enforce_order(q0: u16, q1: u16, transparent_mode: bool) -> (u16, u16) {
    if transparent_mode {
        return if q0 > q1 { (q1, q0) } else { (q0, q1) };
    }
    if q0 > q1 {
        return (q0, q1);
    }
    if q1 > q0 {
        return (q1, q0);
    }
    // Iguales: se separa un paso el extremo menor (o se sube el mayor).
    if q0 > 0 {
        (q0, q0 - 1)
    } else {
        (1, 0)
    }
}

/// Ajusta un bloque completo: candidatos iniciales + pasadas de mínimos
/// cuadrados, quedándose con el de menor error.
fn fit_block(px: &[[u8; 3]], mode: DxtMode, transparent_mode: bool) -> Fitted {
    let w = weights(mode);
    let mut best = Fitted {
        q0: 0,
        q1: 0,
        indices: [0; 16],
        error: f32::INFINITY,
    };
    if px.is_empty() {
        return best;
    }

    for (start0, start1) in candidate_pairs(px) {
        let mut c0 = start0;
        let mut c1 = start1;
        let mut q0 = quant565f(c0);
        let mut q1 = quant565f(c1);
        let mut indices = [0u8; 16];
        let limit = if transparent_mode { 3 } else { 4 };
        for _ in 0..REFINE_ITERS {
            let (oq0, oq1) = enforce_order(q0, q1, transparent_mode);
            let pal = palette(oq0, oq1, transparent_mode);
            for (i, p) in px.iter().enumerate() {
                indices[i] = best_index(*p, &pal, w, limit);
            }
            match least_squares(px, &indices, transparent_mode) {
                Some((n0, n1)) => {
                    c0 = n0;
                    c1 = n1;
                }
                None => break,
            }
            q0 = quant565f(c0);
            q1 = quant565f(c1);
        }
        let (fq0, fq1) = enforce_order(q0, q1, transparent_mode);
        let pal = palette(fq0, fq1, transparent_mode);
        for (i, p) in px.iter().enumerate() {
            indices[i] = best_index(*p, &pal, w, limit);
        }
        let mut error = 0.0f32;
        for (i, p) in px.iter().enumerate() {
            error += dist2(*p, pal[indices[i] as usize], w);
        }
        if error < best.error {
            best = Fitted {
                q0: fq0,
                q1: fq1,
                indices,
                error,
            };
        }
    }
    best
}

/// Tres pares de extremos iniciales: caja envolvente por canal, extremos a
/// partir de la media y del píxel más lejano, y proyección sobre el eje
/// principal (potencia sobre la covarianza).
fn candidate_pairs(px: &[[u8; 3]]) -> Vec<([f32; 3], [f32; 3])> {
    let n = px.len() as f32;
    let mut min_c = [255.0f32; 3];
    let mut max_c = [0.0f32; 3];
    let mut mean = [0.0f32; 3];
    for p in px {
        for c in 0..3 {
            min_c[c] = min_c[c].min(p[c] as f32);
            max_c[c] = max_c[c].max(p[c] as f32);
            mean[c] += p[c] as f32 / n;
        }
    }
    let mut out = vec![(min_c, max_c)];

    // Eje desde la media hasta el píxel más alejado.
    let mut far = mean;
    let mut far2 = -1.0f32;
    for p in px {
        let d = [
            p[0] as f32 - mean[0],
            p[1] as f32 - mean[1],
            p[2] as f32 - mean[2],
        ];
        let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        if d2 > far2 {
            far2 = d2;
            far = [p[0] as f32, p[1] as f32, p[2] as f32];
        }
    }
    if far2 > 0.25 {
        out.push((mean, far));
        out.push((far, mean));
    }

    // Eje principal: potencia sobre la covarianza.
    let mut cov = [[0.0f32; 3]; 3];
    for p in px {
        let d = [
            p[0] as f32 - mean[0],
            p[1] as f32 - mean[1],
            p[2] as f32 - mean[2],
        ];
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j] / n;
            }
        }
    }
    let mut v = [1.0f32, 1.0f32, 1.0f32];
    for _ in 0..8 {
        let mut nv = [0.0f32; 3];
        for i in 0..3 {
            nv[i] = cov[i][0] * v[0] + cov[i][1] * v[1] + cov[i][2] * v[2];
        }
        let len = (nv[0] * nv[0] + nv[1] * nv[1] + nv[2] * nv[2]).sqrt();
        if len < 1e-6 {
            break;
        }
        v = [nv[0] / len, nv[1] / len, nv[2] / len];
    }
    let mut min_p = f32::INFINITY;
    let mut max_p = f32::NEG_INFINITY;
    let mut min_px = mean;
    let mut max_px = mean;
    for p in px {
        let d = [
            p[0] as f32 - mean[0],
            p[1] as f32 - mean[1],
            p[2] as f32 - mean[2],
        ];
        let t = d[0] * v[0] + d[1] * v[1] + d[2] * v[2];
        if t < min_p {
            min_p = t;
            min_px = [p[0] as f32, p[1] as f32, p[2] as f32];
        }
        if t > max_p {
            max_p = t;
            max_px = [p[0] as f32, p[1] as f32, p[2] as f32];
        }
    }
    if min_p.is_finite() && max_p.is_finite() && max_p - min_p > 0.5 {
        out.push((min_px, max_px));
        out.push((max_px, min_px));
    }
    out
}

/// Bloque de color DXT1/BC3 (siempre modo de 4 colores) de 8 bytes.
fn encode_color_block(px: &[[u8; 3]; 16], mode: DxtMode) -> [u8; 8] {
    let fit = fit_block(px, mode, false);
    block_bytes(fit.q0, fit.q1, &fit.indices)
}

/// Bloque DXT1 completo: color + manejo de alfa < 128 en modo de 3 colores.
fn encode_dxt1_block(pixels: &[[u8; 4]; 16], mode: DxtMode) -> [u8; 8] {
    let has_transparent = pixels.iter().any(|p| p[3] < ALPHA_THRESHOLD);
    if !has_transparent {
        let mut px = [[0u8; 3]; 16];
        for (i, p) in pixels.iter().enumerate() {
            px[i] = [p[0], p[1], p[2]];
        }
        return encode_color_block(&px, mode);
    }
    // Modo de 3 colores + transparente: solo se ajusta con los opacos.
    let mut px = Vec::with_capacity(16);
    for p in pixels {
        if p[3] >= ALPHA_THRESHOLD {
            px.push([p[0], p[1], p[2]]);
        }
    }
    if px.is_empty() {
        // Bloque totalmente transparente: c0 == c1 y todos los índices en 3.
        return block_bytes(0, 0, &[3; 16]);
    }
    let fit = fit_block(&px, mode, true);
    let pal = palette(fit.q0, fit.q1, true);
    let w = weights(mode);
    let mut indices = [3u8; 16];
    for (i, p) in pixels.iter().enumerate() {
        if p[3] >= ALPHA_THRESHOLD {
            indices[i] = best_index([p[0], p[1], p[2]], &pal, w, 3);
        }
    }
    block_bytes(fit.q0, fit.q1, &indices)
}

#[inline]
fn block_bytes(q0: u16, q1: u16, indices: &[u8; 16]) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&q0.to_le_bytes());
    out[2..4].copy_from_slice(&q1.to_le_bytes());
    let mut word = 0u32;
    for (i, idx) in indices.iter().enumerate() {
        word |= (*idx as u32 & 3) << (2 * i);
    }
    out[4..8].copy_from_slice(&word.to_le_bytes());
    out
}

/// Bloque de alfa BC3 (8 bytes) con el mínimo error de los dos modos.
fn encode_alpha_block(alpha: &[u8; 16]) -> [u8; 8] {
    let min = *alpha.iter().min().unwrap();
    let max = *alpha.iter().max().unwrap();

    // Tablas de reconstrucción idénticas al decodificador.
    let table = |a0: u8, a1: u8| -> [u8; 8] {
        let mut t = [0u8; 8];
        t[0] = a0;
        t[1] = a1;
        if a0 > a1 {
            for (i, v) in t.iter_mut().enumerate().take(8).skip(2) {
                *v = ((a0 as u32 * (8 - i) as u32 + a1 as u32 * (i - 1) as u32) / 7) as u8;
            }
        } else {
            for (i, v) in t.iter_mut().enumerate().take(6).skip(2) {
                *v = ((a0 as u32 * (6 - i) as u32 + a1 as u32 * (i - 1) as u32) / 5) as u8;
            }
            t[6] = 0;
            t[7] = 255;
        }
        t
    };
    let nearest = |t: &[u8; 8], a: u8| -> (usize, u32) {
        let mut bi = 0usize;
        let mut be = u32::MAX;
        for (i, v) in t.iter().enumerate() {
            let d = (*v as i32 - a as i32).unsigned_abs();
            if d < be {
                be = d;
                bi = i;
            }
        }
        (bi, be)
    };

    let candidates: Vec<(u8, u8)> = if min == max {
        vec![(min, min)]
    } else {
        // a0 > a1 → 8 valores interpolados; a0 <= a1 → 6 valores + 0 + 255.
        vec![(max, min), (min, max)]
    };
    let mut best: Option<([u8; 8], f32)> = None;
    for (a0, a1) in candidates {
        let t = table(a0, a1);
        let mut err = 0.0f32;
        let mut pack = 0u64;
        for (i, &a) in alpha.iter().enumerate() {
            let (idx, d) = nearest(&t, a);
            err += d as f32;
            pack |= (idx as u64) << (3 * i);
        }
        if best.as_ref().is_none_or(|b| err < b.1) {
            let mut bytes = [0u8; 8];
            bytes[0] = a0;
            bytes[1] = a1;
            bytes[2..8].copy_from_slice(&pack.to_le_bytes()[..6]);
            best = Some((bytes, err));
        }
    }
    best.map(|b| b.0).unwrap_or([0u8; 8])
}

/// Codifica una imagen RGBA8 como DXT1/BC1: 8 bytes por bloque de 4×4.
pub fn encode_dxt1(rgba: &[u8], width: usize, height: usize, mode: DxtMode) -> Vec<u8> {
    let bx = width.div_ceil(4);
    let by = height.div_ceil(4);
    let mut out = Vec::with_capacity(bx * by * 8);
    for y0 in 0..by {
        for x0 in 0..bx {
            let mut pixels = [[0u8; 4]; 16];
            for ty in 0..4 {
                for tx in 0..4 {
                    let x = (x0 * 4 + tx).min(width.saturating_sub(1));
                    let y = (y0 * 4 + ty).min(height.saturating_sub(1));
                    let i = (y * width + x) * 4;
                    pixels[ty * 4 + tx] = [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]];
                }
            }
            out.extend_from_slice(&encode_dxt1_block(&pixels, mode));
        }
    }
    out
}

/// Codifica una imagen RGBA8 como DXT5/BC3: 16 bytes por bloque de 4×4.
pub fn encode_dxt5(rgba: &[u8], width: usize, height: usize, mode: DxtMode) -> Vec<u8> {
    let bx = width.div_ceil(4);
    let by = height.div_ceil(4);
    let mut out = Vec::with_capacity(bx * by * 16);
    for y0 in 0..by {
        for x0 in 0..bx {
            let mut pixels = [[0u8; 4]; 16];
            for ty in 0..4 {
                for tx in 0..4 {
                    let x = (x0 * 4 + tx).min(width.saturating_sub(1));
                    let y = (y0 * 4 + ty).min(height.saturating_sub(1));
                    let i = (y * width + x) * 4;
                    pixels[ty * 4 + tx] = [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]];
                }
            }
            let mut alpha = [0u8; 16];
            for (i, p) in pixels.iter().enumerate() {
                alpha[i] = p[3];
            }
            let mut color = [[0u8; 3]; 16];
            for (i, p) in pixels.iter().enumerate() {
                color[i] = [p[0], p[1], p[2]];
            }
            out.extend_from_slice(&encode_alpha_block(&alpha));
            out.extend_from_slice(&encode_color_block(&color, mode));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodifica con la implementación independiente `texture2ddecoder` y
    /// devuelve píxeles RGBA en orden de fila.
    fn decode1(data: &[u8], w: usize, h: usize, with_alpha: bool) -> Vec<[u8; 4]> {
        let mut buf = vec![0u32; w * h];
        let res = if with_alpha {
            texture2ddecoder::decode_bc1a(data, w, h, &mut buf)
        } else {
            texture2ddecoder::decode_bc1(data, w, h, &mut buf)
        };
        res.expect("decode bc1");
        buf.iter()
            .map(|v| {
                let b = v.to_le_bytes();
                [b[2], b[1], b[0], b[3]]
            })
            .collect()
    }

    fn decode3(data: &[u8], w: usize, h: usize) -> Vec<[u8; 4]> {
        let mut buf = vec![0u32; w * h];
        texture2ddecoder::decode_bc3(data, w, h, &mut buf).expect("decode bc3");
        buf.iter()
            .map(|v| {
                let b = v.to_le_bytes();
                [b[2], b[1], b[0], b[3]]
            })
            .collect()
    }

    fn gradient(w: usize, h: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                out.extend_from_slice(&[
                    (x * 255 / w.max(1)) as u8,
                    (y * 255 / h.max(1)) as u8,
                    ((x + y) * 255 / (w + h).max(1)) as u8,
                    255,
                ]);
            }
        }
        out
    }

    /// Error absoluto medio por canal entre la fuente y lo que devuelve el
    /// decodificador (`nch` canales a partir de los 4 de la fuente).
    fn mean_err(src: &[u8], got: &[[u8; 4]], nch: usize) -> f64 {
        let mut err = 0.0f64;
        let mut n = 0usize;
        for (i, p) in src.chunks_exact(4).enumerate() {
            for c in 0..nch {
                err += (p[c] as f64 - got[i][c] as f64).abs();
            }
            n += 1;
        }
        err / (n * nch) as f64
    }

    /// Codificador de referencia (extremos de la caja envolvente, sin
    /// refinamiento) y su error cuadrático con la métrica del modo pedido.
    fn naive_reference(src: &[u8], w: usize, h: usize, mode: DxtMode) -> f64 {
        let wts = weights(mode);
        let mut err = 0.0f64;
        for y0 in 0..h.div_ceil(4) {
            for x0 in 0..w.div_ceil(4) {
                let mut px = Vec::new();
                for ty in 0..4 {
                    for tx in 0..4 {
                        let x = (x0 * 4 + tx).min(w - 1);
                        let y = (y0 * 4 + ty).min(h - 1);
                        let i = (y * w + x) * 4;
                        px.push([src[i], src[i + 1], src[i + 2]]);
                    }
                }
                let mut lo = [u8::MAX; 3];
                let mut hi = [u8::MIN; 3];
                for p in &px {
                    for c in 0..3 {
                        lo[c] = lo[c].min(p[c]);
                        hi[c] = hi[c].max(p[c]);
                    }
                }
                let mut idx = [0u8; 16];
                let (q0, q1) = enforce_order(quant565(lo), quant565(hi), false);
                let pal = palette(q0, q1, false);
                for (i, p) in px.iter().enumerate() {
                    idx[i] = best_index(*p, &pal, wts, 4);
                }
                for (i, p) in px.iter().enumerate() {
                    err += dist2(*p, pal[idx[i] as usize], wts) as f64;
                }
            }
        }
        err / ((w * h * 3) as f64)
    }

    /// Error cuadrático medio (con la métrica del modo) de nuestro encoder.
    fn our_reference(src: &[u8], w: usize, h: usize, mode: DxtMode) -> f64 {
        let wts = weights(mode);
        let got = decode1(&encode_dxt1(src, w, h, mode), w, h, false);
        let mut err = 0.0f64;
        for (i, p) in src.chunks_exact(4).enumerate() {
            err += dist2([p[0], p[1], p[2]], [got[i][0], got[i][1], got[i][2]], wts) as f64;
        }
        err / ((w * h * 3) as f64)
    }

    #[test]
    fn dxt1_decodes_close_to_the_source() {
        let (w, h) = (16, 16);
        let src = gradient(w, h);
        for mode in [DxtMode::Linear, DxtMode::Perceptual] {
            let data = encode_dxt1(&src, w, h, mode);
            assert_eq!(data.len(), (w / 4) * (h / 4) * 8);
            let got = decode1(&data, w, h, false);
            let mean = mean_err(&src, &got, 3);
            // El contenido tiene variación en dos dimensiones y una paleta
            // lineal de 4 colores no puede seguirlo mejor que esto.
            assert!(mean < 10.0, "{mode:?}: error medio {mean:.2}");
        }
    }

    #[test]
    fn dxt1_follows_a_one_dimensional_ramp_tightly() {
        // Contenido colineal: la paleta de 4 colores sí puede representarlo
        // casi exactamente, así que aquí sí exige precisión.
        let (w, h) = (16, 16);
        let mut src = Vec::with_capacity(w * h * 4);
        for _ in 0..h {
            for x in 0..w {
                let v = (x * 255 / (w - 1)) as u8;
                src.extend_from_slice(&[v, (v as u16 / 2) as u8, 255 - v, 255]);
            }
        }
        for mode in [DxtMode::Linear, DxtMode::Perceptual] {
            let data = encode_dxt1(&src, w, h, mode);
            let got = decode1(&data, w, h, false);
            let mean = mean_err(&src, &got, 3);
            assert!(mean < 3.0, "{mode:?}: error medio {mean:.2}");
        }
    }

    #[test]
    fn dxt1_is_never_worse_than_a_min_max_reference() {
        // Regresión del buscador: candidatos iniciales + mínimos cuadrados
        // no pueden quedarse peor, en error cuadrático con la métrica del
        // modo, que la elección ingenua de los extremos de la caja.
        let (w, h) = (16, 16);
        let src = gradient(w, h);
        for mode in [DxtMode::Linear, DxtMode::Perceptual] {
            let ours = our_reference(&src, w, h, mode);
            let naive = naive_reference(&src, w, h, mode);
            assert!(
                ours <= naive,
                "{mode:?}: nuestro {ours:.3} vs referencia {naive:.3}"
            );
        }
    }

    #[test]
    fn dxt5_keeps_the_alpha_channel() {
        let (w, h) = (16, 16);
        let mut src = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                // Rampa 1D (colineal) para el color + alfa degradado.
                let v = (x * 255 / (w - 1)) as u8;
                src.extend_from_slice(&[v, v, 255 - v, (y * 255 / (h - 1)) as u8]);
            }
        }
        let data = encode_dxt5(&src, w, h, DxtMode::Linear);
        assert_eq!(data.len(), (w / 4) * (h / 4) * 16);
        let got = decode3(&data, w, h);
        let color_mean = mean_err(&src, &got, 3);
        assert!(color_mean < 3.0, "color {color_mean}");
        let mut alpha_err = 0.0f64;
        for (i, p) in src.chunks_exact(4).enumerate() {
            alpha_err += (p[3] as f64 - got[i][3] as f64).abs();
        }
        let alpha_mean = alpha_err / ((w * h) as f64);
        assert!(alpha_mean < 2.0, "alfa {alpha_mean}");
    }

    #[test]
    fn dxt1_marks_fully_transparent_pixels_as_transparent() {
        let (w, h) = (4, 4);
        let mut src = vec![0u8; 4 * 4 * 4];
        for (i, px) in src.chunks_exact_mut(4).enumerate() {
            if i % 2 == 0 {
                px.copy_from_slice(&[200, 100, 50, 255]);
            } else {
                px.copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        let data = encode_dxt1(&src, w, h, DxtMode::Linear);
        let got = decode1(&data, w, h, true);
        for (i, p) in src.chunks_exact(4).enumerate() {
            if p[3] == 0 {
                assert_eq!(got[i][3], 0, "texel {i} debería ser transparente");
            } else {
                assert_eq!(got[i][3], 255, "texel {i} debería ser opaco");
                assert!(
                    (got[i][0] as i32 - p[0] as i32).abs() < 32,
                    "color del texel {i}: {:?} vs {:?}",
                    got[i],
                    p
                );
            }
        }
    }

    #[test]
    fn dxt1_rejects_degenerate_transparent_blocks() {
        let src = vec![0u8; 4 * 4 * 4];
        let data = encode_dxt1(&src, 4, 4, DxtMode::Linear);
        let got = decode1(&data, 4, 4, true);
        assert!(
            got.iter().all(|p| p[3] == 0),
            "bloque enteramente transparente"
        );
    }

    #[test]
    fn perceptual_mode_weights_channels_differently() {
        // Distancia lineal: empate (primer índice, 0). Con luminancia, el
        // verde pesa ~10× el azul y gana el índice 1.
        let pal = [[100, 0, 0], [0, 100, 0], [255, 255, 255], [0, 0, 0]];
        let p = [100, 100, 100];
        assert_eq!(best_index(p, &pal, weights(DxtMode::Linear), 4), 0);
        assert_eq!(best_index(p, &pal, weights(DxtMode::Perceptual), 4), 1);
    }

    #[test]
    fn both_modes_produce_valid_results() {
        let (w, h) = (16, 16);
        // Contenido con croma fuerte: los dos modos tienen que producir
        // bloques decodificables con error razonable.
        let mut src = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let t = (x * 255 / (w - 1)) as u8;
                src.extend_from_slice(&[t, 255 - t, ((x + y) * 255 / (w + h - 2)) as u8, 255]);
            }
        }
        for mode in [DxtMode::Linear, DxtMode::Perceptual] {
            let data = encode_dxt1(&src, w, h, mode);
            let got = decode1(&data, w, h, false);
            let mean = mean_err(&src, &got, 3);
            assert!(mean < 14.0, "{mode:?}: error medio {mean:.2}");
        }
    }

    #[test]
    fn non_multiple_of_four_dimensions_are_clamped() {
        let (w, h) = (6, 5);
        let src = gradient(w, h);
        let d1 = encode_dxt1(&src, w, h, DxtMode::Linear);
        assert_eq!(d1.len(), 2 * 2 * 8);
        decode1(&d1, w, h, false);
        let d3 = encode_dxt5(&src, w, h, DxtMode::Perceptual);
        assert_eq!(d3.len(), 2 * 2 * 16);
        decode3(&d3, w, h);
    }
}
