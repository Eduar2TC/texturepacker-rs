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
/// they fall back to [`ScaleMode::Smooth`](crate::config::ScaleMode::Smooth).
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
}
