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
            a: Colour { r: 0, g: 0, b: 0, a: 15 },
            b: Colour { r: 0, g: 0, b: 0, a: 15 },
            a_opaque: true,
            b_opaque: true,
        }
    }
}

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

/// Fit two canonical colour endpoints for a 4×4 block of RGBA texels.
///
/// - RGB: mean of the visible texels (alpha ≥ 8) plus the two extremes along
///   the axis from the mean to the farthest texel, so the A/B segment brackets
///   the block's colour distribution and the modulation weights sample it.
/// - Alpha: endpoint alpha = max/min of the block (opaque mode when ≥ 250).
fn fit_block(px: &[u8]) -> BlockColours {
    debug_assert_eq!(px.len(), 64);
    let mut min_a = 255u8;
    let mut max_a = 0u8;
    let mut sum_r = 0f64;
    let mut sum_g = 0f64;
    let mut sum_b = 0f64;
    let mut vis = 0f64;
    for c in px.chunks_exact(4) {
        let a = c[3];
        min_a = min_a.min(a);
        max_a = max_a.max(a);
        if a >= 8 {
            sum_r += c[0] as f64;
            sum_g += c[1] as f64;
            sum_b += c[2] as f64;
            vis += 1.0;
        }
    }
    let (mr, mg, mb) = if vis > 0.0 {
        (sum_r / vis, sum_g / vis, sum_b / vis)
    } else {
        (0.0, 0.0, 0.0)
    };

    // Direction: farthest visible texel from the mean.
    let mut dir = (0f64, 0f64, 0f64);
    let mut far2 = -1f64;
    for c in px.chunks_exact(4) {
        if c[3] < 8 {
            continue;
        }
        let (dx, dy, dz) = (c[0] as f64 - mr, c[1] as f64 - mg, c[2] as f64 - mb);
        let d2 = dx * dx + dy * dy + dz * dz;
        if d2 > far2 {
            far2 = d2;
            dir = (dx, dy, dz);
        }
    }

    let (ar, ag, ab, br, bg, bb) = if far2 <= 0.25 {
        (mr, mg, mb, mr, mg, mb)
    } else {
        let len = far2.sqrt();
        let (ux, uy, uz) = (dir.0 / len, dir.1 / len, dir.2 / len);
        let mut minp = f64::INFINITY;
        let mut maxp = f64::NEG_INFINITY;
        for c in px.chunks_exact(4) {
            if c[3] < 8 {
                continue;
            }
            let p = (c[0] as f64 - mr) * ux + (c[1] as f64 - mg) * uy + (c[2] as f64 - mb) * uz;
            minp = minp.min(p);
            maxp = maxp.max(p);
        }
        let clamp = |v: f64| v.clamp(0.0, 255.0);
        (
            clamp(mr + ux * maxp),
            clamp(mg + uy * maxp),
            clamp(mb + uz * maxp),
            clamp(mr + ux * minp),
            clamp(mg + uy * minp),
            clamp(mb + uz * minp),
        )
    };

    let a_opaque = max_a >= 250;
    let b_opaque = min_a >= 250;

    let (ar5, ag5, ab5) = (q5(ar), q5(ag), q5(ab));
    let (br5, bg5, bb5) = (q5(br), q5(bg), q5(bb));

    let a = if a_opaque {
        Colour { r: ar5, g: ag5, b: q4rep(ab5), a: 15 }
    } else {
        Colour { r: q4rep(ar5), g: q4rep(ag5), b: q3rep(ab5), a: qa(max_a) }
    };
    let b = if b_opaque {
        Colour { r: br5, g: bg5, b: q4rep(bb5), a: 15 }
    } else {
        Colour { r: q4rep(br5), g: q4rep(bg5), b: q4rep(bb5), a: qa(min_a) }
    };

    BlockColours { a, b, a_opaque, b_opaque }
}

// ---------------------------------------------------------------------------
// Colour interpolation (mirrors the decoder exactly)
// ---------------------------------------------------------------------------

/// Per-texel weights over the 3 block columns/rows {−1, 0, +1} for texel
/// offsets 0..4 — identical to the reference decoder's `INTERP_WEIGHT`.
const INTERP: [[i32; 3]; 4] = [[2, 2, 0], [1, 3, 0], [0, 4, 0], [0, 3, 1]];

/// Decoded 8-bit colour at texel (tx, ty) of block (bx, by), obtained by
/// bilinearly interpolating the 3×3 colour neighbourhood (toroidal wrap)
/// exactly as the PVRTC decoder does, including the UNORM conversions
/// (RGB: `(c >> 1) + (c >> 6)`; alpha: `c + (c >> 4)`).
fn interpolate(
    blocks: &[BlockColours],
    nb_x: usize,
    nb_y: usize,
    bx: usize,
    by: usize,
    tx: usize,
    ty: usize,
    use_b: bool,
) -> [u8; 4] {
    let mut clr = [0i32; 4];
    for dy in 0..3 {
        let yb = (by + nb_y - 1 + dy) % nb_y;
        for dx in 0..3 {
            let xb = (bx + nb_x - 1 + dx) % nb_x;
            let c = if use_b {
                blocks[yb * nb_x + xb].b
            } else {
                blocks[yb * nb_x + xb].a
            };
            let w = INTERP[tx][dx] * INTERP[ty][dy];
            clr[0] += c.r as i32 * w;
            clr[1] += c.g as i32 * w;
            clr[2] += c.b as i32 * w;
            clr[3] += c.a as i32 * w;
        }
    }
    [
        ((clr[0] >> 1) + (clr[0] >> 6)) as u8,
        ((clr[1] >> 1) + (clr[1] >> 6)) as u8,
        ((clr[2] >> 1) + (clr[2] >> 6)) as u8,
        (clr[3] + (clr[3] >> 4)) as u8,
    ]
}

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

/// Encode an RGBA8 image (row-major, 4 bytes per pixel) as raw PVRTC1 4bpp
/// data: `width * height / 2` bytes, in reflected Morton word order.
///
/// Requires `width` and `height` to be powers of two ≥ 8 (PVRTC1 constraint).
pub fn encode_pvrtc_4bpp(
    rgba: &[u8],
    width: usize,
    height: usize,
) -> Result<Vec<u8>, String> {
    if width < 8 || height < 8 || !width.is_power_of_two() || !height.is_power_of_two() {
        return Err(format!(
            "PVRTC_4BPP requiere dimensiones potencia de dos ≥ 8x8 \
             (se obtuvo {width}x{height})"
        ));
    }
    if rgba.len() != width * height * 4 {
        return Err("PVRTC: buffer RGBA de tamaño incorrecto".into());
    }

    let nb_x = width / 4;
    let nb_y = height / 4;
    let min_dim = nb_x.min(nb_y);

    // Pass 1: independent per-block colour fits.
    let mut blocks = vec![BlockColours::default(); nb_x * nb_y];
    let mut px = [0u8; 64];
    for by in 0..nb_y {
        for bx in 0..nb_x {
            let mut i = 0;
            for ty in 0..4 {
                let y = by * 4 + ty;
                for tx in 0..4 {
                    let s = (y * width + bx * 4 + tx) * 4;
                    px[i..i + 4].copy_from_slice(&rgba[s..s + 4]);
                    i += 4;
                }
            }
            blocks[by * nb_x + bx] = fit_block(&px);
        }
    }

    // Pass 2: per-texel modulation search, then assemble the 64-bit words.
    let mut out = vec![0u8; nb_x * nb_y * 8];
    for by in 0..nb_y {
        for bx in 0..nb_x {
            let mut mod_bits: u32 = 0;
            for ty in 0..4 {
                let y = by * 4 + ty;
                for tx in 0..4 {
                    let x = bx * 4 + tx;
                    let a8 = interpolate(&blocks, nb_x, nb_y, bx, by, tx, ty, false);
                    let b8 = interpolate(&blocks, nb_x, nb_y, bx, by, tx, ty, true);
                    let s = (y * width + x) * 4;
                    let src = [rgba[s], rgba[s + 1], rgba[s + 2], rgba[s + 3]];
                    let bits = choose_mod(&src, &a8, &b8) as u32;
                    mod_bits |= bits << (2 * (ty * 4 + tx));
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

    fn solid(w: usize, h: usize, c: [u8; 4]) -> Vec<u8> {
        (0..w * h).flat_map(|_| c).collect()
    }

    #[test]
    fn sizes_and_errors() {
        assert_eq!(encode_pvrtc_4bpp(&solid(8, 8, [0; 4]), 8, 8).unwrap().len(), 32);
        assert_eq!(encode_pvrtc_4bpp(&solid(16, 8, [0; 4]), 16, 8).unwrap().len(), 64);
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
                rgba.extend_from_slice(&[
                    (x * 17) as u8,
                    (y * 17) as u8,
                    ((x + y) * 8) as u8,
                    255,
                ]);
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
            [255u8, 0, 0, 255],    // top-left
            [0, 255, 0, 255],      // top-right
            [0, 0, 255, 255],      // bottom-left
            [255, 255, 0, 255],    // bottom-right
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
}
