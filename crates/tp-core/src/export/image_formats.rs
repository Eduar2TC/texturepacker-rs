use crate::config::GpuFormat;
use crate::error::{Result, TpError};

/// BMP/TGA/TIFF sin comprimir, con la conversión de pixel format ya aplicada.
pub(super) fn encode_image_format(
    data: &[u8],
    width: usize,
    height: usize,
    color: image::ExtendedColorType,
    format: GpuFormat,
) -> Result<Vec<u8>> {
    use image::ImageEncoder;
    // Cursor: TIFF exige Write + Seek.
    let mut buf = std::io::Cursor::new(Vec::new());
    let name = format.as_str();
    let res = match format {
        GpuFormat::Bmp => image::codecs::bmp::BmpEncoder::new(&mut buf).write_image(
            data,
            width as u32,
            height as u32,
            color,
        ),
        GpuFormat::Tga => image::codecs::tga::TgaEncoder::new(&mut buf).write_image(
            data,
            width as u32,
            height as u32,
            color,
        ),
        GpuFormat::Tiff => image::codecs::tiff::TiffEncoder::new(&mut buf).write_image(
            data,
            width as u32,
            height as u32,
            color,
        ),
        _ => return Err(TpError::Other(format!("{name} no es un formato de imagen"))),
    };
    res.map_err(|e| TpError::Other(format!("Error codificando {name}: {e}")))?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PixelFormat;
    use crate::export::test_helpers::{gradient, opts};
    use crate::export::{encode_to_bytes, EncodeOptions};

    #[test]
    fn bmp_tga_tiff_roundtrip_the_rgba_pixels() {
        let (w, h) = (7, 5);
        let rgba = gradient(w, h);
        for format in [GpuFormat::Bmp, GpuFormat::Tga, GpuFormat::Tiff] {
            let bytes = encode_to_bytes(&rgba, w, h, &opts(format)).unwrap();
            // El TGA no lleva magia: hay que indicar el formato al decodificar.
            let img_format = match format {
                GpuFormat::Bmp => image::ImageFormat::Bmp,
                GpuFormat::Tga => image::ImageFormat::Tga,
                _ => image::ImageFormat::Tiff,
            };
            let decoded = image::load_from_memory_with_format(&bytes, img_format)
                .unwrap_or_else(|e| panic!("{} no decodifica: {e}", format.as_str()))
                .to_rgba8();
            assert_eq!(decoded.as_raw(), &rgba, "{}", format.as_str());
        }
    }

    #[test]
    fn software_image_formats_honour_the_pixel_format() {
        let (w, h) = (4, 2);
        let rgba = gradient(w, h);
        let o = EncodeOptions {
            pixel_format: PixelFormat::Rgb888,
            ..opts(GpuFormat::Tga)
        };
        let bytes = encode_to_bytes(&rgba, w, h, &o).unwrap();
        // TGA sin canal alfa: el decodificador devuelve opacos.
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Tga)
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (w as u32, h as u32));
        assert!(decoded.pixels().all(|p| p.0[3] == 255), "alfa no compuesto");
    }
}
