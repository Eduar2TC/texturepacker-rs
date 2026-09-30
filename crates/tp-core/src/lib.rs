//! # TexturePacker-RS core engine
//!
//! A parallel texture-atlas packing engine implementing the full pipeline of
//! the technical specification:
//!
//! 1. **Ingesta & Preprocesamiento** — parallel image loading, alpha trimming,
//!    pixel hashing / alias detection, normal-map pairing (`ingest`)
//! 2. **Motor Poligonal** — Marching Squares contour, Ramer–Douglas–Peucker
//!    simplification, ear-clipping triangulation (`polygon`)
//! 3. **Empaquetado Espacial** — MaxRects (BSSF/BAF/BLSF/Best/BottomLeft/
//!    ContactPoint), Guillotine, Grid & Basic, size search (Fast/Good/Best),
//!    size constraints (POT/MultipleOf4/WordAligned), fixed size, 90° rotation,
//!    multi-atlas split, polygon occupancy grid (`pack`)
//! 4. **Procesamiento de Píxeles** — extrude, rotation, color quantization
//!    with Floyd–Steinberg / Atkinson dithering, normal-map co-packing,
//!    pivots (`pixels`)
//! 5. **Exportación & Cifrado** — PNG / WebP / ASTC / ETC2 / PVRTC1 4bpp,
//!    AES-256-GCM encryption, Mustache metadata templates (`export`,
//!    `templates`)
//!
//! Entry point: [`pipeline::run`].

pub mod config;
pub mod dataformats;
pub mod dxt;
pub mod error;
pub mod etc2;
pub mod export;
pub mod hash;
pub mod ingest;
pub mod keys;
pub mod pack;
pub mod pipeline;
pub mod pixels;
pub mod polygon;
pub mod pvrtc;
pub mod reader;
pub mod split;
pub mod templates;
pub mod types;

pub use config::{
    AlphaHandling, BasicSortBy, ColorDepth, DitheringAlgorithm, GpuFormat, PackMode,
    PackingAlgorithm, PackingStrategy, ProjectConfig, ScaleMode, SizeConstraint, SortOrder,
    TemplateFormat,
};
pub use error::{Result, TpError};
pub use pipeline::{run, PipelineOutput};
pub use types::{PackResult, SpriteAsset};

/// Convenience: parse a `.tpproj` TOML project file.
pub fn load_project(path: &std::path::Path) -> Result<ProjectConfig> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| TpError::Other(format!("No se pudo leer {}: {e}", path.display())))?;
    ProjectConfig::from_toml(&text).map_err(|e| TpError::Other(format!("Proyecto inválido: {e}")))
}
