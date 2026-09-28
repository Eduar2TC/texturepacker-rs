//! Project configuration (`ProjectConfig`) with serde/TOML persistence.

use crate::error::{Result, TpError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

/// How transparent borders are handled before packing (trim mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TrimMode {
    /// Keep the sprites as they are — no transparent pixels are removed.
    #[serde(rename = "None")]
    None,
    /// Remove transparency; the engine restores the original size using
    /// `spriteSourceSize`/`sourceSize`.
    #[default]
    #[serde(rename = "Trim")]
    Trim,
    /// Remove transparency; the sprite renders smaller but its anchor keeps
    /// pointing at the same place in the original image.
    #[serde(rename = "CropKeepPos")]
    CropKeepPos,
    /// Remove transparency and flush the position to 0/0 — the sprite looks
    /// as if it never had transparency.
    #[serde(rename = "Crop")]
    Crop,
    /// Like `Trim`, but forces polygon outlines (mesh export + polygon packing).
    #[serde(rename = "Polygon")]
    Polygon,
}

impl TrimMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrimMode::None => "None",
            TrimMode::Trim => "Trim",
            TrimMode::CropKeepPos => "CropKeepPos",
            TrimMode::Crop => "Crop",
            TrimMode::Polygon => "Polygon",
        }
    }

    /// Parse a CLI/UX token (`none`, `trim`, `crop-keep-pos`, `crop`, `polygon`).
    pub fn parse(value: &str) -> Option<TrimMode> {
        match value.to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "none" => Some(TrimMode::None),
            "trim" => Some(TrimMode::Trim),
            "crop-keep-pos" | "cropkeeppos" => Some(TrimMode::CropKeepPos),
            "crop" => Some(TrimMode::Crop),
            "polygon" => Some(TrimMode::Polygon),
            _ => None,
        }
    }

    /// Whether transparency is removed at all.
    pub fn trims(&self) -> bool {
        !matches!(self, TrimMode::None)
    }
}

/// Output GPU texture format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GpuFormat {
    #[default]
    #[serde(rename = "PNG")]
    Png,
    /// PNG indexado de 8 bits (hasta 256 colores).
    #[serde(rename = "PNG8")]
    Png8,
    /// JPEG lossy sin canal alfa.
    #[serde(rename = "JPG")]
    Jpg,
    #[serde(rename = "WEBP", alias = "WebP")]
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
            GpuFormat::Png8 => "PNG8",
            GpuFormat::Jpg => "JPG",
            GpuFormat::WebP => "WebP",
            GpuFormat::Astc4x4 => "ASTC_4x4",
            GpuFormat::Etc2Rgba => "ETC2_RGBA",
            GpuFormat::Pvrtc4Bpp => "PVRTC_4BPP",
        }
    }

    /// Whether this build can encode the format (`ASTC_4x4` needs the
    /// `gpu-formats` feature; every other format is always available).
    pub fn is_supported(&self) -> bool {
        !matches!(self, GpuFormat::Astc4x4) || cfg!(feature = "gpu-formats")
    }

    /// Hardware-compressed formats (flip-y solo aplica a estos).
    pub fn is_hardware(&self) -> bool {
        matches!(
            self,
            GpuFormat::Astc4x4 | GpuFormat::Etc2Rgba | GpuFormat::Pvrtc4Bpp
        )
    }

    pub fn file_extension(&self) -> &'static str {
        match self {
            GpuFormat::Png | GpuFormat::Png8 => "png",
            GpuFormat::Jpg => "jpg",
            GpuFormat::WebP => "webp",
            GpuFormat::Astc4x4 => "astc",
            GpuFormat::Etc2Rgba => "ktx",
            GpuFormat::Pvrtc4Bpp => "pvr",
        }
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

/// Formato de píxel de salida para formatos de software (pixel format).
/// Los formatos de hardware comprimen RGBA y lo ignoran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PixelFormat {
    #[default]
    #[serde(rename = "RGBA8888")]
    Rgba8888,
    /// RGB sin alfa; la transparencia se compone sobre negro.
    #[serde(rename = "RGB888")]
    Rgb888,
    /// Solo el canal alfa (escala de grises).
    #[serde(rename = "ALPHA8")]
    Alpha8,
    /// Luminancia (escala de grises).
    #[serde(rename = "INTENSITY8")]
    Intensity8,
    /// Luminancia + alfa (grises + alfa).
    #[serde(rename = "ALPHA_INTENSITY8")]
    AlphaIntensity8,
    /// 16 bits: R5 G5 B5 + 1 bit de transparencia (RGBA5551).
    /// Los archivos siguen siendo PNG estándar con los colores reducidos a la
    /// rejilla 5-5-5-1 (expansión por replicación de bits).
    #[serde(rename = "RGBA5551")]
    Rgba5551,
    /// 20 bits: R5 G5 B5 + 5 bits de transparencia (RGBA5555). Como
    /// RGBA5551 pero el alfa se cuantiza a la rejilla de 5 bits en lugar de
    /// colapsar a 0/255.
    #[serde(rename = "RGBA5555")]
    Rgba5555,
    /// 32 bits con canales reordenados a B,G,R,A (BGRA8888); el PNG
    /// resultante lleva los canales R y B invertidos, para motores que cargan
    /// texturas en orden BGRA (p. ej. cocos2d).
    #[serde(rename = "BGRA8888")]
    Bgra8888,
}

impl PixelFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            PixelFormat::Rgba8888 => "RGBA8888",
            PixelFormat::Rgb888 => "RGB888",
            PixelFormat::Alpha8 => "ALPHA8",
            PixelFormat::Intensity8 => "INTENSITY8",
            PixelFormat::AlphaIntensity8 => "ALPHA_INTENSITY8",
            PixelFormat::Rgba5551 => "RGBA5551",
            PixelFormat::Rgba5555 => "RGBA5555",
            PixelFormat::Bgra8888 => "BGRA8888",
        }
    }
}

/// How transparent pixels are handled before packing (alpha handling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AlphaHandling {
    /// Transparent pixels are copied from sprite to sheet unchanged.
    #[default]
    #[serde(rename = "KeepTransparentPixels")]
    KeepTransparentPixels,
    /// Color values of transparent pixels are set to 0 (transparent black),
    /// improving packing ratio and identical sprite detection.
    #[serde(rename = "ClearTransparentPixels")]
    ClearTransparentPixels,
    /// Transparent pixels get the color of the nearest solid pixel
    /// (a.k.a. *alpha bleeding*), removing dark halos around sprites.
    #[serde(rename = "ReduceBorderArtifacts")]
    ReduceBorderArtifacts,
    /// All color values are multiplied with their alpha value
    /// (required by some frameworks for faster rendering).
    #[serde(rename = "PremultiplyAlpha")]
    PremultiplyAlpha,
}

impl AlphaHandling {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlphaHandling::KeepTransparentPixels => "KeepTransparentPixels",
            AlphaHandling::ClearTransparentPixels => "ClearTransparentPixels",
            AlphaHandling::ReduceBorderArtifacts => "ReduceBorderArtifacts",
            AlphaHandling::PremultiplyAlpha => "PremultiplyAlpha",
        }
    }

    /// Parse a CLI/UX token (`keep`, `clear`, `bleed`, `premultiply`).
    pub fn parse(value: &str) -> Option<AlphaHandling> {
        match value.to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "keep" | "keeppixels" | "keeppixel" | "keep-transparent-pixels" => {
                Some(AlphaHandling::KeepTransparentPixels)
            }
            "clear" | "clearpixels" | "clear-transparent-pixels" => {
                Some(AlphaHandling::ClearTransparentPixels)
            }
            "bleed" | "reduce" | "reduceborderartifacts" | "reduce-border-artifacts" => {
                Some(AlphaHandling::ReduceBorderArtifacts)
            }
            "premultiply" | "premultiplyalpha" | "premultiply-alpha" => {
                Some(AlphaHandling::PremultiplyAlpha)
            }
            _ => None,
        }
    }
}

/// Resampling algorithm used for scale variants (scale mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScaleMode {
    /// Bilinear blending — smooth output, best when scaling down.
    #[default]
    #[serde(rename = "Smooth")]
    Smooth,
    /// Nearest neighbour — keeps hard pixel edges.
    #[serde(rename = "Fast")]
    Fast,
}

impl ScaleMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScaleMode::Smooth => "Smooth",
            ScaleMode::Fast => "Fast",
        }
    }

    /// Parse a CLI/UX token (`smooth` | `fast`).
    pub fn parse(value: &str) -> Option<ScaleMode> {
        match value.to_ascii_lowercase().as_str() {
            "smooth" | "linear" => Some(ScaleMode::Smooth),
            "fast" | "nearest" | "nearestneighbour" => Some(ScaleMode::Fast),
            _ => None,
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
    /// MaxRects — tries every heuristic and keeps the tightest result.
    #[serde(rename = "Best")]
    Best,
    /// MaxRects — Bottom Left (keeps sprites low, then left).
    #[serde(rename = "BottomLeft")]
    BottomLeft,
    /// MaxRects — Contact Point (maximizes contact with placed sprites).
    #[serde(rename = "ContactPoint")]
    ContactPoint,
}

impl PackingStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            PackingStrategy::Bssf => "BSSF",
            PackingStrategy::Baf => "BAF",
            PackingStrategy::Blsf => "BLSF",
            PackingStrategy::Guillotine => "Guillotine",
            PackingStrategy::Best => "Best",
            PackingStrategy::BottomLeft => "BottomLeft",
            PackingStrategy::ContactPoint => "ContactPoint",
        }
    }

    /// Parse a CLI/UI token (case-insensitive, separators ignored).
    pub fn parse(value: &str) -> Option<Self> {
        let v: String = value
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        match v.as_str() {
            "bssf" | "shortsidefit" => Some(PackingStrategy::Bssf),
            "baf" | "areafit" => Some(PackingStrategy::Baf),
            "blsf" | "longsidefit" => Some(PackingStrategy::Blsf),
            "guillotine" => Some(PackingStrategy::Guillotine),
            "best" => Some(PackingStrategy::Best),
            "bottomleft" => Some(PackingStrategy::BottomLeft),
            "contactpoint" => Some(PackingStrategy::ContactPoint),
            _ => None,
        }
    }

    /// The concrete heuristics behind `Best`.
    pub fn all_heuristics() -> [PackingStrategy; 5] {
        [
            PackingStrategy::Bssf,
            PackingStrategy::Baf,
            PackingStrategy::Blsf,
            PackingStrategy::BottomLeft,
            PackingStrategy::ContactPoint,
        ]
    }
}

/// Packing algorithm. The polygon algorithm is selected
/// with `enable_polygon`, which takes precedence over this setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PackingAlgorithm {
    /// MaxRects with a placement heuristic (the default).
    #[default]
    #[serde(rename = "MaxRects")]
    MaxRects,
    /// Guillotine splitting.
    #[serde(rename = "Guillotine")]
    Guillotine,
    /// Regular grid: the largest sprite defines the cell size.
    #[serde(rename = "Grid")]
    Grid,
    /// Row based left-to-right layout (good for fixed-size sprites).
    #[serde(rename = "Basic")]
    Basic,
    /// Positions fixed by hand (GUI): `manual_positions[id] = (x, y)`.
    /// Sprites without an entry fall back to the Basic row layout.
    #[serde(rename = "Manual")]
    Manual,
    /// Polygon packing via MaxRects on bounding boxes + polygon occupancy
    /// Selected automatically when
    /// `trim_mode = "Polygon"`.
    #[serde(rename = "Polygon")]
    Polygon,
}

impl PackingAlgorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            PackingAlgorithm::MaxRects => "MaxRects",
            PackingAlgorithm::Guillotine => "Guillotine",
            PackingAlgorithm::Grid => "Grid",
            PackingAlgorithm::Basic => "Basic",
            PackingAlgorithm::Manual => "Manual",
            PackingAlgorithm::Polygon => "Polygon",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
            "maxrects" | "max" => Some(PackingAlgorithm::MaxRects),
            "polygon" => Some(PackingAlgorithm::Polygon),
            "guillotine" => Some(PackingAlgorithm::Guillotine),
            "grid" => Some(PackingAlgorithm::Grid),
            "basic" => Some(PackingAlgorithm::Basic),
            "manual" => Some(PackingAlgorithm::Manual),
            _ => None,
        }
    }
}

/// Atlas size constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SizeConstraint {
    /// Any size; the packer picks the smallest fitting dimensions.
    #[default]
    #[serde(rename = "AnySize")]
    AnySize,
    /// Power-of-two dimensions (2, 4, 8, 16, ...).
    #[serde(rename = "POT")]
    Pot,
    /// Dimensions that are multiples of 4.
    #[serde(rename = "MultipleOf4")]
    MultipleOf4,
    /// Rows fill complete memory words (width aligned to the pixel format).
    #[serde(rename = "WordAligned")]
    WordAligned,
}

impl SizeConstraint {
    pub fn as_str(&self) -> &'static str {
        match self {
            SizeConstraint::AnySize => "AnySize",
            SizeConstraint::Pot => "POT",
            SizeConstraint::MultipleOf4 => "MultipleOf4",
            SizeConstraint::WordAligned => "WordAligned",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
            "anysize" | "any" => Some(SizeConstraint::AnySize),
            "pot" | "poweroftwo" => Some(SizeConstraint::Pot),
            "multipleof4" | "mult4" | "4" => Some(SizeConstraint::MultipleOf4),
            "wordaligned" | "word" => Some(SizeConstraint::WordAligned),
            _ => None,
        }
    }
}

/// How much time is spent searching the minimum texture size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PackMode {
    /// Fastest: a single packing pass, results are cropped to fit.
    #[serde(rename = "Fast")]
    Fast,
    /// Searches the minimum size but aborts after a short budget.
    #[default]
    #[serde(rename = "Good")]
    Good,
    /// Searches intensively for the minimum size (may take longer).
    #[serde(rename = "Best")]
    Best,
}

impl PackMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PackMode::Fast => "Fast",
            PackMode::Good => "Good",
            PackMode::Best => "Best",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "fast" => Some(PackMode::Fast),
            "good" => Some(PackMode::Good),
            "best" => Some(PackMode::Best),
            _ => None,
        }
    }
}

/// Sort criterion for the `Basic` algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BasicSortBy {
    /// Tests all sorting variants and keeps the tightest result.
    #[default]
    #[serde(rename = "Best")]
    Best,
    #[serde(rename = "Name")]
    Name,
    #[serde(rename = "Width")]
    Width,
    #[serde(rename = "Height")]
    Height,
    #[serde(rename = "Area")]
    Area,
    #[serde(rename = "Circumference")]
    Circumference,
}

impl BasicSortBy {
    pub fn as_str(&self) -> &'static str {
        match self {
            BasicSortBy::Best => "Best",
            BasicSortBy::Name => "Name",
            BasicSortBy::Width => "Width",
            BasicSortBy::Height => "Height",
            BasicSortBy::Area => "Area",
            BasicSortBy::Circumference => "Circumference",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "best" => Some(BasicSortBy::Best),
            "name" => Some(BasicSortBy::Name),
            "width" => Some(BasicSortBy::Width),
            "height" => Some(BasicSortBy::Height),
            "area" => Some(BasicSortBy::Area),
            "circumference" | "peri" | "perimeter" => Some(BasicSortBy::Circumference),
            _ => None,
        }
    }

    /// Every sort criterion (used by `Best`).
    pub fn all() -> [BasicSortBy; 5] {
        [
            BasicSortBy::Name,
            BasicSortBy::Width,
            BasicSortBy::Height,
            BasicSortBy::Area,
            BasicSortBy::Circumference,
        ]
    }
}

/// Sort direction for the `Basic` algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SortOrder {
    #[default]
    #[serde(rename = "Ascending")]
    Ascending,
    #[serde(rename = "Descending")]
    Descending,
}

impl SortOrder {
    pub fn as_str(&self) -> &'static str {
        match self {
            SortOrder::Ascending => "Ascending",
            SortOrder::Descending => "Descending",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
            "ascending" | "asc" => Some(SortOrder::Ascending),
            "descending" | "desc" => Some(SortOrder::Descending),
            _ => None,
        }
    }

    pub fn all() -> [SortOrder; 2] {
        [SortOrder::Ascending, SortOrder::Descending]
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

/// Manual pack-by-folder group: sprites assigned to `name` are packed in
/// their own sheet(s) inside `<output_directory>/<name>/`, separate from the
/// main sheet. The default group (empty `name`) holds every sprite not
/// assigned elsewhere — its sheet stays in the output root. Sprite ids not
/// present in any group go to the default group as well.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FolderGroup {
    /// Sheet name and output subfolder. Empty = main sheet (output root).
    pub name: String,
    /// Sprite ids (normalized paths as in the pipeline) in this group.
    pub sprites: Vec<String>,
}

impl FolderGroup {
    /// Display name of the group in the GUI.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            "(hoja principal)"
        } else {
            &self.name
        }
    }
}

/// Optional snap grid for the Manual algorithm: while dragging in the GUI,
/// positions round to the nearest multiple of `step` on release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualGrid {
    /// Grid step in atlas pixels (1..=256).
    pub step: i32,
    /// When true, the free row-flow of the Manual algorithm also starts on
    /// the grid (multiples of `step` for the flow origin and row heights).
    pub snap_flow: bool,
}

impl ManualGrid {
    pub fn new(step: i32, snap_flow: bool) -> Self {
        Self { step, snap_flow }
    }

    /// Round `v` to the nearest multiple of the step (ties away from zero;
    /// negatives clamp up to 0).
    pub fn snap(&self, v: i32) -> i32 {
        let s = self.step.max(1);
        let r = v.rem_euclid(s);
        if r * 2 < s { v - r } else { v + (s - r) }.max(0)
    }

    /// `(x, y)` variant of [`Self::snap`].
    pub fn snap_pos(&self, pos: (i32, i32)) -> (i32, i32) {
        (self.snap(pos.0), self.snap(pos.1))
    }
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
    /// Which trim mode to apply when `enable_trim` is set.
    #[serde(default)]
    pub trim_mode: TrimMode,
    /// Transparent margin kept around the trimmed bounding box, in pixels.
    #[serde(default)]
    pub trim_margin: i32,
    /// Enable polygon (mesh) extraction and polygon-aware packing.
    pub enable_polygon: bool,
    /// Ramer-Douglas-Peucker simplification tolerance, in pixels.
    pub polygon_tolerance: f32,
    /// Deduplicate identical sprites via pixel hashing (alias detection).
    pub enable_aliasing: bool,
    pub color_depth: ColorDepth,
    pub dithering_algorithm: DitheringAlgorithm,
    pub gpu_format: GpuFormat,
    /// Esfuerzo de optimización PNG sin pérdida, 0-7. El valor 1 (por
    /// defecto) escribe PNG indexado de 8 bits
    /// cuando la imagen tiene 256 colores o menos.
    #[serde(default = "default_png_opt_level")]
    pub png_opt_level: u8,
    /// Dithering de la paleta al publicar PNG-8
    /// (PngQuant Low/Medium/High).
    #[serde(default)]
    pub png8_dither: PngDither,
    /// Calidad JPEG 0-100 (`--jpg-quality`; 80 por defecto).
    #[serde(default = "default_jpg_quality")]
    pub jpg_quality: u8,
    /// Calidad WebP: 0-100 = lossy, ≥101 = sin pérdidas
    /// (`--webp-quality`; por defecto sin pérdidas).
    #[serde(default = "default_webp_quality")]
    pub webp_quality: u16,
    /// Formato de píxel de salida (pixel format); solo formatos de
    /// software (PNG/PNG8/JPG/WebP).
    #[serde(default)]
    pub pixel_format: PixelFormat,
    /// Voltear la textura verticalmente (`--flip-y`); solo formatos
    /// de hardware (ASTC/ETC2/PVRTC).
    #[serde(default)]
    pub flip_vertical: bool,
    /// Optional AES-256-GCM key; texture files are encrypted when set.
    pub encryption_key: Option<String>,
    /// Path to a custom Mustache template; when `None` a built-in is used
    /// according to `template_format`.
    pub export_template: Option<PathBuf>,
    /// Metadata output language.
    pub template_format: TemplateFormat,
    /// Packing algorithm / heuristic.
    pub packing_strategy: PackingStrategy,
    /// Packing algorithm family. `enable_polygon` takes
    /// precedence; a legacy `packing_strategy = "Guillotine"` also selects it.
    #[serde(default)]
    pub algorithm: PackingAlgorithm,
    /// Effort spent searching the minimum atlas size.
    #[serde(default)]
    pub pack_mode: PackMode,
    /// Required atlas dimensions.
    #[serde(default)]
    pub size_constraints: SizeConstraint,
    /// Force the atlas to be square.
    #[serde(default)]
    pub force_squared: bool,
    /// Fixed atlas width; `0` lets the packer decide.
    #[serde(default)]
    pub fixed_width: i32,
    /// Fixed atlas height; `0` lets the packer decide.
    #[serde(default)]
    pub fixed_height: i32,
    /// Sort criterion for the `Basic` algorithm.
    #[serde(default)]
    pub basic_sort_by: BasicSortBy,
    /// Sort direction for the `Basic` algorithm.
    #[serde(default)]
    pub basic_order: SortOrder,
    /// Manual algorithm: atlas position (x, y) fixed by hand per sprite id,
    /// in trimmed sprite coordinates (as shown in the GUI preview). Sprites
    /// without an entry fall back to the Basic row layout.
    #[serde(default)]
    pub manual_positions: HashMap<String, (i32, i32)>,
    /// Optional snap grid for the Manual algorithm (None = free dragging).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_grid: Option<ManualGrid>,
    /// Pack-by-folder groups: one sheet per group inside its own output
    /// subfolder. The first entry is always the default/main group (empty
    /// name, output root); it also receives every unassigned sprite.
    #[serde(default = "default_folder_groups")]
    pub folder_groups: Vec<FolderGroup>,
    /// Automatic pack-by-folder (like the original TexturePacker): every
    /// input subfolder becomes an output subfolder — sprites in `<in>/ui/`
    /// land in `<out>/ui/atlas.png`, root-level sprites in `<out>/atlas.png`.
    /// Overrides manual groups when enabled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_folder_groups: bool,
    /// Scale variants to emit, e.g. `[1.0, 0.5]` produces `atlas.png` and
    /// `atlas-hd.png` (sufijos de variante tipo `-hd`, `@2x`...).
    pub scale_variants: Vec<f32>,
    /// Named variants (scaling variants): `(scale, name)` pairs whose
    /// name replaces the automatic `{v}` suffix (p. ej. `1.0 → -ipadhd`,
    /// `0.5 → -hd`). Empty = automatic suffixes.
    #[serde(default)]
    pub variant_names: Vec<(f32, String)>,
    /// Auto co-pack `*_normal.png` companions in the same frames.
    pub enable_normal_maps: bool,
    /// Default normalized pivot for all sprites.
    pub default_pivot_x: f32,
    pub default_pivot_y: f32,
    /// Per-channel tolerance (0-255) for automatic 9-patch border detection:
    /// two pixels count as "same color" when no channel differs more than this.
    #[serde(default)]
    pub detect_border_tolerance: i32,
    /// Max rows/columns inspected per side when auto-detecting 9-patch
    /// borders (0 = unlimited).
    #[serde(default = "default_detect_border_max_search")]
    pub detect_border_max_search: i32,
    /// Naming scheme for atlas pages: `atlas`, `atlas_1`, `atlas_2`, ...
    /// Placeholders `{n}` (índice desde 0), `{n1}` (desde 1) y `{v}`
    /// (sufijo de variante) se expanden al nombrar cada hoja y su data file.
    pub base_file_name: String,
    /// Allow emitting more than one sprite sheet when the sprites do not fit
    /// in a single texture (multipack). When `false`, such a pack
    /// fails with an error instead.
    #[serde(default = "default_true")]
    pub multipack: bool,
    /// Recurse into subdirectories of `input_directory`.
    pub recursive: bool,
    /// Extra sprite files or folders added on top of `input_directory`
    /// (the GUI's "Add sprites" / "Add smart folder" actions).
    #[serde(default)]
    pub extra_inputs: Vec<PathBuf>,
    /// Sprite files removed from the input set (the GUI's "Remove sprites"
    /// action). Matched against every discovered file path.
    #[serde(default)]
    pub excluded_inputs: Vec<PathBuf>,
    /// Space kept between the sprites and the border of the sprite sheet
    /// (border padding). Independent from `padding`.
    #[serde(default)]
    pub border_padding: i32,
    /// Extend sprite sizes (with transparency) to be divisible by this value
    /// (common divisor). `1` keeps sizes untouched.
    #[serde(default = "default_divisor")]
    pub common_divisor_x: i32,
    /// Same as [`Self::common_divisor_x`] for the vertical axis.
    #[serde(default = "default_divisor")]
    pub common_divisor_y: i32,
    /// Place the top-left corners of sprites on atlas coordinates divisible
    /// by this value (align to grid). `0` disables the option.
    #[serde(default)]
    pub align_to_grid: i32,
    /// How transparent pixels are handled before packing (alpha handling).
    #[serde(default)]
    pub alpha_handling: AlphaHandling,
    /// Resampling used when generating scale variants (scale mode).
    #[serde(default)]
    pub scale_mode: ScaleMode,
    /// Path prepended to the texture file name inside the metadata
    /// (texture path), e.g. `/assets`.
    #[serde(default)]
    pub texture_path: Option<String>,
    /// Remove image file extensions from the sprite names (trim sprite
    /// names). When `false` the names keep e.g. `.png`.
    #[serde(default = "default_true")]
    pub trim_sprite_names: bool,
    /// Prepend the smart folder's name to the sprite names of its files
    /// (prepend folder name).
    #[serde(default)]
    pub prepend_folder_name: bool,
    /// Group sprites sharing a base name plus numeric suffix into animations
    /// exposed in the metadata (auto-detect animations). Sprites like
    /// `walk_001.png`, `walk_002.png`, `walk_003.png` define `walk`.
    #[serde(default = "default_true")]
    pub enable_auto_detect_animations: bool,
}

fn default_detect_border_max_search() -> i32 {
    64
}

fn default_true() -> bool {
    true
}

fn default_png_opt_level() -> u8 {
    1
}

fn default_jpg_quality() -> u8 {
    80
}

fn default_webp_quality() -> u16 {
    101
}

fn default_divisor() -> i32 {
    1
}

fn gcd(a: i32, b: i32) -> i32 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

fn lcm(a: i32, b: i32) -> i32 {
    (a / gcd(a, b)).saturating_mul(b)
}

fn default_folder_groups() -> Vec<FolderGroup> {
    vec![FolderGroup::default()]
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
            trim_mode: TrimMode::default(),
            trim_margin: 0,
            enable_polygon: false,
            polygon_tolerance: 1.5,
            enable_aliasing: true,
            color_depth: ColorDepth::Rgba8888,
            dithering_algorithm: DitheringAlgorithm::FloydSteinberg,
            gpu_format: GpuFormat::Png,
            png_opt_level: 1,
            png8_dither: PngDither::default(),
            jpg_quality: 80,
            webp_quality: 101,
            pixel_format: PixelFormat::default(),
            flip_vertical: false,
            encryption_key: None,
            export_template: None,
            template_format: TemplateFormat::Json,
            packing_strategy: PackingStrategy::Bssf,
            algorithm: PackingAlgorithm::default(),
            pack_mode: PackMode::default(),
            size_constraints: SizeConstraint::default(),
            force_squared: false,
            fixed_width: 0,
            fixed_height: 0,
            basic_sort_by: BasicSortBy::default(),
            basic_order: SortOrder::default(),
            manual_positions: HashMap::new(),
            folder_groups: default_folder_groups(),
            scale_variants: vec![1.0],
            variant_names: Vec::new(),
            enable_normal_maps: true,
            default_pivot_x: 0.5,
            default_pivot_y: 0.5,
            detect_border_tolerance: 0,
            detect_border_max_search: 64,
            base_file_name: "atlas".to_string(),
            multipack: true,
            recursive: true,
            extra_inputs: Vec::new(),
            excluded_inputs: Vec::new(),
            border_padding: 0,
            common_divisor_x: 1,
            common_divisor_y: 1,
            align_to_grid: 0,
            alpha_handling: AlphaHandling::default(),
            scale_mode: ScaleMode::default(),
            texture_path: None,
            trim_sprite_names: true,
            prepend_folder_name: false,
            enable_auto_detect_animations: true,
            manual_grid: None,
            auto_folder_groups: false,
        }
    }
}

impl ProjectConfig {
    /// Trim mode actually applied by the pipeline: `enable_trim` acts as the
    /// master switch (docs treat *Trim* and *Trim mode* as one setting).
    pub fn effective_trim_mode(&self) -> TrimMode {
        if self.enable_trim {
            self.trim_mode
        } else {
            TrimMode::None
        }
    }

    /// Effective per-axis divisor: *Common divisor* and *Align to grid* both
    /// extend sprite sizes, so the effective value is their LCM.
    pub fn effective_divisors(&self) -> (i32, i32) {
        let align = self.align_to_grid.max(1);
        (
            lcm(self.common_divisor_x.max(1), align),
            lcm(self.common_divisor_y.max(1), align),
        )
    }

    /// Algorithm actually used by the packer. Selecting the *Polygon* trim
    /// mode switches the algorithm to *Polygon*
    /// automatically; the legacy `enable_polygon` / `--polygon` switches map
    /// to the same behavior, and a legacy `packing_strategy = "Guillotine"`
    /// still selects the Guillotine algorithm.
    pub fn effective_algorithm(&self) -> PackingAlgorithm {
        // Las mallas explícitas ganan a todo.
        if self.enable_polygon {
            return PackingAlgorithm::Polygon;
        }
        // Manual es una elección explícita del usuario: gana al «auto» del
        // trim mode Polygon (legacy), que solo aplica a los otros algoritmos.
        if self.algorithm == PackingAlgorithm::Manual {
            return PackingAlgorithm::Manual;
        }
        if self.effective_trim_mode() == TrimMode::Polygon {
            return PackingAlgorithm::Polygon;
        }
        if self.packing_strategy == PackingStrategy::Guillotine {
            PackingAlgorithm::Guillotine
        } else {
            self.algorithm
        }
    }

    /// Heuristic actually used by MaxRects (the legacy `Guillotine` strategy
    /// value is consumed by [`Self::effective_algorithm`]).
    pub fn effective_strategy(&self) -> PackingStrategy {
        match self.packing_strategy {
            PackingStrategy::Guillotine => PackingStrategy::Bssf,
            s => s,
        }
    }

    /// Width alignment (in pixels) for `WordAligned`: every row must fill
    /// complete memory words of `color_depth`.
    pub fn word_align_mod(&self) -> i32 {
        match self.color_depth.bytes_per_pixel() {
            1 => 4,
            2 => 2,
            3 => 4,
            _ => 1,
        }
    }

    /// Validate the configuration, returning a human-readable error on failure.
    pub fn validate(&self) -> Result<()> {
        if self.max_texture_size <= 0 || (self.max_texture_size & (self.max_texture_size - 1)) != 0
        {
            return Err(TpError::Config(format!(
                "max_texture_size debe ser una potencia de dos positiva (se obtuvo {})",
                self.max_texture_size
            )));
        }
        if self.padding < 0 || self.extrude < 0 || self.border_padding < 0 {
            return Err(TpError::Config(
                "padding, border_padding y extrude no pueden ser negativos".to_string(),
            ));
        }
        if self.border_padding * 2 >= self.max_texture_size {
            return Err(TpError::Config(format!(
                "border_padding ({}) deja el atlas interior vacío en un atlas de {}",
                self.border_padding, self.max_texture_size
            )));
        }
        for (name, value) in [
            ("common_divisor_x", self.common_divisor_x),
            ("common_divisor_y", self.common_divisor_y),
        ] {
            if !(1..=2048).contains(&value) {
                return Err(TpError::Config(format!(
                    "{name} debe estar entre 1 y 2048 (se obtuvo {value})"
                )));
            }
        }
        if !(0..=2048).contains(&self.align_to_grid) {
            return Err(TpError::Config(format!(
                "align_to_grid debe estar entre 0 y 2048 (se obtuvo {})",
                self.align_to_grid
            )));
        }
        if self.png_opt_level > 7 {
            return Err(TpError::Config(format!(
                "png_opt_level debe estar entre 0 y 7 (se obtuvo {})",
                self.png_opt_level
            )));
        }
        if self.jpg_quality > 100 {
            return Err(TpError::Config(format!(
                "jpg_quality debe estar entre 0 y 100 (se obtuvo {})",
                self.jpg_quality
            )));
        }
        let (div_x, div_y) = self.effective_divisors();
        if div_x > 2048 || div_y > 2048 {
            return Err(TpError::Config(format!(
                "El múltiplo común de common divisor y align_to_grid supera 2048 ({div_x}x{div_y})"
            )));
        }
        if let Some(path) = &self.texture_path {
            if path.trim().is_empty() {
                return Err(TpError::Config(
                    "texture_path no puede ser una cadena vacía".to_string(),
                ));
            }
        }
        // Umbral de transparencia admitido: 1 a 255.
        if !(1..=255).contains(&self.trim_threshold) {
            return Err(TpError::Config(
                "trim_threshold debe estar entre 1 y 255".to_string(),
            ));
        }
        if !(0..=256).contains(&self.trim_margin) {
            return Err(TpError::Config(
                "trim_margin debe estar entre 0 y 256".to_string(),
            ));
        }
        if self.polygon_tolerance < 0.0 {
            return Err(TpError::Config(
                "polygon_tolerance no puede ser negativa".to_string(),
            ));
        }
        if self.scale_variants.is_empty() {
            return Err(TpError::Config(
                "scale_variants no puede estar vacío".to_string(),
            ));
        }
        // La escala puede ser >1 (p. ej. @2x Retina) hasta 8.
        for s in &self.scale_variants {
            if *s <= 0.0 || *s > 8.0 {
                return Err(TpError::Config(format!(
                    "scale_variants debe estar en (0, 8] (se obtuvo {s})"
                )));
            }
        }
        for (s, name) in &self.variant_names {
            if *s <= 0.0 || *s > 8.0 {
                return Err(TpError::Config(format!(
                    "variant_names debe estar en (0, 8] (se obtuvo {s})"
                )));
            }
            if name.contains('/') || name.contains('\\') || name.contains("..") {
                return Err(TpError::Config(format!(
                    "variant_names no admite rutas (se obtuvo {name:?})"
                )));
            }
        }
        if self.encryption_key.as_deref() == Some("") {
            return Err(TpError::Config(
                "encryption_key no puede ser una cadena vacía".to_string(),
            ));
        }
        for (name, value) in [
            ("fixed_width", self.fixed_width),
            ("fixed_height", self.fixed_height),
        ] {
            if !(0..=8192).contains(&value) {
                return Err(TpError::Config(format!(
                    "{name} debe estar entre 0 (auto) y 8192 (se obtuvo {value})"
                )));
            }
        }
        if self.fixed_width > 0
            && self.fixed_height > 0
            && self.border_padding * 2 >= self.fixed_width.min(self.fixed_height)
        {
            return Err(TpError::Config(format!(
                "border_padding ({}) deja el interior vacío en un atlas fijo de {}x{}",
                self.border_padding, self.fixed_width, self.fixed_height
            )));
        }
        Ok(())
    }

    /// Serialize the config to a TOML string (`.tpproj` project file).
    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Parse a `.tpproj` TOML project file.
    ///
    /// Como `serde` aborta en el **primer** campo ausente, un proyecto
    /// incompleto daba errores de uno en uno («missing field input_directory»,
    /// corregir, volver a cargar, «missing field padding»...). Aquí se
    /// pre-analiza el TOML y se validan **todos** los campos obligatorios de
    /// una vez: un solo mensaje lista los que faltan.
    pub fn from_toml(text: &str) -> Result<Self> {
        let value: toml::Value = toml::from_str(text)?;
        if let Some(table) = value.as_table() {
            let missing: Vec<&str> = REQUIRED_TOML_FIELDS
                .iter()
                .copied()
                .filter(|f| !table.contains_key(*f))
                .collect();
            if !missing.is_empty() {
                return Err(TpError::Config(if missing.len() == 1 {
                    format!(
                        "falta un campo obligatorio en el proyecto TOML: `{}`",
                        missing[0]
                    )
                } else {
                    format!(
                        "faltan {} campos obligatorios en el proyecto TOML: {}",
                        missing.len(),
                        missing
                            .iter()
                            .map(|f| format!("`{f}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }));
            }
        }
        // Delegar en serde con la lista ya verificada: los errores restantes
        // son de tipo/valor con línea y columna del TOML.
        Ok(toml::from_str(text)?)
    }
}

/// Campos obligatorios de un `.tpproj`: todo lo que `ProjectConfig` serializa
/// sin `#[serde(default)]` ni `skip_serializing_if` y que no sea `Option`
/// (un `Option` ausente ya se deserializa como `None`, y `toml` ni siquiera
/// lo escribe cuando es `None`). Los campos con default quedan fuera a
/// propósito para que los proyectos antiguos sigan cargando.
///
/// El test `required_fields_list_is_honest` mantiene esta lista sincronizada
/// con el struct: si añades/quitas un campo sin default, el test falla y te
/// pide actualizarla (orden = orden de aparición en el struct).
const REQUIRED_TOML_FIELDS: &[&str] = &[
    "input_directory",
    "output_directory",
    "max_texture_size",
    "padding",
    "extrude",
    "allow_rotation",
    "enable_trim",
    "trim_threshold",
    "enable_polygon",
    "polygon_tolerance",
    "enable_aliasing",
    "color_depth",
    "dithering_algorithm",
    "gpu_format",
    "template_format",
    "packing_strategy",
    "scale_variants",
    "enable_normal_maps",
    "default_pivot_x",
    "default_pivot_y",
    "base_file_name",
    "recursive",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_grid_snap_rounds_to_multiples() {
        let g = ManualGrid::new(8, true);
        assert_eq!(g.snap(0), 0);
        assert_eq!(g.snap(3), 0);
        assert_eq!(g.snap(4), 8); // empate → arriba
        assert_eq!(g.snap(7), 8);
        assert_eq!(g.snap(16), 16);
        assert_eq!(g.snap(29), 32);
        assert_eq!(g.snap_pos((-3, 10)), (0, 8));
    }

    #[test]
    fn manual_grid_defaults_to_none_and_survives_toml() {
        let cfg = ProjectConfig::default();
        assert!(cfg.manual_grid.is_none());
        let cfg = ProjectConfig {
            manual_grid: Some(ManualGrid::new(16, false)),
            ..ProjectConfig::default()
        };
        let text = cfg.to_toml().unwrap();
        let back = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(back.manual_grid, Some(ManualGrid::new(16, false)));
        // Sin rejilla el TOML no la serializa (proyectos antiguos siguen
        // cargando igual).
        assert!(!ProjectConfig::default()
            .to_toml()
            .unwrap()
            .contains("manual_grid"));
    }

    #[test]
    fn missing_fields_are_reported_all_at_once() {
        // Proyecto mínimo con SOLO 3 campos: el error debe listar TODOS
        // los obligatorios ausentes de una vez (no el primero que serde
        // encuentre).
        let err = ProjectConfig::from_toml("input_directory = \"in\"\n").unwrap_err();
        let msg = err.to_string();
        for field in [
            "output_directory",
            "max_texture_size",
            "padding",
            "recursive",
        ] {
            assert!(
                msg.contains(field),
                "el error debería mencionar `{field}`: {msg}"
            );
        }
        // Un solo campo ausente → mensaje singular.
        let mut text = ProjectConfig::default().to_toml().unwrap();
        let line = text
            .lines()
            .find(|l| l.starts_with("recursive"))
            .map(str::to_string)
            .unwrap();
        text = text.replace(&line, "");
        let err = ProjectConfig::from_toml(&text).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("falta un campo obligatorio"),
            "mensaje singular esperado: {msg}"
        );
        // Un TOML que no es una tabla (p. ej. vacío) tampoco rompe: sin
        // campos no hay lista que verificar y serde da su error normal.
        assert!(ProjectConfig::from_toml("").is_err());
    }

    #[test]
    fn required_fields_list_is_honest() {
        // La lista REQUIRED_TOML_FIELDS debe describir la realidad de
        // serde: cada campo listado DEBE ser exigido por serde (quitarlo
        // del TOML rompe la carga), y nada fuera de la lista puede ser
        // obligatorio (proyectos antiguos deben seguir cargando).
        let cfg = ProjectConfig::default();
        let full = cfg.to_toml().unwrap();
        for field in REQUIRED_TOML_FIELDS {
            let line = full
                .lines()
                .find(|l| l.starts_with(*field))
                .unwrap_or_else(|| panic!("el campo {field} no aparece en el TOML generado"));
            let stripped = full.replace(line, "");
            let err = match ProjectConfig::from_toml(&stripped) {
                Ok(_) => panic!(
                    "{field} está en REQUIRED_TOML_FIELDS pero serde no lo exige (tiene default o es Option): sácalo de la lista"
                ),
                Err(e) => e.to_string(),
            };
            assert!(
                err.contains(field),
                "al quitar `{field}` el error no lo menciona: {err}"
            );
        }
    }

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
        let too_small = ProjectConfig {
            max_texture_size: 1000,
            ..ProjectConfig::default()
        };
        assert!(too_small.validate().is_err());

        let empty_variants = ProjectConfig {
            scale_variants: vec![],
            ..ProjectConfig::default()
        };
        assert!(empty_variants.validate().is_err());

        let bad_png_opt = ProjectConfig {
            png_opt_level: 8,
            ..ProjectConfig::default()
        };
        assert!(bad_png_opt.validate().is_err());

        let bad_jpg = ProjectConfig {
            jpg_quality: 101,
            ..ProjectConfig::default()
        };
        assert!(bad_jpg.validate().is_err());

        let ok = ProjectConfig {
            png_opt_level: 7,
            jpg_quality: 100,
            ..ProjectConfig::default()
        };
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn roundtrip_keeps_new_settings() {
        let cfg = ProjectConfig {
            border_padding: 8,
            common_divisor_x: 4,
            common_divisor_y: 2,
            align_to_grid: 8,
            alpha_handling: AlphaHandling::PremultiplyAlpha,
            scale_mode: ScaleMode::Fast,
            texture_path: Some("/assets".into()),
            trim_sprite_names: false,
            prepend_folder_name: true,
            multipack: false,
            png_opt_level: 4,
            png8_dither: PngDither::Low,
            jpg_quality: 90,
            webp_quality: 75,
            pixel_format: PixelFormat::Rgb888,
            flip_vertical: true,
            ..ProjectConfig::default()
        };

        let back = ProjectConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert_eq!(back.border_padding, 8);
        assert_eq!(back.common_divisor_x, 4);
        assert_eq!(back.common_divisor_y, 2);
        assert_eq!(back.align_to_grid, 8);
        assert_eq!(back.alpha_handling, AlphaHandling::PremultiplyAlpha);
        assert_eq!(back.scale_mode, ScaleMode::Fast);
        assert_eq!(back.texture_path.as_deref(), Some("/assets"));
        assert!(!back.trim_sprite_names);
        assert!(back.prepend_folder_name);
        assert!(!back.multipack);
        assert_eq!(back.png_opt_level, 4);
        assert_eq!(back.png8_dither, PngDither::Low);
        assert_eq!(back.jpg_quality, 90);
        assert_eq!(back.webp_quality, 75);
        assert_eq!(back.pixel_format, PixelFormat::Rgb888);
        assert!(back.flip_vertical);
    }

    #[test]
    fn legacy_project_defaults_for_new_settings() {
        // A `.tpproj` written before the Lote 5 fields must keep working.
        let cfg = ProjectConfig::default();
        let mut text = cfg.to_toml().unwrap();
        for field in [
            "border_padding",
            "common_divisor_x",
            "common_divisor_y",
            "align_to_grid",
            "alpha_handling",
            "scale_mode",
            "texture_path",
            "trim_sprite_names",
            "prepend_folder_name",
            "algorithm",
            "pack_mode",
            "size_constraints",
            "force_squared",
            "fixed_width",
            "fixed_height",
            "basic_sort_by",
            "basic_order",
            "multipack",
            "png_opt_level",
            "png8_dither",
            "jpg_quality",
            "webp_quality",
            "pixel_format",
            "flip_vertical",
        ] {
            if let Some(line) = text
                .lines()
                .find(|l| l.starts_with(field))
                .map(str::to_string)
            {
                text = text.replace(&line, "");
            }
        }
        assert!(!text.contains("border_padding"));
        let back = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(back.border_padding, 0);
        assert_eq!(back.common_divisor_x, 1);
        assert_eq!(back.common_divisor_y, 1);
        assert_eq!(back.align_to_grid, 0);
        assert_eq!(back.alpha_handling, AlphaHandling::KeepTransparentPixels);
        assert_eq!(back.scale_mode, ScaleMode::Smooth);
        assert_eq!(back.texture_path, None);
        assert!(back.trim_sprite_names);
        assert!(!back.prepend_folder_name);
        assert_eq!(back.algorithm, PackingAlgorithm::MaxRects);
        assert_eq!(back.pack_mode, PackMode::Good);
        assert_eq!(back.size_constraints, SizeConstraint::AnySize);
        assert!(!back.force_squared);
        assert_eq!(back.fixed_width, 0);
        assert_eq!(back.fixed_height, 0);
        assert_eq!(back.basic_sort_by, BasicSortBy::Best);
        assert_eq!(back.basic_order, SortOrder::Ascending);
        assert!(back.multipack);
        assert_eq!(back.png_opt_level, 1);
        assert_eq!(back.png8_dither, PngDither::High);
        assert_eq!(back.jpg_quality, 80);
        assert_eq!(back.webp_quality, 101);
        assert_eq!(back.pixel_format, PixelFormat::Rgba8888);
        assert!(!back.flip_vertical);
        assert!(back.validate().is_ok());
    }

    #[test]
    fn lote6_settings_roundtrip_and_parsing() {
        let cfg = ProjectConfig {
            algorithm: PackingAlgorithm::Grid,
            pack_mode: PackMode::Best,
            size_constraints: SizeConstraint::Pot,
            force_squared: true,
            fixed_width: 256,
            fixed_height: 128,
            basic_sort_by: BasicSortBy::Circumference,
            basic_order: SortOrder::Descending,
            ..ProjectConfig::default()
        };
        assert!(cfg.validate().is_ok());

        let back = ProjectConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert_eq!(back.algorithm, PackingAlgorithm::Grid);
        assert_eq!(back.pack_mode, PackMode::Best);
        assert_eq!(back.size_constraints, SizeConstraint::Pot);
        assert!(back.force_squared);
        assert_eq!(back.fixed_width, 256);
        assert_eq!(back.fixed_height, 128);
        assert_eq!(back.basic_sort_by, BasicSortBy::Circumference);
        assert_eq!(back.basic_order, SortOrder::Descending);

        assert_eq!(
            SizeConstraint::parse("multiple-of-4"),
            Some(SizeConstraint::MultipleOf4)
        );
        assert_eq!(
            SizeConstraint::parse("WordAligned"),
            Some(SizeConstraint::WordAligned)
        );
        assert_eq!(
            PackingAlgorithm::parse("basic"),
            Some(PackingAlgorithm::Basic)
        );
        assert_eq!(PackMode::parse("best"), Some(PackMode::Best));
        assert_eq!(
            BasicSortBy::parse("circumference"),
            Some(BasicSortBy::Circumference)
        );
        assert_eq!(SortOrder::parse("desc"), Some(SortOrder::Descending));
        assert_eq!(
            PackingStrategy::parse("bottom-left"),
            Some(PackingStrategy::BottomLeft)
        );
        assert_eq!(PackingStrategy::parse("nope"), None);

        // Effective algorithm: legacy Guillotine strategy and polygon override.
        let mut legacy = ProjectConfig {
            packing_strategy: PackingStrategy::Guillotine,
            ..ProjectConfig::default()
        };
        assert_eq!(legacy.effective_algorithm(), PackingAlgorithm::Guillotine);
        assert_eq!(legacy.effective_strategy(), PackingStrategy::Bssf);
        legacy.packing_strategy = PackingStrategy::Bssf;
        legacy.algorithm = PackingAlgorithm::Basic;
        legacy.enable_polygon = true;
        // Polygon packing (the Polygon algorithm) takes over.
        assert_eq!(legacy.effective_algorithm(), PackingAlgorithm::Polygon);
        // Trim mode Polygon also switches the algorithm automatically.
        let poly_trim = ProjectConfig {
            trim_mode: TrimMode::Polygon,
            ..ProjectConfig::default()
        };
        assert_eq!(poly_trim.effective_algorithm(), PackingAlgorithm::Polygon);
        let no_trim = ProjectConfig {
            trim_mode: TrimMode::Polygon,
            enable_trim: false, // sin trim, el modo no aplica
            ..ProjectConfig::default()
        };
        assert_eq!(no_trim.effective_algorithm(), PackingAlgorithm::MaxRects);
    }

    #[test]
    fn fixed_size_validation() {
        let mut cfg = ProjectConfig {
            fixed_width: 9999,
            ..ProjectConfig::default()
        };
        assert!(cfg.validate().is_err());
        cfg.fixed_width = 0;
        cfg.fixed_width = 256;
        cfg.fixed_height = 64;
        cfg.border_padding = 128;
        assert!(cfg.validate().is_err());
        cfg.border_padding = 8;
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.word_align_mod(), 1);
        cfg.color_depth = ColorDepth::Rgb565;
        assert_eq!(cfg.word_align_mod(), 2);
    }

    #[test]
    fn effective_divisors_combine_common_and_grid() {
        let mut cfg = ProjectConfig::default();
        assert_eq!(cfg.effective_divisors(), (1, 1));
        cfg.common_divisor_x = 4;
        cfg.align_to_grid = 6;
        // lcm(4, 6) = 12
        assert_eq!(cfg.effective_divisors(), (12, 6));
        cfg.align_to_grid = 0;
        assert_eq!(cfg.effective_divisors(), (4, 1));
        assert!(cfg.validate().is_ok());
        cfg.common_divisor_x = 0;
        assert!(cfg.validate().is_err());
        cfg.common_divisor_x = 4;
        cfg.border_padding = -1;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn texture_path_and_alpha_parsing() {
        let blank_texture_path = ProjectConfig {
            texture_path: Some("  ".into()),
            ..ProjectConfig::default()
        };
        assert!(blank_texture_path.validate().is_err());
        assert_eq!(
            AlphaHandling::parse("premultiply-alpha"),
            Some(AlphaHandling::PremultiplyAlpha)
        );
        assert_eq!(
            AlphaHandling::parse("reduce_border_artifacts"),
            Some(AlphaHandling::ReduceBorderArtifacts)
        );
        assert_eq!(AlphaHandling::parse("nope"), None);
        assert_eq!(ScaleMode::parse("fast"), Some(ScaleMode::Fast));
        assert_eq!(ScaleMode::parse("nope"), None);
    }
}
