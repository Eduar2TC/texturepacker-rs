//! Subsystem 4: Procesamiento de Píxeles & VRAM.
//!
//! - Extrude (border-pixel replication, anti-texture-bleeding)
//! - 90° CW rotation while blitting
//! - Color quantization (RGBA4444 / RGB565) with error diffusion
//!   (Floyd–Steinberg / Atkinson)

use crate::config::{AlphaHandling, ColorDepth, DitheringAlgorithm};
use crate::types::Rect;

/// Layout of one sprite inside the atlas page: visible frame (inset by
/// `padding`), extrusion and 90° CW rotation.
#[derive(Debug, Clone, Copy)]
pub struct BlitLayout {
    pub frame: Rect,
    pub padding: i32,
    pub extrude: i32,
    pub rotated: bool,
}

/// The trimmed RGBA8 pixels of a sprite and their dimensions.
#[derive(Debug, Clone, Copy)]
pub struct TrimmedSprite<'a> {
    pub pixels: &'a [u8],
    pub width: i32,
    pub height: i32,
}

/// Blit a trimmed sprite into an atlas page at its frame, applying extrusion
/// and rotation.
///
/// `page` is the RGBA8 canvas. The visible region of `layout.frame` (inset by
/// `layout.padding`) receives the sprite. Extrusion extends the border pixels
/// by `layout.extrude` pixels into the padding (clamped so it never leaves
/// the frame).
pub fn blit_sprite(
    page: &mut [u8],
    page_w: i32,
    page_h: i32,
    layout: BlitLayout,
    sprite: TrimmedSprite<'_>,
) {
    let Rect {
        x: frame_x,
        y: frame_y,
        ..
    } = layout.frame;
    let rotated = layout.rotated;
    let extrude = layout.extrude.clamp(0, layout.padding.max(0));
    let e = extrude;
    let (trimmed, trim_w, trim_h) = (sprite.pixels, sprite.width, sprite.height);

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
    let vx = frame_x + layout.padding;
    let vy = frame_y + layout.padding;
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
        ColorDepth::Rgba4444 => {
            quantize_page(page, width, height, 4, dither.dithers_alpha(), dither)
        }
        ColorDepth::Rgb565 => quantize_rgb565_page(page, width, height, dither),
    }
}

/// Apply the *Transparency Handling* pre-pass to an RGBA8 page.
///
/// - `KeepTransparentPixels`: no-op (transparent pixels keep their colors).
/// - `ClearTransparentPixels`: transparent pixels become transparent black,
///   improving packing ratio and identical sprite detection.
/// - `ReduceBorderArtifacts`: transparent pixels get the color of the nearest
///   solid pixel (alpha bleeding), removing dark halos around sprites.
/// - `PremultiplyAlpha`: `rgb = rgb * a / 255`.
pub fn apply_alpha_handling(page: &mut [u8], width: usize, height: usize, mode: AlphaHandling) {
    match mode {
        AlphaHandling::KeepTransparentPixels => {}
        AlphaHandling::ClearTransparentPixels => {
            for px in page.chunks_exact_mut(4) {
                if px[3] == 0 {
                    px[0] = 0;
                    px[1] = 0;
                    px[2] = 0;
                }
            }
        }
        AlphaHandling::PremultiplyAlpha => {
            for px in page.chunks_exact_mut(4) {
                let a = u32::from(px[3]);
                for channel in &mut px[..3] {
                    *channel = ((u32::from(*channel) * a + 127) / 255) as u8;
                }
            }
        }
        AlphaHandling::ReduceBorderArtifacts => bleed_alpha(page, width, height),
    }
}

/// Alpha bleeding: transparent pixels receive the color of the nearest solid
/// pixel. A few 4-neighbour passes spread the color outwards; alpha stays 0.
fn bleed_alpha(page: &mut [u8], width: usize, height: usize) {
    if width == 0 || height == 0 {
        return;
    }
    // `has_color[i]` = the pixel already carries a usable color (solid, or
    // bled into in a previous pass).
    let mut has_color: Vec<bool> = page.chunks_exact(4).map(|px| px[3] > 0).collect();
    let passes = width.max(height).min(16);
    const NEIGHBOURS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

    for _ in 0..passes {
        let mut changed = false;
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                if has_color[i] {
                    continue;
                }
                let mut source: Option<[u8; 3]> = None;
                for (dx, dy) in NEIGHBOURS {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        continue;
                    }
                    let ni = ny as usize * width + nx as usize;
                    if has_color[ni] {
                        source = Some([page[ni * 4], page[ni * 4 + 1], page[ni * 4 + 2]]);
                        break;
                    }
                }
                if let Some(rgb) = source {
                    page[i * 4..i * 4 + 3].copy_from_slice(&rgb);
                    has_color[i] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
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
    let dither_ctx = |x: usize, y: usize, channel: usize| DitherContext {
        width,
        height,
        x,
        y,
        channel,
        dither,
    };

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            for c in 0..4 {
                let i = idx + c;
                let old = f[i];
                let q = quant(old);
                let err = old - q;
                f[i] = q;
                // The alpha channel is always quantized; only the `*Alpha`
                // algorithms diffuse error into it.
                if c < 3 || dither_alpha {
                    distribute_error(&mut f, dither_ctx(x, y, c), err);
                }
            }
        }
    }

    for (i, v) in f.iter().enumerate() {
        page[i] = v.round().clamp(0.0, 255.0) as u8;
    }
}

/// Where and how to spread quantization error: the pixel being quantized,
/// the page dimensions, the channel and the diffusion algorithm.
struct DitherContext {
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    channel: usize,
    dither: DitheringAlgorithm,
}

fn distribute_error(f: &mut [f32], ctx: DitherContext, err: f32) {
    let DitherContext {
        width,
        height,
        x,
        y,
        channel: c,
        dither,
    } = ctx;
    let add = |f: &mut [f32], x: usize, y: usize, w: f32| {
        if x < width && y < height {
            let i = (y * width + x) * 4 + c;
            f[i] += err * w;
        }
    };
    match dither {
        DitheringAlgorithm::None | DitheringAlgorithm::NearestNeighbour => {}
        // Diffuses the whole error to the right neighbour of the same row, so
        // every level shows up in proportion to its share of the input: a
        // linear color distribution instead of plain rounding.
        DitheringAlgorithm::Linear => add(f, x + 1, y, 1.0),
        DitheringAlgorithm::FloydSteinberg | DitheringAlgorithm::FloydSteinbergAlpha => {
            add(f, x + 1, y, 7.0 / 16.0);
            if x > 0 {
                add(f, x - 1, y + 1, 3.0 / 16.0);
            }
            add(f, x, y + 1, 5.0 / 16.0);
            add(f, x + 1, y + 1, 1.0 / 16.0);
        }
        DitheringAlgorithm::Atkinson | DitheringAlgorithm::AtkinsonAlpha => {
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
    let dither_ctx = |x: usize, y: usize, channel: usize| DitherContext {
        width,
        height,
        x,
        y,
        channel,
        dither,
    };

    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) * 4;
            for (c, (step, levels)) in [
                (0usize, (r_step, 31.0)),
                (1, (g_step, 63.0)),
                (2, (b_step, 31.0)),
            ] {
                let old = f[i + c];
                let q = quant(old, step, levels);
                let err = old - q;
                f[i + c] = q;
                distribute_error(&mut f, dither_ctx(x, y, c), err);
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
            BlitLayout {
                frame: Rect::new(0, 0, 6, 6),
                padding: 2,
                extrude: 1,
                rotated: false,
            },
            TrimmedSprite {
                pixels: &trimmed,
                width: 2,
                height: 2,
            },
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
            BlitLayout {
                frame: Rect::new(1, 1, 4, 5),
                padding: 1,
                extrude: 0,
                rotated: true,
            },
            TrimmedSprite {
                pixels: &trimmed,
                width: 2,
                height: 1,
            },
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
        apply_quantization(
            &mut buf,
            1,
            1,
            ColorDepth::Rgba4444,
            DitheringAlgorithm::None,
        );
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
        for (i, v) in buf.iter_mut().enumerate() {
            *v = (i * 7 % 256) as u8;
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

    #[test]
    fn nearest_neighbour_dither_rounds_without_diffusion() {
        // 8 píxeles idénticos (105) en una fila: 4 bits -> paso 17, nivel 102.
        let make = || [105u8, 105, 105, 255].repeat(8);

        let mut none = make();
        apply_quantization(
            &mut none,
            8,
            1,
            ColorDepth::Rgba4444,
            DitheringAlgorithm::None,
        );
        let mut nn = make();
        apply_quantization(
            &mut nn,
            8,
            1,
            ColorDepth::Rgba4444,
            DitheringAlgorithm::NearestNeighbour,
        );
        assert_eq!(none, nn, "NearestNeighbour no difunde error");
        assert!(nn.chunks_exact(4).all(|px| px[0] == 102 && px[3] == 255));

        // Linear reparte el error hacia la derecha y saca el nivel siguiente.
        let mut lin = make();
        apply_quantization(
            &mut lin,
            8,
            1,
            ColorDepth::Rgba4444,
            DitheringAlgorithm::Linear,
        );
        let reds: Vec<u8> = lin.chunks_exact(4).map(|px| px[0]).collect();
        assert!(
            reds.iter().all(|v| v % 17 == 0),
            "fuera de la rejilla: {reds:?}"
        );
        assert!(
            reds.contains(&119),
            "Linear debe alternar niveles, no cllearse en 102: {reds:?}"
        );
        assert_ne!(reds, vec![102u8; 8]);
    }

    fn alpha_sample() -> Vec<u8> {
        // 4x1: sólido semitransparente + tres transparentes con color residual.
        vec![
            200, 100, 50, 128, // sólido
            123, 45, 67, 0, // transparente con color sobrante
            0, 0, 0, 0, // transparente negro
            10, 20, 30, 0, // transparente con color
        ]
    }

    #[test]
    fn alpha_handling_keep_clear_and_premultiply() {
        let mut keep = alpha_sample();
        apply_alpha_handling(&mut keep, 4, 1, AlphaHandling::KeepTransparentPixels);
        assert_eq!(keep, alpha_sample());

        let mut clear = alpha_sample();
        apply_alpha_handling(&mut clear, 4, 1, AlphaHandling::ClearTransparentPixels);
        assert_eq!(&clear[0..4], &[200, 100, 50, 128], "el sólido no cambia");
        assert_eq!(&clear[4..8], &[0, 0, 0, 0], "color residual limpiado");
        assert_eq!(&clear[8..12], &[0, 0, 0, 0]);
        assert_eq!(&clear[12..16], &[0, 0, 0, 0]);

        let mut pre = alpha_sample();
        apply_alpha_handling(&mut pre, 4, 1, AlphaHandling::PremultiplyAlpha);
        // 200*128/255 = 100.39 -> 100 (redondeo con +127)
        assert_eq!(&pre[0..4], &[100, 50, 25, 128]);
        assert_eq!(&pre[4..7], &[0, 0, 0], "a=0 anula el color");
        assert_eq!(pre[3], 128, "el alfa no cambia");
    }

    #[test]
    fn alpha_bleeding_fills_neighbours_without_touching_alpha() {
        let mut page = alpha_sample();
        apply_alpha_handling(&mut page, 4, 1, AlphaHandling::ReduceBorderArtifacts);
        // Los tres transparentes toman el color del sólido vecino…
        assert_eq!(&page[4..8], &[200, 100, 50, 0]);
        assert_eq!(&page[8..12], &[200, 100, 50, 0]);
        assert_eq!(&page[12..16], &[200, 100, 50, 0]);
        // …pero el alfa sigue siendo 0 y el sólido intacto.
        assert_eq!(&page[0..4], &[200, 100, 50, 128]);
    }

    #[test]
    fn only_alpha_variants_diffuse_error_into_alpha() {
        let make = || {
            let mut buf = Vec::with_capacity(8 * 8 * 4);
            for y in 0..8 {
                for x in 0..8 {
                    buf.extend_from_slice(&[128, 64, 200, (x * 20 + y * 3) as u8]);
                }
            }
            buf
        };
        let quant17 = |v: u8| (((v as f32) / 17.0).round().clamp(0.0, 15.0) * 17.0) as u8;

        let mut plain = make();
        apply_quantization(
            &mut plain,
            8,
            8,
            ColorDepth::Rgba4444,
            DitheringAlgorithm::FloydSteinberg,
        );
        for (i, px) in plain.chunks_exact(4).enumerate() {
            let x = i % 8;
            let y = i / 8;
            assert_eq!(
                px[3],
                quant17((x * 20 + y * 3) as u8),
                "FS sin alpha debe cuantizar sin difundir"
            );
        }

        let mut with_alpha = make();
        apply_quantization(
            &mut with_alpha,
            8,
            8,
            ColorDepth::Rgba4444,
            DitheringAlgorithm::FloydSteinbergAlpha,
        );
        assert!(
            with_alpha.chunks_exact(4).enumerate().any(|(i, px)| {
                let x = i % 8;
                let y = i / 8;
                px[3] != quant17((x * 20 + y * 3) as u8)
            }),
            "FloydSteinbergAlpha debe difundir error en el canal alfa"
        );
    }
}
