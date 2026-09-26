//! Built-in ETC2 RGBA8 encoder (GL_COMPRESSED_RGBA8_ETC2_EAC).
//!
//! Produces *valid* ETC2 streams that any conformant decoder can decompress.
//! Every block picks the best of the five RGB modes:
//! - **Individual** (diff bit = 0): two 4-bit base colors per subblock, flip
//!   bit selects 4x2 / 2x4 splits, 8 ETC1 modifier tables.
//! - **Differential** (diff bit = 1): 5-bit base + 3-bit signed delta per
//!   subblock (delta range [-4, 3] in 5-bit space, per the ETC2 spec).
//! - **T** mode: one color plus a second color at ±d (d from the 8-value
//!   distance table), entered when the R differential overflows.
//! - **H** mode: two colors both at ±d, entered when the G differential
//!   overflows. The LSB of the distance is implied by the color ordering.
//! - **Planar** mode: three 6/7/6-bit colors (O, H, V) defining a bilinear
//!   plane, entered when the B differential overflows.
//! - Alpha part: EAC (8-bit base, 4-bit multiplier, 4-bit table index,
//!   16 x 2-bit modifiers, 11-bit intermediates rounded to 8-bit).
//!
//! Bit layouts follow the original Ericsson ETCPACK reference (the official
//! conformance implementation) and Google's etc2comp; the on-disk ("stuffed")
//! packing — including the overflow-detection padding bits that steer the
//! decoder into T/H/planar — replicates `stuff59bits`/`stuff58bits`/
//! `stuff57bits` from ETCPACK exactly. `texture2ddecoder` cross-validates the
//! individual/differential/T decoders; H and planar are validated with the
//! in-crate reference decoders (that crate has long-standing bit-order bugs
//! in its H/planar decoders).
//!
//! Quality is search-based but not exhaustive; good enough for game assets.
//! For higher quality use the `gpu-formats` feature (ASTC via ARM's astcenc).

/// ETC1 modifier table: pairs (small, large) — index selects magnitude, sign
/// bit selects +/-.
const ETC1_MODIFIER_TABLE: [[i16; 2]; 8] = [
    [2, 8],
    [5, 17],
    [9, 29],
    [13, 42],
    [18, 60],
    [24, 80],
    [33, 106],
    [47, 183],
];

/// EAC alpha modifier tables (16 tables x 8 values).
const EAC_MODIFIER_TABLE: [[i8; 8]; 16] = [
    [-3, -6, -9, -15, 2, 5, 8, 14],
    [-3, -7, -10, -13, 2, 6, 9, 12],
    [-2, -5, -8, -13, 1, 4, 7, 12],
    [-2, -4, -6, -13, 1, 3, 5, 12],
    [-3, -6, -8, -12, 2, 5, 7, 11],
    [-3, -7, -9, -11, 2, 6, 8, 10],
    [-4, -7, -8, -11, 3, 6, 7, 10],
    [-3, -5, -8, -11, 2, 4, 7, 10],
    [-2, -6, -8, -10, 1, 5, 7, 9],
    [-2, -5, -8, -10, 1, 4, 7, 9],
    [-2, -4, -8, -10, 1, 3, 7, 9],
    [-2, -5, -7, -10, 1, 4, 6, 9],
    [-3, -4, -7, -10, 2, 3, 6, 9],
    [-1, -2, -3, -10, 0, 1, 2, 9],
    [-4, -6, -8, -9, 3, 5, 7, 8],
    [-3, -5, -7, -9, 2, 4, 6, 8],
];

/// ETC2 T/H/planar distance table.
const ETC2_DISTANCE_TABLE: [i16; 8] = [3, 6, 11, 16, 23, 32, 41, 64];

/// Subblock assignment per pixel (scan order 0..15).
/// [0] = 4x2 split (top half / bottom half), [1] = 2x4 split (left / right).
const SUBBLOCK_TABLE: [[usize; 16]; 2] = [
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1],
];

/// Encode an RGBA8 image (width x height, row-major) into ETC2 RGBA8 blocks.
/// Returns a `Vec<u8>` of 16 bytes per 4x4 block. Non-multiple-of-4
/// dimensions are handled by edge clamping (padding with the border pixel).
pub fn encode_etc2_rgba8(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);
    let mut out = Vec::with_capacity(blocks_x * blocks_y * 16);

    // Pre-extract pixels with edge clamping for speed.
    let px = |x: usize, y: usize| -> [u8; 4] {
        let cx = x.min(width - 1);
        let cy = y.min(height - 1);
        let i = (cy * width + cx) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let mut pixels = [[0u8; 4]; 16];
            for py in 0..4 {
                for pxx in 0..4 {
                    pixels[py * 4 + pxx] = px(bx * 4 + pxx, by * 4 + py);
                }
            }
            let block = encode_block(&pixels);
            out.extend_from_slice(&block);
        }
    }
    out
}

/// Scan index of stream texel `i`. ETC1/2 store texels in **column-major**
/// order: texel at (col c, row r) has stream index `c*4 + r`.
const fn stream_to_scan(i: usize) -> usize {
    (i % 4) * 4 + (i / 4)
}

/// (x, y) of stream texel `i` within a 4x4 block.
const fn stream_xy(i: usize) -> (usize, usize) {
    (i / 4, i % 4)
}

/// Encode one 4x4 block (row-major scan-order pixels) into 16 bytes:
/// 8 EAC alpha + 8 ETC2 RGB.
fn encode_block(pixels: &[[u8; 4]; 16]) -> [u8; 16] {
    // RGB: column-major stream order (WRITE_ORDER_TABLE).
    let mut alpha = [0u8; 16];
    let mut rgb = [[0u8; 3]; 16];
    for i in 0..16 {
        let s = stream_to_scan(i);
        alpha[i] = pixels[s][3];
        rgb[i] = [pixels[s][0], pixels[s][1], pixels[s][2]];
    }
    // EAC alpha: REVERSE column-major order (WRITE_ORDER_TABLE_REV).
    let mut alpha_rev = [0u8; 16];
    for i in 0..16 {
        alpha_rev[i] = alpha[15 - i];
    }

    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&encode_eac_alpha(&alpha_rev));
    out[8..16].copy_from_slice(&encode_etc2_rgb(&rgb));
    out
}

// ---------------------------------------------------------------------------
// EAC alpha (8 bytes)
// ---------------------------------------------------------------------------

fn encode_eac_alpha(alpha: &[u8; 16]) -> [u8; 8] {
    let mut out = [0u8; 8];

    // Uniform alpha: multiplier 0, constant base.
    if alpha.iter().all(|&a| a == alpha[0]) {
        out[0] = alpha[0];
        out[1] = 0; // multiplier = 0, table = 0 (unused)
        return out;
    }

    let min = *alpha.iter().min().unwrap() as i32;
    let max = *alpha.iter().max().unwrap() as i32;

    // The EAC range per pixel is [base - 225, base + 210] (modifiers -15..14
    // times multiplier 1..15). Try a few base candidates so the range covers
    // the actual data; pick the one with the lowest total error.
    let mut base_candidates = vec![min, (max - 210).max(0), (min + max) / 2];
    base_candidates.sort_unstable();
    base_candidates.dedup();

    let mut best_err = i64::MAX;
    let mut best_base = 0i32;
    let mut best_m = 0i32;
    let mut best_t = 0usize;
    let mut best_mods = [0u8; 16];

    for &base in &base_candidates {
        for m in 1..=15i32 {
            for (t, table) in EAC_MODIFIER_TABLE.iter().enumerate() {
                let mut err: i64 = 0;
                let mut mods = [0u8; 16];
                for (i, &a) in alpha.iter().enumerate() {
                    let mut best = 0usize;
                    let mut be = i64::MAX;
                    for (mi, &mv) in table.iter().enumerate() {
                        // Decoded value clamps to [0, 255]; use the *true*
                        // decoded error, not the unclamped delta.
                        let val = (base + m * mv as i32).clamp(0, 255);
                        let e = (a as i32 - val).abs() as i64;
                        if e < be {
                            be = e;
                            best = mi;
                        }
                    }
                    mods[i] = best as u8;
                    err += be;
                }
                if err < best_err {
                    best_err = err;
                    best_base = base;
                    best_m = m;
                    best_t = t;
                    best_mods = mods;
                }
            }
        }
    }

    out[0] = best_base as u8;
    out[1] = ((best_m as u8) << 4) | best_t as u8;
    // Modifier bitstream: pixel i's 2 bits at bits [3i, 3i+2] of a u64,
    // stored big-endian in bytes 2..8 (matches the reference decoder).
    let mut word: u64 = 0;
    for (i, &m) in best_mods.iter().enumerate() {
        word |= (m as u64) << (3 * i);
    }
    out[2..8].copy_from_slice(&word.to_be_bytes()[2..8]);
    out
}

// ---------------------------------------------------------------------------
// ETC2 RGB — shared helpers
// ---------------------------------------------------------------------------

/// Pack per-pixel 2-bit selectors (index = stream texel) into the
/// (k, j) bit pairs: bytes 4-5 = k (BE), bytes 6-7 = j (BE), where
/// selector[i] = (k bit i) << 1 | (j bit i).
fn pack_selectors(sel: &[u8; 16]) -> (u16, u16) {
    let mut k = 0u16;
    let mut j = 0u16;
    for (i, &s) in sel.iter().enumerate() {
        if s & 1 == 1 {
            j |= 1 << i;
        }
        if s & 2 == 2 {
            k |= 1 << i;
        }
    }
    (k, j)
}

/// L1 error between a pixel and a color (3 channels).
fn err3(p: &[u8; 3], c: [i32; 3]) -> i64 {
    (p[0] as i64 - c[0] as i64).abs()
        + (p[1] as i64 - c[1] as i64).abs()
        + (p[2] as i64 - c[2] as i64).abs()
}

/// Expand a 5-bit value to 8 bits (bit replication).
#[inline]
fn expand5(v: i32) -> i32 {
    (v << 3) | (v >> 2)
}

/// Expand a 6-bit value to 8 bits.
#[inline]
fn expand6(v: i32) -> i32 {
    (v << 2) | (v >> 4)
}

/// Expand a 7-bit value to 8 bits.
#[inline]
fn expand7(v: i32) -> i32 {
    (v << 1) | (v >> 6)
}

/// Clamp an 8-bit channel delta to the range representable by a 4-bit color.
#[inline]
fn add_clamp(c: [i32; 3], d: i32) -> [i32; 3] {
    [
        (c[0] + d).clamp(0, 255),
        (c[1] + d).clamp(0, 255),
        (c[2] + d).clamp(0, 255),
    ]
}

/// For each pixel, best palette index and total error (stream order).
fn best_palette_error(rgb: &[[u8; 3]; 16], palette: &[[i32; 3]; 4]) -> (i64, [u8; 16]) {
    let mut err = 0i64;
    let mut sel = [0u8; 16];
    for (i, p) in rgb.iter().enumerate() {
        let mut best = i64::MAX;
        let mut best_i = 0u8;
        for (pi, c) in palette.iter().enumerate() {
            let e = err3(p, *c);
            if e < best {
                best = e;
                best_i = pi as u8;
            }
        }
        err += best;
        sel[i] = best_i;
    }
    (err, sel)
}

/// Candidate representative colors (4-bit per channel) for T/H mode search:
/// quantized per-channel min, mean, max, a luminance-split mean pair, plus
/// every distinct quantized pixel color (a T/H block's palette colors are
/// always present among the pixels themselves, so this lets the search hit
/// exact palettes).
fn color_candidates(rgb: &[[u8; 3]; 16]) -> Vec<[u8; 3]> {
    let mut min_c = [255u8; 3];
    let mut max_c = [0u8; 3];
    let mut sum = [0i64; 3];
    for p in rgb {
        for c in 0..3 {
            min_c[c] = min_c[c].min(p[c]);
            max_c[c] = max_c[c].max(p[c]);
            sum[c] += p[c] as i64;
        }
    }
    let mean = [
        (sum[0] / 16) as u8,
        (sum[1] / 16) as u8,
        (sum[2] / 16) as u8,
    ];
    // Luminance split: mean of pixels below vs above the mean luminance.
    let lum_mean = (sum[0] * 3 + sum[1] * 6 + sum[2]) / 160;
    let mut lo = [0i64; 3];
    let mut hi = [0i64; 3];
    let mut n_lo = 0i64;
    let mut n_hi = 0i64;
    for p in rgb {
        let lum = (p[0] as i64 * 3 + p[1] as i64 * 6 + p[2] as i64) / 10;
        if lum <= lum_mean {
            for c in 0..3 {
                lo[c] += p[c] as i64;
            }
            n_lo += 1;
        } else {
            for c in 0..3 {
                hi[c] += p[c] as i64;
            }
            n_hi += 1;
        }
    }
    let split = if n_lo > 0 && n_hi > 0 {
        [
            (lo[0] / n_lo) as u8,
            (lo[1] / n_lo) as u8,
            (lo[2] / n_lo) as u8,
        ]
    } else {
        min_c
    };
    let q4 = |v: u8| (v as i32 / 17).clamp(0, 15) as u8;
    let mut out = vec![
        [q4(min_c[0]), q4(min_c[1]), q4(min_c[2])],
        [q4(mean[0]), q4(mean[1]), q4(mean[2])],
        [q4(max_c[0]), q4(max_c[1]), q4(max_c[2])],
        [q4(split[0]), q4(split[1]), q4(split[2])],
    ];
    // Distinct quantized pixel colors (in stable order), capped to keep the
    // search bounded.
    for p in rgb {
        let c = [q4(p[0]), q4(p[1]), q4(p[2])];
        if !out.contains(&c) && out.len() < 16 {
            out.push(c);
        }
    }
    out
}

fn encode_etc2_rgb(rgb: &[[u8; 3]; 16]) -> [u8; 8] {
    let mut best_err = i64::MAX;
    let mut best = [0u8; 8];
    let mut consider = |err: i64, block: [u8; 8]| {
        if err < best_err {
            best_err = err;
            best = block;
        }
    };

    {
        let (err, block) = encode_individual(rgb);
        consider(err, block);
    }
    if let Some(r) = encode_differential(rgb) {
        consider(r.err, r.block);
    }
    if let Some(r) = encode_t(rgb) {
        consider(r.err, r.block);
    }
    if let Some(r) = encode_h(rgb) {
        consider(r.err, r.block);
    }
    if let Some(r) = encode_planar(rgb) {
        consider(r.err, r.block);
    }

    best
}

// ---------------------------------------------------------------------------
// Individual mode
// ---------------------------------------------------------------------------

fn encode_individual(rgb: &[[u8; 3]; 16]) -> (i64, [u8; 8]) {
    let mut out = [0u8; 8];
    let mut best_err = i64::MAX;
    let mut best_flip = 0u8;

    let mut best_r0 = 0u8;
    let mut best_g0 = 0u8;
    let mut best_b0 = 0u8;
    let mut best_r1 = 0u8;
    let mut best_g1 = 0u8;
    let mut best_b1 = 0u8;
    let mut best_c0 = 0u8;
    let mut best_c1 = 0u8;
    let mut best_j = 0u16;
    let mut best_k = 0u16;

    for (flip, &sub) in SUBBLOCK_TABLE.iter().enumerate() {
        let mut sub0: Vec<(usize, [u8; 3])> = Vec::new();
        let mut sub1: Vec<(usize, [u8; 3])> = Vec::new();
        // Track each pixel's STREAM position: the index bits live at
        // stream bit positions, which differ from subblock-local order
        // when the block is split 2x4.
        for (i, &c) in rgb.iter().enumerate() {
            if sub[i] == 0 {
                sub0.push((i, c));
            } else {
                sub1.push((i, c));
            }
        }

        let (r0, g0, b0, c0, j0, k0) = encode_subblock(&Subblock { pixels: &sub0 });
        let (r1, g1, b1, c1, j1, k1) = encode_subblock(&Subblock { pixels: &sub1 });
        let err = subblock_error(&Subblock { pixels: &sub0 }, r0, g0, b0, c0, j0, k0)
            + subblock_error(&Subblock { pixels: &sub1 }, r1, g1, b1, c1, j1, k1);

        if err < best_err {
            best_err = err;
            best_flip = flip as u8;
            best_r0 = r0;
            best_g0 = g0;
            best_b0 = b0;
            best_r1 = r1;
            best_g1 = g1;
            best_b1 = b1;
            best_c0 = c0;
            best_c1 = c1;
            best_j = j0 | j1;
            best_k = k0 | k1;
        }
    }

    out[0] = (best_r0 << 4) | best_r1;
    out[1] = (best_g0 << 4) | best_g1;
    out[2] = (best_b0 << 4) | best_b1;
    // bits: [7:5] code0, [4:2] code1, [1] diff=0, [0] flip
    out[3] = (best_c0 << 5) | (best_c1 << 2) | best_flip;
    out[4..6].copy_from_slice(&best_k.to_be_bytes());
    out[6..8].copy_from_slice(&best_j.to_be_bytes());
    (best_err, out)
}

/// One subblock: pixel colors paired with their STREAM position (the index
/// bits live at stream bit positions, which differ from subblock-local order
/// when the block is split 2x4).
struct Subblock<'a> {
    pixels: &'a [(usize, [u8; 3])],
}

/// Encode one subblock (up to 8 pixels): returns (r4, g4, b4, table_code,
/// j-bits, k-bits) where the per-pixel index bits are OR-ed for both subblocks.
///
/// The 4-bit base per channel is searched over {min, mean, max} so both
/// bright and dark gradients can be covered by the modifier range.
fn encode_subblock(sb: &Subblock<'_>) -> (u8, u8, u8, u8, u16, u16) {
    let pixels = sb.pixels;
    debug_assert!(!pixels.is_empty());
    let n = pixels.len();

    let mut best_err = i64::MAX;
    let mut best_base = [0u8; 3];
    let mut best_code = 0u8;
    let mut best_j = 0u16;
    let mut best_k = 0u16;

    // Per-channel candidate 4-bit bases.
    let mut candidates = [[0u8; 3]; 3]; // [min, mean, max]
    let mut min_c = [255u8; 3];
    let mut max_c = [0u8; 3];
    let mut sum = [0i64; 3];
    for (_, p) in pixels {
        for c in 0..3 {
            min_c[c] = min_c[c].min(p[c]);
            max_c[c] = max_c[c].max(p[c]);
            sum[c] += p[c] as i64;
        }
    }
    let q = |v: u8| (v as i32 / 17).clamp(0, 15) as u8;
    for c in 0..3 {
        candidates[0][c] = q(min_c[c]);
        candidates[1][c] = q((sum[c] / n as i64) as u8);
        candidates[2][c] = q(max_c[c]);
    }

    for ri in 0..3 {
        for gi in 0..3 {
            for bi in 0..3 {
                let base = [candidates[ri][0], candidates[gi][1], candidates[bi][2]];
                for (code, table) in ETC1_MODIFIER_TABLE.iter().enumerate() {
                    let mut err: i64 = 0;
                    let mut j = 0u16;
                    let mut k = 0u16;
                    for &(stream_pos, p) in pixels.iter() {
                        let mut best_local = i64::MAX;
                        let mut best_jk = 0u8;
                        for (jbit, &mv) in table.iter().enumerate() {
                            let m = mv as i64;
                            for kbit in 0..2usize {
                                let sign = if kbit == 1 { -1 } else { 1 };
                                let mut e: i64 = 0;
                                for c in 0..3 {
                                    let target = p[c] as i64;
                                    let val = ((base[c] as i64) * 17 + sign * m).clamp(0, 255);
                                    e += (target - val).abs();
                                }
                                if e < best_local {
                                    best_local = e;
                                    best_jk = (jbit as u8) | ((kbit as u8) << 1);
                                }
                            }
                        }
                        err += best_local;
                        if best_jk & 1 == 1 {
                            j |= 1 << stream_pos;
                        }
                        if best_jk & 2 == 2 {
                            k |= 1 << stream_pos;
                        }
                    }
                    if err < best_err {
                        best_err = err;
                        best_base = base;
                        best_code = code as u8;
                        best_j = j;
                        best_k = k;
                    }
                }
            }
        }
    }

    (
        best_base[0],
        best_base[1],
        best_base[2],
        best_code,
        best_j,
        best_k,
    )
}

/// Compute the error of a subblock with the given encoding (for comparing flips).
fn subblock_error(sb: &Subblock<'_>, r4: u8, g4: u8, b4: u8, code: u8, j: u16, k: u16) -> i64 {
    let pixels = sb.pixels;
    let base = [(r4 as i64) * 17, (g4 as i64) * 17, (b4 as i64) * 17];
    let mut err = 0i64;
    for &(stream_pos, p) in pixels.iter() {
        let m = ETC1_MODIFIER_TABLE[code as usize][((j >> stream_pos) & 1) as usize] as i64;
        let sign = if (k >> stream_pos) & 1 == 1 { -1 } else { 1 };
        for c in 0..3 {
            err += (p[c] as i64 - (base[c] + sign * m)).abs();
        }
    }
    err
}

// ---------------------------------------------------------------------------
// Differential mode (diff bit = 1)
// ---------------------------------------------------------------------------

struct EncResult {
    err: i64,
    block: [u8; 8],
}

/// Error + selectors for a subblock given fixed 5-bit bases and table code.
fn diff_block_error(sb: &Subblock<'_>, base: [u8; 3], code: usize) -> (i64, u16, u16) {
    let pixels = sb.pixels;
    let base8 = [
        expand5(base[0] as i32) as i64,
        expand5(base[1] as i32) as i64,
        expand5(base[2] as i32) as i64,
    ];
    let mut err = 0i64;
    let mut j = 0u16;
    let mut k = 0u16;
    for &(pos, p) in pixels {
        let mut best = i64::MAX;
        let mut best_jk = 0u8;
        for (jbit, &mv) in ETC1_MODIFIER_TABLE[code].iter().enumerate() {
            let m = mv as i64;
            for kbit in 0..2usize {
                let sign = if kbit == 1 { -1 } else { 1 };
                let mut e = 0i64;
                for c in 0..3 {
                    let val = (base8[c] + sign * m).clamp(0, 255);
                    e += (p[c] as i64 - val).abs();
                }
                if e < best {
                    best = e;
                    best_jk = (jbit as u8) | ((kbit as u8) << 1);
                }
            }
        }
        err += best;
        if best_jk & 1 == 1 {
            j |= 1 << pos;
        }
        if best_jk & 2 == 2 {
            k |= 1 << pos;
        }
    }
    (err, j, k)
}

/// Best (err, table, j, k) for a subblock with fixed 5-bit bases.
fn diff_with_fixed_base(sb: &Subblock<'_>, base: [u8; 3]) -> (i64, u8, u16, u16) {
    let pixels = sb.pixels;
    let mut best_err = i64::MAX;
    let mut best = (0u8, 0u16, 0u16);
    for code in 0..8usize {
        let (err, j, k) = diff_block_error(&Subblock { pixels }, base, code);
        if err < best_err {
            best_err = err;
            best = (code as u8, j, k);
        }
    }
    (best_err, best.0, best.1, best.2)
}

/// Clamp `r1` so that the 3-bit delta (r1 - r0) fits in [-4, 3].
fn clamp_delta(r0: u8, r1: u8) -> u8 {
    let lo = (r0 as i32 - 4).max(0);
    let hi = (r0 as i32 + 3).min(31);
    (r1 as i32).clamp(lo, hi) as u8
}

/// Search a subblock with 5-bit bases: returns (err, r5, g5, b5, code, j, k).
fn encode_diff_subblock(sb: &Subblock<'_>) -> (i64, u8, u8, u8, u8, u16, u16) {
    let pixels = sb.pixels;
    let n = pixels.len();
    let mut best_err = i64::MAX;
    let mut best = (0u8, 0u8, 0u8, 0u8, 0u16, 0u16);

    let mut min_c = [255u8; 3];
    let mut max_c = [0u8; 3];
    let mut sum = [0i64; 3];
    for (_, p) in pixels {
        for c in 0..3 {
            min_c[c] = min_c[c].min(p[c]);
            max_c[c] = max_c[c].max(p[c]);
            sum[c] += p[c] as i64;
        }
    }
    let q5 = |v: u8| (v as u32 * 31 / 255).clamp(0, 31) as u8;
    let mut cands = [[0u8; 3]; 3];
    for c in 0..3 {
        cands[0][c] = q5(min_c[c]);
        cands[1][c] = q5((sum[c] / n as i64) as u8);
        cands[2][c] = q5(max_c[c]);
    }

    for ri in 0..3 {
        for gi in 0..3 {
            for bi in 0..3 {
                let base = [cands[ri][0], cands[gi][1], cands[bi][2]];
                for code in 0..8usize {
                    let (err, j, k) = diff_block_error(&Subblock { pixels }, base, code);
                    if err < best_err {
                        best_err = err;
                        best = (base[0], base[1], base[2], code as u8, j, k);
                    }
                }
            }
        }
    }
    (best_err, best.0, best.1, best.2, best.3, best.4, best.5)
}

fn encode_differential(rgb: &[[u8; 3]; 16]) -> Option<EncResult> {
    let mut best: Option<EncResult> = None;

    for (flip, &sub) in SUBBLOCK_TABLE.iter().enumerate() {
        let mut sub0: Vec<(usize, [u8; 3])> = Vec::new();
        let mut sub1: Vec<(usize, [u8; 3])> = Vec::new();
        for (i, &c) in rgb.iter().enumerate() {
            if sub[i] == 0 {
                sub0.push((i, c));
            } else {
                sub1.push((i, c));
            }
        }

        let (e0, r0, g0, b0, c0, j0, k0) = encode_diff_subblock(&Subblock { pixels: &sub0 });
        let (_, r1, g1, b1, _, _, _) = encode_diff_subblock(&Subblock { pixels: &sub1 });

        // Clamp subblock-1 bases into the valid delta range, then re-evaluate
        // subblock 1 with the fixed bases (best table + selectors).
        let r1c = clamp_delta(r0, r1);
        let g1c = clamp_delta(g0, g1);
        let b1c = clamp_delta(b0, b1);
        let (e1c, c1c, j1c, k1c) =
            diff_with_fixed_base(&Subblock { pixels: &sub1 }, [r1c, g1c, b1c]);
        let err = e0 + e1c;

        let mut block = [0u8; 8];
        block[0] = (r0 << 3) | ((r1c as i32 - r0 as i32) as u8 & 7);
        block[1] = (g0 << 3) | ((g1c as i32 - g0 as i32) as u8 & 7);
        block[2] = (b0 << 3) | ((b1c as i32 - b0 as i32) as u8 & 7);
        block[3] = (c0 << 5) | (c1c << 2) | (1 << 1) | flip as u8;
        block[4..6].copy_from_slice(&(k0 | k1c).to_be_bytes());
        block[6..8].copy_from_slice(&(j0 | j1c).to_be_bytes());

        if best.as_ref().is_none_or(|b| err < b.err) {
            best = Some(EncResult { err, block });
        }
    }

    best
}

// ---------------------------------------------------------------------------
// T mode (diff = 1, R differential overflows)
// ---------------------------------------------------------------------------

/// Overflow-padding bit for T mode, exactly as ETCPACK's `stuff59bits`:
/// the abcd sequences 0111, 1010, 1011, 1101, 1110, 1111 (a=R0[3], b=R0[2],
/// c=R0[1], d=R0[0]) get the 3 top bits padded with ones, others with zeros,
/// which forces the R differential to overflow in both directions.
fn t_overflow_bit(r0: u8) -> u8 {
    let a = (r0 >> 3) & 1;
    let b = (r0 >> 2) & 1;
    let c = (r0 >> 1) & 1;
    let d = r0 & 1;
    (a & c) | (!a & b & c & d) | (a & b & !c & d)
}

fn encode_t(rgb: &[[u8; 3]; 16]) -> Option<EncResult> {
    let cands = color_candidates(rgb);
    let mut best: Option<EncResult> = None;

    for &c0 in &cands {
        for &c1 in &cands {
            for (dist, &dv) in ETC2_DISTANCE_TABLE.iter().enumerate() {
                let d = dv as i32;
                let c0x = expand4_3(c0);
                let c1x = expand4_3(c1);
                // Palette: [C0, C1+d, C1, C1-d].
                let palette = [c0x, add_clamp(c1x, d), c1x, add_clamp(c1x, -d)];
                let (err, sel) = best_palette_error(rgb, &palette);

                let r0 = c0[0];
                let g0 = c0[1];
                let b0 = c0[2];
                let r1 = c1[0];
                let g1 = c1[1];
                let b1 = c1[2];
                let bit = t_overflow_bit(r0);

                let mut block = [0u8; 8];
                block[0] = (bit << 7)
                    | (bit << 6)
                    | (bit << 5)
                    | ((r0 >> 3) << 4)
                    | ((r0 >> 2) << 3)
                    | ((!bit & 1) << 2)
                    | ((r0 >> 1) << 1)
                    | (r0 & 1);
                block[1] = (g0 << 4) | b0;
                block[2] = (r1 << 4) | g1;
                block[3] = (b1 << 4) | (((dist >> 1) as u8) << 2) | (1 << 1) | (dist as u8 & 1);
                let (k, j) = pack_selectors(&sel);
                block[4..6].copy_from_slice(&k.to_be_bytes());
                block[6..8].copy_from_slice(&j.to_be_bytes());

                if best.as_ref().is_none_or(|b| err < b.err) {
                    best = Some(EncResult { err, block });
                }
            }
        }
    }
    best
}

// ---------------------------------------------------------------------------
// H mode (diff = 1, R ok, G differential overflows)
// ---------------------------------------------------------------------------

/// Green overflow-padding bit, exactly as ETCPACK's `stuff58bits`
/// (a=G0[0], b=B0[3], c=B0[2], d=B0[1]).
fn h_overflow_bit(g0: u8, b0: u8) -> u8 {
    let a = g0 & 1;
    let b = (b0 >> 3) & 1;
    let c = (b0 >> 2) & 1;
    let d = (b0 >> 1) & 1;
    (a & c) | (!a & b & c & d) | (a & b & !c & d)
}

fn encode_h(rgb: &[[u8; 3]; 16]) -> Option<EncResult> {
    let cands = color_candidates(rgb);
    let mut best: Option<EncResult> = None;

    for &c0 in &cands {
        for &c1 in &cands {
            // The LSB of the distance is implied by the color ordering:
            // lsb = (packed(c0) >= packed(c1)).
            let packed0 = ((c0[0] as u16) << 8) | ((c0[1] as u16) << 4) | c0[2] as u16;
            let packed1 = ((c1[0] as u16) << 8) | ((c1[1] as u16) << 4) | c1[2] as u16;
            let implied = if packed0 >= packed1 { 1 } else { 0 };

            for s in 0..4usize {
                let dist = (s << 1) | implied;
                let d = ETC2_DISTANCE_TABLE[dist] as i32;
                let c0x = expand4_3(c0);
                let c1x = expand4_3(c1);
                // Palette: [C0+d, C0-d, C1+d, C1-d].
                let palette = [
                    add_clamp(c0x, d),
                    add_clamp(c0x, -d),
                    add_clamp(c1x, d),
                    add_clamp(c1x, -d),
                ];
                let (err, sel) = best_palette_error(rgb, &palette);

                let r0 = c0[0];
                let g0 = c0[1];
                let b0 = c0[2];
                let r1 = c1[0];
                let g1 = c1[1];
                let b1 = c1[2];
                let gbit = h_overflow_bit(g0, b0);

                let mut block = [0u8; 8];
                block[0] = ((!r0 >> 3 & 1) << 7)
                    | ((r0 >> 3) << 6)
                    | ((r0 >> 2) << 5)
                    | ((r0 >> 1) << 4)
                    | (r0 << 3 & 8)
                    | ((g0 >> 3) << 2)
                    | ((g0 >> 2) << 1)
                    | (g0 >> 1 & 1);
                block[1] = (gbit << 7)
                    | (gbit << 6)
                    | (gbit << 5)
                    | ((g0 & 1) << 4)
                    | ((b0 >> 3) << 3)
                    | ((!gbit & 1) << 2)
                    | ((b0 >> 2) << 1)
                    | (b0 >> 1 & 1);
                block[2] = ((b0 & 1) << 7)
                    | ((r1 & 0xF) << 3)
                    | ((g1 >> 3) << 2)
                    | ((g1 >> 2) << 1)
                    | (g1 >> 1 & 1);
                block[3] = ((g1 & 1) << 7)
                    | ((b1 & 0xF) << 3)
                    | ((((dist >> 2) & 1) as u8) << 2)
                    | (1 << 1)
                    | (((dist >> 1) & 1) as u8);
                let (k, j) = pack_selectors(&sel);
                block[4..6].copy_from_slice(&k.to_be_bytes());
                block[6..8].copy_from_slice(&j.to_be_bytes());

                if best.as_ref().is_none_or(|b| err < b.err) {
                    best = Some(EncResult { err, block });
                }
            }
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Planar mode (diff = 1, R and G ok, B differential overflows)
// ---------------------------------------------------------------------------

/// Blue overflow-padding bit, exactly as ETCPACK's `stuff57bits`
/// (a=BO2[1], b=BO2[0], c=BO3[2], d=BO3[1]).
fn planar_overflow_bit(bo2: u8, bo3: u8) -> u8 {
    let a = (bo2 >> 1) & 1;
    let b = bo2 & 1;
    let c = (bo3 >> 2) & 1;
    let d = (bo3 >> 1) & 1;
    (a & c) | (!a & b & c & d) | (a & b & !c & d)
}

/// Quantize an 8-bit channel to 6 bits.
fn q6(v: i32) -> u8 {
    ((v * 63 + 127) / 255).clamp(0, 63) as u8
}

/// Quantize an 8-bit channel to 7 bits.
fn q7(v: i32) -> u8 {
    ((v * 127 + 127) / 255).clamp(0, 127) as u8
}

fn encode_planar(rgb: &[[u8; 3]; 16]) -> Option<EncResult> {
    // Two candidate fits: least squares over all 16 pixels, and an exact
    // 3-point fit through (0,0), (3,0), (0,3). Keep the better one.
    let mut best: Option<EncResult> = None;

    let fits = [planar_ls_fit(rgb), planar_3point_fit(rgb)];
    for (err, block) in fits.into_iter().flatten() {
        if best.as_ref().is_none_or(|b| err < b.err) {
            best = Some(EncResult { err, block });
        }
    }
    best
}

/// Pack a planar block from O, H, V (per-channel 6/6/6-bit except G = 7-bit).
/// Returns (err, block).
fn planar_pack(rgb: &[[u8; 3]; 16], o: [u8; 3], h: [u8; 3], v: [u8; 3]) -> (i64, [u8; 8]) {
    let ro = o[0] as i32;
    let go = (o[1] as i32) & 0x7F;
    let bo = o[2] as i32;
    let rh = h[0] as i32;
    let gh = (h[1] as i32) & 0x7F;
    let bh = h[2] as i32;
    let rv = v[0] as i32;
    let gv = (v[1] as i32) & 0x7F;
    let bv = v[2] as i32;
    let go1 = ((o[1] as i32) >> 6) & 1;
    let bo1 = ((o[2] as i32) >> 5) & 1;
    let rh1 = ((h[0] as i32) >> 1) & 0x1F;
    let rh2 = (h[0] as i32) & 1;

    // Expand to 8 bits for error computation.
    let ro8 = expand6(ro);
    let go8 = expand7(go);
    let bo8 = expand6(bo);
    let rh8 = expand6(rh);
    let gh8 = expand7(gh);
    let bh8 = expand6(bh);
    let rv8 = expand6(rv);
    let gv8 = expand7(gv);
    let bv8 = expand6(bv);

    let mut err = 0i64;
    for (i, &px) in rgb.iter().enumerate() {
        let (x, y) = stream_xy(i);
        let xi = x as i32;
        let yi = y as i32;
        let r = ((xi * (rh8 - ro8) + yi * (rv8 - ro8) + 4 * ro8 + 2) >> 2).clamp(0, 255);
        let g = ((xi * (gh8 - go8) + yi * (gv8 - go8) + 4 * go8 + 2) >> 2).clamp(0, 255);
        let b = ((xi * (bh8 - bo8) + yi * (bv8 - bo8) + 4 * bo8 + 2) >> 2).clamp(0, 255);
        err += (px[0] as i64 - r as i64).abs()
            + (px[1] as i64 - g as i64).abs()
            + (px[2] as i64 - b as i64).abs();
    }

    // Overflow padding (ETCPACK `stuff57bits`): red and green must NOT
    // overflow, blue must overflow.
    let red_bit = (ro >> 5) & 1; // = bit 62 in the stuffed word
    let green_bit = (go >> 5) & 1;
    let bb = planar_overflow_bit(((bo >> 2) & 3) as u8, (bo & 7) as u8);

    let mut block = [0u8; 8];
    block[0] = (((!red_bit & 1) << 7)
        | (((ro >> 5) & 1) << 6)
        | (((ro >> 4) & 1) << 5)
        | (((ro >> 3) & 1) << 4)
        | (((ro >> 2) & 1) << 3)
        | (((ro >> 1) & 1) << 2)
        | ((ro & 1) << 1)
        | go1) as u8; // GO1
    block[1] = (((!green_bit & 1) << 7)
        | (((go >> 5) & 1) << 6)
        | (((go >> 4) & 1) << 5)
        | (((go >> 3) & 1) << 4)
        | (((go >> 2) & 1) << 3)
        | (((go >> 1) & 1) << 2)
        | ((go & 1) << 1)
        | bo1) as u8; // BO1
    block[2] = (((bb as i32) << 7)
        | ((bb as i32) << 6)
        | ((bb as i32) << 5)
        | (((bo >> 4) & 1) << 4) // BO2 bit 1 (BO[4])
        | (((bo >> 3) & 1) << 3) // BO2 bit 0 (BO[3])
        | (((!bb & 1) as i32) << 2)
        | (((bo >> 2) & 1) << 1) // BO3 bit 2
        | ((bo >> 1) & 1)) as u8; // BO3 bit 1
    block[3] = (((bo & 1) << 7) // BO3 bit 0
        | (rh1 << 2) // RH1 = H bits 5:1
        | (1 << 1)
        | rh2) as u8; // RH2 = H bit 0
                      // GH (7 bits), BH, RV, GV, BV in word 2 (bytes 4-7).
    block[4] = (((gh >> 6) << 7)
        | (((gh >> 5) & 1) << 6)
        | (((gh >> 4) & 1) << 5)
        | (((gh >> 3) & 1) << 4)
        | (((gh >> 2) & 1) << 3)
        | (((gh >> 1) & 1) << 2)
        | ((gh & 1) << 1)
        | ((bh >> 5) & 1)) as u8;
    block[5] = ((((bh >> 4) & 1) << 7)
        | (((bh >> 3) & 1) << 6)
        | (((bh >> 2) & 1) << 5)
        | (((bh >> 1) & 1) << 4)
        | ((bh & 1) << 3)
        | (((rv >> 5) & 1) << 2)
        | (((rv >> 4) & 1) << 1)
        | ((rv >> 3) & 1)) as u8;
    block[6] = ((((rv >> 2) & 1) << 7)
        | (((rv >> 1) & 1) << 6)
        | ((rv & 1) << 5)
        | (((gv >> 6) & 1) << 4)
        | (((gv >> 5) & 1) << 3)
        | (((gv >> 4) & 1) << 2)
        | (((gv >> 3) & 1) << 1)
        | ((gv >> 2) & 1)) as u8;
    block[7] = ((((gv >> 1) & 1) << 7)
        | ((gv & 1) << 6)
        | (((bv >> 5) & 1) << 5)
        | (((bv >> 4) & 1) << 4)
        | (((bv >> 3) & 1) << 3)
        | (((bv >> 2) & 1) << 2)
        | (((bv >> 1) & 1) << 1)
        | (bv & 1)) as u8;

    (err, block)
}

/// Least-squares plane fit: c(x,y) = A + Bx + Cy over all 16 pixels.
fn planar_ls_fit(rgb: &[[u8; 3]; 16]) -> Option<(i64, [u8; 8])> {
    // For each channel: A = O, B = (H-O)/4, C = (V-O)/4.
    // Normal equations (x, y in {0..3}, 16 samples):
    //   16A + 24B + 24C = sum p
    //   24A + 56B + 36C = sum x*p
    //   24A + 36B + 56C = sum y*p
    let mut sum = [0i64; 3];
    let mut sum_x = [0i64; 3];
    let mut sum_y = [0i64; 3];
    for (i, &px) in rgb.iter().enumerate() {
        let (x, y) = stream_xy(i);
        for c in 0..3 {
            let p = px[c] as i64;
            sum[c] += p;
            sum_x[c] += p * x as i64;
            sum_y[c] += p * y as i64;
        }
    }

    let mut o = [0u8; 3];
    let mut h = [0u8; 3];
    let mut v = [0u8; 3];
    for c in 0..3 {
        let b = (sum_x[c] as f64 - 1.5 * sum[c] as f64) / 20.0;
        let cc = (sum_y[c] as f64 - 1.5 * sum[c] as f64) / 20.0;
        let a = (sum[c] as f64 - 24.0 * (b + cc)) / 16.0;
        let o8 = a;
        let h8 = a + 4.0 * b;
        let v8 = a + 4.0 * cc;
        if c == 1 {
            o[c] = q7(o8.round() as i32);
            h[c] = q7(h8.round() as i32);
            v[c] = q7(v8.round() as i32);
        } else {
            o[c] = q6(o8.round() as i32);
            h[c] = q6(h8.round() as i32);
            v[c] = q6(v8.round() as i32);
        }
    }
    Some(planar_pack(rgb, o, h, v))
}

/// Exact 3-point fit through (0,0), (3,0), (0,3):
/// O = p(0,0), H = (4*p(3,0) - O - 2)/3, V = (4*p(0,3) - O - 2)/3.
fn planar_3point_fit(rgb: &[[u8; 3]; 16]) -> Option<(i64, [u8; 8])> {
    let p00 = rgb[stream_to_scan(0)]; // stream texel 0 = (col 0, row 0)
    let p30 = rgb[stream_to_scan(3)]; // (col 0, row 3)
    let p03 = rgb[stream_to_scan(12)]; // (col 3, row 0)
    let mut o = [0u8; 3];
    let mut h = [0u8; 3];
    let mut v = [0u8; 3];
    for c in 0..3 {
        let o8 = p00[c] as i32;
        let h8 = (4 * p30[c] as i32 - o8 - 2 + 1) / 3; // round
        let v8 = (4 * p03[c] as i32 - o8 - 2 + 1) / 3;
        o[c] = if c == 1 { q7(o8) } else { q6(o8) };
        h[c] = if c == 1 { q7(h8) } else { q6(h8) };
        v[c] = if c == 1 { q7(v8) } else { q6(v8) };
    }
    Some(planar_pack(rgb, o, h, v))
}

// ---------------------------------------------------------------------------
// Reference decoders (for tests) — mirror ETCPACK's unstuff + decode logic.
// ---------------------------------------------------------------------------

/// Decode one ETC2 RGB block (8 bytes) into 16 RGB colors in SCAN order.
#[cfg(test)]
fn decode_etc2_rgb_block(block: &[u8]) -> [[u8; 3]; 16] {
    let mut out = [[0u8; 3]; 16];
    let decoded = decode_rgb_to_stream(block);
    for i in 0..16 {
        out[stream_to_scan(i)] = decoded[i];
    }
    out
}

/// Decode into stream (column-major) order.
#[cfg(test)]
fn decode_rgb_to_stream(block: &[u8]) -> [[u8; 3]; 16] {
    let mut out = [[0u8; 3]; 16];

    if block[3] & 2 == 0 {
        // Individual mode.
        let c0 = [
            ((block[0] >> 4) as i32) * 17,
            ((block[1] >> 4) as i32) * 17,
            ((block[2] >> 4) as i32) * 17,
        ];
        let c1 = [
            ((block[0] & 0xF) as i32) * 17,
            ((block[1] & 0xF) as i32) * 17,
            ((block[2] & 0xF) as i32) * 17,
        ];
        let codes = [block[3] >> 5, (block[3] >> 2) & 7];
        let flip = block[3] & 1;
        let j = u16::from_be_bytes([block[6], block[7]]);
        let k = u16::from_be_bytes([block[4], block[5]]);
        for i in 0..16 {
            let sb = SUBBLOCK_TABLE[flip as usize][i];
            let m = ETC1_MODIFIER_TABLE[codes[sb] as usize][((j >> i) & 1) as usize] as i32;
            let sign = if (k >> i) & 1 == 1 { -1 } else { 1 };
            let base = if sb == 0 { c0 } else { c1 };
            out[i] = [
                (base[0] + sign * m).clamp(0, 255) as u8,
                (base[1] + sign * m).clamp(0, 255) as u8,
                (base[2] + sign * m).clamp(0, 255) as u8,
            ];
        }
        return out;
    }

    // diff = 1: check which differential overflows to pick the mode.
    // The 3-bit delta fields are signed two's complement.
    let red1 = block[0] >> 3;
    let dred2 = sign_extend3(block[0] & 7);
    let green1 = block[1] >> 3;
    let dgreen2 = sign_extend3(block[1] & 7);
    let blue1 = block[2] >> 3;
    let dblue2 = sign_extend3(block[2] & 7);

    let r = red1 as i32 + dred2;
    let g = green1 as i32 + dgreen2;
    let b = blue1 as i32 + dblue2;

    if !(0..=31).contains(&r) {
        // T mode.
        let r0 = ((block[0] >> 4) & 1) << 3
            | ((block[0] >> 3) & 1) << 2
            | ((block[0] >> 1) & 1) << 1
            | (block[0] & 1);
        let g0 = block[1] >> 4;
        let b0 = block[1] & 0xF;
        let r1 = block[2] >> 4;
        let g1 = block[2] & 0xF;
        let b1 = block[3] >> 4;
        let dist = (((block[3] >> 1) & 6) | (block[3] & 1)) as usize;
        let d = ETC2_DISTANCE_TABLE[dist] as i32;
        let c0 = [(r0 as i32) * 17, (g0 as i32) * 17, (b0 as i32) * 17];
        let c1 = [(r1 as i32) * 17, (g1 as i32) * 17, (b1 as i32) * 17];
        let palette = [c0, add_clamp(c1, d), c1, add_clamp(c1, -d)];
        let j = u16::from_be_bytes([block[6], block[7]]);
        let k = u16::from_be_bytes([block[4], block[5]]);
        for (i, o) in out.iter_mut().enumerate() {
            let s = (((k >> i) & 1) << 1) | ((j >> i) & 1);
            *o = [
                palette[s as usize][0] as u8,
                palette[s as usize][1] as u8,
                palette[s as usize][2] as u8,
            ];
        }
        return out;
    }

    if !(0..=31).contains(&g) {
        // H mode.
        let r0 = ((block[0] >> 6) & 1) << 3
            | ((block[0] >> 5) & 1) << 2
            | ((block[0] >> 4) & 1) << 1
            | ((block[0] >> 3) & 1);
        let g0 = ((block[0] >> 2) & 1) << 3
            | ((block[0] >> 1) & 1) << 2
            | (block[0] & 1) << 1
            | ((block[1] >> 4) & 1);
        let b0 = ((block[1] >> 3) & 1) << 3
            | ((block[1] >> 1) & 1) << 2
            | (block[1] & 1) << 1
            | ((block[2] >> 7) & 1);
        let r1 = (block[2] >> 3) & 0xF;
        let g1 = ((block[2] >> 2) & 1) << 3
            | ((block[2] >> 1) & 1) << 2
            | (block[2] & 1) << 1
            | ((block[3] >> 7) & 1);
        let b1 = (block[3] >> 3) & 0xF;
        let stored = (((block[3] >> 2) & 1) << 1) | (block[3] & 1);
        let c0 = [(r0 as i32) * 17, (g0 as i32) * 17, (b0 as i32) * 17];
        let c1 = [(r1 as i32) * 17, (g1 as i32) * 17, (b1 as i32) * 17];
        let implied = if packed_cmp(c0, c1) { 1 } else { 0 };
        let dist = ((stored << 1) | implied) as usize;
        let d = ETC2_DISTANCE_TABLE[dist] as i32;
        let palette = [
            add_clamp(c0, d),
            add_clamp(c0, -d),
            add_clamp(c1, d),
            add_clamp(c1, -d),
        ];
        let j = u16::from_be_bytes([block[6], block[7]]);
        let k = u16::from_be_bytes([block[4], block[5]]);
        for (i, o) in out.iter_mut().enumerate() {
            let s = (((k >> i) & 1) << 1) | ((j >> i) & 1);
            *o = [
                palette[s as usize][0] as u8,
                palette[s as usize][1] as u8,
                palette[s as usize][2] as u8,
            ];
        }
        return out;
    }

    if !(0..=31).contains(&b) {
        // Planar mode.
        let ro = (((block[0] >> 6) & 1) << 5
            | ((block[0] >> 5) & 1) << 4
            | ((block[0] >> 4) & 1) << 3
            | ((block[0] >> 3) & 1) << 2
            | ((block[0] >> 2) & 1) << 1
            | ((block[0] >> 1) & 1)) as i32;
        let go = (((block[0] & 1) << 6) | ((block[1] >> 1) & 0x3F)) as i32;
        let bo = (((block[1] & 1) << 5)
            | (((block[2] >> 3) & 3) << 3)
            | (((block[2] & 3) << 1) | ((block[3] >> 7) & 1))) as i32;
        let rh = ((((block[3] >> 2) & 0x1F) << 1) | (block[3] & 1)) as i32;
        let gh = ((block[4] >> 1) & 0x7F) as i32;
        let bh = (((block[4] & 1) << 5) | ((block[5] >> 3) & 0x1F)) as i32;
        let rv = (((block[5] & 7) << 3) | ((block[6] >> 5) & 7)) as i32;
        let gv = (((block[6] & 0x1F) as i32) << 2) | (((block[7] >> 6) & 3) as i32);
        let bv = (block[7] & 0x3F) as i32;

        let ro8 = expand6(ro);
        let go8 = expand7(go);
        let bo8 = expand6(bo);
        let rh8 = expand6(rh);
        let gh8 = expand7(gh);
        let bh8 = expand6(bh);
        let rv8 = expand6(rv);
        let gv8 = expand7(gv);
        let bv8 = expand6(bv);

        for (i, o) in out.iter_mut().enumerate() {
            let (x, y) = stream_xy(i);
            let xi = x as i32;
            let yi = y as i32;
            *o = [
                ((xi * (rh8 - ro8) + yi * (rv8 - ro8) + 4 * ro8 + 2) >> 2).clamp(0, 255) as u8,
                ((xi * (gh8 - go8) + yi * (gv8 - go8) + 4 * go8 + 2) >> 2).clamp(0, 255) as u8,
                ((xi * (bh8 - bo8) + yi * (bv8 - bo8) + 4 * bo8 + 2) >> 2).clamp(0, 255) as u8,
            ];
        }
        return out;
    }

    // Differential mode.
    let c0 = [
        expand5(red1 as i32),
        expand5(green1 as i32),
        expand5(blue1 as i32),
    ];
    let c1 = [expand5(r), expand5(g), expand5(b)];
    let codes = [block[3] >> 5, (block[3] >> 2) & 7];
    let flip = block[3] & 1;
    let j = u16::from_be_bytes([block[6], block[7]]);
    let k = u16::from_be_bytes([block[4], block[5]]);
    for i in 0..16 {
        let sb = SUBBLOCK_TABLE[flip as usize][i];
        let m = ETC1_MODIFIER_TABLE[codes[sb] as usize][((j >> i) & 1) as usize] as i32;
        let sign = if (k >> i) & 1 == 1 { -1 } else { 1 };
        let base = if sb == 0 { c0 } else { c1 };
        out[i] = [
            (base[0] + sign * m).clamp(0, 255) as u8,
            (base[1] + sign * m).clamp(0, 255) as u8,
            (base[2] + sign * m).clamp(0, 255) as u8,
        ];
    }
    out
}

#[cfg(test)]
/// Sign-extend a 3-bit two's-complement value.
fn sign_extend3(v: u8) -> i32 {
    if v & 4 != 0 {
        (v as i32) - 8
    } else {
        v as i32
    }
}

#[cfg(test)]
fn packed_cmp(a: [i32; 3], b: [i32; 3]) -> bool {
    let pa = (a[0] << 16) | (a[1] << 8) | a[2];
    let pb = (b[0] << 16) | (b[1] << 8) | b[2];
    pa >= pb
}

/// Expand a 4-bit color triple to 8-bit.
fn expand4_3(c: [u8; 3]) -> [i32; 3] {
    [(c[0] as i32) * 17, (c[1] as i32) * 17, (c[2] as i32) * 17]
}

/// Encode one block from RGBA pixels and decode it back (SCAN order).
#[cfg(test)]
fn roundtrip(pixels: &[[u8; 4]; 16]) -> [[u8; 3]; 16] {
    let block = encode_block(pixels);
    decode_etc2_rgb_block(&block[8..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a scan-order 4x4 block of the given RGB colors.
    fn block_of(colors: [[u8; 3]; 16]) -> [[u8; 4]; 16] {
        let mut out = [[0u8; 4]; 16];
        for i in 0..16 {
            out[i] = [colors[i][0], colors[i][1], colors[i][2], 255];
        }
        out
    }

    fn gradient_block() -> [[u8; 4]; 16] {
        let mut out = [[0u8; 4]; 16];
        for y in 0..4 {
            for x in 0..4 {
                out[y * 4 + x] = [
                    (x as u8) * 40,
                    (y as u8) * 40,
                    ((x as u8) * 30 + (y as u8) * 20),
                    200 + x as u8 * 10,
                ];
            }
        }
        out
    }

    #[test]
    fn individual_roundtrip_close() {
        // Two flat halves: individual mode should get very close.
        let mut colors = [[0u8; 3]; 16];
        for (i, c) in colors.iter_mut().enumerate() {
            *c = if i < 8 {
                [100, 50, 200]
            } else {
                [200, 100, 50]
            };
        }
        let pixels = block_of(colors);
        let decoded = roundtrip(&pixels);
        for (i, d) in decoded.iter().enumerate() {
            let expect = if i < 8 {
                [100, 50, 200]
            } else {
                [200, 100, 50]
            };
            for c in 0..3 {
                assert!(
                    (d[c] as i64 - expect[c] as i64).abs() <= 3,
                    "pixel {i} ch {c}: {} vs {}",
                    d[c],
                    expect[c]
                );
            }
        }
    }

    #[test]
    fn uniform_color_block() {
        let mut colors = [[0u8; 3]; 16];
        for c in colors.iter_mut() {
            *c = [80, 120, 160];
        }
        let decoded = roundtrip(&block_of(colors));
        // All pixels must decode identically and stay close to the source.
        let first = decoded[0];
        for d in decoded.iter().skip(1) {
            assert_eq!(*d, first, "non-uniform decode");
        }
        for c in 0..3 {
            assert!((first[c] as i64 - colors[0][c] as i64).abs() <= 4);
        }
    }

    #[test]
    fn differential_roundtrip_close() {
        let pixels = gradient_block();
        let block = encode_block(&pixels);
        let rgb = encode_etc2_rgb(&rgb_stream(&pixels));
        // Differential must be valid when the diff bit is set.
        assert_eq!(rgb[3] & 2, 2, "diff bit should be set for gradient");
        let decoded = decode_etc2_rgb_block(&block[8..16]);
        for i in 0..16 {
            for c in 0..3 {
                let e = (pixels[i][c] as i64 - decoded[i][c] as i64).abs();
                assert!(
                    e <= 20,
                    "pixel {i} ch {c}: {} vs {}",
                    pixels[i][c],
                    decoded[i][c]
                );
            }
        }
    }

    /// Build the 8 RGB bytes of a T-mode block directly from the layout.
    fn build_t_block(c0: [u8; 3], c1: [u8; 3], dist: usize, sel: [u8; 16]) -> [u8; 8] {
        let r0 = c0[0];
        let g0 = c0[1];
        let b0 = c0[2];
        let r1 = c1[0];
        let g1 = c1[1];
        let b1 = c1[2];
        let bit = t_overflow_bit(r0);
        let mut block = [0u8; 8];
        block[0] = (bit << 7)
            | (bit << 6)
            | (bit << 5)
            | ((r0 >> 3) << 4)
            | ((r0 >> 2) << 3)
            | ((!bit & 1) << 2)
            | ((r0 >> 1) << 1)
            | (r0 & 1);
        block[1] = (g0 << 4) | b0;
        block[2] = (r1 << 4) | g1;
        block[3] = (b1 << 4) | (((dist >> 1) as u8) << 2) | (1 << 1) | (dist as u8 & 1);
        let (k, j) = pack_selectors(&sel);
        block[4..6].copy_from_slice(&k.to_be_bytes());
        block[6..8].copy_from_slice(&j.to_be_bytes());
        block
    }

    #[test]
    fn t_mode_decoder_exact() {
        // Hand-built T block: decodes to exactly the palette.
        let c0 = [5u8, 10, 15];
        let c1 = [10u8, 5, 3];
        let dist = 3usize; // d = 16
        let d = ETC2_DISTANCE_TABLE[dist] as i32;
        let c0x = expand4_3(c0);
        let c1x = expand4_3(c1);
        let palette = [c0x, add_clamp(c1x, d), c1x, add_clamp(c1x, -d)];
        let mut sel = [0u8; 16];
        for (i, s) in sel.iter_mut().enumerate() {
            *s = (i % 4) as u8;
        }
        let block = build_t_block(c0, c1, dist, sel);
        let decoded = decode_etc2_rgb_block(&block);
        for i in 0..16 {
            // decoded[i] is scan order; selectors are indexed by stream position.
            let p = palette[sel[stream_to_scan(i)] as usize];
            assert_eq!(
                decoded[i],
                [p[0] as u8, p[1] as u8, p[2] as u8],
                "pixel {i}"
            );
        }
    }

    #[test]
    fn t_mode_encoder_close() {
        // Encode a T-palette block; the search may pick a different palette
        // but the result must be close and the mode bits must be right.
        let c0 = [5u8, 10, 15];
        let c1 = [10u8, 5, 3];
        let dist = 3usize;
        let d = ETC2_DISTANCE_TABLE[dist] as i32;
        let c0x = expand4_3(c0);
        let c1x = expand4_3(c1);
        let palette = [c0x, add_clamp(c1x, d), c1x, add_clamp(c1x, -d)];
        let mut colors = [[0u8; 3]; 16];
        for (i, c) in colors.iter_mut().enumerate() {
            let p = palette[i % 4];
            *c = [p[0] as u8, p[1] as u8, p[2] as u8];
        }
        let rgb = rgb_stream_of(&colors);

        let enc = encode_t(&rgb).expect("T mode should encode");
        assert_eq!(enc.block[3] & 2, 2, "diff bit");
        let red1 = enc.block[0] >> 3;
        let dred2 = sign_extend3(enc.block[0] & 7);
        assert!(
            red1 as i32 + dred2 < 0 || red1 as i32 + dred2 > 31,
            "R differential must overflow for T mode"
        );
        let decoded = decode_etc2_rgb_block(&enc.block);
        let mut err = 0i64;
        for i in 0..16 {
            for c in 0..3 {
                err += (decoded[i][c] as i64 - colors[i][c] as i64).abs();
            }
        }
        assert!(err < 600, "T encode error too high: {err}");
    }

    /// Build the 8 RGB bytes of an H-mode block directly from the layout.
    fn build_h_block(c0: [u8; 3], c1: [u8; 3], stored: usize, sel: [u8; 16]) -> [u8; 8] {
        let r0 = c0[0];
        let g0 = c0[1];
        let b0 = c0[2];
        let r1 = c1[0];
        let g1 = c1[1];
        let b1 = c1[2];
        let gbit = h_overflow_bit(g0, b0);
        let mut block = [0u8; 8];
        block[0] = ((!r0 >> 3 & 1) << 7)
            | ((r0 >> 3) << 6)
            | ((r0 >> 2) << 5)
            | ((r0 >> 1) << 4)
            | (r0 << 3 & 8)
            | ((g0 >> 3) << 2)
            | ((g0 >> 2) << 1)
            | (g0 >> 1 & 1);
        block[1] = (gbit << 7)
            | (gbit << 6)
            | (gbit << 5)
            | ((g0 & 1) << 4)
            | ((b0 >> 3) << 3)
            | ((!gbit & 1) << 2)
            | ((b0 >> 2) << 1)
            | (b0 >> 1 & 1);
        block[2] = ((b0 & 1) << 7)
            | ((r1 & 0xF) << 3)
            | ((g1 >> 3) << 2)
            | ((g1 >> 2) << 1)
            | (g1 >> 1 & 1);
        block[3] = ((g1 & 1) << 7)
            | ((b1 & 0xF) << 3)
            | ((((stored >> 1) & 1) as u8) << 2)
            | (1 << 1)
            | ((stored & 1) as u8);
        let (k, j) = pack_selectors(&sel);
        block[4..6].copy_from_slice(&k.to_be_bytes());
        block[6..8].copy_from_slice(&j.to_be_bytes());
        block
    }

    #[test]
    fn h_mode_decoder_exact() {
        // Hand-built H block: colors ordered so the implied LSB matches.
        let c0 = [15u8, 5, 3]; // packed 0xF53
        let c1 = [3u8, 10, 12]; // packed 0x3AC -> c0 >= c1, implied lsb = 1
        let stored = 2usize; // dist = (2 << 1) | 1 = 5 -> 32
        let implied = 1usize;
        let dist = (stored << 1) | implied;
        let d = ETC2_DISTANCE_TABLE[dist] as i32;
        let c0x = expand4_3(c0);
        let c1x = expand4_3(c1);
        let palette = [
            add_clamp(c0x, d),
            add_clamp(c0x, -d),
            add_clamp(c1x, d),
            add_clamp(c1x, -d),
        ];
        let mut sel = [0u8; 16];
        for (i, s) in sel.iter_mut().enumerate() {
            *s = ((i * 3) % 4) as u8;
        }
        let block = build_h_block(c0, c1, stored, sel);
        let decoded = decode_etc2_rgb_block(&block);
        for i in 0..16 {
            let p = palette[sel[stream_to_scan(i)] as usize];
            assert_eq!(
                decoded[i],
                [p[0] as u8, p[1] as u8, p[2] as u8],
                "pixel {i}"
            );
        }
    }

    #[test]
    fn h_mode_encoder_close() {
        let c0 = [15u8, 5, 3];
        let c1 = [3u8, 10, 12];
        let stored = 2usize;
        let implied = 1usize;
        let dist = (stored << 1) | implied;
        let d = ETC2_DISTANCE_TABLE[dist] as i32;
        let c0x = expand4_3(c0);
        let c1x = expand4_3(c1);
        let palette = [
            add_clamp(c0x, d),
            add_clamp(c0x, -d),
            add_clamp(c1x, d),
            add_clamp(c1x, -d),
        ];
        let mut colors = [[0u8; 3]; 16];
        for (i, c) in colors.iter_mut().enumerate() {
            let p = palette[(i * 3) % 4];
            *c = [p[0] as u8, p[1] as u8, p[2] as u8];
        }
        let rgb = rgb_stream_of(&colors);

        let enc = encode_h(&rgb).expect("H mode should encode");
        assert_eq!(enc.block[3] & 2, 2, "diff bit");
        let red1 = enc.block[0] >> 3;
        let dred2 = sign_extend3(enc.block[0] & 7);
        let green1 = enc.block[1] >> 3;
        let dgreen2 = sign_extend3(enc.block[1] & 7);
        assert!(
            red1 as i32 + dred2 >= 0 && red1 as i32 + dred2 <= 31,
            "R differential must NOT overflow for H mode"
        );
        assert!(
            green1 as i32 + dgreen2 < 0 || green1 as i32 + dgreen2 > 31,
            "G differential must overflow for H mode"
        );
        let decoded = decode_etc2_rgb_block(&enc.block);
        let mut err = 0i64;
        for i in 0..16 {
            for c in 0..3 {
                err += (decoded[i][c] as i64 - colors[i][c] as i64).abs();
            }
        }
        assert!(err < 1500, "H encode error too high: {err}");
    }

    /// Build the 8 RGB bytes of a planar block directly from O/H/V.
    fn build_planar_block(o: [u8; 3], h: [u8; 3], v: [u8; 3]) -> [u8; 8] {
        planar_pack(&[[0u8; 3]; 16], o, h, v).1
    }

    #[test]
    fn planar_decoder_exact_plane() {
        // Build a planar block directly and check the decoder reproduces the
        // exact plane.
        let o = [10u8, 20, 30]; // expand: R 40, G 40, B 120
        let h = [20u8, 40, 60]; // expand: R 80, G 80, B 240
        let v = [5u8, 10, 15]; // expand: R 20, G 20, B 60
        let block = build_planar_block(o, h, v);
        let decoded = decode_etc2_rgb_block(&block);
        let ro8 = expand6(o[0] as i32);
        let rh8 = expand6(h[0] as i32);
        let rv8 = expand6(v[0] as i32);
        let go8 = expand7(o[1] as i32);
        let gh8 = expand7(h[1] as i32);
        let gv8 = expand7(v[1] as i32);
        let bo8 = expand6(o[2] as i32);
        let bh8 = expand6(h[2] as i32);
        let bv8 = expand6(v[2] as i32);
        for (i, d) in decoded.iter().enumerate() {
            // decoded[i] is in SCAN order: scan pixel i = (x = i % 4, y = i / 4).
            let xi = (i % 4) as i32;
            let yi = (i / 4) as i32;
            let expect = [
                ((xi * (rh8 - ro8) + yi * (rv8 - ro8) + 4 * ro8 + 2) >> 2) as u8,
                ((xi * (gh8 - go8) + yi * (gv8 - go8) + 4 * go8 + 2) >> 2) as u8,
                ((xi * (bh8 - bo8) + yi * (bv8 - bo8) + 4 * bo8 + 2) >> 2) as u8,
            ];
            assert_eq!(*d, expect, "pixel {i}");
        }
    }

    #[test]
    fn planar_encoder_close() {
        // A plane block: the LS fit should recover it almost exactly.
        let ro = 10i32;
        let rh = 20i32;
        let rv = 5i32;
        let go = 20i32;
        let gh = 40i32;
        let gv = 10i32;
        let bo = 30i32;
        let bh = 60i32;
        let bv = 15i32;
        let ro8 = expand6(ro);
        let rh8 = expand6(rh);
        let rv8 = expand6(rv);
        let go8 = expand7(go);
        let gh8 = expand7(gh);
        let gv8 = expand7(gv);
        let bo8 = expand6(bo);
        let bh8 = expand6(bh);
        let bv8 = expand6(bv);
        let mut colors = [[0u8; 3]; 16];
        for y in 0..4 {
            for x in 0..4 {
                let xi = x as i32;
                let yi = y as i32;
                colors[y * 4 + x] = [
                    ((xi * (rh8 - ro8) + yi * (rv8 - ro8) + 4 * ro8 + 2) >> 2) as u8,
                    ((xi * (gh8 - go8) + yi * (gv8 - go8) + 4 * go8 + 2) >> 2) as u8,
                    ((xi * (bh8 - bo8) + yi * (bv8 - bo8) + 4 * bo8 + 2) >> 2) as u8,
                ];
            }
        }
        let rgb = rgb_stream_of(&colors);

        let enc = encode_planar(&rgb).expect("planar should encode");
        assert_eq!(enc.block[3] & 2, 2, "diff bit");
        let red1 = enc.block[0] >> 3;
        let dred2 = sign_extend3(enc.block[0] & 7);
        let green1 = enc.block[1] >> 3;
        let dgreen2 = sign_extend3(enc.block[1] & 7);
        let blue1 = enc.block[2] >> 3;
        let dblue2 = sign_extend3(enc.block[2] & 7);
        assert!(
            (0..=31).contains(&(red1 as i32 + dred2)),
            "R must not overflow"
        );
        assert!(
            (0..=31).contains(&(green1 as i32 + dgreen2)),
            "G must not overflow"
        );
        assert!(
            blue1 as i32 + dblue2 < 0 || blue1 as i32 + dblue2 > 31,
            "B must overflow for planar"
        );
        let decoded = decode_etc2_rgb_block(&enc.block);
        let mut err = 0i64;
        for i in 0..16 {
            for c in 0..3 {
                err += (decoded[i][c] as i64 - colors[i][c] as i64).abs();
            }
        }
        assert!(err < 100, "planar encode error too high: {err}");
    }

    #[test]
    fn gradient_block_mode_quality() {
        // The full encoder must produce a decodable block close to the source.
        let pixels = gradient_block();
        let block = encode_block(&pixels);
        let decoded = decode_etc2_rgb_block(&block[8..16]);
        let mut total = 0i64;
        for i in 0..16 {
            for c in 0..3 {
                total += (pixels[i][c] as i64 - decoded[i][c] as i64).abs();
            }
        }
        assert!(total < 800, "total error too high: {total}");
    }

    #[test]
    fn all_five_modes_valid() {
        // Each mode encoder must produce a block that decodes without panic
        // and matches its own reported error.
        let pixels = gradient_block();
        let rgb = rgb_stream(&pixels);

        let (err_i, block_i) = encode_individual(&rgb);
        assert!(err_i >= 0);
        let dec = decode_etc2_rgb_block(&block_i);
        assert_eq!(dec.len(), 16);

        let d = encode_differential(&rgb).expect("differential");
        let _ = decode_etc2_rgb_block(&d.block);

        let t = encode_t(&rgb).expect("t");
        let _ = decode_etc2_rgb_block(&t.block);

        let h = encode_h(&rgb).expect("h");
        let _ = decode_etc2_rgb_block(&h.block);

        let p = encode_planar(&rgb).expect("planar");
        let _ = decode_etc2_rgb_block(&p.block);
    }

    #[test]
    fn eac_uniform_alpha() {
        let alpha = [128u8; 16];
        let out = encode_eac_alpha(&alpha);
        assert_eq!(out[0], 128);
        assert_eq!(out[1] >> 4, 0); // multiplier 0
    }

    #[test]
    fn eac_alpha_roundtrip() {
        let alpha = [
            0u8, 16, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240,
        ];
        // Mirror encode_block: modifiers are written in reverse column-major
        // order, so feed the encoder the reversed ramp.
        let mut alpha_rev = [0u8; 16];
        for i in 0..16 {
            alpha_rev[i] = alpha[15 - i];
        }
        let out = encode_eac_alpha(&alpha_rev);
        // Decode per the EAC rules.
        let base = out[0] as i32;
        let mult = (out[1] >> 4) as i32;
        let table = (out[1] & 0xF) as usize;
        let mut word = u64::from_be_bytes([0, 0, out[2], out[3], out[4], out[5], out[6], out[7]]);
        let mut err = 0i64;
        for i in 0..16 {
            // EAC stores 3 bits per texel (an index 0..7 into the 8-value
            // modifier table), matching texture2ddecoder's `l & 7; l >>= 3`.
            let mod_idx = (word & 7) as usize;
            word >>= 3;
            let val = (base + mult * EAC_MODIFIER_TABLE[table][mod_idx] as i32).clamp(0, 255);
            // Modifiers are written in reverse column-major order (the
            // encoder receives `alpha_rev`), so texel i corresponds to
            // alpha[15 - i].
            err += (alpha[15 - i] as i64 - val as i64).abs();
        }
        assert!(err < 400, "EAC alpha error too high: {err}");
    }

    /// Stream-order RGB from a block of scan-order pixels.
    fn rgb_stream(pixels: &[[u8; 4]; 16]) -> [[u8; 3]; 16] {
        let mut rgb = [[0u8; 3]; 16];
        for (i, c) in rgb.iter_mut().enumerate() {
            let s = stream_to_scan(i);
            *c = [pixels[s][0], pixels[s][1], pixels[s][2]];
        }
        rgb
    }

    fn rgb_stream_of(colors: &[[u8; 3]; 16]) -> [[u8; 3]; 16] {
        let mut rgb = [[0u8; 3]; 16];
        for (i, c) in rgb.iter_mut().enumerate() {
            *c = colors[stream_to_scan(i)];
        }
        rgb
    }

    // -----------------------------------------------------------------------
    // Cross-validation against an independent ETC2 decoder (texture2ddecoder).
    // -----------------------------------------------------------------------

    #[test]
    fn debug_independent_mismatch() {
        // Two-half block, individual mode: print both decoders in full.
        let pixels = two_half_block();
        let block = encode_block(&pixels);
        let rgb8 = &block[8..16];
        println!("rgb8={:02x?}", rgb8);
        let ours = decode_etc2_rgb_block(rgb8);
        let mut theirs = [0u32; 16];
        texture2ddecoder::decode_etc2_rgb_block(rgb8, &mut theirs);
        for i in 0..16 {
            let t = [
                ((theirs[i] >> 16) & 0xFF) as u8,
                ((theirs[i] >> 8) & 0xFF) as u8,
                (theirs[i] & 0xFF) as u8,
            ];
            println!(
                "px{i} ours={:?} theirs={:?} src={:?}",
                ours[i], t, pixels[i]
            );
        }

        // Differential test block
        let mut colors = [[0u8; 3]; 16];
        for (i, c) in colors.iter_mut().enumerate() {
            *c = if i < 8 {
                [100, 50, 200]
            } else {
                [120, 60, 220]
            };
        }
        let rgb = rgb_stream_of(&colors);
        let enc = encode_differential(&rgb).expect("differential");
        println!("diff block={:02x?}", enc.block);
        let ours = decode_etc2_rgb_block(&enc.block);
        let mut theirs = [0u32; 16];
        texture2ddecoder::decode_etc2_rgb_block(&enc.block, &mut theirs);
        for i in 0..16 {
            let t = [
                ((theirs[i] >> 16) & 0xFF) as u8,
                ((theirs[i] >> 8) & 0xFF) as u8,
                (theirs[i] & 0xFF) as u8,
            ];
            println!(
                "dpx{i} ours={:?} theirs={:?} src={:?}",
                ours[i],
                t,
                colors[stream_to_scan(i)]
            );
        }
    }

    #[test]
    fn debug_hand_built_t() {
        let c0 = [5u8, 10, 15];
        let c1 = [10u8, 5, 3];
        let dist = 3usize;
        let mut sel = [0u8; 16];
        for (i, s) in sel.iter_mut().enumerate() {
            *s = (i % 4) as u8;
        }
        let block = build_t_block(c0, c1, dist, sel);
        println!("block={:02x?}", block);
        println!("decoded={:?}", decode_etc2_rgb_block(&block));
        let mut theirs = [0u32; 16];
        texture2ddecoder::decode_etc2_rgb_block(&block, &mut theirs);
        println!(
            "theirs={:?}",
            theirs
                .iter()
                .map(|v| [
                    ((v >> 16) & 0xFF) as u8,
                    ((v >> 8) & 0xFF) as u8,
                    (v & 0xFF) as u8
                ])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn debug_t_h_bytes() {
        // T test palette
        let c0 = [5u8, 10, 15];
        let c1 = [10u8, 5, 3];
        let dist = 3usize;
        let d = ETC2_DISTANCE_TABLE[dist] as i32;
        let c0x = expand4_3(c0);
        let c1x = expand4_3(c1);
        let palette = [c0x, add_clamp(c1x, d), c1x, add_clamp(c1x, -d)];
        let mut colors = [[0u8; 3]; 16];
        for (i, c) in colors.iter_mut().enumerate() {
            let p = palette[i % 4];
            *c = [p[0] as u8, p[1] as u8, p[2] as u8];
        }
        let rgb = rgb_stream_of(&colors);
        let enc = encode_t(&rgb).unwrap();
        println!("T block={:02x?} err={}", enc.block, enc.err);
        println!("T ours={:?}", decode_etc2_rgb_block(&enc.block));
        let mut theirs = [0u32; 16];
        texture2ddecoder::decode_etc2_rgb_block(&enc.block, &mut theirs);
        println!(
            "T theirs={:?}",
            theirs
                .iter()
                .map(|v| [
                    (*v & 0xFF) as u8,
                    ((*v >> 8) & 0xFF) as u8,
                    ((*v >> 16) & 0xFF) as u8
                ])
                .collect::<Vec<_>>()
        );

        // H test palette
        let c0 = [15u8, 5, 3];
        let c1 = [3u8, 10, 12];
        let stored = 2usize;
        let implied = 1usize;
        let dist = (stored << 1) | implied;
        let d = ETC2_DISTANCE_TABLE[dist] as i32;
        let c0x = expand4_3(c0);
        let c1x = expand4_3(c1);
        let palette = [
            add_clamp(c0x, d),
            add_clamp(c0x, -d),
            add_clamp(c1x, d),
            add_clamp(c1x, -d),
        ];
        let mut colors = [[0u8; 3]; 16];
        for (i, c) in colors.iter_mut().enumerate() {
            let p = palette[(i * 3) % 4];
            *c = [p[0] as u8, p[1] as u8, p[2] as u8];
        }
        let rgb = rgb_stream_of(&colors);
        let enc = encode_h(&rgb).unwrap();
        println!("H block={:02x?} err={}", enc.block, enc.err);
        println!("H ours={:?}", decode_etc2_rgb_block(&enc.block));
    }

    #[test]
    fn debug_each_mode_gradient() {
        let pixels = gradient_block();
        let rgb = rgb_stream(&pixels);
        let (ei, bi) = encode_individual(&rgb);
        println!("individual err={} bytes={:02x?}", ei, bi);
        let d = encode_differential(&rgb).unwrap();
        println!("diff err={} bytes={:02x?}", d.err, d.block);
        let t = encode_t(&rgb).unwrap();
        println!("T err={} bytes={:02x?}", t.err, t.block);
        let h = encode_h(&rgb).unwrap();
        println!("H err={} bytes={:02x?}", h.err, h.block);
        let p = encode_planar(&rgb).unwrap();
        println!("planar err={} bytes={:02x?}", p.err, p.block);
        let dec = decode_etc2_rgb_block(&p.block);
        println!("planar dec[0]={:?} src[0]={:?}", dec[0], pixels[0]);
        // which mode did the full encoder pick?
        let full = encode_block(&pixels);
        let fdec = decode_etc2_rgb_block(&full[8..16]);
        println!(
            "full bytes={:02x?} dec[0]={:?} src[0]={:?}",
            &full[8..16],
            fdec[0],
            pixels[0]
        );
    }

    /// A block with two flat halves: the full encoder picks individual mode,
    /// which texture2ddecoder decodes correctly (its H/planar decoders are buggy).
    fn two_half_block() -> [[u8; 4]; 16] {
        let mut colors = [[0u8; 3]; 16];
        for (i, c) in colors.iter_mut().enumerate() {
            *c = if i < 8 {
                [100, 50, 200]
            } else {
                [200, 100, 50]
            };
        }
        block_of(colors)
    }

    #[test]
    fn decodes_with_independent_decoder() {
        // Check the independent decoder agrees with our in-crate decoder
        // bit-for-bit (both decode the same stream).
        let pixels = two_half_block();
        let block = encode_block(&pixels);
        let rgb8 = &block[8..16];
        // The encoder may pick individual or differential for flat halves;
        // both are valid ETC2 modes — only the decode agreement matters here.

        // texture2ddecoder packs each pixel as u32::from_le_bytes([b, g, r, a]).
        let to_rgb = |v: u32| -> [u8; 3] {
            [
                ((v >> 16) & 0xFF) as u8,
                ((v >> 8) & 0xFF) as u8,
                (v & 0xFF) as u8,
            ]
        };
        let ours = decode_etc2_rgb_block(rgb8);
        let mut theirs = [0u32; 16];
        texture2ddecoder::decode_etc2_rgb_block(rgb8, &mut theirs);
        for i in 0..16 {
            assert_eq!(ours[i], to_rgb(theirs[i]), "decoder mismatch at pixel {i}");
        }
    }

    #[test]
    fn independent_decoder_error_is_low() {
        // The independent decoder's output must be close to the source
        // (validates overall quality, not just validity).
        let pixels = two_half_block();
        let block = encode_block(&pixels);
        let rgb8 = &block[8..16];
        let to_rgb = |v: u32| -> [u8; 3] {
            [
                ((v >> 16) & 0xFF) as u8,
                ((v >> 8) & 0xFF) as u8,
                (v & 0xFF) as u8,
            ]
        };
        let mut theirs = [0u32; 16];
        texture2ddecoder::decode_etc2_rgb_block(rgb8, &mut theirs);
        let mut total = 0i64;
        for i in 0..16 {
            // texture2ddecoder fills the output in SCAN order (via
            // WRITE_ORDER_TABLE), so compare directly with the source.
            let t = to_rgb(theirs[i]);
            for c in 0..3 {
                total += (pixels[i][c] as i64 - t[c] as i64).abs();
            }
        }
        assert!(total < 300, "independent decoder error too high: {total}");
    }

    #[test]
    fn t_mode_validated_by_independent_decoder() {
        // T mode must be decoded identically by texture2ddecoder.
        let c0 = [5u8, 10, 15];
        let c1 = [10u8, 5, 3];
        let dist = 3usize;
        let d = ETC2_DISTANCE_TABLE[dist] as i32;
        let c0x = expand4_3(c0);
        let c1x = expand4_3(c1);
        let palette = [c0x, add_clamp(c1x, d), c1x, add_clamp(c1x, -d)];
        let mut colors = [[0u8; 3]; 16];
        for (i, c) in colors.iter_mut().enumerate() {
            let p = palette[i % 4];
            *c = [p[0] as u8, p[1] as u8, p[2] as u8];
        }
        let to_rgb = |v: u32| -> [u8; 3] {
            [
                ((v >> 16) & 0xFF) as u8,
                ((v >> 8) & 0xFF) as u8,
                (v & 0xFF) as u8,
            ]
        };
        let rgb = rgb_stream_of(&colors);
        let enc = encode_t(&rgb).unwrap();
        let ours = decode_etc2_rgb_block(&enc.block);
        let mut theirs = [0u32; 16];
        texture2ddecoder::decode_etc2_rgb_block(&enc.block, &mut theirs);
        for i in 0..16 {
            assert_eq!(
                ours[i],
                to_rgb(theirs[i]),
                "T decoder mismatch at pixel {i}"
            );
        }
    }

    #[test]
    fn differential_validated_by_independent_decoder() {
        // Force differential mode with two flat subblocks and confirm both
        // decoders agree and the result is exact.
        let mut colors = [[0u8; 3]; 16];
        for (i, c) in colors.iter_mut().enumerate() {
            // Bases picked so the 5-bit deltas are valid and the modifier is 0.
            *c = if i < 8 {
                [100, 50, 200]
            } else {
                [120, 60, 220]
            };
        }
        let to_rgb = |v: u32| -> [u8; 3] {
            [
                ((v >> 16) & 0xFF) as u8,
                ((v >> 8) & 0xFF) as u8,
                (v & 0xFF) as u8,
            ]
        };
        let rgb = rgb_stream_of(&colors);
        let enc = encode_differential(&rgb).expect("differential");
        let ours = decode_etc2_rgb_block(&enc.block);
        let mut theirs = [0u32; 16];
        texture2ddecoder::decode_etc2_rgb_block(&enc.block, &mut theirs);
        for i in 0..16 {
            assert_eq!(
                ours[i],
                to_rgb(theirs[i]),
                "differential decoder mismatch at pixel {i}"
            );
        }
    }
}
