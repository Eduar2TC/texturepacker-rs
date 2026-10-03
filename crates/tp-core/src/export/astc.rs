use super::options::EncodeOptions;
use crate::error::{Result, TpError};

/// ASTC (bloque elegido por el pixel format `ASTC_*`, 4x4 por defecto) con
/// preset según `--astc-quality` 0-4: fastest, fast, medium, thorough,
/// exhaustive. El contenedor es `.astc`.
pub(super) fn encode_astc(
    rgba: &[u8],
    width: usize,
    height: usize,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    let (bx, by) = opts.pixel_format.astc_block().unwrap_or((4, 4));
    #[cfg(feature = "gpu-formats")]
    {
        use astcenc_rs::{
            ConfigBuilder, Context, Extents, Image, Profile, Swizzle, PRESET_EXHAUSTIVE,
            PRESET_FAST, PRESET_FASTEST, PRESET_MEDIUM, PRESET_THOROUGH,
        };
        let preset = match opts.astc_quality.min(4) {
            0 => PRESET_FASTEST,
            1 => PRESET_FAST,
            2 => PRESET_MEDIUM,
            3 => PRESET_THOROUGH,
            _ => PRESET_EXHAUSTIVE,
        };
        let cfg = ConfigBuilder::new()
            .with_block_size(Extents::new(bx as u32, by as u32))
            .with_preset(preset)
            .with_profile(Profile::LdrRgba)
            .build()
            .map_err(|e| TpError::Other(format!("Config ASTC inválida: {e:?}")))?;
        let mut ctx =
            Context::new(cfg).map_err(|e| TpError::Other(format!("Contexto ASTC: {e:?}")))?;
        let img = Image {
            extents: Extents::new(width as u32, height as u32),
            data: &[rgba][..],
        };
        let blocks = ctx
            .compress(&img, Swizzle::rgba())
            .map_err(|e| TpError::Other(format!("Error comprimiendo ASTC: {e:?}")))?;

        // .astc header (16 bytes): magic, block dims, x/y/z size (24-bit LE).
        let mut out = Vec::with_capacity(16 + blocks.len());
        out.extend_from_slice(&[0x13, 0xAB, 0xA1, 0x5C]); // magic 0x5CA1AB13
        out.push(bx); // block_x
        out.push(by); // block_y
        out.push(1); // block_z
        out.extend_from_slice(&(width as u32).to_le_bytes()[..3]);
        out.extend_from_slice(&(height as u32).to_le_bytes()[..3]);
        out.extend_from_slice(&1u32.to_le_bytes()[..3]); // zsize
        out.extend_from_slice(&blocks);
        Ok(out)
    }
    #[cfg(not(feature = "gpu-formats"))]
    {
        let _ = (rgba, width, height, opts, bx, by);
        Err(TpError::Other(
            "ASTC requiere compilar con la feature `gpu-formats` \
             (cargo build --features gpu-formats)"
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "gpu-formats")]
    use super::*;
    #[cfg(feature = "gpu-formats")]
    use crate::config::{GpuFormat, PixelFormat};
    #[cfg(feature = "gpu-formats")]
    use crate::export::test_helpers::{four_color_image, opts};

    #[cfg(feature = "gpu-formats")]
    #[test]
    fn astc_block_size_and_quality_follow_the_pixel_format() {
        let rgba = four_color_image();
        let mut o = opts(GpuFormat::Astc4x4);
        o.pixel_format = PixelFormat::Astc8x8;
        o.astc_quality = 0;
        let file = encode_astc(&rgba, 8, 8, &o).unwrap();
        assert_eq!(&file[..4], &[0x13, 0xAB, 0xA1, 0x5C], "magic ASTC");
        assert_eq!(file[4], 8, "block_x");
        assert_eq!(file[5], 8, "block_y");
        // 1 bloque de 8x8 con 16 bytes por bloque.
        assert_eq!(file.len(), 16 + 16);

        o.pixel_format = PixelFormat::Astc4x4;
        let four = encode_astc(&rgba, 8, 8, &o).unwrap();
        assert_eq!(four[4], 4, "block_x");
        assert_eq!(four[5], 4, "block_y");
        assert_eq!(four.len(), 16 + 4 * 16, "4 bloques de 4x4");
    }
}
