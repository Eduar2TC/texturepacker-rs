use super::options::EncodeOptions;
use super::pixel::{color_channels, expand_to_rgba};
use crate::error::{Result, TpError};

/// WebP: sin pérdidas por defecto (calidad ≥ 101) o lossy 0-100
/// (`--webp-quality`).
pub(super) fn encode_webp(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    use image::ExtendedColorType as Ct;
    if data.len() != width * height * color_channels(color) {
        return Err(TpError::Other(
            "Dimensiones de imagen inválidas para WebP".to_string(),
        ));
    }
    if opts.webp_quality >= 101 {
        return encode_webp_lossless(&expand_to_rgba(data, color), width, height);
    }
    // WebP lossy solo admite RGB/RGBA (libwebp).
    let rgba = match color {
        Ct::Rgba8 => data.to_vec(),
        _ => expand_to_rgba(data, color),
    };
    let encoder = webp::Encoder::from_rgba(&rgba, width as u32, height as u32);
    let memory = encoder.encode(opts.webp_quality as f32);
    Ok(memory.to_vec())
}

fn encode_webp_lossless(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut buf)
        .encode(
            rgba,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| TpError::Other(format!("Error codificando WebP: {e}")))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GpuFormat;
    use crate::export::encode_to_bytes;
    use crate::export::test_helpers::opts;

    #[test]
    fn webp_lossless_default_and_lossy_smaller() {
        // Ruido pseudoaleatorio (xorshift32): casi incompresible sin
        // pérdidas, así el lossy queda claramente por debajo.
        let mut state = 0x2545_F491u32;
        let mut noise = Vec::with_capacity(64 * 64 * 4);
        for _ in 0..64 * 64 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let r = state as u8;
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let g = state as u8;
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let b = state as u8;
            noise.extend_from_slice(&[r, g, b, 255]);
        }
        // Por defecto (≥101): sin pérdidas, decodificación exacta.
        let lossless = encode_to_bytes(&noise, 64, 64, &opts(GpuFormat::WebP)).unwrap();
        let decoded = image::load_from_memory(&lossless).unwrap().to_rgba8();
        assert_eq!(decoded.as_raw(), &noise);

        // Lossy q=10: decodificable y más pequeño que el lossless.
        let lossy_opts = EncodeOptions {
            webp_quality: 10,
            ..opts(GpuFormat::WebP)
        };
        let lossy = encode_to_bytes(&noise, 64, 64, &lossy_opts).unwrap();
        assert!(
            lossy.len() < lossless.len(),
            "{} >= {}",
            lossy.len(),
            lossless.len()
        );
        let decoded = image::load_from_memory(&lossy).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (64, 64));
    }
}
