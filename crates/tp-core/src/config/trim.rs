use serde::{Deserialize, Serialize};

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
