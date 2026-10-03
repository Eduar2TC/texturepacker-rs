use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProjectConfig, TrimMode};

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
}
