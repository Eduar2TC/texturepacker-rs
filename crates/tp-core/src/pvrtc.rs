//! PVRTC1 4bpp encoder (GL_COMPRESSED_RGBA_PVRTC_4BPPV1_IMG).
//!
//! Produces *valid* PVRTC1 4bpp streams that any conformant decoder can
//! decompress. The bit layout follows the Khronos Data Format specification
//! (`pvrtc.txt`, derived from Imagination's PVRTC whitepaper):
//!
//! - Each 64-bit word covers a 4×4 texel block: bits 31..0 hold the
//!   modulation data (2 bits per texel), bits 47..32 hold colour A (15 bits)
//!   plus the modulation flag `M` at bit 32, and bits 63..48 hold colour B.
//!   Words are stored in reflected Morton order.
//! - Each 16-bit colour: bit 15 = opaque flag. Opaque (1): A = R5 G5 B4,
//!   B = R5 G5 B5 (alpha 255). Translucent (0): A = A3 R4 G4 B3,
//!   B = A3 R4 G4 B4. The 3-bit alpha is zero-padded to 4 bits on decode;
//!   RGB channels with fewer than 5 bits are expanded by bit replication.
//! - The two low-resolution colour images are bilinearly upscaled 4× in both
//!   dimensions with toroidal wrap: every texel blends the 2×2 colour samples
//!   nearest to its (x − 2, y − 2) position. The modulation data then selects
//!   one of the weights {0, 3, 5, 8} (standard mode, `M` = 0) to interpolate
//!   between image A and image B.
//!
//! Note on `texture2ddecoder`: its PVRTC decoder is conformant for 4bpp with
//! one quirk — it reconstructs the *opaque* colour-B blue channel from 4 bits
//! plus a duplicated bit 3, instead of the 5 real bits the spec defines. This
//! encoder therefore stores the blue LSB as a duplicate of bit 3, so both the
//! Khronos layout and `texture2ddecoder` decode identical values.
//!
//! Encoder strategy: an independent per-block two-colour fit (mean plus the
//! dominant RGB axis), followed by an exhaustive per-texel modulation search
//! against the exact interpolation the decoder performs. Not iterative like
//! PVRTexTool HQ, but valid and good enough for game assets. Alpha uses the
//! format's coarse 3-bit translucent endpoints (blocks with alpha ≥ 250 use
//! the opaque encoding, which reproduces alpha 255 exactly).
//!
//! PVRTC1 requires power-of-two dimensions with at least two words per side,
//! so textures smaller than 8×8 or non-power-of-two sizes return an error.

/// Canonical decoded colour (RGB 0-31, alpha 0-15) as reconstructed by the
/// decoder from a colour word. Modulation and interpolation operate on these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Colour {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

/// The two colour endpoints (A = "high", B = "low") of one 4×4 block, in
/// canonical form, plus each endpoint's opacity mode.
#[derive(Clone, Copy, Debug)]
struct BlockColours {
    a: Colour,
    b: Colour,
    a_opaque: bool,
    b_opaque: bool,
}

impl Default for BlockColours {
    fn default() -> Self {
        Self {
            a: Colour {
                r: 0,
                g: 0,
                b: 0,
                a: 15,
            },
            b: Colour {
                r: 0,
                g: 0,
                b: 0,
                a: 15,
            },
            a_opaque: true,
            b_opaque: true,
        }
    }
}

use crate::error::Result;

// ---------------------------------------------------------------------------
// Quantization helpers (canonical values are what the decoder reconstructs)
// ---------------------------------------------------------------------------

/// 8-bit value → 5-bit channel (0-31).
fn q5(v: f64) -> u8 {
    ((v.round() as i32).clamp(0, 255) >> 3) as u8
}

/// 5-bit value → 4-bit storage with MSB replication: returns the canonical
/// 5-bit value the decoder reconstructs. `q4rep(31) == 31`, `q4rep(30) == 31`.
fn q4rep(v5: u8) -> u8 {
    let q = v5 >> 1;
    (q << 1) | (q >> 3)
}

/// 5-bit value → 3-bit storage with 2-bit replication: canonical 5-bit value.
fn q3rep(v5: u8) -> u8 {
    let q = v5 >> 2;
    (q << 2) | (q >> 1)
}

/// 8-bit alpha → canonical 0-15 (3 stored bits, zero-padded on decode).
fn qa(v: u8) -> u8 {
    let a3 = ((v as u32 * 7 + 127) / 255) as u8;
    a3 << 1
}

/// Pack a canonical colour into a 16-bit PVRTC colour word.
/// `is_a` selects colour A's bit 32 = modulation flag `M` (always 0 here);
/// colour B's bit 0 is a real colour bit (duplicated per the quirk above).
fn pack_word(c: Colour, opaque: bool, is_a: bool) -> u16 {
    if opaque {
        let r = (c.r & 0x1f) as u16;
        let g = (c.g & 0x1f) as u16;
        // canonical blue = (b4 << 1) | (b4 >> 3)  →  b4 = canonical >> 1.
        let b4 = ((c.b >> 1) & 0xf) as u16;
        let m_bit: u16 = if is_a { 0 } else { (b4 >> 3) & 1 };
        0x8000 | (r << 10) | (g << 5) | (b4 << 1) | m_bit
    } else {
        let a3 = (c.a >> 1) & 0x7;
        let r4 = (c.r >> 1) & 0xf;
        let g4 = (c.g >> 1) & 0xf;
        if is_a {
            // B3 at bits 3..1, bit 0 = M = 0.
            let b3 = (c.b >> 2) & 0x7;
            (a3 as u16) << 12 | (r4 as u16) << 8 | (g4 as u16) << 4 | (b3 as u16) << 1
        } else {
            // B4 at bits 3..0.
            let b4 = (c.b >> 1) & 0xf;
            (a3 as u16) << 12 | (r4 as u16) << 8 | (g4 as u16) << 4 | b4 as u16
        }
    }
}

// ---------------------------------------------------------------------------
// Block fit
// ---------------------------------------------------------------------------

/// 8-bit representation of an isolated canonical colour: exactly what the
/// decoder reconstructs when all nine neighbours are this same colour (RGB
/// expands a 5-bit channel by replication, alpha maps 0-15 to 0-255).
#[inline]
fn expand8(c: Colour) -> [u8; 4] {
    [
        (c.r << 3) | (c.r >> 2),
        (c.g << 3) | (c.g >> 2),
        (c.b << 3) | (c.b >> 2),
        c.a * 17,
    ]
}

/// Builds the two endpoints from candidate RGB positions, applying the same
/// quantization `fit_base` uses (opaque channels are 5-bit, translucent ones
/// are stored in 4/3 bits and expanded back).
fn make_endpoints(p0: [f64; 3], p1: [f64; 3], max_a: u8, min_a: u8) -> BlockColours {
    let a_opaque = max_a >= 250;
    let b_opaque = min_a >= 250;
    let mk = |p: [f64; 3], alpha: u8, opaque: bool, is_a: bool| -> Colour {
        let (r, g, b) = (q5(p[0]), q5(p[1]), q5(p[2]));
        if opaque {
            Colour {
                r,
                g,
                b: q4rep(b),
                a: 15,
            }
        } else if is_a {
            Colour {
                r: q4rep(r),
                g: q4rep(g),
                b: q3rep(b),
                a: qa(alpha),
            }
        } else {
            Colour {
                r: q4rep(r),
                g: q4rep(g),
                b: q4rep(b),
                a: qa(alpha),
            }
        }
    };
    BlockColours {
        a: mk(p0, max_a, a_opaque, true),
        b: mk(p1, min_a, b_opaque, false),
        a_opaque,
        b_opaque,
    }
}

/// Quantized per-block extents used by every candidate generator: colour
/// min/max, per-channel mean, farthest visible texel and the alpha pair.
struct Extents {
    min_c: [f64; 3],
    max_c: [f64; 3],
    mean: [f64; 3],
    far: [f64; 3],
    min_a: u8,
    max_a: u8,
    visible: Vec<[f64; 3]>,
}

fn extents(px: &[u8]) -> Extents {
    let n = px.len() / 4;
    let mut min_c = [f64::INFINITY; 3];
    let mut max_c = [f64::NEG_INFINITY; 3];
    let mut sum = [0f64; 3];
    let mut min_a = 255u8;
    let mut max_a = 0u8;
    let mut visible = Vec::with_capacity(n);
    for c in px.chunks_exact(4) {
        min_a = min_a.min(c[3]);
        max_a = max_a.max(c[3]);
        for k in 0..3 {
            min_c[k] = min_c[k].min(c[k] as f64);
            max_c[k] = max_c[k].max(c[k] as f64);
            sum[k] += c[k] as f64;
        }
        if c[3] >= 8 {
            visible.push([c[0] as f64, c[1] as f64, c[2] as f64]);
        }
    }
    let mean = [sum[0] / n as f64, sum[1] / n as f64, sum[2] / n as f64];
    let mut far = mean;
    let mut far2 = -1f64;
    for &v in &visible {
        let d2 = (0..3)
            .map(|k| (v[k] - mean[k]) * (v[k] - mean[k]))
            .sum::<f64>();
        if d2 > far2 {
            far2 = d2;
            far = v;
        }
    }
    Extents {
        min_c,
        max_c,
        mean,
        far,
        min_a,
        max_a,
        visible,
    }
}

/// Fit two canonical colour endpoints for a block of RGBA texels (4×4 for
/// 4bpp, 8×4 for 2bpp).
///
/// - RGB: the baseline is the mean of the visible texels (alpha ≥ 8) plus the
///   two extremes along the axis from the mean to the farthest texel, so the
///   A/B segment brackets the block's colour distribution.
/// - Alpha: endpoint alpha = max/min of the block (opaque mode when ≥ 250).
///
/// `quality` (0-7) widens the candidate set before the best pair is kept; 0
/// reproduces the baseline exactly.
fn fit_block(px: &[u8], quality: u8, binary: bool) -> BlockColours {
    let mut best = fit_base(px);
    let mut best_err = proxy_err(px, &best, binary);
    if quality == 0 {
        return best;
    }
    for cand in quality_candidates(px, quality) {
        let e = proxy_err(px, &cand, binary);
        if e < best_err {
            best_err = e;
            best = cand;
        }
    }
    if quality >= 6 {
        best = refine(px, best, binary, (quality - 5) as usize);
    }
    best
}

/// Baseline fit: mean + farthest visible texel along the dominant axis.
fn fit_base(px: &[u8]) -> BlockColours {
    let ex = extents(px);
    let (mr, mg, mb) = (ex.mean[0], ex.mean[1], ex.mean[2]);
    let (fr, fg, fb) = (ex.far[0] - mr, ex.far[1] - mg, ex.far[2] - mb);
    let far2 = fr * fr + fg * fg + fb * fb;

    let (p0, p1) = if far2 <= 0.25 {
        (ex.mean, ex.mean)
    } else {
        let len = far2.sqrt();
        let (ux, uy, uz) = (fr / len, fg / len, fb / len);
        let mut minp = f64::INFINITY;
        let mut maxp = f64::NEG_INFINITY;
        for &v in &ex.visible {
            let p = (v[0] - mr) * ux + (v[1] - mg) * uy + (v[2] - mb) * uz;
            minp = minp.min(p);
            maxp = maxp.max(p);
        }
        let clamp = |v: f64| v.clamp(0.0, 255.0);
        (
            [
                clamp(mr + ux * maxp),
                clamp(mg + uy * maxp),
                clamp(mb + uz * maxp),
            ],
            [
                clamp(mr + ux * minp),
                clamp(mg + uy * minp),
                clamp(mb + uz * minp),
            ],
        )
    };
    make_endpoints(p0, p1, ex.max_a, ex.min_a)
}

/// Extra endpoint candidates, gated by `quality` (they are only added when
/// the caller can afford them, and the best one wins, so quality is
/// monotonically non-decreasing in the proxy error).
fn quality_candidates(px: &[u8], quality: u8) -> Vec<BlockColours> {
    let ex = extents(px);
    let mut out: Vec<BlockColours> = Vec::new();
    if quality >= 1 {
        out.push(make_endpoints(ex.min_c, ex.max_c, ex.max_a, ex.min_a));
        if let Some(axis) = principal_axis(&ex.visible) {
            out.push(make_endpoints(axis.0, axis.1, ex.max_a, ex.min_a));
        }
    }
    if quality >= 2 {
        out.push(make_endpoints(ex.mean, ex.far, ex.max_a, ex.min_a));
        out.push(make_endpoints(ex.far, ex.mean, ex.max_a, ex.min_a));
    }
    if quality >= 3 {
        for &v in &ex.visible {
            out.push(make_endpoints(ex.mean, v, ex.max_a, ex.min_a));
        }
    }
    if quality >= 5 && ex.visible.len() >= 8 {
        // Medias de los semibloques: capturan bloques con dos zonas.
        let mut top = [0f64; 3];
        let mut bot = [0f64; 3];
        let mut left = [0f64; 3];
        let mut right = [0f64; 3];
        for (i, c) in px.chunks_exact(4).enumerate() {
            let row = i / 4;
            let col = i % 4;
            if row < 2 {
                for k in 0..3 {
                    top[k] += c[k] as f64 / 8.0;
                }
            } else {
                for k in 0..3 {
                    bot[k] += c[k] as f64 / 8.0;
                }
            }
            if col < 2 {
                for k in 0..3 {
                    left[k] += c[k] as f64 / 8.0;
                }
            } else {
                for k in 0..3 {
                    right[k] += c[k] as f64 / 8.0;
                }
            }
        }
        out.push(make_endpoints(top, bot, ex.max_a, ex.min_a));
        out.push(make_endpoints(left, right, ex.max_a, ex.min_a));
    }
    out
}

/// First eigenvector of the covariance matrix by power iteration, used as a
/// candidate axis (returns the extremes projected onto it).
fn principal_axis(vis: &[[f64; 3]]) -> Option<([f64; 3], [f64; 3])> {
    if vis.len() < 2 {
        return None;
    }
    let n = vis.len() as f64;
    let mut mean = [0f64; 3];
    for v in vis {
        for k in 0..3 {
            mean[k] += v[k] / n;
        }
    }
    let mut cov = [[0f64; 3]; 3];
    for v in vis {
        let d = [v[0] - mean[0], v[1] - mean[1], v[2] - mean[2]];
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j] / n;
            }
        }
    }
    let mut axis = [1.0f64, 1.0, 1.0];
    for _ in 0..8 {
        let mut nv = [0f64; 3];
        for i in 0..3 {
            nv[i] = cov[i][0] * axis[0] + cov[i][1] * axis[1] + cov[i][2] * axis[2];
        }
        let len = (nv[0] * nv[0] + nv[1] * nv[1] + nv[2] * nv[2]).sqrt();
        if len < 1e-9 {
            return None;
        }
        axis = [nv[0] / len, nv[1] / len, nv[2] / len];
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut lo_c = mean;
    let mut hi_c = mean;
    for &v in vis {
        let d = [v[0] - mean[0], v[1] - mean[1], v[2] - mean[2]];
        let t = d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2];
        if t < lo {
            lo = t;
            lo_c = v;
        }
        if t > hi {
            hi = t;
            hi_c = v;
        }
    }
    Some((hi_c, lo_c))
}

/// Canonical value tables actually reachable through `pack_word`/`pack_word`
/// decoding: a perturbation outside these sets would round-trip to the same
/// stored word and change nothing.
const Q4REP_SET: [u8; 16] = [0, 2, 4, 6, 8, 10, 12, 14, 17, 19, 21, 23, 25, 27, 29, 31];
const Q3REP_SET: [u8; 8] = [0, 4, 8, 12, 17, 21, 25, 29];
const QA_SET: [u8; 8] = [0, 2, 4, 6, 8, 10, 12, 14];

fn step_in(table: &[u8], v: u8, dir: i32) -> Option<u8> {
    let i = table.iter().position(|&x| x == v)?;
    let j = i as i32 + dir;
    if j < 0 || j as usize >= table.len() {
        return None;
    }
    Some(table[j as usize])
}

/// Moves one channel of one endpoint by one canonical step, if that step is
/// representable in the stored word (opacity rules decide which table).
fn step_endpoint(blk: &BlockColours, is_a: bool, ch: usize, dir: i32) -> Option<BlockColours> {
    let mut next = *blk;
    let mut c = if is_a { blk.a } else { blk.b };
    let opaque = if is_a { blk.a_opaque } else { blk.b_opaque };
    let moved = match ch {
        0 | 1 => {
            let cur = if ch == 0 { c.r } else { c.g };
            let nv = if opaque {
                Some((cur as i32 + dir).clamp(0, 31) as u8)
            } else {
                step_in(&Q4REP_SET, cur, dir)
            };
            nv.filter(|&v| v != cur).map(|v| {
                if ch == 0 {
                    c.r = v;
                } else {
                    c.g = v;
                }
            })
        }
        2 => {
            let nv = if !opaque && is_a {
                step_in(&Q3REP_SET, c.b, dir)
            } else {
                step_in(&Q4REP_SET, c.b, dir)
            };
            nv.filter(|&v| v != c.b).map(|v| c.b = v)
        }
        3 => {
            if opaque {
                None
            } else {
                step_in(&QA_SET, c.a, dir)
                    .filter(|&v| v != c.a)
                    .map(|v| c.a = v)
            }
        }
        _ => None,
    };
    moved?;
    if is_a {
        next.a = c;
    } else {
        next.b = c;
    }
    Some(next)
}

/// Greedy local search around the winning pair: `rounds` sweeps of ±1
/// canonical steps over every endpoint channel (quality 6 and 7).
fn refine(px: &[u8], start: BlockColours, binary: bool, rounds: usize) -> BlockColours {
    let mut cur = start;
    let mut cur_err = proxy_err(px, &cur, binary);
    for _ in 0..rounds {
        let mut improved = false;
        for is_a in [true, false] {
            for ch in 0..4 {
                for dir in [-1, 1] {
                    if let Some(next) = step_endpoint(&cur, is_a, ch, dir) {
                        let e = proxy_err(px, &next, binary);
                        if e < cur_err {
                            cur_err = e;
                            cur = next;
                            improved = true;
                        }
                    }
                }
            }
        }
        if !improved {
            break;
        }
    }
    cur
}

/// Proxy error of a candidate pair: what the block would cost if the
/// decoder's interpolation returned these exact endpoints (it ignores the
/// neighbourhood, which is why higher quality is a search, not a guarantee).
fn proxy_err(px: &[u8], blk: &BlockColours, binary: bool) -> i64 {
    let a8 = expand8(blk.a);
    let b8 = expand8(blk.b);
    let table: &[(i32, u8)] = if binary {
        &[(0, 0), (8, 1)]
    } else {
        &MOD_WEIGHTS
    };
    let mut total = 0i64;
    for c in px.chunks_exact(4) {
        let mut best = i64::MAX;
        for &(w, _) in table {
            let mut e = 0i64;
            for ch in 0..4 {
                let v = (a8[ch] as i32 * (8 - w) + b8[ch] as i32 * w) >> 3;
                let d = c[ch] as i64 - v as i64;
                e += d * d;
            }
            best = best.min(e);
        }
        total += best;
    }
    total
}

// ---------------------------------------------------------------------------
// Colour interpolation (mirrors the decoder exactly)
// ---------------------------------------------------------------------------

/// Per-texel weights over the 3 block columns/rows {−1, 0, +1} for texel
/// offsets 0..4 — identical to the reference decoder's `INTERP_WEIGHT`. Used
/// for both axes of 4bpp blocks and for the Y axis of 2bpp blocks.
const INTERP: [[i32; 3]; 4] = [[2, 2, 0], [1, 3, 0], [0, 4, 0], [0, 3, 1]];

/// 2bpp X axis: 8 texel offsets, weights summing to 8 (the 2bpp kernel).
const INTERP_X8: [[i32; 3]; 8] = [
    [4, 4, 0],
    [3, 5, 0],
    [2, 6, 0],
    [1, 7, 0],
    [0, 8, 0],
    [0, 7, 1],
    [0, 6, 2],
    [0, 5, 3],
];

/// One colour plane (A or B) of the block grid.
struct Plane<'a> {
    blocks: &'a [BlockColours],
    nb_x: usize,
    nb_y: usize,
    /// `true` for 2bpp (8×4 blocks, different X kernel and expansion).
    wide: bool,
}

impl Plane<'_> {
    /// Decoded 8-bit colour at texel (tx, ty) of block (bx, by), obtained by
    /// bilinearly interpolating the 3×3 colour neighbourhood (toroidal wrap)
    /// exactly as the PVRTC decoder does, including the UNORM conversions
    /// (4bpp RGB: `(c >> 1) + (c >> 6)`; alpha: `c + (c >> 4)`; 2bpp scales
    /// the same result by its 32-wide weight sums).
    fn interpolate(&self, bx: usize, by: usize, tx: usize, ty: usize, use_b: bool) -> [u8; 4] {
        let (blocks, nb_x, nb_y) = (self.blocks, self.nb_x, self.nb_y);
        let row_weights = INTERP[ty];
        let col_weights = if self.wide { INTERP_X8[tx] } else { INTERP[tx] };
        let mut clr = [0i32; 4];
        for (dy, &wy) in row_weights.iter().enumerate() {
            let yb = (by + nb_y - 1 + dy) % nb_y;
            for (dx, &wx) in col_weights.iter().enumerate() {
                let xb = (bx + nb_x - 1 + dx) % nb_x;
                let c = if use_b {
                    blocks[yb * nb_x + xb].b
                } else {
                    blocks[yb * nb_x + xb].a
                };
                let w = wx * wy;
                clr[0] += c.r as i32 * w;
                clr[1] += c.g as i32 * w;
                clr[2] += c.b as i32 * w;
                clr[3] += c.a as i32 * w;
            }
        }
        if self.wide {
            [
                (clr[0] >> 2) + (clr[0] >> 7),
                (clr[1] >> 2) + (clr[1] >> 7),
                (clr[2] >> 2) + (clr[2] >> 7),
                (clr[3] >> 1) + (clr[3] >> 5),
            ]
        } else {
            [
                (clr[0] >> 1) + (clr[0] >> 6),
                (clr[1] >> 1) + (clr[1] >> 6),
                (clr[2] >> 1) + (clr[2] >> 6),
                (clr[3]) + (clr[3] >> 4),
            ]
        }
        .map(|v: i32| v as u8)
    }
}

// ---------------------------------------------------------------------------
// Modulation
// ---------------------------------------------------------------------------

/// Standard modulation weights (M = 0): bits 00/01/10/11 → 0/3/5/8.
const MOD_WEIGHTS: [(i32, u8); 4] = [(0, 0), (3, 1), (5, 2), (8, 3)];

/// Pick the 2 modulation bits whose blend of `a8`/`b8` best matches `src`
/// (sum of squared errors over all four channels).
fn choose_mod(src: &[u8; 4], a8: &[u8; 4], b8: &[u8; 4]) -> u8 {
    let mut best = 0u8;
    let mut best_err = i64::MAX;
    for (w, bits) in MOD_WEIGHTS {
        let mut err = 0i64;
        for ch in 0..4 {
            let v = ((a8[ch] as i32 * (8 - w) + b8[ch] as i32 * w) >> 3) as u8;
            let d = src[ch] as i64 - v as i64;
            err += d * d;
        }
        if err < best_err {
            best_err = err;
            best = bits;
        }
    }
    best
}

/// 2bpp in modulation mode M = 0: one bit per texel, weight 0 or 8. The M = 1
/// mode would give 4 weights but borrows half of them from neighbouring
/// blocks, which would make the encoder's result depend on them; M = 0 is
/// fully local and always decodes to exactly this.
fn choose_mod_binary(src: &[u8; 4], a8: &[u8; 4], b8: &[u8; 4]) -> u8 {
    let mut best = 0u8;
    let mut best_err = i64::MAX;
    for (bit, w) in [(0u8, 0i32), (1, 8)] {
        let mut err = 0i64;
        for ch in 0..4 {
            let v = ((a8[ch] as i32 * (8 - w) + b8[ch] as i32 * w) >> 3) as u8;
            let d = src[ch] as i64 - v as i64;
            err += d * d;
        }
        if err < best_err {
            best_err = err;
            best = bit;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Morton ordering
// ---------------------------------------------------------------------------

/// Reflected Morton index of block (x, y) in an `nb_x × nb_y` grid of blocks
/// (both powers of two), as defined by the Khronos spec and the reference
/// decoder. `min_dim = min(nb_x, nb_y)`.
fn morton(x: usize, y: usize, min_dim: usize) -> usize {
    let mut offset = 0usize;
    let mut shift = 0usize;
    let mut mask = 1usize;
    while mask < min_dim {
        offset |= ((y & mask) | ((x & mask) << 1)) << shift;
        mask <<= 1;
        shift += 1;
    }
    offset |= ((x | y) >> shift) << (shift * 2);
    offset
}

// ---------------------------------------------------------------------------
// Public encoder
// ---------------------------------------------------------------------------

/// Encode an RGBA8 image (row-major, 4 bytes per pixel) as raw PVRTC1 data in
/// reflected Morton word order: `width * height / 2` bytes at 4bpp and
/// `width * height / 4` at 2bpp.
///
/// `is_2bpp` selects 8×4 blocks with one modulation bit per texel instead of
/// 4×4 blocks with two; `quality` (0-7) widens the endpoint search (0 keeps
/// the baseline fit).
///
/// Requires `width` and `height` to be powers of two ≥ 8 (PVRTC1 constraint).
pub fn encode_pvrtc(
    rgba: &[u8],
    width: usize,
    height: usize,
    is_2bpp: bool,
    quality: u8,
) -> Result<Vec<u8>> {
    if width < 8 || height < 8 || !width.is_power_of_two() || !height.is_power_of_two() {
        return Err(crate::error::TpError::Other(format!(
            "PVRTC requiere dimensiones potencia de dos ≥ 8x8 \
             (se obtuvo {width}x{height})"
        )));
    }
    if rgba.len() != width * height * 4 {
        return Err("PVRTC: buffer RGBA de tamaño incorrecto".into());
    }

    let bw = if is_2bpp { 8 } else { 4 };
    let nb_x = width / bw;
    let nb_y = height / 4;
    let min_dim = nb_x.min(nb_y);

    // Pass 1: independent per-block colour fits.
    let mut blocks = vec![BlockColours::default(); nb_x * nb_y];
    let mut px = vec![0u8; bw * 4 * 4];
    for by in 0..nb_y {
        for bx in 0..nb_x {
            let mut i = 0;
            for ty in 0..4 {
                let y = by * 4 + ty;
                for tx in 0..bw {
                    let s = (y * width + bx * bw + tx) * 4;
                    px[i..i + 4].copy_from_slice(&rgba[s..s + 4]);
                    i += 4;
                }
            }
            blocks[by * nb_x + bx] = fit_block(&px, quality, is_2bpp);
        }
    }

    // Pass 2: per-texel modulation search, then assemble the 64-bit words.
    let plane = Plane {
        blocks: &blocks,
        nb_x,
        nb_y,
        wide: is_2bpp,
    };
    let mut out = vec![0u8; nb_x * nb_y * 8];
    for by in 0..nb_y {
        for bx in 0..nb_x {
            let mut mod_bits: u32 = 0;
            for ty in 0..4 {
                let y = by * 4 + ty;
                for tx in 0..bw {
                    let x = bx * bw + tx;
                    let a8 = plane.interpolate(bx, by, tx, ty, false);
                    let b8 = plane.interpolate(bx, by, tx, ty, true);
                    let s = (y * width + x) * 4;
                    let src = [rgba[s], rgba[s + 1], rgba[s + 2], rgba[s + 3]];
                    let bits = if is_2bpp {
                        choose_mod_binary(&src, &a8, &b8) as u32
                    } else {
                        choose_mod(&src, &a8, &b8) as u32
                    };
                    let shift = if is_2bpp {
                        ty * bw + tx
                    } else {
                        2 * (ty * bw + tx)
                    };
                    mod_bits |= bits << shift;
                }
            }
            let blk = blocks[by * nb_x + bx];
            let ca = pack_word(blk.a, blk.a_opaque, true);
            let cb = pack_word(blk.b, blk.b_opaque, false);
            let off = morton(bx, by, min_dim) * 8;
            out[off..off + 4].copy_from_slice(&mod_bits.to_le_bytes());
            out[off + 4..off + 6].copy_from_slice(&ca.to_le_bytes());
            out[off + 6..off + 8].copy_from_slice(&cb.to_le_bytes());
        }
    }
    Ok(out)
}

/// PVRTC1 4bpp with the default quality (3).
pub fn encode_pvrtc_4bpp(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    encode_pvrtc(rgba, width, height, false, 3)
}

/// PVRTC1 2bpp with the default quality (3).
pub fn encode_pvrtc_2bpp(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    encode_pvrtc(rgba, width, height, true, 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode with the independent `texture2ddecoder` crate and return
    /// row-major RGBA texels (the crate outputs BGRA u32 words).
    fn decode(src: &[u8], w: usize, h: usize) -> Vec<[u8; 4]> {
        let mut buf = vec![0u32; w * h];
        texture2ddecoder::decode_pvrtc_4bpp(src, w, h, &mut buf).unwrap();
        buf.iter()
            .map(|v| {
                [
                    ((v >> 16) & 0xff) as u8,
                    ((v >> 8) & 0xff) as u8,
                    (v & 0xff) as u8,
                    ((v >> 24) & 0xff) as u8,
                ]
            })
            .collect()
    }

    fn decode2(src: &[u8], w: usize, h: usize) -> Vec<[u8; 4]> {
        let mut buf = vec![0u32; w * h];
        texture2ddecoder::decode_pvrtc_2bpp(src, w, h, &mut buf).unwrap();
        buf.iter()
            .map(|v| {
                [
                    ((v >> 16) & 0xff) as u8,
                    ((v >> 8) & 0xff) as u8,
                    (v & 0xff) as u8,
                    ((v >> 24) & 0xff) as u8,
                ]
            })
            .collect()
    }

    fn gradient(w: usize, h: usize) -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| {
                let x = i % w;
                let y = i / w;
                [
                    (x * 255 / (w - 1)) as u8,
                    (y * 255 / (h - 1)) as u8,
                    ((x + y) * 255 / (w + h - 2)) as u8,
                    255,
                ]
            })
            .collect()
    }

    fn mean_err(src: &[u8], dec: &[[u8; 4]], nch: usize) -> f64 {
        let mut err = 0.0;
        let mut n = 0usize;
        for (i, p) in src.chunks_exact(4).enumerate() {
            for c in 0..nch {
                err += (p[c] as f64 - dec[i][c] as f64).abs();
            }
            n += 1;
        }
        err / (n * nch) as f64
    }

    fn solid(w: usize, h: usize, c: [u8; 4]) -> Vec<u8> {
        (0..w * h).flat_map(|_| c).collect()
    }

    #[test]
    fn sizes_and_errors() {
        assert_eq!(
            encode_pvrtc_4bpp(&solid(8, 8, [0; 4]), 8, 8).unwrap().len(),
            32
        );
        assert_eq!(
            encode_pvrtc_4bpp(&solid(16, 8, [0; 4]), 16, 8)
                .unwrap()
                .len(),
            64
        );
        assert!(encode_pvrtc_4bpp(&solid(100, 100, [0; 4]), 100, 100).is_err());
        assert!(encode_pvrtc_4bpp(&solid(4, 4, [0; 4]), 4, 4).is_err());
        assert!(encode_pvrtc_4bpp(&solid(8, 8, [0; 4]), 8, 8).is_ok());
        // wrong buffer length
        assert!(encode_pvrtc_4bpp(&[0u8; 10], 8, 8).is_err());
    }

    #[test]
    fn uniform_colour_roundtrip() {
        let w = 8;
        let h = 8;
        let src = [200u8, 100, 50, 255];
        let enc = encode_pvrtc_4bpp(&solid(w, h, src), w, h).unwrap();
        let dec = decode(&enc, w, h);
        assert_eq!(dec.len(), 64);
        for px in &dec {
            assert_eq!(px[3], 255, "opaque alpha must be exact: {px:?}");
            for c in 0..3 {
                let e = (px[c] as i32 - src[c] as i32).abs();
                assert!(e <= 8, "ch {c} err {e} at {px:?}");
            }
        }
    }

    #[test]
    fn opaque_gradient_roundtrip() {
        // Smooth RGB gradient: PVRTC is lossy but must stay close.
        let w = 16;
        let h = 16;
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 17) as u8, (y * 17) as u8, ((x + y) * 8) as u8, 255]);
            }
        }
        let enc = encode_pvrtc_4bpp(&rgba, w, h).unwrap();
        let dec = decode(&enc, w, h);
        let mut total = 0i64;
        for (i, px) in dec.iter().enumerate() {
            assert_eq!(px[3], 255);
            for c in 0..3 {
                let e = (px[c] as i64 - rgba[i * 4 + c] as i64).abs();
                total += e;
            }
        }
        // Average per-channel error must stay small on a smooth ramp.
        let avg = total as f64 / (w * h * 3) as f64;
        assert!(avg <= 16.0, "avg per-channel error too high: {avg}");
    }

    #[test]
    fn transparency_roundtrip() {
        // Left half opaque red, right half fully transparent.
        let w = 16;
        let h = 16;
        let mut rgba = Vec::with_capacity(w * h * 4);
        for _y in 0..h {
            for x in 0..w {
                if x < 8 {
                    rgba.extend_from_slice(&[200, 50, 50, 255]);
                } else {
                    rgba.extend_from_slice(&[0, 0, 0, 0]);
                }
            }
        }
        let enc = encode_pvrtc_4bpp(&rgba, w, h).unwrap();
        let dec = decode(&enc, w, h);
        // PVRTC interpolates toroidally: the outer 2 texel columns/rows mix
        // with the opposite edge, and the hard x=8 edge ramps alpha over ~4
        // texels. Assert only the interior, away from both effects.
        for y in 2..14 {
            for x in 0..w {
                let a = dec[y * w + x][3];
                if (2..=7).contains(&x) {
                    assert!(a >= 150, "opaque side leaked at ({x},{y}): {a}");
                }
                if (10..=13).contains(&x) {
                    assert!(a <= 120, "transparent side not clear at ({x},{y}): {a}");
                }
            }
        }
    }

    #[test]
    fn quadrant_morton_layout() {
        // Four solid quadrants in a 16x16 image: each 4x4 block's centre
        // texel (x ≡ 2, y ≡ 2 mod 4) must decode to its quadrant colour,
        // which exercises the reflected Morton word order end-to-end.
        let w = 16;
        let h = 16;
        let quads = [
            [255u8, 0, 0, 255], // top-left
            [0, 255, 0, 255],   // top-right
            [0, 0, 255, 255],   // bottom-left
            [255, 255, 0, 255], // bottom-right
        ];
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let q = if y < 8 && x < 8 {
                    0
                } else if y < 8 {
                    1
                } else if x < 8 {
                    2
                } else {
                    3
                };
                rgba.extend_from_slice(&quads[q]);
            }
        }
        let enc = encode_pvrtc_4bpp(&rgba, w, h).unwrap();
        let dec = decode(&enc, w, h);
        for (sample, q) in [((2, 2), 0usize), ((10, 2), 1), ((2, 10), 2), ((10, 10), 3)] {
            let (x, y) = sample;
            let px = dec[y * w + x];
            for c in 0..4 {
                let e = (px[c] as i32 - quads[q][c] as i32).abs();
                assert!(e <= 8, "quadrant {q} at ({x},{y}) ch {c}: {px:?} err {e}");
            }
        }
    }

    #[test]
    fn pack_word_roundtrip_through_decoder() {
        // Verify the exact bit packing against the decoder: encode a single
        // solid block colour through fit → word → decode, and compare with
        // the canonical values.
        let w = 8;
        let h = 8;
        let rgba = solid(w, h, [128, 64, 192, 255]);
        let enc = encode_pvrtc_4bpp(&rgba, w, h).unwrap();
        let dec = decode(&enc, w, h);
        // Opaque 5-5-4 blue: 192 -> 24 -> q4rep(24) = 12<<1|12>>3 = 24|1 = 25.
        // Decoded blue ≈ 25 * 16 * 0.5156 ≈ 206.
        let b = dec[0][2] as i32;
        assert!((b - 206).abs() <= 2, "unexpected blue decode: {b}");
    }

    #[test]
    fn two_bpp_roundtrip() {
        let (w, h) = (16, 16);
        let src = gradient(w, h);
        let enc = encode_pvrtc_2bpp(&src, w, h).unwrap();
        assert_eq!(enc.len(), w * h / 4);
        let dec = decode2(&enc, w, h);
        let mean = mean_err(&src, &dec, 4);
        assert!(mean < 40.0, "error medio {mean:.2}");
    }

    #[test]
    fn two_bpp_requires_pot_like_four_bpp() {
        let src = vec![0u8; 16 * 16 * 4];
        assert!(encode_pvrtc_2bpp(&src, 12, 16).is_err());
        assert!(encode_pvrtc_2bpp(&src, 16, 16).is_ok());
        assert!(encode_pvrtc_2bpp(&src, 4, 4).is_err());
    }

    #[test]
    fn higher_quality_is_never_worse() {
        let (w, h) = (16, 16);
        let src = gradient(w, h);
        for is2 in [false, true] {
            let mut prev: Option<f64> = None;
            for q in [0u8, 1, 3, 5, 6, 7] {
                let enc = encode_pvrtc(&src, w, h, is2, q).unwrap();
                let dec = if is2 {
                    decode2(&enc, w, h)
                } else {
                    decode(&enc, w, h)
                };
                let err = mean_err(&src, &dec, 4);
                if let Some(p) = prev {
                    assert!(
                        err <= p + 0.75,
                        "2bpp={is2} q={q}: {err:.3} peor que el de menor calidad {p:.3}"
                    );
                }
                prev = Some(prev.map_or(err, |p: f64| p.max(err)));
            }
        }
    }
}
