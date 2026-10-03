use crate::config::PixelFormat;

/// Convierte RGBA8 al formato de píxel de salida (pixel format).
/// `Rgb888` compone la transparencia sobre negro; los formatos de hardware
/// no pasan por aquí.
pub(super) fn apply_pixel_format(
    rgba: &[u8],
    format: PixelFormat,
) -> (Vec<u8>, image::ExtendedColorType) {
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
        // RGBA4444 (16 bits): cada canal a 4 bits con replicación al
        // expandir (el archivo sigue siendo RGBA8 con la rejilla 4 bits).
        PixelFormat::Rgba4444 => {
            let q4 = |c: u8| ((c as u16 * 15 + 127) / 255) as u8;
            let expand = |v: u8| (v << 4) | v;
            let mut out = Vec::with_capacity(rgba.len());
            for px in rgba.chunks_exact(4) {
                out.extend_from_slice(&[
                    expand(q4(px[0])),
                    expand(q4(px[1])),
                    expand(q4(px[2])),
                    expand(q4(px[3])),
                ]);
            }
            (out, Ct::Rgba8)
        }
        // RGB565 (16 bits): sin alfa (compuesto sobre negro) y con la
        // rejilla 5-6-5 expandida por replicación.
        PixelFormat::Rgb565 => {
            let q5 = |c: u8| ((c as u16 * 31 + 127) / 255) as u8;
            let q6 = |c: u8| ((c as u16 * 63 + 127) / 255) as u8;
            let e5 = |v: u8| (v << 3) | (v >> 2);
            let e6 = |v: u8| (v << 2) | (v >> 4);
            let mut out = Vec::with_capacity(rgba.len() / 4 * 3);
            for px in rgba.chunks_exact(4) {
                let c = composite_black(px);
                out.extend_from_slice(&[e5(q5(c[0])), e6(q6(c[1])), e5(q5(c[2]))]);
            }
            (out, Ct::Rgb8)
        }
        // Un pixel format de hardware no puede embeberse en un formato de
        // software: `ProjectConfig::validate` lo rechaza, y aquí se cae de
        // forma segura a RGBA8888 por si se codifica en caliente.
        other => {
            let _ = other;
            (rgba.to_vec(), Ct::Rgba8)
        }
    }
}

/// Compone un píxel RGBA sobre fondo negro (para formatos sin alfa).
pub(super) fn composite_black(px: &[u8]) -> [u8; 3] {
    let alpha = px[3] as u16;
    let mix = |c: u8| ((c as u16 * alpha + 127) / 255) as u8;
    [mix(px[0]), mix(px[1]), mix(px[2])]
}

/// Luminancia BT.601 en enteros: 0.299 R + 0.587 G + 0.114 B.
fn luma(px: &[u8]) -> u8 {
    ((77u16 * px[0] as u16 + 150 * px[1] as u16 + 29 * px[2] as u16 + 128) >> 8) as u8
}

/// Muestra por píxel de cada formato de píxel de software.
pub(super) fn color_channels(color: image::ExtendedColorType) -> usize {
    use image::ExtendedColorType as Ct;
    match color {
        Ct::Rgba8 => 4,
        Ct::Rgb8 => 3,
        Ct::La8 => 2,
        _ => 1,
    }
}

/// Expande cualquier formato de píxel de software a RGBA8.
pub(super) fn expand_to_rgba(data: &[u8], color: image::ExtendedColorType) -> Vec<u8> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GpuFormat;
    use crate::export::encode_to_bytes;
    use crate::export::png::encode_png;
    use crate::export::test_helpers::{opts, png_color_type};
    use crate::export::EncodeOptions;

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
}
