//! Subsystem 4: Procesamiento de Píxeles & VRAM.
//!
//! - Extrude (border-pixel replication, anti-texture-bleeding)
//! - 90° CW rotation while blitting
//! - Color quantization (RGBA4444 / RGB565) with error diffusion
//!   (Floyd–Steinberg / Atkinson)

use crate::config::{ColorDepth, DitheringAlgorithm};
use crate::types::Rect;

/// Blit a trimmed sprite into an atlas page at its frame, applying extrusion
/// and rotation.
///
/// `page` is the RGBA8 canvas. The visible region of `frame` (inset by
/// `padding`) receives the sprite. Extrusion extends the border pixels by
/// `extrude` pixels into the padding (clamped so it never leaves the frame).
pub fn blit_sprite(
    page: &mut [u8],
    page_w: i32,
    page_h: i32,
    frame: Rect,
    padding: i32,
    extrude: i32,
    trimmed: &[u8],
    trim_w: i32,
    trim_h: i32,
    rotated: bool,
) {
    let extrude = extrude.clamp(0, padding.max(0));
    let e = extrude;

    // Build the extruded buffer: (tw + 2e) x (th + 2e), border-clamped.
    let ew = trim_w + 2 * e;
    let eh = trim_h + 2 * e;
    let mut ext = vec![0u8; (ew * eh * 4) as usize];
    for ey in 0..eh {
        let sy = (ey - e).clamp(0, trim_h - 1);
        for ex in 0..ew {
            let sx = (ex - e).clamp(0, trim_w - 1);
            let src = ((sy * trim_w + sx) * 4) as usize;
            let dst = ((ey * ew + ex) * 4) as usize;
            ext[dst..dst + 4].copy_from_slice(&trimmed[src..src + 4]);
        }
    }

    // The extruded region sits in the frame at (vx - e, vy - e).
    let vx = frame.x + padding;
    let vy = frame.y + padding;
    let ox = vx - e;
    let oy = vy - e;

    for ey in 0..eh {
        for ex in 0..ew {
            let src = ((ey * ew + ex) * 4) as usize;
            let (ax, ay) = if rotated {
                // Rotate 90° CW: (ex, ey) -> (eh - 1 - ey, ex)
                (ox + (eh - 1 - ey), oy + ex)
            } else {
                (ox + ex, oy + ey)
            };
            if ax < 0 || ay < 0 || ax >= page_w || ay >= page_h {
                continue;
            }
            let dst = ((ay * page_w + ax) * 4) as usize;
            page[dst..dst + 4].copy_from_slice(&ext[src..src + 4]);
        }
    }
}

/// Quantize the whole page according to `depth`, optionally dithering.
/// Operates in place on RGBA8 data.
pub fn apply_quantization(
    page: &mut [u8],
    width: usize,
    height: usize,
    depth: ColorDepth,
    dither: DitheringAlgorithm,
) {
    match depth {
        ColorDepth::Rgba8888 => {}
        ColorDepth::Rgba4444 => quantize_page(page, width, height, 4, true, dither),
        ColorDepth::Rgb565 => quantize_rgb565_page(page, width, height, dither),
    }
}

/// Exact quantization with known dimensions.
pub fn quantize_page(
    page: &mut [u8],
    width: usize,
    height: usize,
    bits: u32,
    dither_alpha: bool,
    dither: DitheringAlgorithm,
) {
    let max_val = 255.0f32;
    let levels = (1u32 << bits) - 1;
    let step = max_val / levels as f32;

    let quant = |v: f32| -> f32 {
        let q = (v / step).round().clamp(0.0, levels as f32);
        q * step
    };

    let mut f: Vec<f32> = page.iter().map(|&v| v as f32).collect();
    let channels = if dither_alpha { 4 } else { 3 };

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            for c in 0..channels {
                let i = idx + c;
                let old = f[i];
                let q = quant(old);
                let err = old - q;
                f[i] = q;
                distribute_error(&mut f, width, height, x, y, c, err, dither);
            }
        }
    }

    for (i, v) in f.iter().enumerate() {
        page[i] = v.round().clamp(0.0, 255.0) as u8;
    }
}

fn distribute_error(
    f: &mut [f32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    c: usize,
    err: f32,
    dither: DitheringAlgorithm,
) {
    let add = |f: &mut [f32], x: usize, y: usize, w: f32| {
        if x < width && y < height {
            let i = (y * width + x) * 4 + c;
            f[i] += err * w;
        }
    };
    match dither {
        DitheringAlgorithm::None => {}
        DitheringAlgorithm::FloydSteinberg => {
            add(f, x + 1, y, 7.0 / 16.0);
            if x > 0 {
                add(f, x - 1, y + 1, 3.0 / 16.0);
            }
            add(f, x, y + 1, 5.0 / 16.0);
            add(f, x + 1, y + 1, 1.0 / 16.0);
        }
        DitheringAlgorithm::Atkinson => {
            add(f, x + 1, y, 1.0 / 8.0);
            add(f, x + 2, y, 1.0 / 8.0);
            if x > 0 {
                add(f, x - 1, y + 1, 1.0 / 8.0);
            }
            add(f, x, y + 1, 1.0 / 8.0);
            add(f, x + 1, y + 1, 1.0 / 8.0);
            add(f, x, y + 2, 1.0 / 8.0);
        }
    }
}

#[cfg(test)]
mod simple_rgb565 {
    use super::*;

    /// Simple RGB565 rounding without error diffusion (no dimensions needed).
    pub fn quantize_rgb565(page: &mut [u8], _dither: DitheringAlgorithm) {
        for px in page.chunks_exact_mut(4) {
            px[0] = quant_channel(px[0], 5);
            px[1] = quant_channel(px[1], 6);
            px[2] = quant_channel(px[2], 5);
            px[3] = if px[3] >= 128 { 255 } else { 0 };
        }
    }

    fn quant_channel(v: u8, bits: u32) -> u8 {
        let levels = (1u32 << bits) - 1;
        let step = 255.0 / levels as f32;
        let q = (v as f32 / step).round().clamp(0.0, levels as f32);
        (q * step).round() as u8
    }
}

/// Exact RGB565 quantization with dimensions and dithering.
pub fn quantize_rgb565_page(
    page: &mut [u8],
    width: usize,
    height: usize,
    dither: DitheringAlgorithm,
) {
    let r_step = 255.0 / 31.0;
    let g_step = 255.0 / 63.0;
    let b_step = 255.0 / 31.0;
    let quant = |v: f32, step: f32, levels: f32| -> f32 {
        let q = (v / step).round().clamp(0.0, levels);
        q * step
    };

    let mut f: Vec<f32> = page.iter().map(|&v| v as f32).collect();

    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) * 4;
            for (c, (step, levels)) in [(0usize, (r_step, 31.0)), (1, (g_step, 63.0)), (2, (b_step, 31.0))] {
                let old = f[i + c];
                let q = quant(old, step, levels);
                let err = old - q;
                f[i + c] = q;
                distribute_error(&mut f, width, height, x, y, c, err, dither);
            }
        }
    }

    for (i, v) in f.iter().enumerate() {
        let c = i % 4;
        if c == 3 {
            page[i] = if *v >= 128.0 { 255 } else { 0 };
        } else {
            page[i] = v.round().clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: i32, h: i32, color: [u8; 4]) -> Vec<u8> {
        let mut b = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            b.extend_from_slice(&color);
        }
        b
    }

    #[test]
    fn extrude_extends_border() {
        let mut page = vec![0u8; 10 * 10 * 4];
        let trimmed = solid(2, 2, [10, 20, 30, 255]);
        // padding 2, extrude 1 -> visible at (2,2), extruded region at (1,1)
        blit_sprite(
            &mut page,
            10,
            10,
            Rect::new(0, 0, 6, 6),
            2,
            1,
            &trimmed,
            2,
            2,
            false,
        );
        // Border-adjacent pixel inside the frame must be the border color.
        let at = |x: i32, y: i32| {
            let i = ((y * 10 + x) * 4) as usize;
            page[i..i + 4].to_vec()
        };
        assert_eq!(at(1, 1), vec![10, 20, 30, 255]); // extruded corner
        assert_eq!(at(3, 1), vec![10, 20, 30, 255]); // extruded top
        assert_eq!(at(3, 3), vec![10, 20, 30, 255]); // actual sprite pixel
        assert_eq!(at(0, 0), vec![0, 0, 0, 0]); // outside frame stays empty
    }

    #[test]
    fn rotation_maps_corners() {
        let mut page = vec![0u8; 8 * 8 * 4];
        // 2x1 sprite: [red, green]
        let mut trimmed = Vec::new();
        trimmed.extend_from_slice(&[255, 0, 0, 255]);
        trimmed.extend_from_slice(&[0, 255, 0, 255]);
        blit_sprite(
            &mut page,
            8,
            8,
            Rect::new(1, 1, 4, 5),
            1,
            0,
            &trimmed,
            2,
            1,
            true,
        );
        let at = |x: i32, y: i32| {
            let i = ((y * 8 + x) * 4) as usize;
            page[i..i + 4].to_vec()
        };
        // Rotated 90° CW: red (0,0) -> atlas (vx + (H-1-0), vy + 0) = (2+0, 2)
        // frame (1,1,4,5): visible at (2,2), size 1x2
        assert_eq!(at(2, 2), vec![255, 0, 0, 255]); // red at top-left of rotated
        assert_eq!(at(2, 3), vec![0, 255, 0, 255]); // green below red
    }

    #[test]
    fn rgba4444_quantizes() {
        let mut buf = vec![200u8, 100, 50, 10];
        apply_quantization(&mut buf, 1, 1, ColorDepth::Rgba4444, DitheringAlgorithm::None);
        // 200 -> 12*17=204, 100 -> 6*17=102, 50 -> 3*17=51, 10 -> 1*17=17
        assert_eq!(buf[0], 204);
        assert_eq!(buf[1], 102);
        assert_eq!(buf[2], 51);
        assert_eq!(buf[3], 17);
    }

    #[test]
    fn rgb565_quantizes_alpha() {
        let mut buf = vec![255u8, 255, 255, 200];
        simple_rgb565::quantize_rgb565(&mut buf, DitheringAlgorithm::None);
        assert_eq!(buf[0], 255); // 5-bit max
        assert_eq!(buf[1], 255); // 6-bit max
        assert_eq!(buf[2], 255);
        assert_eq!(buf[3], 255); // alpha >= 128

        let mut buf2 = vec![255u8, 255, 255, 100];
        simple_rgb565::quantize_rgb565(&mut buf2, DitheringAlgorithm::None);
        assert_eq!(buf2[3], 0); // alpha < 128
    }

    #[test]
    fn dithering_changes_values_but_stays_in_range() {
        let mut buf = vec![100u8; 8 * 8 * 4];
        // give it a gradient-ish variance
        for i in 0..buf.len() {
            buf[i] = (i * 7 % 256) as u8;
        }
        apply_quantization(
            &mut buf,
            8,
            8,
            ColorDepth::Rgba4444,
            DitheringAlgorithm::FloydSteinberg,
        );
        assert!(buf.iter().all(|&v| v % 17 == 0 || v == 0));
    }
}
