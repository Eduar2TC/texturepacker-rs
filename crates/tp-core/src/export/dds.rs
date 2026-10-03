use super::options::EncodeOptions;
use crate::config::PixelFormat;
use crate::dxt;
use crate::error::{Result, TpError};

/// DDS sin comprimir: cabecera legacy de 124 bytes + píxeles. Las máscaras
/// de canal describen el orden real de los bytes, así que `RGBA8888` viaja
/// como R,G,B,A en memoria y `RGB888` como 24 bits con la misma disposición.
pub(super) fn encode_dds(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
) -> Result<Vec<u8>> {
    use image::ExtendedColorType as Ct;
    // La8 no tiene equivalente directo: se expande a RGBA conservando el alfa.
    let (pixels, bit_count, flags, masks): (Vec<u8>, u32, u32, [u32; 4]) = match color {
        Ct::Rgba8 => (
            data.to_vec(),
            32u32,
            0x41u32,
            [0x0000_00ff, 0x0000_ff00, 0x00ff_0000, 0xff00_0000],
        ),
        Ct::Rgb8 => (
            data.to_vec(),
            24,
            0x40,
            [0x0000_00ff, 0x0000_ff00, 0x00ff_0000, 0],
        ),
        Ct::L8 => (data.to_vec(), 8, 0x2_0000, [0x0000_00ff, 0, 0, 0]),
        Ct::La8 => {
            let mut out = Vec::with_capacity(data.len() / 2 * 4);
            for px in data.chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            (
                out,
                32,
                0x41,
                [0x0000_00ff, 0x0000_ff00, 0x00ff_0000, 0xff00_0000],
            )
        }
        _ => {
            return Err(TpError::Other(format!(
                "DDS no admite el formato de píxel resultante ({color:?})"
            )))
        }
    };
    let stride = (bit_count / 8) as usize;
    if data.len() != width * height * stride {
        return Err(TpError::Other(
            "Tamaño de imagen inconsistente al codificar DDS".to_string(),
        ));
    }

    let mut out = Vec::with_capacity(128 + pixels.len());
    out.extend_from_slice(b"DDS ");
    out.extend_from_slice(&124u32.to_le_bytes()); // dwSize
                                                  // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
    out.extend_from_slice(&0x0000_100Fu32.to_le_bytes());
    out.extend_from_slice(&(height as u32).to_le_bytes());
    out.extend_from_slice(&(width as u32).to_le_bytes());
    out.extend_from_slice(&((width * stride) as u32).to_le_bytes()); // pitch
    out.extend_from_slice(&0u32.to_le_bytes()); // dwDepth
    out.extend_from_slice(&0u32.to_le_bytes()); // dwMipMapCount
    for _ in 0..11 {
        out.extend_from_slice(&0u32.to_le_bytes()); // dwReserved1
    }
    // DDS_PIXELFORMAT (32 bytes)
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // dwFourCC
    out.extend_from_slice(&bit_count.to_le_bytes());
    for m in masks {
        out.extend_from_slice(&m.to_le_bytes());
    }
    out.extend_from_slice(&0x0000_1000u32.to_le_bytes()); // dwCaps: TEXTURE
    out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps2
    out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps3
    out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps4
    out.extend_from_slice(&0u32.to_le_bytes()); // dwReserved2
    out.extend_from_slice(&pixels);
    Ok(out)
}

/// DDS con bloques DXT (fourcc `DXT1`/`DXT5`): cabecera legacy de 128 bytes
/// con `DDSD_LINEARSIZE` y el payload ya comprimido por [`dxt`].
pub(super) fn encode_dds_dxt(
    rgba: &[u8],
    width: usize,
    height: usize,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    let (fourcc, blocks) = match opts.pixel_format {
        PixelFormat::Dxt1 => (
            *b"DXT1",
            dxt::encode_dxt1(rgba, width, height, opts.dxt_mode),
        ),
        PixelFormat::Dxt5 => (
            *b"DXT5",
            dxt::encode_dxt5(rgba, width, height, opts.dxt_mode),
        ),
        other => {
            return Err(TpError::Other(format!(
                "{} no es un formato DXT comprimible en DDS",
                other.as_str()
            )))
        }
    };
    let mut out = Vec::with_capacity(128 + blocks.len());
    out.extend_from_slice(b"DDS ");
    out.extend_from_slice(&124u32.to_le_bytes()); // dwSize
                                                  // CAPS | HEIGHT | WIDTH | PIXELFORMAT | LINEARSIZE
    out.extend_from_slice(&0x0008_1007u32.to_le_bytes());
    out.extend_from_slice(&(height as u32).to_le_bytes());
    out.extend_from_slice(&(width as u32).to_le_bytes());
    out.extend_from_slice(&(blocks.len() as u32).to_le_bytes()); // linear size
    out.extend_from_slice(&0u32.to_le_bytes()); // dwDepth
    out.extend_from_slice(&0u32.to_le_bytes()); // dwMipMapCount
    for _ in 0..11 {
        out.extend_from_slice(&0u32.to_le_bytes()); // dwReserved1
    }
    // DDS_PIXELFORMAT (32 bytes): FOURCC, sin máscaras de canal.
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&0x4u32.to_le_bytes()); // DDPF_FOURCC
    out.extend_from_slice(&fourcc);
    out.extend_from_slice(&0u32.to_le_bytes()); // dwRGBBitCount
    for _ in 0..4 {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    out.extend_from_slice(&0x0000_1000u32.to_le_bytes()); // dwCaps: TEXTURE
    out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps2
    out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps3
    out.extend_from_slice(&0u32.to_le_bytes()); // dwCaps4
    out.extend_from_slice(&0u32.to_le_bytes()); // dwReserved2
    out.extend_from_slice(&blocks);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DxtMode, GpuFormat};
    use crate::export::encode_to_bytes;
    use crate::export::test_helpers::{four_color_image, gradient, le32, mean_channel_error, opts};

    #[test]
    fn dds_writes_a_valid_legacy_header_and_raw_pixels() {
        let (w, h) = (4, 3);
        let rgba = gradient(w, h);
        let bytes = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Dds)).unwrap();
        assert_eq!(&bytes[..4], b"DDS ");
        assert_eq!(le32(&bytes, 4), 124, "dwSize");
        assert_eq!(le32(&bytes, 12), h as u32, "dwHeight");
        assert_eq!(le32(&bytes, 16), w as u32, "dwWidth");
        assert_eq!(le32(&bytes, 20), (w * 4) as u32, "pitch");
        // DDS_PIXELFORMAT en el offset 76 de la cabecera.
        assert_eq!(le32(&bytes, 76), 32, "pixelformat dwSize");
        assert_eq!(le32(&bytes, 80), 0x41, "DDPF_RGB | DDPF_ALPHAPIXELS");
        assert_eq!(le32(&bytes, 88), 32, "dwRGBBitCount");
        assert_eq!(le32(&bytes, 92), 0x0000_00FF, "máscara R");
        assert_eq!(le32(&bytes, 104), 0xFF00_0000, "máscara A");
        assert_eq!(le32(&bytes, 108), 0x1000, "DDSCAPS_TEXTURE");
        assert_eq!(&bytes[128..], &rgba[..], "payload en orden R,G,B,A");
    }

    #[test]
    fn dds_supports_the_rgb_and_gray_pixel_formats() {
        let (w, h) = (4, 2);
        let rgba = gradient(w, h);
        let rgb = EncodeOptions {
            pixel_format: PixelFormat::Rgb888,
            ..opts(GpuFormat::Dds)
        };
        let bytes = encode_to_bytes(&rgba, w, h, &rgb).unwrap();
        assert_eq!(le32(&bytes, 88), 24, "dwRGBBitCount");
        assert_eq!(le32(&bytes, 80), 0x40, "solo DDPF_RGB");
        assert_eq!(bytes.len(), 128 + w * h * 3);

        let alpha = EncodeOptions {
            pixel_format: PixelFormat::Alpha8,
            ..opts(GpuFormat::Dds)
        };
        let bytes = encode_to_bytes(&rgba, w, h, &alpha).unwrap();
        assert_eq!(le32(&bytes, 88), 8, "dwRGBBitCount");
        assert_eq!(le32(&bytes, 80), 0x2_0000, "DDPF_LUMINANCE");
        assert_eq!(bytes.len(), 128 + w * h);
    }

    #[test]
    fn dds_with_dxt_pixel_format_uses_the_fourcc() {
        let rgba = four_color_image();
        let mut o = opts(GpuFormat::Dds);

        o.pixel_format = PixelFormat::Dxt1;
        let dds = encode_to_bytes(&rgba, 8, 8, &o).unwrap();
        assert_eq!(&dds[..4], b"DDS ");
        // CAPS | HEIGHT | WIDTH | PIXELFORMAT | LINEARSIZE
        assert_eq!(u32::from_le_bytes(dds[8..12].try_into().unwrap()), 0x81007);
        assert_eq!(&dds[84..88], b"DXT1");
        let linear = u32::from_le_bytes(dds[20..24].try_into().unwrap()) as usize;
        assert_eq!(linear, 4 * 8, "4 bloques de 4x4 a 8 bytes");
        assert_eq!(dds.len(), 128 + linear);
        let mut buf = vec![0u32; 64];
        texture2ddecoder::decode_bc1(&dds[128..], 8, 8, &mut buf).expect("bc1");

        o.pixel_format = PixelFormat::Dxt5;
        let dds5 = encode_to_bytes(&rgba, 8, 8, &o).unwrap();
        assert_eq!(&dds5[84..88], b"DXT5");
        assert_eq!(dds5.len(), 128 + 4 * 16, "BC3 son 16 bytes por bloque");
        let mut buf5 = vec![0u32; 64];
        texture2ddecoder::decode_bc3(&dds5[128..], 8, 8, &mut buf5).expect("bc3");
        assert!(mean_channel_error(&rgba, &buf5) < 48.0);
    }

    #[test]
    fn dxt_mode_changes_the_encoded_blocks() {
        // Ruido determinista: con pesos de luminancia el codificador elige
        // extremos e índices distintos para los mismos bloques.
        let mut rgba = Vec::with_capacity(8 * 8 * 4);
        let mut st = 12345u32;
        for _ in 0..64 {
            st = st.wrapping_mul(1103515245).wrapping_add(12345);
            let r = (st >> 16) as u8;
            st = st.wrapping_mul(1103515245).wrapping_add(12345);
            let g = (st >> 16) as u8;
            st = st.wrapping_mul(1103515245).wrapping_add(12345);
            let b = (st >> 16) as u8;
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
        let mut o = opts(GpuFormat::Dds);
        o.pixel_format = PixelFormat::Dxt1;

        o.dxt_mode = DxtMode::Linear;
        let linear = encode_to_bytes(&rgba, 8, 8, &o).unwrap();
        o.dxt_mode = DxtMode::Perceptual;
        let perceptual = encode_to_bytes(&rgba, 8, 8, &o).unwrap();
        assert_ne!(linear, perceptual, "DXT_LINEAR y DXT_PERCEPTUAL difieren");
        assert_eq!(&linear[84..88], b"DXT1");
        assert_eq!(&perceptual[84..88], b"DXT1");

        // Ambos siguen siendo BC1 válido y fieles al original.
        let mut buf = vec![0u32; 64];
        texture2ddecoder::decode_bc1(&perceptual[128..], 8, 8, &mut buf).expect("bc1");
        assert!(
            mean_channel_error(&rgba, &buf) < 64.0,
            "error {}",
            mean_channel_error(&rgba, &buf)
        );
    }
}
