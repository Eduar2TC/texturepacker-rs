use super::EncodeOptions;
use crate::config::GpuFormat;

pub(super) fn opts(format: GpuFormat) -> EncodeOptions {
    EncodeOptions {
        format,
        ..EncodeOptions::default()
    }
}

/// Byte del campo `color type` del IHDR de un PNG (offset 25).
pub(super) fn png_color_type(bytes: &[u8]) -> u8 {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "no es un PNG");
    assert_eq!(&bytes[12..16], b"IHDR");
    bytes[25]
}

/// Imagen de prueba con 4 colores distintos (uno transparente).
pub(super) fn four_color_image() -> Vec<u8> {
    let colors = [
        [255, 0, 0, 255],
        [0, 255, 0, 128],
        [0, 0, 255, 255],
        [10, 20, 30, 0],
    ];
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..8 {
            rgba.extend_from_slice(&colors[(y / 4) * 2 + x / 4]);
        }
    }
    rgba
}

/// Imagen de prueba determinista (bordes no transparentes alfa 128/255).
pub(super) fn gradient(w: usize, h: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            v.extend_from_slice(&[
                (x * 7 % 256) as u8,
                (y * 11 % 256) as u8,
                ((x + y) * 5 % 256) as u8,
                if (x + y) % 3 == 0 { 255 } else { 128 },
            ]);
        }
    }
    v
}

pub(super) fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

pub(super) fn be16(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap())
}

/// Diferencia media por canal entre RGBA de origen y un buffer `u32` del
/// decodificador (empaquetado `[b, g, r, a]` en little-endian).
pub(super) fn mean_channel_error(rgba: &[u8], decoded: &[u32]) -> f64 {
    let mut total = 0u64;
    for (px, out) in rgba.chunks_exact(4).zip(decoded) {
        let bytes = out.to_le_bytes();
        let (b, g, r) = (
            u64::from(bytes[0]),
            u64::from(bytes[1]),
            u64::from(bytes[2]),
        );
        total += (u64::from(px[0])).abs_diff(r);
        total += (u64::from(px[1])).abs_diff(g);
        total += (u64::from(px[2])).abs_diff(b);
    }
    total as f64 / (rgba.len() / 4 * 3) as f64
}
