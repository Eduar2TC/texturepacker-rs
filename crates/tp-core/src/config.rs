//! Project configuration (`ProjectConfig`) with serde/TOML persistence.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}

/// Output GPU texture format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GpuFormat {
    #[default]
    #[serde(rename = "PNG")]
    Png,
    #[serde(rename = "WebP")]
    WebP,
    #[serde(rename = "ASTC_4x4")]
    Astc4x4,
    #[serde(rename = "ETC2_RGBA")]
    Etc2Rgba,
    #[serde(rename = "PVRTC_4BPP")]
    Pvrtc4Bpp,
}

impl GpuFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            GpuFormat::Png => "PNG",
            GpuFormat::WebP => "WebP",
            GpuFormat::Astc4x4 => "ASTC_4x4",
            GpuFormat::Etc2Rgba => "ETC2_RGBA",
            GpuFormat::Pvrtc4Bpp => "PVRTC_4BPP",
        }
    }

    pub fn file_extension(&self) -> &'static str {
        match self {
            GpuFormat::Png => "png",
            GpuFormat::WebP => "webp",
            GpuFormat::Astc4x4 => "astc",
            GpuFormat::Etc2Rgba => "ktx",
            GpuFormat::Pvrtc4Bpp => "pvr",
        }
    }
}

/// MaxRects / Guillotine placement heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PackingStrategy {
    /// MaxRects — Best Short Side Fit (spec default).
    #[default]
    #[serde(rename = "BSSF")]
    Bssf,
    /// MaxRects — Best Area Fit.
    #[serde(rename = "BAF")]
    Baf,
    /// MaxRects — Best Long Side Fit.
    #[serde(rename = "BLSF")]
    Blsf,
    /// Guillotine bin packing (Best Short Side split).
    #[serde(rename = "Guillotine")]
    Guillotine,
}

impl PackingStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            PackingStrategy::Bssf => "BSSF",
            PackingStrategy::Baf => "BAF",
            PackingStrategy::Blsf => "BLSF",
            PackingStrategy::Guillotine => "Guillotine",
        }
    }
}

/// Built-in metadata template languages (the spec mentions JSON/XML/Plist/C++).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TemplateFormat {
    #[default]
    #[serde(rename = "JSON")]
    Json,
    #[serde(rename = "XML")]
    Xml,
    #[serde(rename = "Plist")]
    Plist,
    #[serde(rename = "CppHeader")]
    CppHeader,
    #[serde(rename = "TSV")]
    Tsv,
    #[serde(rename = "PlainText")]
    PlainText,
}

/// The project configuration. Mirrors the spec's `ProjectConfig` plus a few
/// sensible extensions (packing strategy, variants, template format, pivots).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub input_directory: PathBuf,
    pub output_directory: PathBuf,
    /// Maximum texture size per axis (power of two, e.g. 2048, 4096).
    pub max_texture_size: i32,
    /// Pixels of transparent padding around every frame (anti-bleeding).
    pub padding: i32,
    /// Pixels of border-pixel extrusion around every frame (anti-bleeding).
    pub extrude: i32,
    /// Allow 90° rotation while packing.
    pub allow_rotation: bool,
    /// Trim fully-transparent borders before packing.
    pub enable_trim: bool,
    /// Alpha threshold (0-255) below which a pixel is considered transparent.
    pub trim_threshold: i32,
    /// Enable polygon (mesh) extraction and polygon-aware packing.
    pub enable_polygon: bool,
    /// Ramer-Douglas-Peucker simplification tolerance, in pixels.
    pub polygon_tolerance: f32,
    /// Deduplicate identical sprites via pixel hashing (alias detection).
    pub enable_aliasing: bool,
    pub color_depth: ColorDepth,
    pub dithering_algorithm: DitheringAlgorithm,
    pub gpu_format: GpuFormat,
    /// Optional AES-256-GCM key; texture files are encrypted when set.
    pub encryption_key: Option<String>,
    /// Path to a custom Mustache template; when `None` a built-in is used
    /// according to `template_format`.
    pub export_template: Option<PathBuf>,
    /// Metadata output language.
    pub template_format: TemplateFormat,
    /// Packing algorithm / heuristic.
    pub packing_strategy: PackingStrategy,
    /// Scale variants to emit, e.g. `[1.0, 0.5]` produces `atlas.png` and `atlas_0.5x.png`.
    pub scale_variants: Vec<f32>,
    /// Auto co-pack `*_normal.png` companions in the same frames.
    pub enable_normal_maps: bool,
    /// Default normalized pivot for all sprites.
    pub default_pivot_x: f32,
    pub default_pivot_y: f32,
    /// Naming scheme for atlas pages: `atlas`, `atlas_1`, `atlas_2`, ...
    pub base_file_name: String,
    /// Recurse into subdirectories of `input_directory`.
    pub recursive: bool,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            input_directory: PathBuf::new(),
            output_directory: PathBuf::new(),
            max_texture_size: 2048,
            padding: 2,
            extrude: 1,
            allow_rotation: true,
            enable_trim: true,
            trim_threshold: 1,
            enable_polygon: false,
            polygon_tolerance: 1.5,
            enable_aliasing: true,
            color_depth: ColorDepth::Rgba8888,
            dithering_algorithm: DitheringAlgorithm::FloydSteinberg,
            gpu_format: GpuFormat::Png,
            encryption_key: None,
            export_template: None,
            template_format: TemplateFormat::Json,
            packing_strategy: PackingStrategy::Bssf,
            scale_variants: vec![1.0],
            enable_normal_maps: true,
            default_pivot_x: 0.5,
            default_pivot_y: 0.5,
            base_file_name: "atlas".to_string(),
            recursive: true,
        }
    }
}

impl ProjectConfig {
    /// Validate the configuration, returning a human-readable error on failure.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_texture_size <= 0 || (self.max_texture_size & (self.max_texture_size - 1)) != 0 {
            return Err(format!(
                "max_texture_size debe ser una potencia de dos positiva (se obtuvo {})",
                self.max_texture_size
            ));
        }
        if self.padding < 0 || self.extrude < 0 {
            return Err("padding y extrude no pueden ser negativos".into());
        }
        if !(0..=255).contains(&self.trim_threshold) {
            return Err("trim_threshold debe estar entre 0 y 255".into());
        }
        if self.polygon_tolerance < 0.0 {
            return Err("polygon_tolerance no puede ser negativa".into());
        }
        if self.scale_variants.is_empty() {
            return Err("scale_variants no puede estar vacío".into());
        }
        for s in &self.scale_variants {
            if *s <= 0.0 || *s > 1.0 {
                return Err(format!("scale_variants debe estar en (0, 1] (se obtuvo {s})"));
            }
        }
        if self.encryption_key.as_deref() == Some("") {
            return Err("encryption_key no puede ser una cadena vacía".into());
        }
        Ok(())
    }

    /// Serialize the config to a TOML string (`.tpproj` project file).
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Parse a `.tpproj` TOML project file.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_toml() {
        let cfg = ProjectConfig::default();
        let text = cfg.to_toml().unwrap();
        let back = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(back.max_texture_size, cfg.max_texture_size);
        assert_eq!(back.packing_strategy, cfg.packing_strategy);
        assert_eq!(back.encryption_key, cfg.encryption_key);
    }

    #[test]
    fn validation() {
        let mut cfg = ProjectConfig::default();
        cfg.max_texture_size = 1000;
        assert!(cfg.validate().is_err());
        cfg.max_texture_size = 4096;
        assert!(cfg.validate().is_ok());
        cfg.scale_variants = vec![];
        assert!(cfg.validate().is_err());
    }
}
