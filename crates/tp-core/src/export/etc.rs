use super::ktx::ktx1_container;
use super::options::EncodeOptions;
use crate::config::PixelFormat;
use crate::etc2;

/// ETC2 in a KTX container. `ETC2_RGBA` (8 bytes de alfa EAC + 8 de ETC2) o
/// `ETC2_RGB` según el pixel format, con `--etc2-quality` (0-100).
pub(super) fn encode_etc2_ktx(
    rgba: &[u8],
    width: usize,
    height: usize,
    opts: &EncodeOptions,
) -> Vec<u8> {
    let q = opts.etc2_quality;
    if opts.pixel_format == PixelFormat::Etc2Rgb {
        let blocks = etc2::encode_etc2_rgb_blocks(rgba, width, height, q);
        // GL_COMPRESSED_RGB8_ETC2 sobre GL_RGB.
        ktx1_container(0, 0, 0x9274, 0x1907, width, height, &blocks)
    } else {
        let blocks = etc2::encode_etc2_rgba8(rgba, width, height, q);
        // GL_COMPRESSED_RGBA8_ETC2_EAC sobre GL_RGBA.
        ktx1_container(0, 0, 0x9278, 0x1908, width, height, &blocks)
    }
}

/// ETC1 RGB in a PKM container (`.pkm`), the format PowerVR/GLES tools expect.
pub(super) fn encode_etc1_pkm(rgba: &[u8], width: usize, height: usize, quality: u8) -> Vec<u8> {
    let blocks = etc2::encode_etc1_rgb(rgba, width, height, quality);
    let ext_w = width.next_multiple_of(4);
    let ext_h = height.next_multiple_of(4);
    let mut out = Vec::with_capacity(16 + blocks.len());
    out.extend_from_slice(b"PKM 10"); // magic + version "10"
    out.extend_from_slice(&0u16.to_be_bytes()); // dataFormat: ETC1
    out.extend_from_slice(&(ext_w as u16).to_be_bytes());
    out.extend_from_slice(&(ext_h as u16).to_be_bytes());
    out.extend_from_slice(&(width as u16).to_be_bytes());
    out.extend_from_slice(&(height as u16).to_be_bytes());
    out.extend_from_slice(&blocks);
    out
}

/// ETC1 RGB in a KTX container (`.ktx`).
pub(super) fn encode_etc1_ktx(rgba: &[u8], width: usize, height: usize, quality: u8) -> Vec<u8> {
    let blocks = etc2::encode_etc1_rgb(rgba, width, height, quality);
    // GL_ETC1_RGB8_OES sobre GL_RGB.
    ktx1_container(0, 0, 0x8D60, 0x1907, width, height, &blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GpuFormat;
    use crate::export::encode_to_bytes;
    use crate::export::test_helpers::{
        be16, four_color_image, gradient, le32, mean_channel_error, opts,
    };

    #[test]
    fn etc2_ktx_header() {
        let bytes = encode_etc2_ktx(&vec![0u8; 8 * 8 * 4], 8, 8, &EncodeOptions::default());
        assert_eq!(&bytes[..12], b"\xABKTX 11\xBB\r\n\x1A\n");
        // internal format at offset 28 (12 magic + 4*4 header fields)
        let internal = u32::from_le_bytes(bytes[28..32].try_into().unwrap());
        assert_eq!(internal, 0x9278);
        assert_eq!(bytes.len(), 64 + 4 + 4 * 16);
    }

    #[test]
    fn etc1_pkm_and_ktx_decode_close_to_the_source() {
        let (w, h) = (16, 16);
        let rgba = gradient(w, h);

        let pkm = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Etc1)).unwrap();
        assert_eq!(&pkm[..6], b"PKM 10");
        assert_eq!(be16(&pkm, 6), 0, "dataFormat ETC1");
        assert_eq!(be16(&pkm, 8), w as u16, "extended width");
        assert_eq!(be16(&pkm, 10), h as u16, "extended height");
        assert_eq!(be16(&pkm, 12), w as u16, "original width");
        assert_eq!(be16(&pkm, 14), h as u16, "original height");
        let mut buf = vec![0u32; w * h];
        texture2ddecoder::decode_etc1(&pkm[16..], w, h, &mut buf).unwrap();
        let err = mean_channel_error(&rgba, &buf);
        assert!(err < 6.0, "error medio ETC1 demasiado alto: {err:.2}");

        let ktx = encode_to_bytes(&rgba, w, h, &opts(GpuFormat::Etc1Ktx)).unwrap();
        assert_eq!(&ktx[..12], b"\xABKTX 11\xBB\r\n\x1A\n");
        assert_eq!(le32(&ktx, 16), 0, "glType = 0 (comprimido)");
        assert_eq!(le32(&ktx, 24), 0, "glFormat = 0 (comprimido)");
        assert_eq!(le32(&ktx, 28), 0x8D60, "GL_ETC1_RGB8_OES");
        assert_eq!(le32(&ktx, 32), 0x1907, "GL_RGB");
        let image_size = le32(&ktx, 64) as usize;
        assert_eq!(image_size, w * h / 2, "ETC1 son 8 bytes por bloque 4x4");
        let mut buf = vec![0u32; w * h];
        texture2ddecoder::decode_etc1(&ktx[68..68 + image_size], w, h, &mut buf).unwrap();
        let err = mean_channel_error(&rgba, &buf);
        assert!(err < 6.0, "error medio ETC1 (ktx) demasiado alto: {err:.2}");
    }

    #[test]
    fn etc1_encoder_stays_inside_the_etc1_mode_set() {
        // Un bloque que en ETC2 caería en modo T/H/planar (desbordamiento del
        // diferencial) debe seguir siendo decodificable como ETC1.
        let mut rgba = vec![0u8; 4 * 4 * 4];
        for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
            let v = ((i as u16 * 16) % 256) as u8;
            px.copy_from_slice(&[v, 255 - v, ((u16::from(v) * 3) % 256) as u8, 255]);
        }
        let blocks = crate::etc2::encode_etc1_rgb(&rgba, 4, 4, 70);
        assert_eq!(blocks.len(), 8);
        // Ningún bloque puede salirse del conjunto de modos ETC1: con el bit
        // de diferencial activo, base + delta debe caber en 5 bits (si no,
        // ETC2 lo reinterpretaría como modo T/H/planar y ETC1 lo leería mal).
        if (blocks[3] >> 1) & 1 == 1 {
            for channel in blocks[..3].iter() {
                let base = i16::from(channel >> 3);
                let delta = i16::from(channel & 7);
                let delta = if delta >= 4 { delta - 8 } else { delta };
                let sum = base + delta;
                assert!(
                    (0..=31).contains(&sum),
                    "diferencial {sum} fuera del rango ETC1"
                );
            }
        }
        let mut buf = vec![0u32; 16];
        texture2ddecoder::decode_etc1(&blocks, 4, 4, &mut buf).unwrap();
        assert!(mean_channel_error(&rgba, &buf) < 48.0);
    }

    /// Campo `glInternalFormat` (u32 LE) de un KTX v1, en 28..32.
    fn ktx_internal_format(bytes: &[u8]) -> u32 {
        u32::from_le_bytes(bytes[28..32].try_into().unwrap())
    }

    #[test]
    fn etc2_pixel_format_switches_the_ktx_internal_format() {
        let rgba = four_color_image();
        let mut o = opts(GpuFormat::Etc2Rgba);

        o.pixel_format = PixelFormat::Etc2Rgba;
        let rgba_ktx = encode_etc2_ktx(&rgba, 8, 8, &o);
        assert_eq!(ktx_internal_format(&rgba_ktx), 0x9278); // RGBA8_ETC2_EAC

        o.pixel_format = PixelFormat::Etc2Rgb;
        let rgb_ktx = encode_etc2_ktx(&rgba, 8, 8, &o);
        assert_eq!(ktx_internal_format(&rgb_ktx), 0x9274); // RGB8_ETC2
                                                           // 4 bloques de 8x8: RGBA lleva la mitad de datos que RGB no.
        assert_eq!(rgba_ktx.len(), rgb_ktx.len() + 4 * 8);
    }
}
