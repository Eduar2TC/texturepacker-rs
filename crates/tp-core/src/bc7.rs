//! Codificador BC7 propio, modo 6, con la cuantización `--dxt-mode`
//! (`DXT_LINEAR` / `DXT_PERCEPTUAL`).
//!
//! BC7 comprime RGBA en bloques de 4×4 con 16 bytes (128 bits) y define
//! ocho modos de bloque. El modo 6 es el más simple con calidad alta: un
//! solo subconjunto (sin bits de partición), dos extremos RGBA de 7 bits
//! cada uno más un P-bit por extremo —8 bits efectivos por canal, sin
//! pérdida al cuantizar— y un único índice de 4 bits compartido por los
//! cuatro canales, así que cada texel se elige de una paleta lineal de
//! 16 colores RGBA.
//!
//! Reglas del formato que respeta el codificador (spec de Microsoft,
//! «BC7 Format» / «BC7 Format Mode Reference», bitado en LSB-first):
//!
//! - El modo va en los bits menos significativos del bloque: el modo 6 se
//!   escribe como un 1 seguido de seis 0 (`0b1000000` en 7 bits).
//! - El stream es: 7 de modo, 56 de extremos (canal a canal, R0, R1, G0,
//!   G1, B0, B1, A0, A1), 2 P-bits (uno por extremo, aplicado a los cuatro
//!   canales) y 63 de índices: el primer texel («ancla») guarda el suyo en
//!   3 bits porque su MSB debe valer 0 —el codificador ordena los extremos
//!   para que así sea y ese bit se descarta—, el resto en 4 bits.
//! - La reconstrucción es `(e0*(64-w) + e1*w + 32) >> 6` con los pesos
//!   `w = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64]`;
//!   los extremos de 7 bits + P-bit ya son 8 bits, sin replicación.
//!
//! El ajuste sigue el patrón de [`crate::dxt`]: pares de extremos
//! iniciales (caja envolvente por canal, eje desde la media al píxel más
//! lejano y eje principal por potencia sobre la covarianza en RGBA) con
//! pasadas de mínimos cuadrados contra la paleta ya cuantizada. Como el
//! P-bit es único por extremo y se aplica a los cuatro canales, cada
//! cuantización rebusca los cuatro combos de paridad (uno por extremo)
//! y se queda con el que deja la paleta más cerca del bloque. El error
//! que se minimiza usa la métrica pedida: uniforme en modo lineal y con
//! luminancia BT.709 en los colores —más el alfa a peso pleno, que para
//! sprites es donde menos margen de error hay— en modo perceptual.
//!
//! Los bloques incompletos del borde se rellenan con clamp de borde, como
//! el resto de codificadores de hardware del proyecto.

use crate::config::DxtMode;
use crate::dxt::block_pixels;
use crate::error::Result;
use crate::pixels::ensure_rgba8;

/// Pesos de reconstrucción del índice de 4 bits (spec: `aWeights4`),
/// divididos entre 64.
const WEIGHTS4: [f32; 16] = [
    0.0, 4.0, 9.0, 13.0, 17.0, 21.0, 26.0, 30.0, 34.0, 38.0, 43.0, 47.0, 51.0, 55.0, 60.0, 64.0,
];
/// Ponderación BT.709 usada en `DxtMode::Perceptual`.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
/// Pasadas de refinamiento (mínimos cuadrados) por candidato.
const REFINE_ITERS: usize = 4;

#[inline]
fn weights(mode: DxtMode) -> [f32; 4] {
    match mode {
        DxtMode::Linear => [1.0, 1.0, 1.0, 1.0],
        // El alfa a peso pleno: los bordes de los sprites no admiten el
        // reequilibrio que la luminancia aplicaría al conjunto.
        DxtMode::Perceptual => [LUMA[0], LUMA[1], LUMA[2], 1.0],
    }
}

#[inline]
fn dist2(a: [u8; 4], b: [u8; 4], w: [f32; 4]) -> f32 {
    let mut acc = 0.0f32;
    for c in 0..4 {
        let d = a[c] as f32 - b[c] as f32;
        acc += w[c] * d * d;
    }
    acc
}

/// Paleta de 16 colores RGBA que reconstruye exactamente el decodificador
/// (entero con redondeo, `+32 >> 6`).
fn palette(e0: [u8; 4], e1: [u8; 4]) -> [[u8; 4]; 16] {
    let mut pal = [[0u8; 4]; 16];
    for (i, w) in WEIGHTS4.iter().enumerate() {
        let wi = *w as u32;
        for c in 0..4 {
            pal[i][c] = ((e0[c] as u32 * (64 - wi) + e1[c] as u32 * wi + 32) >> 6) as u8;
        }
    }
    pal
}

/// Índice (0-15) de la paleta más cercana al texel, con la métrica pedida.
fn best_index(p: [u8; 4], pal: &[[u8; 4]; 16], w: [f32; 4]) -> u8 {
    let mut best = 0usize;
    let mut best_err = f32::INFINITY;
    for (i, c) in pal.iter().enumerate() {
        let e = dist2(p, *c, w);
        if e < best_err {
            best_err = e;
            best = i;
        }
    }
    best as u8
}

/// Peso del extremo `e1` para el índice dado.
#[inline]
fn t_of(index: u8) -> f32 {
    WEIGHTS4[index as usize] / 64.0
}

/// Refinamiento por mínimos cuadrados: dado el reparto de índices, busca
/// los extremos ideales (sin cuantizar) que mejor aproximan cada canal.
fn least_squares(px: &[[u8; 4]; 16], idx: &[u8; 16]) -> Option<([f32; 4], [f32; 4])> {
    let mut n = 0.0f64;
    let mut sum_t = 0.0f64;
    let mut sum_p = [0.0f64; 4];
    let mut sum_tt = 0.0f64;
    let mut sum_tp = [0.0f64; 4];
    for (i, p) in px.iter().enumerate() {
        let t = t_of(idx[i]) as f64;
        n += 1.0;
        sum_t += t;
        sum_tt += t * t;
        for c in 0..4 {
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
    let mut c0 = [0.0f32; 4];
    let mut c1 = [0.0f32; 4];
    for c in 0..4 {
        let b = (n * sum_tp[c] - sum_t * sum_p[c]) / denom;
        let a = (sum_p[c] - b * sum_t) / n;
        c0[c] = a as f32;
        c1[c] = (a + b) as f32;
    }
    Some((c0, c1))
}

/// Resultado de ajustar un bloque: extremos de 8 bits, índices y error.
struct Fitted {
    e0: [u8; 4],
    e1: [u8; 4],
    indices: [u8; 16],
    error: f32,
}

/// Cuantiza un canal a la paridad del P-bit dado: el P-bit es único por
/// extremo y se aplica a los cuatro canales, así que todos los canales de
/// un extremo quedan con la misma paridad (el valor decodificado es
/// `(v7 << 1) | p`, sin replicación).
fn parity_quantize(c: f32, p: u8) -> u8 {
    let f = c.clamp(0.0, 255.0);
    let v = f.round() as i32;
    if (v as u8) & 1 == p {
        return v as u8;
    }
    // Los dos vecinos con la paridad pedida; se queda el más cerca.
    let a = (v - 1).clamp(0, 255);
    let b = (v + 1).clamp(0, 255);
    if (a as f32 - f).abs() <= (b as f32 - f).abs() {
        a as u8
    } else {
        b as u8
    }
}

fn parity_quantize_rgba(c: [f32; 4], p: u8) -> [u8; 4] {
    [
        parity_quantize(c[0], p),
        parity_quantize(c[1], p),
        parity_quantize(c[2], p),
        parity_quantize(c[3], p),
    ]
}

/// Extremos cuantizados eligiendo, entre los cuatro combos de P-bits
/// (uno por extremo), el que deja la paleta más cerca del bloque: la
/// paridad compartida impide cuantizar cada canal por separado.
fn quantize_pair(
    c0: [f32; 4],
    c1: [f32; 4],
    px: &[[u8; 4]; 16],
    w: [f32; 4],
) -> ([u8; 4], [u8; 4]) {
    let mut best = (parity_quantize_rgba(c0, 0), parity_quantize_rgba(c1, 0));
    let mut best_err = f32::INFINITY;
    for p0 in 0..=1u8 {
        for p1 in 0..=1u8 {
            let e0 = parity_quantize_rgba(c0, p0);
            let e1 = parity_quantize_rgba(c1, p1);
            let pal = palette(e0, e1);
            let mut err = 0.0f32;
            for p in px {
                let nearest = pal
                    .iter()
                    .map(|c| dist2(*p, *c, w))
                    .fold(f32::INFINITY, f32::min);
                err += nearest;
            }
            if err < best_err {
                best_err = err;
                best = (e0, e1);
            }
        }
    }
    best
}

/// Regla del ancla: el texel 0 guarda su índice en 3 bits, así que su
/// MSB debe ser 0. Si no lo es, se intercambian extremos: la paleta es
/// simétrica (`w[k] + w[15-k] = 64`), así que el intercambio (e0, e1) →
/// (e1, e0) acompañado de i → 15-i en todos los índices conserva el color
/// decodificado y deja al ancla por debajo de 8 siempre.
fn enforce_anchor(fit: &mut Fitted) {
    if fit.indices[0] < 8 {
        return;
    }
    std::mem::swap(&mut fit.e0, &mut fit.e1);
    for i in fit.indices.iter_mut() {
        *i = 15 - *i;
    }
}

/// Ajusta un bloque completo: candidatos iniciales + pasadas de mínimos
/// cuadrados, quedándose con el de menor error. El error se mide contra
/// la paleta ya cuantizada y con los índices ya ajustados al ancla.
fn fit_block(px: &[[u8; 4]; 16], mode: DxtMode) -> Fitted {
    let w = weights(mode);
    let mut best = Fitted {
        e0: [0; 4],
        e1: [0; 4],
        indices: [0; 16],
        error: f32::INFINITY,
    };
    for (start0, start1) in candidate_pairs(px) {
        let mut c0 = start0;
        let mut c1 = start1;
        let (mut e0, mut e1) = quantize_pair(c0, c1, px, w);
        let mut indices = [0u8; 16];
        for _ in 0..REFINE_ITERS {
            let pal = palette(e0, e1);
            for (i, p) in px.iter().enumerate() {
                indices[i] = best_index(*p, &pal, w);
            }
            let Some((n0, n1)) = least_squares(px, &indices) else {
                break;
            };
            c0 = n0;
            c1 = n1;
            (e0, e1) = quantize_pair(c0, c1, px, w);
        }
        let mut cand = Fitted {
            e0,
            e1,
            indices,
            error: 0.0,
        };
        let pal = palette(cand.e0, cand.e1);
        for (i, p) in px.iter().enumerate() {
            cand.indices[i] = best_index(*p, &pal, w);
        }
        enforce_anchor(&mut cand);
        let pal = palette(cand.e0, cand.e1);
        for (i, p) in px.iter().enumerate() {
            cand.error += dist2(*p, pal[cand.indices[i] as usize], w);
        }
        if cand.error < best.error {
            best = cand;
        }
    }
    best
}

/// Pares de extremos iniciales, como en [`crate::dxt`]: caja envolvente
/// por canal, eje desde la media hasta el píxel más lejano (en los dos
/// sentidos) y proyección sobre el eje principal (potencia sobre la
/// covarianza en RGBA, en los dos sentidos).
fn candidate_pairs(px: &[[u8; 4]; 16]) -> Vec<([f32; 4], [f32; 4])> {
    const N: f32 = 16.0;
    let mut min_c = [255.0f32; 4];
    let mut max_c = [0.0f32; 4];
    let mut mean = [0.0f32; 4];
    for p in px {
        for c in 0..4 {
            min_c[c] = min_c[c].min(p[c] as f32);
            max_c[c] = max_c[c].max(p[c] as f32);
            mean[c] += p[c] as f32 / N;
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
            p[3] as f32 - mean[3],
        ];
        let d2 = d.iter().map(|v| v * v).sum::<f32>();
        if d2 > far2 {
            far2 = d2;
            far = [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32];
        }
    }
    if far2 > 0.25 {
        out.push((mean, far));
        out.push((far, mean));
    }

    // Eje principal: potencia sobre la covarianza.
    let mut cov = [[0.0f32; 4]; 4];
    for p in px {
        let d = [
            p[0] as f32 - mean[0],
            p[1] as f32 - mean[1],
            p[2] as f32 - mean[2],
            p[3] as f32 - mean[3],
        ];
        for i in 0..4 {
            for j in 0..4 {
                cov[i][j] += d[i] * d[j] / N;
            }
        }
    }
    let mut v = [1.0f32; 4];
    for _ in 0..8 {
        let mut nv = [0.0f32; 4];
        for i in 0..4 {
            nv[i] = (0..4).map(|j| cov[i][j] * v[j]).sum();
        }
        let len = nv.iter().map(|x| x * x).sum::<f32>().sqrt();
        if len < 1e-6 {
            break;
        }
        v = [nv[0] / len, nv[1] / len, nv[2] / len, nv[3] / len];
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
            p[3] as f32 - mean[3],
        ];
        let t: f32 = (0..4).map(|j| d[j] * v[j]).sum();
        if t < min_p {
            min_p = t;
            min_px = [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32];
        }
        if t > max_p {
            max_p = t;
            max_px = [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32];
        }
    }
    if min_p.is_finite() && max_p.is_finite() && max_p - min_p > 0.5 {
        out.push((min_px, max_px));
        out.push((max_px, min_px));
    }
    out
}

/// Escritor de bits LSB-first (el orden del spec de BC7).
struct Bits {
    acc: u128,
    pos: usize,
}

impl Bits {
    const fn new() -> Self {
        Bits { acc: 0, pos: 0 }
    }

    fn push(&mut self, value: u32, n: usize) {
        debug_assert!(n == 0 || (value as u128) < (1u128 << n));
        debug_assert!(self.pos + n <= 128);
        self.acc |= (value as u128) << self.pos;
        self.pos += n;
    }
}

/// Bloque BC7 de 16 bytes en modo 6.
fn encode_bc7_block(px: &[[u8; 4]; 16], mode: DxtMode) -> [u8; 16] {
    let fit = fit_block(px, mode);
    let mut bits = Bits::new();
    // Modo 6: un 1 tras seis 0 (7 bits).
    bits.push(0b1000000, 7);
    // Extremos: 7 bits por canal, canal a canal. El P-bit del extremo
    // (su bit 0) se aplica a los cuatro canales, así que un extremo solo
    // admite valores con paridad uniforme (ya decidida al ajustar).
    for c in 0..4 {
        bits.push((fit.e0[c] >> 1) as u32, 7);
        bits.push((fit.e1[c] >> 1) as u32, 7);
    }
    bits.push((fit.e0[0] & 1) as u32, 1);
    bits.push((fit.e1[0] & 1) as u32, 1);
    // Índices: el ancla en 3 bits (su MSB ya es 0), el resto en 4.
    bits.push(fit.indices[0] as u32, 3);
    for i in 1..16 {
        bits.push(fit.indices[i] as u32, 4);
    }
    debug_assert_eq!(bits.pos, 128);
    let bytes = bits.acc.to_le_bytes();
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    out
}

/// Codifica una imagen RGBA8 como BC7 (modo 6): 16 bytes por bloque de 4×4.
pub fn encode_bc7(rgba: &[u8], width: usize, height: usize, mode: DxtMode) -> Result<Vec<u8>> {
    ensure_rgba8(rgba, width, height)?;
    let bx = width.div_ceil(4);
    let by = height.div_ceil(4);
    let mut out = Vec::with_capacity(bx * by * 16);
    for y0 in 0..by {
        for x0 in 0..bx {
            let pixels = block_pixels(rgba, width, height, x0, y0);
            out.extend_from_slice(&encode_bc7_block(&pixels, mode));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodifica con la implementación independiente `texture2ddecoder`
    /// (la misma que valida PVRTC y ETC2) y devuelve píxeles RGBA en orden
    /// de fila.
    fn decode7(data: &[u8], w: usize, h: usize) -> Vec<[u8; 4]> {
        let mut buf = vec![0u32; w * h];
        texture2ddecoder::decode_bc7(data, w, h, &mut buf).expect("decode bc7");
        buf.iter()
            .map(|v| {
                let b = v.to_le_bytes();
                [b[2], b[1], b[0], b[3]]
            })
            .collect()
    }

    fn gradient_rgba(w: usize, h: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                out.extend_from_slice(&[
                    (x * 255 / w.max(1)) as u8,
                    (y * 255 / h.max(1)) as u8,
                    ((x + y) * 255 / (w + h).max(1)) as u8,
                    (y * 255 / h.max(1)) as u8,
                ]);
            }
        }
        out
    }

    /// Error absoluto medio por canal entre la fuente y lo que devuelve el
    /// decodificador (los 4 canales).
    fn mean_err(src: &[u8], got: &[[u8; 4]]) -> f64 {
        let mut err = 0.0f64;
        let mut n = 0usize;
        for (i, p) in src.chunks_exact(4).enumerate() {
            for c in 0..4 {
                err += (p[c] as f64 - got[i][c] as f64).abs();
            }
            n += 1;
        }
        err / (n * 4) as f64
    }

    /// Codificador de referencia (extremos de la caja envolvente, sin
    /// refinamiento) y su error cuadrático con la métrica del modo pedido.
    fn naive_reference(src: &[u8], w: usize, h: usize, mode: DxtMode) -> f64 {
        let wts = weights(mode);
        let mut err = 0.0f64;
        for y0 in 0..h.div_ceil(4) {
            for x0 in 0..w.div_ceil(4) {
                let mut px = [[0u8; 4]; 16];
                for ty in 0..4 {
                    for tx in 0..4 {
                        let x = (x0 * 4 + tx).min(w - 1);
                        let y = (y0 * 4 + ty).min(h - 1);
                        let i = (y * w + x) * 4;
                        px[ty * 4 + tx] = [src[i], src[i + 1], src[i + 2], src[i + 3]];
                    }
                }
                let mut lo = [u8::MAX; 4];
                let mut hi = [u8::MIN; 4];
                for p in &px {
                    for c in 0..4 {
                        lo[c] = lo[c].min(p[c]);
                        hi[c] = hi[c].max(p[c]);
                    }
                }
                let pal = palette(lo, hi);
                let mut idx = [0u8; 16];
                for (i, p) in px.iter().enumerate() {
                    idx[i] = best_index(*p, &pal, wts);
                }
                for (i, p) in px.iter().enumerate() {
                    err += dist2(*p, pal[idx[i] as usize], wts) as f64;
                }
            }
        }
        err / ((w * h * 4) as f64)
    }

    /// Error cuadrático medio (con la métrica del modo) de nuestro encoder.
    fn our_error(src: &[u8], w: usize, h: usize, mode: DxtMode) -> f64 {
        let wts = weights(mode);
        let got = decode7(&encode_bc7(src, w, h, mode).unwrap(), w, h);
        let mut err = 0.0f64;
        for (i, p) in src.chunks_exact(4).enumerate() {
            let p: [u8; 4] = p.try_into().unwrap();
            err += dist2(p, got[i], wts) as f64;
        }
        err / ((w * h * 4) as f64)
    }

    #[test]
    fn bc7_decodes_close_to_the_source() {
        let (w, h) = (16, 16);
        let src = gradient_rgba(w, h);
        for mode in [DxtMode::Linear, DxtMode::Perceptual] {
            let data = encode_bc7(&src, w, h, mode).unwrap();
            assert_eq!(data.len(), (w / 4) * (h / 4) * 16);
            let got = decode7(&data, w, h);
            let mean = mean_err(&src, &got);
            // El contenido es una superficie 2D en el espacio RGBA (el
            // color en dos ejes independientes): una recta de un solo
            // subconjunto —el modo 6— no puede seguirla mejor que esto
            // (medido: 7.04 lineal, 6.26 perceptual); la calidad relativa
            // la fijan los tests de referencia y el de BC3.
            assert!(mean < 9.0, "{mode:?}: error medio {mean:.2}");
        }
    }

    #[test]
    fn bc7_follows_a_one_dimensional_ramp_tightly() {
        // Contenido colineal (un solo eje RGBA compartido por los cuatro
        // canales): la paleta de 16 pasos sí puede representarlo casi
        // exactamente.
        let (w, h) = (16, 16);
        let mut src = Vec::with_capacity(w * h * 4);
        for _ in 0..h {
            for x in 0..w {
                let v = (x * 255 / (w - 1)) as u8;
                src.extend_from_slice(&[v, v, v, v]);
            }
        }
        for mode in [DxtMode::Linear, DxtMode::Perceptual] {
            let got = decode7(&encode_bc7(&src, w, h, mode).unwrap(), w, h);
            let mean = mean_err(&src, &got);
            assert!(mean < 2.0, "{mode:?}: error medio {mean:.2}");
        }
    }

    #[test]
    fn bc7_is_never_worse_than_a_min_max_reference() {
        // Regresión del buscador: candidatos iniciales + mínimos cuadrados
        // no pueden quedarse peor, en error cuadrático con la métrica del
        // modo, que la elección ingenua de los extremos de la caja.
        let (w, h) = (16, 16);
        let src = gradient_rgba(w, h);
        for mode in [DxtMode::Linear, DxtMode::Perceptual] {
            let ours = our_error(&src, w, h, mode);
            let naive = naive_reference(&src, w, h, mode);
            assert!(
                ours <= naive,
                "{mode:?}: nuestro {ours:.3} vs referencia {naive:.3}"
            );
        }
    }

    /// El alfa binario de un sprite con dos zonas (opaco y transparente)
    /// se representa exacto: dos extremos bastan y el índice compartido
    /// no tiene por qué comprometer el corte. Los colores usan una
    /// paridad de canal uniforme porque el P-bit es compartido dentro de
    /// cada extremo.
    #[test]
    fn bc7_keeps_hard_alpha_edges_exact() {
        let (w, h) = (4, 4);
        let mut src = Vec::with_capacity(w * h * 4);
        for i in 0..w * h {
            if i % 2 == 0 {
                src.extend_from_slice(&[201, 101, 51, 255]);
            } else {
                src.extend_from_slice(&[10, 220, 90, 0]);
            }
        }
        let got = decode7(&encode_bc7(&src, w, h, DxtMode::Linear).unwrap(), w, h);
        for (i, p) in src.chunks_exact(4).enumerate() {
            assert_eq!(got[i][3], p[3], "texel {i}: el alfa debe salir exacto");
            for c in 0..3 {
                assert_eq!(got[i][c], p[c], "texel {i}: el color debe salir exacto");
            }
        }
    }

    /// El bloque completamente plano sale sin error (colores con paridad
    /// de canal uniforme, como exige el P-bit compartido del extremo).
    #[test]
    fn bc7_solid_blocks_are_exact() {
        let (w, h) = (8, 8);
        for color in [[0u8, 0, 0, 0], [255, 255, 255, 255], [123, 77, 201, 151]] {
            let src: Vec<u8> = color.iter().cycle().take(w * h * 4).copied().collect();
            let got = decode7(&encode_bc7(&src, w, h, DxtMode::Linear).unwrap(), w, h);
            assert!(
                got.iter().all(|p| *p == color),
                "bloque plano {color:?}: sale exacto"
            );
        }
    }

    /// El texel ancla (índice 0) solo admite la mitad baja de la paleta:
    /// el codificador invierte extremos si hace falta. Un bloque cuyo
    /// primer texel es el más extremo obliga a esa inversión.
    #[test]
    fn bc7_anchor_index_stays_in_the_low_half() {
        let mut px = [[0u8; 4]; 16];
        for (i, p) in px.iter_mut().enumerate() {
            let v = (i * 255 / 15) as u8;
            *p = [v, v, v, 255];
        }
        // El primer texel pasa a ser el más claro (índice natural 15).
        px.swap(0, 15);
        let fit = fit_block(&px, DxtMode::Linear);
        assert!(
            fit.indices[0] < 8,
            "el ancla debe quedar en la mitad baja: {}",
            fit.indices[0]
        );
        // Y la inversión no rompe la decodificación del propio texel.
        let pal = palette(fit.e0, fit.e1);
        let got = pal[fit.indices[0] as usize];
        let err: i32 = (0..4)
            .map(|c| (got[c] as i32 - px[0][c] as i32).abs())
            .sum();
        assert!(err <= 24, "el ancla decodificada se queda cerca: {err}");
    }

    /// Los bits del bloque llenan los 128 exactos y el modo es el 6.
    #[test]
    fn bc7_mode_bits_and_block_size() {
        let src = gradient_rgba(4, 4);
        let block = encode_bc7(&src, 4, 4, DxtMode::Linear).unwrap();
        assert_eq!(block.len(), 16);
        // Los 7 bits del modo: un 1 tras seis 0, en LSB-first.
        assert_eq!(block[0] & 0x7F, 0b1000000, "cabecera del modo 6");
    }

    #[test]
    fn non_multiple_of_four_dimensions_are_clamped() {
        let (w, h) = (6, 5);
        let src = gradient_rgba(w, h);
        let data = encode_bc7(&src, w, h, DxtMode::Perceptual).unwrap();
        assert_eq!(data.len(), 2 * 2 * 16);
        decode7(&data, w, h);
    }

    /// Como en DXT (M11): un buffer corto devuelve `Err`, no paniquea.
    #[test]
    fn buffer_corto_devuelve_error_en_vez_de_paniquear() {
        let corto = [0u8; 16];
        assert!(encode_bc7(&corto, 4, 4, DxtMode::Linear).is_err());
        let justo = [0u8; 64];
        assert!(encode_bc7(&justo, 4, 4, DxtMode::Linear).is_ok());
    }

    /// BC7 es el formato de alta calidad: en el mismo contenido con alfa,
    /// su error (4 canales) debe quedar por debajo del de BC3/DXT5.
    #[test]
    fn bc7_supera_a_dxt5_en_calidad() {
        let (w, h) = (32, 32);
        let src = gradient_rgba(w, h);
        let bc7 = our_error(&src, w, h, DxtMode::Perceptual);
        let dxt5 = {
            let data = crate::dxt::encode_dxt5(&src, w, h, DxtMode::Perceptual).unwrap();
            let mut buf = vec![0u32; w * h];
            texture2ddecoder::decode_bc3(&data, w, h, &mut buf).expect("decode bc3");
            let got: Vec<[u8; 4]> = buf
                .iter()
                .map(|v| {
                    let b = v.to_le_bytes();
                    [b[2], b[1], b[0], b[3]]
                })
                .collect();
            let wts = weights(DxtMode::Perceptual);
            let mut err = 0.0f64;
            for (i, p) in src.chunks_exact(4).enumerate() {
                let p: [u8; 4] = p.try_into().unwrap();
                err += dist2(p, got[i], wts) as f64;
            }
            err / ((w * h * 4) as f64)
        };
        assert!(bc7 < dxt5, "BC7 debe mejorar a BC3: {bc7:.4} vs {dxt5:.4}");
    }
}
