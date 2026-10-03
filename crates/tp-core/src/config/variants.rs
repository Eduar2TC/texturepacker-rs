use super::glob_match;
use super::project::default_true;
use serde::{Deserialize, Serialize};

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
    /// Scale2x (AdvMAME2x) — pixel-art upscaler for the exact factor 2.
    #[serde(rename = "Scale2x")]
    Scale2x,
    /// Scale3x (AdvMAME3x) — pixel-art upscaler for the exact factor 3.
    #[serde(rename = "Scale3x")]
    Scale3x,
    /// Scale4x — Scale2x applied twice, for the exact factor 4.
    #[serde(rename = "Scale4x")]
    Scale4x,
    /// Eagle — pixel-art upscaler for the exact factor 2.
    #[serde(rename = "Eagle")]
    Eagle,
}

impl ScaleMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScaleMode::Smooth => "Smooth",
            ScaleMode::Fast => "Fast",
            ScaleMode::Scale2x => "Scale2x",
            ScaleMode::Scale3x => "Scale3x",
            ScaleMode::Scale4x => "Scale4x",
            ScaleMode::Eagle => "Eagle",
        }
    }

    /// Exact integer factor the mode understands, or `None` for the generic
    /// resamplers (`Smooth`/`Fast`), which accept any factor.
    ///
    /// Pixel-art modes only run when the requested scale matches their factor;
    /// any other scale falls back to [`ScaleMode::Smooth`].
    pub fn required_factor(&self) -> Option<u32> {
        match self {
            ScaleMode::Smooth | ScaleMode::Fast => None,
            ScaleMode::Scale2x | ScaleMode::Eagle => Some(2),
            ScaleMode::Scale3x => Some(3),
            ScaleMode::Scale4x => Some(4),
        }
    }

    /// Parse a CLI/UX token (`smooth` | `fast` | `scale2x` | …).
    pub fn parse(value: &str) -> Option<ScaleMode> {
        match value.to_ascii_lowercase().as_str() {
            "smooth" | "linear" => Some(ScaleMode::Smooth),
            "fast" | "nearest" | "nearestneighbour" => Some(ScaleMode::Fast),
            "scale2x" | "scale2" => Some(ScaleMode::Scale2x),
            "scale3x" | "scale3" => Some(ScaleMode::Scale3x),
            "scale4x" | "scale4" => Some(ScaleMode::Scale4x),
            "eagle" => Some(ScaleMode::Eagle),
            _ => None,
        }
    }
}

/// Per-variant options of a scaling variant (diálogo «scaling variants»):
/// qué sprites empaqueta la variante, su tope de tamaño de
/// textura y si reutiliza la hoja base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantOptions {
    /// Scale these options configure; must match an entry of
    /// [`ProjectConfig::scale_variants`](crate::config::ProjectConfig::scale_variants)
    /// (entries for other scales are ignored and rejected by
    /// [`ProjectConfig::validate`](crate::config::ProjectConfig::validate)).
    pub scale: f32,
    /// Comma-separated include patterns (`*` = any run of characters,
    /// `?` = one character) matched against the sprite id and its file
    /// name. Empty = every sprite belongs to the variant.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sprite_filter: String,
    /// Canvas cap for this variant (absent/0 = the project's own
    /// `max_texture_size`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_texture_size: Option<i32>,
    /// Reuse the base sheet scaled instead of packing from scratch. A
    /// sprite filter or a different max texture size forces a fresh pack
    /// either way: neither can be honoured by scaling the base sheet.
    #[serde(default = "default_true")]
    pub force_identical_layout: bool,
    /// «Identical layout — accept fractional values»: la
    /// variante se queda **fuera** del común divisor, así que su hoja
    /// idéntica admite subpíxeles (aquí se redondean) a cambio de no
    /// estirar el resto de variantes para encajar su denominador.
    #[serde(default)]
    pub accept_fractional: bool,
}

/// Smallest denominator `q <= 64` such that `scale` is (almost) `p/q` — the
/// factor the base sheet has to be divisible by so rescaling it to this
/// scale stays on integers. `None` when the scale is not representable
/// (e.g. `0.999`), which is the case solved with «accept fractional
/// values».
pub fn scale_denominator(scale: f32) -> Option<i32> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    (1..=64).find(|&q| {
        let p = (scale * q as f32).round();
        p >= 1.0 && (p / q as f32 - scale).abs() <= 5e-4
    })
}

/// `lcm(a, b)` when it fits in `cap`, `cap` otherwise (a divisor bigger than
/// the validated maximum would only be rejected downstream).
pub(super) fn lcm_capped(a: i32, b: i32, cap: i32) -> i32 {
    if a <= 0 || b <= 0 {
        return a.max(b).max(1);
    }
    let gcd = |mut x: i32, mut y: i32| {
        while y != 0 {
            let t = x % y;
            x = y;
            y = t;
        }
        x.abs()
    };
    let l = a as i64 * b as i64 / gcd(a, b) as i64;
    if l > cap as i64 {
        cap
    } else {
        l as i32
    }
}

/// Preset of the scaling variants dialog: a name plus the scale/suffix
/// pairs it applies (`fractional` marks the scales that must opt out of the
/// common divisor).
#[derive(Debug, Clone, Copy)]
pub struct VariantPreset {
    pub name: &'static str,
    pub variants: &'static [(f32, &'static str)],
    pub fractional: &'static [f32],
}

/// Presets shipped with the scaling variants dialog. `apply` overwrites the
/// current variants, exactly like pressing *Apply*.
pub const VARIANT_PRESETS: &[VariantPreset] = &[
    VariantPreset {
        name: "Ninguna",
        variants: &[(1.0, "")],
        fractional: &[],
    },
    VariantPreset {
        name: "iPad + iPhone (documentación)",
        variants: &[(1.0, "-ipadhd"), (0.5, "-hd"), (0.25, "")],
        fractional: &[],
    },
    VariantPreset {
        name: "Retina @1x / @2x / @3x",
        variants: &[(1.0, ""), (2.0, "@2x"), (3.0, "@3x")],
        fractional: &[],
    },
    VariantPreset {
        name: "Android mdpi … xxxhdpi",
        variants: &[
            (1.0, "-mdpi"),
            (1.5, "-hdpi"),
            (2.0, "-xhdpi"),
            (3.0, "-xxhdpi"),
            (4.0, "-xxxhdpi"),
        ],
        fractional: &[],
    },
    VariantPreset {
        name: "Descuentos 1/2, 1/3 y 1/4",
        variants: &[
            (1.0, ""),
            (0.5, "-half"),
            (1.0 / 3.0, "-third"),
            (0.25, "-quarter"),
        ],
        fractional: &[1.0 / 3.0],
    },
];

impl Default for VariantOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            sprite_filter: String::new(),
            max_texture_size: None,
            force_identical_layout: true,
            accept_fractional: false,
        }
    }
}

impl VariantOptions {
    /// True when the sprite identified by `names` (id and file name) belongs
    /// to this variant. An empty filter includes everything.
    pub fn includes(&self, names: &[&str]) -> bool {
        let filter = self.sprite_filter.trim();
        if filter.is_empty() {
            return true;
        }
        filter.split(',').any(|pat| {
            let pat = pat.trim();
            !pat.is_empty() && names.iter().any(|n| glob_match(pat, n))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_denominator_covers_the_usual_and_rejects_the_odd() {
        assert_eq!(scale_denominator(1.0), Some(1));
        assert_eq!(scale_denominator(2.0), Some(1));
        assert_eq!(scale_denominator(4.0), Some(1));
        assert_eq!(scale_denominator(0.5), Some(2));
        assert_eq!(scale_denominator(1.5), Some(2));
        assert_eq!(scale_denominator(0.25), Some(4));
        assert_eq!(scale_denominator(0.75), Some(4));
        assert_eq!(scale_denominator(0.333), Some(3));
        assert_eq!(scale_denominator(0.999), None);
        assert_eq!(scale_denominator(0.0), None);
    }

    #[test]
    fn variant_filter_matches_ids_and_file_names() {
        let opts = VariantOptions {
            scale: 0.5,
            sprite_filter: "hero*, coin".into(),
            ..VariantOptions::default()
        };
        assert!(opts.includes(&["hero", "hero.png"]));
        assert!(opts.includes(&["hero_alt", "hero_alt.png"]));
        assert!(opts.includes(&["coin", "coin.png"]));
        assert!(!opts.includes(&["bg", "bg.png"]));
        // Filtro vacío: todos los sprites pertenecen a la variante.
        assert!(VariantOptions::default().includes(&["cualquiera", "cualquiera.png"]));
        // `?` cuenta exactamente un carácter y basta con que el patrón
        // coincida con uno de los dos nombres (id o fichero).
        let one = VariantOptions {
            scale: 1.0,
            sprite_filter: "b?g".into(),
            ..VariantOptions::default()
        };
        assert!(one.includes(&["bug", "bug.png"]));
        assert!(!one.includes(&["bg", "bg.png"]));
        assert!(!one.includes(&["bigger", "bigger.png"]));
        // Varios patrones separados por coma: basta con que coincida uno,
        // con cualquiera de los dos nombres del sprite.
        assert!(!one.includes(&["zzz", "zzz.png"]));
        let two = VariantOptions {
            scale: 1.0,
            sprite_filter: "hero*, coin".into(),
            ..VariantOptions::default()
        };
        assert!(two.includes(&["coin", "coin.png"]));
        assert!(two.includes(&["hero_alt", "hero_alt.png"]));
        assert!(!two.includes(&["zzz", "zzz.png"]));
    }
}
