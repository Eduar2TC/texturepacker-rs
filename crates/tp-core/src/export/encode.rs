use super::astc::encode_astc;
use super::basis::encode_basis;
use super::dds::{encode_dds, encode_dds_dxt};
use super::etc::{encode_etc1_ktx, encode_etc1_pkm, encode_etc2_ktx};
use super::image_formats::encode_image_format;
use super::jpg::encode_jpg;
use super::ktx::{encode_ktx2, ktx1_rgba8, zlib_bytes};
use super::options::EncodeOptions;
use super::pixel::apply_pixel_format;
use super::png::{encode_png, encode_png8};
use super::pvr::{ccz_bytes, encode_pvrtc_pvr, gzip_bytes};
use super::webp::encode_webp;
use crate::config::{GpuFormat, PixelFormat};
use crate::error::Result;

/// Encode an RGBA8 image into the *file* bytes of the requested format.
pub fn encode_to_bytes(
    rgba: &[u8],
    width: usize,
    height: usize,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    match opts.format {
        GpuFormat::Png => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_png(&data, width, height, color, opts)
        }
        GpuFormat::Png8 => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_png8(&data, width, height, color, opts)
        }
        GpuFormat::Jpg => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_jpg(&data, width, height, color, opts)
        }
        GpuFormat::WebP => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_webp(&data, width, height, color, opts)
        }
        // Formatos de hardware: comprimen RGBA directamente (ignoran el
        // pixel format salvo que este fije el bloque, la variante o la
        // calidad).
        GpuFormat::Astc4x4 => encode_astc(rgba, width, height, opts),
        GpuFormat::Basis => encode_basis(rgba, width, height, opts),
        GpuFormat::Etc2Rgba => encode_etc2_ktx(rgba, width, height, opts),
        GpuFormat::Pvrtc4Bpp => encode_pvrtc_pvr(rgba, width, height, opts),
        GpuFormat::Pvr3Gz => encode_pvrtc_pvr(rgba, width, height, opts).map(gzip_bytes),
        GpuFormat::Pvr3Ccz => encode_pvrtc_pvr(rgba, width, height, opts).map(ccz_bytes),
        GpuFormat::Etc1 => encode_etc1_pkm(rgba, width, height, opts.etc1_quality),
        GpuFormat::Etc1Ktx => encode_etc1_ktx(rgba, width, height, opts.etc1_quality),
        // Formatos de software: pasan por la conversión de pixel format.
        GpuFormat::Bmp | GpuFormat::Tga | GpuFormat::Tiff => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            encode_image_format(&data, width, height, color, opts.format)
        }
        GpuFormat::Dds => match opts.pixel_format {
            // Bloques DXT: la cabecera lleva el fourcc y el payload son los
            // bloques BC1/BC3 codificados con el dxt-mode elegido.
            PixelFormat::Dxt1 | PixelFormat::Dxt5 => encode_dds_dxt(rgba, width, height, opts),
            _ => {
                let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
                encode_dds(&data, width, height, color)
            }
        },
        GpuFormat::Zktx => {
            let (data, color) = apply_pixel_format(rgba, opts.pixel_format);
            let ktx = ktx1_rgba8(width, height, color, &data);
            Ok(zlib_bytes(&ktx))
        }
        GpuFormat::Ktx2 => encode_ktx2(rgba, width, height, opts),
    }
}
