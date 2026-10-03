use crate::config::{DxtMode, GpuFormat, PixelFormat, PngDither, ProjectConfig};

/// Opciones de codificación de textura (nivel de optimización PNG, dithering,
/// calidad, formato de píxel).
#[derive(Debug, Clone)]
pub struct EncodeOptions {
    pub format: GpuFormat,
    pub png_opt_level: u8,
    pub png8_dither: PngDither,
    pub jpg_quality: u8,
    pub webp_quality: u16,
    pub pixel_format: PixelFormat,
    /// Calidad PVRTC (`--pvr-quality`), 0-7, controla el refinamiento de los
    /// extremos de cada bloque.
    pub pvr_quality: u8,
    /// Calidad ETC1 (`--etc1-quality`), 0-100, controla el esfuerzo del
    /// buscador de estructuras y candidatos.
    pub etc1_quality: u8,
    /// Calidad ETC2 (`--etc2-quality`), 0-100.
    pub etc2_quality: u8,
    /// Calidad ASTC (`--astc-quality`), 0-4, se traduce al preset de astcenc.
    pub astc_quality: u8,
    /// Calidad Basis ETC1S (`--basis-quality`), 0-100, se traduce al nivel de
    /// calidad 1-255 de Basis.
    pub basis_quality: u8,
    /// Cuantización de la métrica de error DXT (`--dxt-mode`).
    pub dxt_mode: DxtMode,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            format: GpuFormat::Png,
            png_opt_level: 1,
            png8_dither: PngDither::default(),
            jpg_quality: 80,
            webp_quality: 101,
            pixel_format: PixelFormat::default(),
            pvr_quality: 3,
            etc1_quality: 70,
            etc2_quality: 70,
            astc_quality: 2,
            basis_quality: 50,
            dxt_mode: DxtMode::default(),
        }
    }
}

impl EncodeOptions {
    /// Opciones derivadas de la configuración del proyecto.
    pub fn from_config(config: &ProjectConfig) -> Self {
        Self {
            format: config.gpu_format,
            png_opt_level: config.png_opt_level,
            png8_dither: config.png8_dither,
            jpg_quality: config.jpg_quality,
            webp_quality: config.webp_quality,
            pixel_format: config.pixel_format,
            pvr_quality: config.pvr_quality,
            etc1_quality: config.etc1_quality,
            etc2_quality: config.etc2_quality,
            astc_quality: config.astc_quality,
            basis_quality: config.basis_quality,
            dxt_mode: config.dxt_mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_options_from_config_carry_the_quality_knobs() {
        let cfg = ProjectConfig {
            pvr_quality: 7,
            etc1_quality: 12,
            etc2_quality: 88,
            astc_quality: 4,
            basis_quality: 63,
            dxt_mode: DxtMode::Perceptual,
            ..Default::default()
        };
        let o = EncodeOptions::from_config(&cfg);
        assert_eq!(
            (
                o.pvr_quality,
                o.etc1_quality,
                o.etc2_quality,
                o.astc_quality,
                o.basis_quality
            ),
            (7, 12, 88, 4, 63)
        );
        assert_eq!(o.dxt_mode, DxtMode::Perceptual);
    }
}
