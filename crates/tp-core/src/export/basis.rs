use super::options::EncodeOptions;
use crate::error::{Result, TpError};

/// Basis Universal ETC1S (`.basis`) con `--basis-quality` (0-100): el motor
/// lo transcodifica en tiempo de carga a cualquier formato de hardware. Como
/// el resto de formatos de hardware, comprime el RGBA directamente e ignora
/// el pixel format.
pub(super) fn encode_basis(
    rgba: &[u8],
    width: usize,
    height: usize,
    opts: &EncodeOptions,
) -> Result<Vec<u8>> {
    #[cfg(feature = "gpu-formats")]
    {
        tp_basis::encode_etc1s(rgba, width as u32, height as u32, opts.basis_quality)
            .map_err(TpError::Other)
    }
    #[cfg(not(feature = "gpu-formats"))]
    {
        let _ = (rgba, width, height, opts);
        Err(TpError::Other(
            "Basis requiere compilar con la feature `gpu-formats` \
             (cargo build --features gpu-formats)"
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::config::GpuFormat;
    use crate::export::encode_to_bytes;
    #[cfg(not(feature = "gpu-formats"))]
    use crate::export::test_helpers::gradient;
    use crate::export::test_helpers::opts;
    #[cfg(feature = "gpu-formats")]
    use crate::export::test_helpers::{four_color_image, mean_channel_error};

    #[cfg(feature = "gpu-formats")]
    #[test]
    fn basis_file_transcodes_back_to_rgba() {
        let rgba = four_color_image();
        let file = encode_to_bytes(&rgba, 8, 8, &opts(GpuFormat::Basis)).unwrap();
        assert_eq!(&file[..2], b"sB", "firma de un .basis");

        let (w, h, out) = tp_basis::transcode_rgba(&file).unwrap();
        assert_eq!((w, h), (8, 8));
        assert_eq!(out.len(), rgba.len());
        let decoded: Vec<u32> = out
            .chunks_exact(4)
            .map(|px| u32::from_le_bytes([px[2], px[1], px[0], px[3]]))
            .collect();
        let err = mean_channel_error(&rgba, &decoded);
        assert!(err < 5.0, "error medio Basis demasiado alto: {err:.2}");
    }

    #[cfg(not(feature = "gpu-formats"))]
    #[test]
    fn basis_requires_the_gpu_formats_feature() {
        let rgba = gradient(4, 4);
        let err = encode_to_bytes(&rgba, 4, 4, &opts(GpuFormat::Basis))
            .unwrap_err()
            .to_string();
        assert!(err.contains("gpu-formats"), "{err}");
    }
}
