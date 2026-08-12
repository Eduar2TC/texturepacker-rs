//! Subsystem 1: Ingesta & Preprocesamiento.
//!
//! - Parallel loading of images (PNG / WebP / JPEG)
//! - Alpha trimming & bounding-box computation
//! - Pixel hashing for alias detection
//! - Normal-map (`*_normal.*`) pairing
//! - Per-sprite pivot overrides (`pivots.json` in the input directory)

use crate::hash::{hash_pixels_rgba, AliasTable};
use crate::types::Rect;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A sprite loaded and preprocessed (trimmed), before packing.
#[derive(Debug, Clone)]
pub struct IngestedSprite {
    pub id: String,
    pub source_path: PathBuf,
    pub raw_width: i32,
    pub raw_height: i32,
    /// Bounding box of visible pixels in the original image.
    pub trimmed_bounds: Rect,
    /// Trimmed RGBA pixels (width = trimmed_bounds.width, height = trimmed_bounds.height).
    pub pixels: Vec<u8>,
    pub pixel_hash: String,
    pub normal_path: Option<PathBuf>,
}

/// Result of the ingest stage.
#[derive(Debug, Default)]
pub struct IngestResult {
    pub sprites: Vec<IngestedSprite>,
    pub warnings: Vec<String>,
}

/// Normalized pivot overrides: sprite id -> (x, y) in 0..=1.
pub type PivotOverrides = HashMap<String, crate::types::Point2D>;

/// Discover image files under `dir` (recursively when `recursive`).
fn discover_images(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    stack.push(path);
                }
            } else if is_image_file(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn is_image_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
        Some(e) if matches!(e.as_str(), "png" | "webp" | "jpg" | "jpeg")
    )
}

/// Load one image file into RGBA8 pixels. Returns `Err` with a message.
pub fn load_image_rgba(path: &Path) -> Result<(i32, i32, Vec<u8>), String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((w as i32, h as i32, rgba.into_raw()))
}

/// Compute the bounding box of pixels with alpha > `threshold` and return the
/// trimmed buffer. `(0,0,0,0)` bounds mean the sprite is fully transparent.
pub fn trim_rgba(
    pixels: &[u8],
    width: i32,
    height: i32,
    threshold: u8,
) -> (Rect, Vec<u8>) {
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = -1i32;
    let mut max_y = -1i32;

    for y in 0..height {
        for x in 0..width {
            let a = pixels[((y * width + x) * 4 + 3) as usize];
            if a > threshold {
                if x < min_x {
                    min_x = x;
                }
                if x > max_x {
                    max_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if y > max_y {
                    max_y = y;
                }
            }
        }
    }

    if max_x < 0 {
        return (Rect::default(), Vec::new());
    }

    let tw = max_x - min_x + 1;
    let th = max_y - min_y + 1;
    let mut trimmed = vec![0u8; (tw * th * 4) as usize];
    for y in 0..th {
        let src_row = (min_y + y) * width + min_x;
        let dst_row = y * tw;
        trimmed[(dst_row * 4) as usize..((dst_row + tw) * 4) as usize]
            .copy_from_slice(&pixels[(src_row * 4) as usize..((src_row + tw) * 4) as usize]);
    }

    (
        Rect::new(min_x, min_y, tw, th),
        trimmed,
    )
}

/// Load `pivots.json` (map of sprite id -> {x, y} normalized 0..1) from `dir`,
/// if present.
pub fn load_pivot_overrides(dir: &Path) -> Result<PivotOverrides, String> {
    let path = dir.join("pivots.json");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("pivots.json: {e}"))?;
    let map: HashMap<String, crate::types::Point2D> =
        serde_json::from_str(&text).map_err(|e| format!("pivots.json: {e}"))?;
    Ok(map)
}

/// Main ingest entry point.
///
/// Loads every image under `input_directory` (in parallel), trims it, hashes
/// it, and pairs normal maps. `*_normal.*` files are *not* returned as sprites
/// when `enable_normal_maps` is set — they are attached as companions.
pub fn ingest(
    input_directory: &Path,
    trim_threshold: i32,
    enable_normal_maps: bool,
    recursive: bool,
) -> IngestResult {
    let mut result = IngestResult::default();
    let threshold = trim_threshold.clamp(0, 255) as u8;

    let files = discover_images(input_directory, recursive);
    if files.is_empty() {
        result
            .warnings
            .push(format!("No se encontraron imágenes en {}", input_directory.display()));
        return result;
    }

    // Build a map base -> path so we can find `foo_normal.png` for `foo.png`.
    let normal_candidates: HashMap<String, PathBuf> = if enable_normal_maps {
        files
            .iter()
            .filter(|p| is_normal_file(p))
            .filter_map(|p| {
                file_stem(p).map(|s| {
                    let base = s.strip_suffix("_normal").unwrap_or(&s).to_string();
                    (base, p.clone())
                })
            })
            .collect()
    } else {
        HashMap::new()
    };

    // Parallel load + trim + hash.
    let loaded: Vec<Result<Option<IngestedSprite>, String>> = files
        .par_iter()
        .filter(|p| !(enable_normal_maps && is_normal_file(p)))
        .map(|path| {
            let (w, h, rgba) = load_image_rgba(path)?;
            let (bounds, trimmed) = trim_rgba(&rgba, w, h, threshold);
            if trimmed.is_empty() {
                return Ok(None); // fully transparent: skip
            }
            let id = file_stem(path).unwrap_or_else(|| "sprite".to_string());
            let normal_path = normal_candidates.get(&id).cloned();
            let hash = hash_pixels_rgba(&trimmed);
            Ok(Some(IngestedSprite {
                id,
                source_path: path.clone(),
                raw_width: w,
                raw_height: h,
                trimmed_bounds: bounds,
                pixels: trimmed,
                pixel_hash: hash,
                normal_path,
            }))
        })
        .collect();

    let mut used_normals: Vec<PathBuf> = Vec::new();
    for item in loaded {
        match item {
            Err(e) => result.warnings.push(e),
            Ok(None) => {}
            Ok(Some(sprite)) => {
                if let Some(n) = &sprite.normal_path {
                    used_normals.push(n.clone());
                }
                result.sprites.push(sprite);
            }
        }
    }

    if enable_normal_maps {
        // Warn about normal maps without a matching diffuse.
        for path in files.iter().filter(|p| is_normal_file(p)) {
            if !used_normals.contains(path) {
                result
                    .warnings
                    .push(format!("Mapa de normales sin difusa asociada: {}", path.display()));
            }
        }
    }

    result
}

fn is_normal_file(path: &Path) -> bool {
    let Some(stem) = file_stem(path) else {
        return false;
    };
    stem.ends_with("_normal")
}

/// File stem without extension (lowercased handling kept as-is).
fn file_stem(path: &Path) -> Option<String> {
    path.file_stem().and_then(|s| s.to_str()).map(String::from)
}

/// Resolve aliases: sprites whose trimmed pixels are byte-identical become
/// aliases of the first occurrence. Returns (is_alias, alias_target_id) per
/// sprite, in the same order as `sprites`.
pub fn resolve_aliases(sprites: &[IngestedSprite]) -> Vec<(bool, Option<String>)> {
    let mut table = AliasTable::new();
    sprites
        .iter()
        .map(|s| {
            let tw = s.trimmed_bounds.width as usize;
            let th = s.trimmed_bounds.height as usize;
            match table.lookup_or_register(&s.pixel_hash, &s.id, &s.pixels, tw, th) {
                Some(owner) => (true, Some(owner)),
                None => (false, None),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(w: i32, h: i32, solid: impl Fn(i32, i32) -> [u8; 4]) -> Vec<u8> {
        let mut buf = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                buf.extend_from_slice(&solid(x, y));
            }
        }
        buf
    }

    #[test]
    fn trim_finds_bounds() {
        // 8x8 with a visible 3x2 blob at (2,3).
        let buf = rgba(8, 8, |x, y| {
            if (2..5).contains(&x) && (3..5).contains(&y) {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let (bounds, trimmed) = trim_rgba(&buf, 8, 8, 1);
        assert_eq!(bounds, Rect::new(2, 3, 3, 2));
        assert_eq!(trimmed.len(), 3 * 2 * 4);
        assert_eq!(&trimmed[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn trim_threshold_ignores_faint_alpha() {
        // alpha == threshold is not considered visible (strictly greater).
        let buf = rgba(4, 4, |_, _| [10, 10, 10, 1]);
        let (bounds, trimmed) = trim_rgba(&buf, 4, 4, 1);
        assert!(trimmed.is_empty());
        assert_eq!(bounds, Rect::default());
    }

    #[test]
    fn fully_transparent_is_empty() {
        let buf = rgba(4, 4, |_, _| [0, 0, 0, 0]);
        let (_, trimmed) = trim_rgba(&buf, 4, 4, 1);
        assert!(trimmed.is_empty());
    }

    #[test]
    fn aliases_resolve_to_first() {
        let mk = |color: [u8; 4]| IngestedSprite {
            id: "x".into(),
            source_path: PathBuf::new(),
            raw_width: 2,
            raw_height: 2,
            trimmed_bounds: Rect::new(0, 0, 2, 2),
            pixels: rgba(2, 2, |_, _| color),
            pixel_hash: String::new(),
            normal_path: None,
        };
        let mut a = mk([255, 0, 0, 255]);
        let b = mk([255, 0, 0, 255]);
        let c = mk([0, 255, 0, 255]);
        a.id = "a".into();
        a.pixel_hash = hash_pixels_rgba(&a.pixels);
        let mut sprites = vec![a, b, c];
        sprites[1].id = "b".into();
        sprites[1].pixel_hash = hash_pixels_rgba(&sprites[1].pixels);
        sprites[2].id = "c".into();
        sprites[2].pixel_hash = hash_pixels_rgba(&sprites[2].pixels);

        let aliases = resolve_aliases(&sprites);
        assert_eq!(aliases[0], (false, None));
        assert_eq!(aliases[1], (true, Some("a".into())));
        assert_eq!(aliases[2], (false, None));
    }
}
