use super::options::EncodeOptions;
use super::pixel::{color_channels, composite_black};
use crate::error::{Result, TpError};

/// JPG lossy (`--jpg-quality`, 0-100). Sin canal
/// alfa: compone sobre negro.
pub(super) fn encode_jpg(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    use image::ExtendedColorType as Ct;
    if data.len() != width * height * color_channels(color) {
        return Err(TpError::Other(
            "Dimensiones de imagen inválidas para JPG".to_string(),
        ));
    }
    let mut buf = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, opts.jpg_quality);
    let result: std::result::Result<(), image::ImageError> = match color {
        Ct::L8 => enc.encode(data, width as u32, height as u32, Ct::L8),
        Ct::Rgb8 => enc.encode(data, width as u32, height as u32, Ct::Rgb8),
        Ct::La8 => {
            let gray: Vec<u8> = data.chunks_exact(2).map(|p| p[0]).collect();
            enc.encode(&gray, width as u32, height as u32, Ct::L8)
        }
        Ct::Rgba8 => {
            let mut rgb = Vec::with_capacity(data.len() / 4 * 3);
            for px in data.chunks_exact(4) {
                rgb.extend_from_slice(&composite_black(px));
            }
            enc.encode(&rgb, width as u32, height as u32, Ct::Rgb8)
        }
        _ => {
            return Err(TpError::Other(
                "Formato de píxel no compatible con JPG".to_string(),
            ))
        }
    };
    result.map_err(|e| TpError::Other(format!("Error codificando JPG: {e}")))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use crate::config::GpuFormat;
    use crate::export::encode_to_bytes;
    use crate::export::test_helpers::opts;

    #[test]
    fn jpg_solid_color_and_transparency_composited() {
        // Rojo sólido opaco.
        let red = [255u8, 0, 0, 255].repeat(16 * 16);
        let bytes = encode_to_bytes(&red, 16, 16, &opts(GpuFormat::Jpg)).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        for p in decoded.pixels() {
            let [r, g, b] = p.0;
            assert!(r.abs_diff(255) <= 10 && g <= 8 && b <= 8, "{p:?}");
        }

        // Totalmente transparente: se compone sobre negro.
        let clear = [200u8, 100, 50, 0].repeat(16 * 16);
        let bytes = encode_to_bytes(&clear, 16, 16, &opts(GpuFormat::Jpg)).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        for p in decoded.pixels() {
            let [r, g, b] = p.0;
            assert!(r <= 8 && g <= 8 && b <= 8, "{p:?}");
        }
    }
}
