//! Project configuration (`ProjectConfig`) with serde/TOML persistence.

mod formats;
mod manual;
mod packing;
mod project;
mod quantization;
mod trim;
mod variants;

pub use formats::{GdxFilter, GpuFormat, PixelFormat, TemplateFormat};
pub use manual::{FolderGroup, ManualGrid};
pub use packing::{
    BasicSortBy, PackMode, PackingAlgorithm, PackingStrategy, SizeConstraint, SortOrder,
};
pub use project::ProjectConfig;
pub use quantization::{ColorDepth, DitheringAlgorithm, DxtMode, PngDither};
pub use trim::{AlphaHandling, TrimMode};
pub use variants::{scale_denominator, ScaleMode, VariantOptions, VariantPreset, VARIANT_PRESETS};

/// Glob match with `*` (any run, including `/`) and `?` (one character).
///
/// Shared with [`crate::ingest`] for the `--ignore-files` patterns.
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'*') => (0..=t.len()).any(|i| go(&p[1..], &t[i..])),
            Some(b'?') => !t.is_empty() && go(&p[1..], &t[1..]),
            Some(b) => t.first() == Some(b) && go(&p[1..], &t[1..]),
        }
    }
    go(pattern.as_bytes(), text.as_bytes())
}
