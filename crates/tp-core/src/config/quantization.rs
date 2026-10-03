use serde::{Deserialize, Serialize};

/// Output color depth / channel quantization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ColorDepth {
    #[default]
    #[serde(rename = "RGBA8888")]
    Rgba8888,
    #[serde(rename = "RGBA4444")]
    Rgba4444,
    #[serde(rename = "RGB565")]
    Rgb565,
}

impl ColorDepth {
    pub fn as_str(&self) -> &'static str {
        match self {
            ColorDepth::Rgba8888 => "RGBA8888",
            ColorDepth::Rgba4444 => "RGBA4444",
            ColorDepth::Rgb565 => "RGB565",
        }
    }

    /// Bytes per pixel of the exported image (used by *WordAligned*).
    pub fn bytes_per_pixel(&self) -> i32 {
        match self {
            ColorDepth::Rgba8888 => 4,
            ColorDepth::Rgba4444 | ColorDepth::Rgb565 => 2,
        }
    }
}

/// Dithering algorithm applied during quantization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DitheringAlgorithm {
    #[default]
    #[serde(rename = "None")]
    None,
    #[serde(rename = "FloydSteinberg")]
    FloydSteinberg,
    #[serde(rename = "Atkinson")]
    Atkinson,
    /// Floyd–Steinberg including the alpha channel.
    #[serde(rename = "FloydSteinbergAlpha")]
    FloydSteinbergAlpha,
    /// Atkinson including the alpha channel.
    #[serde(rename = "AtkinsonAlpha")]
    AtkinsonAlpha,
    /// Nearest neighbour: each pixel is rounded to the closest level on its
    /// own, with no error diffusion (smallest color error, no contrast gain).
    #[serde(rename = "NearestNeighbour")]
    NearestNeighbour,
    /// Linear: the quantization error travels to the right neighbour along the
    /// row, spreading the levels evenly (better contrast than NearestNeighbour).
    #[serde(rename = "Linear")]
    Linear,
}

impl DitheringAlgorithm {
    /// Whether the algorithm diffuses error into the alpha channel as well.
    pub fn dithers_alpha(&self) -> bool {
        matches!(
            self,
            DitheringAlgorithm::FloydSteinbergAlpha | DitheringAlgorithm::AtkinsonAlpha
        )
    }
}

/// Dithering de la cuantización a paleta al publicar PNG-8
/// (PngQuant Low/Medium/High).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PngDither {
    /// Cuantización directa sin difusión de error (archivo más pequeño).
    #[serde(rename = "PngQuantLow")]
    Low,
    /// Floyd–Steinberg sobre la paleta.
    #[serde(rename = "PngQuantMedium")]
    Medium,
    /// Floyd–Steinberg + refinamiento de la paleta (mejor fidelidad).
    #[default]
    #[serde(rename = "PngQuantHigh")]
    High,
}

impl PngDither {
    pub fn as_str(&self) -> &'static str {
        match self {
            PngDither::Low => "PngQuantLow",
            PngDither::Medium => "PngQuantMedium",
            PngDither::High => "PngQuantHigh",
        }
    }
}

/// Cuantización DXT (`--dxt-mode`): cómo pondera el error el ajuste de los
/// extremos de cada bloque BC1/BC3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DxtMode {
    /// Error uniforme por canal (el más rápido de entender, por defecto).
    #[default]
    #[serde(rename = "DXT_LINEAR")]
    Linear,
    /// Error ponderado por luminancia: prioriza lo que se ve.
    #[serde(rename = "DXT_PERCEPTUAL")]
    Perceptual,
}

impl DxtMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            DxtMode::Linear => "DXT_LINEAR",
            DxtMode::Perceptual => "DXT_PERCEPTUAL",
        }
    }

    /// Acepta el token y sus formas cortas.
    pub fn parse(v: &str) -> Option<Self> {
        Some(match v.to_ascii_lowercase().as_str() {
            "dxt_linear" | "linear" => DxtMode::Linear,
            "dxt_perceptual" | "perceptual" => DxtMode::Perceptual,
            _ => return None,
        })
    }
}
