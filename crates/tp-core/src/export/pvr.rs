use super::ktx::zlib_bytes;
use super::options::EncodeOptions;

use crate::error::Result;
use crate::pvrtc;

/// PVRTC1 2/4bpp (según el pixel format `PVRTC*`) in a PVR v3 container
/// (`.pvr`). El pixel format también decide si se descarta el alfa y el
/// código `pixelFormat` de la cabecera; la calidad (`--pvr-quality`, 0-7)
/// controla el refinamiento de extremos.
pub(super) fn encode_pvrtc_pvr(
    rgba: &[u8],
    width: usize,
    height: usize,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    let pf = opts.pixel_format;
    let src: Vec<u8> = if pf.drops_alpha() {
        let mut v = rgba.to_vec();
        for px in v.chunks_exact_mut(4) {
            px[3] = 255;
        }
        v
    } else {
        rgba.to_vec()
    };
    let blocks = pvrtc::encode_pvrtc(&src, width, height, pf.is_pvrtc_2bpp(), opts.pvr_quality)?;
    let mut out = Vec::with_capacity(52 + blocks.len());
    // PVR v3 header (52 bytes), all little-endian.
    out.extend_from_slice(b"PVR\x03"); // version
    out.extend_from_slice(&0u32.to_le_bytes()); // flags
    out.extend_from_slice(&pf.pvr_header_code().to_le_bytes()); // pixel_format
    out.extend_from_slice(&0u32.to_le_bytes()); // colour_space: linearRGB
    out.extend_from_slice(&0u32.to_le_bytes()); // channel_type: unsigned byte
    out.extend_from_slice(&(height as u32).to_le_bytes());
    out.extend_from_slice(&(width as u32).to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // depth
    out.extend_from_slice(&1u32.to_le_bytes()); // num_surfaces
    out.extend_from_slice(&1u32.to_le_bytes()); // num_faces
    out.extend_from_slice(&1u32.to_le_bytes()); // mip_map_count
    out.extend_from_slice(&0u32.to_le_bytes()); // meta_data_size
    out.extend_from_slice(&blocks);
    Ok(out)
}

/// gzip del fichero completo (`.pvr.gz`).
pub(super) fn gzip_bytes(data: Vec<u8>) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    let _ = enc.write_all(&data);
    enc.finish().unwrap_or_default()
}

/// Contenedor CCZ de Cocos2D: cabecera de 16 bytes (campos en big-endian)
/// + el PVR v3 en zlib. `compression_type` 0 = zlib.
pub(super) fn ccz_bytes(data: Vec<u8>) -> Vec<u8> {
    let payload = zlib_bytes(&data);
    let mut out = Vec::with_capacity(16 + payload.len());
    out.extend_from_slice(b"CCZ!");
    out.extend_from_slice(&0u16.to_be_bytes()); // compression_type: zlib
    out.extend_from_slice(&0u16.to_be_bytes()); // version
    out.extend_from_slice(&0u32.to_be_bytes()); // reserved
    out.extend_from_slice(&(data.len() as u32).to_be_bytes()); // len
    out.extend_from_slice(&payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GpuFormat, PixelFormat};
    use crate::export::encode_to_bytes;
    use crate::export::test_helpers::{be16, four_color_image, gradient, opts};

    #[test]
    fn pvrtc_pvr_file() {
        let rgba = vec![255u8; 8 * 8 * 4];
        let bytes = encode_to_bytes(&rgba, 8, 8, &opts(GpuFormat::Pvrtc4Bpp)).unwrap();
        assert_eq!(&bytes[..4], b"PVR\x03");
        assert_eq!(bytes.len(), 52 + 8 * 8 / 2);
        let payload = &bytes[52..];
        let mut buf = vec![0u32; 8 * 8];
        texture2ddecoder::decode_pvrtc_4bpp(payload, 8, 8, &mut buf).unwrap();
        // Non-power-of-two sizes error clearly (e.g. odd scale variants).
        assert!(encode_to_bytes(&rgba, 96, 96, &opts(GpuFormat::Pvrtc4Bpp)).is_err());
    }

    #[test]
    fn pvr3_gz_and_ccz_wrap_the_pvr3_file() {
        use std::io::Read;
        let rgba = gradient(8, 8);
        let base = encode_to_bytes(&rgba, 8, 8, &opts(GpuFormat::Pvrtc4Bpp)).unwrap();
        assert_eq!(&base[..4], b"PVR\x03");

        let gz = encode_to_bytes(&rgba, 8, 8, &opts(GpuFormat::Pvr3Gz)).unwrap();
        assert_eq!(&gz[..2], &[0x1F, 0x8B], "cabecera gzip");
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&gz[..])
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, base, "pvr.gz debe descomprimir al PVR3 original");

        let ccz = encode_to_bytes(&rgba, 8, 8, &opts(GpuFormat::Pvr3Ccz)).unwrap();
        assert_eq!(&ccz[..4], b"CCZ!");
        assert_eq!(be16(&ccz, 4), 0, "compression_type = zlib");
        assert_eq!(be16(&ccz, 6), 0, "version");
        assert_eq!(
            u32::from_be_bytes(ccz[12..16].try_into().unwrap()) as usize,
            base.len()
        );
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&ccz[16..])
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, base, "pvr.ccz debe descomprimir al PVR3 original");
    }

    /// Campo `pixelFormat` (u64 LE) de la cabecera PVR v3, en 8..16.
    fn pvr_pixel_format(bytes: &[u8]) -> u64 {
        u64::from_le_bytes(bytes[8..16].try_into().unwrap())
    }

    #[test]
    fn pvr_container_reports_the_selected_pvrtc_variant() {
        let rgba = four_color_image();
        let mut o = opts(GpuFormat::Pvrtc4Bpp);

        o.pixel_format = PixelFormat::Pvrtc4BppRgba;
        let pvr4 = encode_pvrtc_pvr(&rgba, 8, 8, &o).unwrap();
        assert_eq!(pvr_pixel_format(&pvr4), 3, "4bpp RGBA = 3");
        assert_eq!(pvr4.len(), 52 + 8 * 8 / 2, "4bpp son w*h/2 bytes");

        o.pixel_format = PixelFormat::Pvrtc2BppRgba;
        let pvr2 = encode_pvrtc_pvr(&rgba, 8, 8, &o).unwrap();
        assert_eq!(pvr_pixel_format(&pvr2), 1, "2bpp RGBA = 1");
        assert_eq!(pvr2.len(), 52 + 8 * 8 / 4, "2bpp son w*h/4 bytes");
        assert_ne!(
            &pvr2[52..],
            &pvr4[52..],
            "el número de bits cambia el payload"
        );

        // La variante RGB aplana el alfa: su payload es el de la misma imagen
        // ya opaca.
        o.pixel_format = PixelFormat::Pvrtc4BppRgb;
        let pvr_rgb = encode_pvrtc_pvr(&rgba, 8, 8, &o).unwrap();
        assert_eq!(pvr_pixel_format(&pvr_rgb), 2, "4bpp RGB = 2");
        let mut flat = rgba.clone();
        for px in flat.chunks_exact_mut(4) {
            px[3] = 255;
        }
        o.pixel_format = PixelFormat::Pvrtc4BppRgba;
        let pvr_flat = encode_pvrtc_pvr(&flat, 8, 8, &o).unwrap();
        assert_eq!(&pvr_rgb[52..], &pvr_flat[52..], "RGB descarta el alfa");
    }
}
