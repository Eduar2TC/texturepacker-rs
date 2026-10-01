//! Subsystem 1: Ingesta & Preprocesamiento.
//!
//! - Parallel loading of images (PNG / WebP / JPEG)
//! - Alpha trimming & bounding-box computation
//! - Pixel hashing for alias detection
//! - Normal-map (`*_normal.*`) pairing
//! - Per-sprite pivot overrides (`pivots.json` in the input directory)

use crate::config::{glob_match, TrimMode};
use crate::error::{Result, TpError};
use crate::hash::{hash_pixels_rgba, AliasTable};
use crate::types::Rect;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
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

/// Discovery & loading options for [`ingest`].
#[derive(Debug, Clone)]
pub struct IngestOptions<'a> {
    /// Root directory scanned first.
    pub input_directory: &'a Path,
    /// Alpha threshold (0-255) below which a pixel counts as transparent.
    pub trim_threshold: i32,
    /// Trim mode; [`TrimMode::None`] keeps the whole image untouched.
    pub trim_mode: TrimMode,
    /// Transparent margin kept around the trimmed bounding box.
    pub trim_margin: i32,
    /// Pair `*_normal.*` companions instead of treating them as sprites.
    pub enable_normal_maps: bool,
    /// Suffix of the normal-map file of a sprite (`hero` → `hero<n>`); empty
    /// disables matching by name.
    pub normal_map_suffix: String,
    /// Substring the relative path must contain for the file to be treated as
    /// a normal map (e.g. `normals/`); empty disables the path filter.
    pub normal_map_filter: String,
    /// Classify the remaining images as normal maps from their color.
    pub normal_map_auto_detect: bool,
    /// Recurse into subdirectories of `input_directory`.
    pub recursive: bool,
    /// Extra sprite files or folders added on top of `input_directory`
    /// (folders are always scanned recursively).
    pub extra_inputs: &'a [PathBuf],
    /// Files to skip, whatever their origin.
    pub excluded_inputs: &'a [PathBuf],
    /// Remove image file extensions from sprite ids (trim sprite
    /// names). When `false` the id keeps e.g. `.png`.
    pub trim_sprite_names: bool,
    /// Prepend the smart folder's name to the ids of the files inside it
    /// (prepend folder name).
    pub prepend_folder_name: bool,
    /// Extend sprite sizes (with transparency) to be divisible by this value
    /// (common divisor). `1` leaves sizes untouched.
    pub common_divisor_x: i32,
    /// Same as [`Self::common_divisor_x`] for the vertical axis.
    pub common_divisor_y: i32,
    /// Wildcard patterns (`*` and `?`, `/` included) of paths left out of the
    /// atlas (`--ignore-files`). Matched against the path relative to its
    /// root, the absolute path and the bare file name.
    pub ignore_patterns: &'a [String],
    /// Name substitutions (`--replace`) applied to every sprite id, in order,
    /// after it is computed. Invalid patterns are skipped.
    pub name_replacements: &'a [(String, String)],
    /// Turn the flat colour of a fully opaque sprite into transparency
    /// (`--heuristic-mask`), before trimming.
    pub heuristic_mask: bool,
}

/// Normalized pivot overrides: sprite id -> (x, y) in 0..=1.
pub type PivotOverrides = HashMap<String, crate::types::Point2D>;

/// Canonical path used to compare sprites across roots and exclusion lists.
pub fn normalize_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn walk_images(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                walk_images(&path, recursive, out);
            }
        } else if is_image_file(&path) {
            out.push(path);
        }
    }
}

/// Discover image files from `input_directory` plus `extra_inputs`, skipping
/// `excluded_inputs` and de-duplicating across roots.
fn discover_images(options: &IngestOptions) -> Vec<PathBuf> {
    let mut collected: Vec<PathBuf> = Vec::new();
    if !options.input_directory.as_os_str().is_empty() {
        if options.input_directory.is_file() {
            if is_image_file(options.input_directory) {
                collected.push(options.input_directory.to_path_buf());
            }
        } else {
            walk_images(options.input_directory, options.recursive, &mut collected);
        }
    }
    for extra in options.extra_inputs {
        if extra.is_dir() {
            walk_images(extra, true, &mut collected);
        } else if is_image_file(extra) {
            collected.push(extra.clone());
        }
    }

    let excluded: HashSet<PathBuf> = options
        .excluded_inputs
        .iter()
        .map(|p| normalize_path(p))
        .collect();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out: Vec<PathBuf> = Vec::new();
    for path in collected {
        let norm = normalize_path(&path);
        if excluded.contains(&norm) || !seen.insert(norm) {
            continue;
        }
        if matches_ignore_patterns(&path, options) {
            continue;
        }
        out.push(path);
    }
    out.sort();
    out
}

/// True when any `--ignore-files` wildcard matches the path, tried against
/// the path relative to each root, the path as discovered and the bare file
/// name (so `*.tmp` and `*/drafts/*` both work).
fn matches_ignore_patterns(path: &Path, options: &IngestOptions) -> bool {
    if options.ignore_patterns.is_empty() {
        return false;
    }
    let posix = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let mut candidates = vec![posix(path)];
    if let Some(name) = path.file_name() {
        candidates.push(name.to_string_lossy().into_owned());
    }
    for root in std::iter::once(options.input_directory)
        .chain(options.extra_inputs.iter().map(PathBuf::as_path))
    {
        if let Ok(rel) = path.strip_prefix(root) {
            if !rel.as_os_str().is_empty() {
                candidates.push(posix(rel));
            }
        }
    }
    options.ignore_patterns.iter().any(|pattern| {
        candidates
            .iter()
            .any(|c| glob_match(&posix(Path::new(pattern)), c))
    })
}

pub fn is_image_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        // `.pvr.gz`: la extensión es `gz`, así que manda el nombre base.
        Some("gz") => path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.to_ascii_lowercase().ends_with(".pvr")),
        Some(e) => matches!(
            e,
            "png"
                | "webp"
                | "jpg"
                | "jpeg"
                | "tga"
                | "bmp"
                | "gif"
                | "ico"
                | "tiff"
                | "tif"
                | "dds"
                | "qoi"
                | "pbm"
                | "pgm"
                | "ppm"
                | "pnm"
                | "xbm"
                | "xpm"
                | "astc"
                | "ktx"
                | "ktx2"
                | "basis"
                | "psd"
                | "svg"
                | "svgz"
                | "pkm"
                | "pvr"
                | "pvrtc"
                | "ccz"
        ),
        None => false,
    }
}

/// Load one image file into RGBA8 pixels. Returns `Err` with a message.
///
/// Formats `image` cannot read (XBM, XPM, the GPU containers and `.basis`) are
/// handled by [`crate::reader`].
pub fn load_image_rgba(path: &Path) -> Result<(i32, i32, Vec<u8>)> {
    crate::reader::load_image_rgba(path)
}

/// Compute the bounding box of pixels with alpha > `threshold` and return the
/// trimmed buffer. `(0,0,0,0)` bounds mean the sprite is fully transparent.
/// `--heuristic-mask`: for a sprite without transparency, drop the flat
/// colour of its border (the background it was drawn on) by turning every
/// pixel of that colour transparent. Returns `true` when anything changed.
///
/// The most frequent colour on the border ring wins, so a frame with a few
/// antialiased edge pixels still masks cleanly. Runs before trimming, which
/// then crops the newly transparent border away.
pub fn heuristic_mask_rgba(pixels: &mut [u8], width: i32, height: i32) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }
    // A sprite that already has transparency has nothing to guess.
    if pixels.chunks_exact(4).any(|px| px[3] != 255) {
        return false;
    }
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for y in 0..height {
        for x in 0..width {
            if x != 0 && y != 0 && x != width - 1 && y != height - 1 {
                continue;
            }
            let i = ((y * width + x) * 4) as usize;
            *counts
                .entry([pixels[i], pixels[i + 1], pixels[i + 2]])
                .or_default() += 1;
        }
    }
    let Some((color, _)) = counts.into_iter().max_by_key(|(_, n)| *n) else {
        return false;
    };
    let mut changed = false;
    for px in pixels.chunks_exact_mut(4) {
        if [px[0], px[1], px[2]] == color {
            px[3] = 0;
            changed = true;
        }
    }
    changed
}

/// Compute the bounding box of pixels with alpha > `threshold` and return the
/// trimmed buffer. `(0,0,0,0)` bounds mean the sprite is fully transparent.
pub fn trim_rgba(pixels: &[u8], width: i32, height: i32, threshold: u8) -> (Rect, Vec<u8>) {
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

    (Rect::new(min_x, min_y, tw, th), trimmed)
}

/// Expand `bounds` by `margin` transparent pixels (clamped to the image) and
/// re-extract the region from the *original* pixels.
fn apply_trim_margin(
    original: &[u8],
    width: i32,
    height: i32,
    bounds: Rect,
    trimmed: Vec<u8>,
    margin: i32,
) -> (Rect, Vec<u8>) {
    if margin <= 0 || bounds.width <= 0 || bounds.height <= 0 {
        return (bounds, trimmed);
    }
    let x0 = (bounds.x - margin).max(0);
    let y0 = (bounds.y - margin).max(0);
    let x1 = (bounds.x + bounds.width + margin).min(width);
    let y1 = (bounds.y + bounds.height + margin).min(height);
    let w = x1 - x0;
    let h = y1 - y0;
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let src = (((y0 + y) * width + x0) * 4) as usize;
        let dst = (y * w * 4) as usize;
        out[dst..dst + (w as usize * 4)].copy_from_slice(&original[src..src + (w as usize * 4)]);
    }
    (Rect::new(x0, y0, w, h), out)
}

/// Load `pivots.json` (map of sprite id -> {x, y} normalized 0..1) from `dir`,
/// if present.
pub fn load_pivot_overrides(dir: &Path) -> Result<PivotOverrides> {
    let path = dir.join("pivots.json");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| TpError::Other(format!("pivots.json: {e}")))?;
    let map: HashMap<String, crate::types::Point2D> =
        serde_json::from_str(&text).map_err(|e| TpError::Other(format!("pivots.json: {e}")))?;
    Ok(map)
}

/// Load `borders.json` (map of sprite id -> [left, top, right, bottom] in
/// pixels of the untrimmed source image) from `dir`, if present.
pub fn load_border_overrides(dir: &Path) -> Result<HashMap<String, [i32; 4]>> {
    let path = dir.join("borders.json");
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let text =
        std::fs::read_to_string(&path).map_err(|e| TpError::Other(format!("borders.json: {e}")))?;
    let map: HashMap<String, [i32; 4]> =
        serde_json::from_str(&text).map_err(|e| TpError::Other(format!("borders.json: {e}")))?;
    Ok(map)
}

/// Main ingest entry point.
///
/// Loads every discovered image (in parallel), trims it, hashes it, and pairs
/// normal maps. `*_normal.*` files are *not* returned as sprites when
/// `enable_normal_maps` is set — they are attached as companions.
pub fn ingest(options: &IngestOptions) -> IngestResult {
    let mut result = IngestResult::default();
    let threshold = options.trim_threshold.clamp(0, 255) as u8;

    let files = discover_images(options);
    if files.is_empty() {
        result.warnings.push(format!(
            "No se encontraron imágenes en {}",
            options.input_directory.display()
        ));
        return result;
    }

    // Build the set of normal maps: suffix and/or path filter first, then
    // (optionally) the color heuristic over whatever is left. Normals are
    // attached as companions and never returned as sprites.
    let normal_set: HashSet<PathBuf> = if options.enable_normal_maps {
        let mut set: HashSet<PathBuf> = files
            .iter()
            .filter(|p| {
                is_normal_map(
                    &sprite_rel_path(p, options),
                    &options.normal_map_suffix,
                    &options.normal_map_filter,
                )
            })
            .cloned()
            .collect();
        if options.normal_map_auto_detect {
            let detected: Vec<PathBuf> = files
                .par_iter()
                .filter(|p| !set.contains(*p))
                .filter_map(|p| {
                    let (w, h, rgba) = load_image_rgba(p).ok()?;
                    (w > 0 && h > 0 && looks_like_normal_map(&rgba)).then(|| p.clone())
                })
                .collect();
            if !detected.is_empty() {
                result.warnings.push(format!(
                    "{} imagen(es) clasificada(s) como mapa de normales por su color",
                    detected.len()
                ));
            }
            set.extend(detected);
        }
        set
    } else {
        HashSet::new()
    };

    // Matching indexes: by id base (suffix, or the last `_`/`-` group of the
    // name) and by bare file name (normales kept in another folder). A name
    // shared by two normal maps is dropped so the fallback never guesses.
    let mut normal_by_base: HashMap<String, PathBuf> = HashMap::new();
    let mut normal_by_name: HashMap<String, PathBuf> = HashMap::new();
    let mut duplicated_names: HashSet<String> = HashSet::new();
    for p in &normal_set {
        let rel = sprite_rel_path(p, options);
        normal_by_base
            .entry(normal_pair_key(&rel, &options.normal_map_suffix))
            .or_insert_with(|| p.clone());
        let name = file_name_of(&rel).to_string();
        if normal_by_name.insert(name.clone(), p.clone()).is_some() {
            duplicated_names.insert(name);
        }
    }
    normal_by_name.retain(|name, _| !duplicated_names.contains(name));

    // Sprites sharing a file name also block the fallback pairing.
    let mut sprite_name_counts: HashMap<String, usize> = HashMap::new();
    for p in files.iter().filter(|p| !normal_set.contains(*p)) {
        *sprite_name_counts
            .entry(file_name_of(&sprite_rel_path(p, options)).to_string())
            .or_default() += 1;
    }

    let trim_mode = options.trim_mode;
    let margin = options.trim_margin.max(0);
    // `--replace`: compiladas una sola vez para todas las imágenes.
    let replacements: Vec<(regex::Regex, String)> = options
        .name_replacements
        .iter()
        .filter_map(|(pattern, text)| regex::Regex::new(pattern).ok().map(|r| (r, text.clone())))
        .collect();
    let masked = std::sync::atomic::AtomicUsize::new(0);

    // Parallel load + trim + hash.
    let loaded: Vec<Result<Option<IngestedSprite>>> = files
        .par_iter()
        .filter(|p| !normal_set.contains(*p))
        .map(|path| {
            let (w, h, mut rgba) = load_image_rgba(path)?;
            // `--heuristic-mask` va antes del trim: el borde que deja
            // transparente es el que el recorte se lleva por delante.
            if options.heuristic_mask && heuristic_mask_rgba(&mut rgba, w, h) {
                masked.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            let (mut bounds, mut pixels) = if trim_mode.trims() {
                let (bounds, trimmed) = trim_rgba(&rgba, w, h, threshold);
                if trimmed.is_empty() {
                    return Ok(None); // fully transparent: skip
                }
                apply_trim_margin(&rgba, w, h, bounds, trimmed, margin)
            } else {
                (Rect::new(0, 0, w, h), rgba)
            };
            // Common divisor — extend sizes (with transparency) to be
            // divisible by the configured values.
            let divisor = (
                options.common_divisor_x.max(1),
                options.common_divisor_y.max(1),
            );
            if divisor != (1, 1) {
                let (b, p) = extend_to_divisor(bounds, &pixels, divisor);
                bounds = b;
                pixels = p;
            }
            let rel = sprite_rel_path(path, options);
            let normal_path = normal_by_base.get(&rel).cloned().or_else(|| {
                let name = file_name_of(&rel);
                // Fallback: same file name in another folder, and only when
                // exactly one sprite carries that name.
                (sprite_name_counts.get(name) == Some(&1))
                    .then(|| normal_by_name.get(name).cloned())
                    .flatten()
            });
            let mut id = if options.trim_sprite_names {
                rel
            } else {
                match path.extension().and_then(|e| e.to_str()) {
                    Some(ext) => format!("{rel}.{ext}"),
                    None => rel,
                }
            };
            // `--replace`: sustituciones en el nombre del sprite, en orden.
            for (re, text) in &replacements {
                id = re.replace_all(&id, text.as_str()).into_owned();
            }
            let hash = hash_pixels_rgba(&pixels);
            Ok(Some(IngestedSprite {
                id,
                source_path: path.clone(),
                raw_width: w,
                raw_height: h,
                trimmed_bounds: bounds,
                pixels,
                pixel_hash: hash,
                normal_path,
            }))
        })
        .collect();

    let mut used_normals: Vec<PathBuf> = Vec::new();
    let mut skipped_transparent = 0usize;
    for item in loaded {
        match item {
            Err(e) => result.warnings.push(e.to_string()),
            Ok(None) => skipped_transparent += 1,
            Ok(Some(sprite)) => {
                if let Some(n) = &sprite.normal_path {
                    used_normals.push(n.clone());
                }
                result.sprites.push(sprite);
            }
        }
    }

    if skipped_transparent > 0 {
        result.warnings.push(format!(
            "{skipped_transparent} sprite(s) totalmente transparentes omitidos (trim mode {})",
            trim_mode.as_str()
        ));
    }
    let masked = masked.load(std::sync::atomic::Ordering::Relaxed);
    if masked > 0 {
        result.warnings.push(format!(
            "{masked} sprite(s) opacos pasaron por la máscara heurística (--heuristic-mask)"
        ));
    }

    if options.enable_normal_maps {
        // Warn about normal maps without a matching diffuse.
        for path in normal_set.iter() {
            if !used_normals.contains(path) {
                result.warnings.push(format!(
                    "Mapa de normales sin difusa asociada: {}",
                    path.display()
                ));
            }
        }
    }

    result
}

/// Is this *file path* the normal map of a sprite, judging by its suffix?
///
/// [`ingest`] works on the relative id instead; this helper is for callers
/// that only hold a path (the app skips border detection on normals).
pub fn is_normal_file(path: &Path, suffix: &str) -> bool {
    if suffix.is_empty() {
        return false;
    }
    file_stem(path).is_some_and(|s| s.ends_with(suffix))
}

/// Is this relative id (extension-stripped, `/` separators) a normal map?
/// Matches the configured suffix *or* the path filter (case-insensitive
/// substring, e.g. `normals/`).
pub fn is_normal_map(rel_path: &str, suffix: &str, filter: &str) -> bool {
    if !filter.is_empty() && rel_path.to_lowercase().contains(&filter.to_lowercase()) {
        return true;
    }
    if suffix.is_empty() {
        return false;
    }
    file_name_of(rel_path).ends_with(suffix)
}

/// Key a normal map is indexed under so its sprite finds it: the id without
/// the suffix, or without the last `_`/`-`/`.` group of the file name so that
/// `hero-n.png` still matches `hero.png`.
fn normal_pair_key(rel: &str, suffix: &str) -> String {
    if !suffix.is_empty() && rel.ends_with(suffix) {
        return rel[..rel.len() - suffix.len()].to_string();
    }
    let name = file_name_of(rel);
    if let Some(i) = name.rfind(['_', '-', '.']) {
        if i > 0 && i + 1 < name.len() {
            let mut key = rel[..rel.len() - name.len()].to_string();
            key.push_str(&name[..i]);
            return key;
        }
    }
    rel.to_string()
}

/// Last `/`-separated component of an extension-stripped relative id.
fn file_name_of(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// Color heuristic for tangent-space normal maps: R and G hover around the
/// middle of the range while B dominates. Used by the optional auto-detect.
pub fn looks_like_normal_map(rgba: &[u8]) -> bool {
    let (mut sr, mut sg, mut sb, mut n) = (0u64, 0u64, 0u64, 0u64);
    for px in rgba.chunks_exact(4) {
        if px[3] == 0 {
            continue;
        }
        sr += u64::from(px[0]);
        sg += u64::from(px[1]);
        sb += u64::from(px[2]);
        n += 1;
    }
    if n == 0 {
        return false;
    }
    let (r, g, b) = (sr / n, sg / n, sb / n);
    b > r && b > g && b >= 160 && r.abs_diff(128) <= 48 && g.abs_diff(128) <= 48
}

/// File stem without extension (lowercased handling kept as-is).
fn file_stem(path: &Path) -> Option<String> {
    path.file_stem().and_then(|s| s.to_str()).map(String::from)
}

/// Sprite id *without* extension: the path relative to `input_directory` (or
/// to the smart folder that contains it), with `/` separators — sub-folder
/// names are always part of the sprite name.
///
/// Falls back to the bare file name when the path lives outside every root
/// (e.g. a single dropped image).
fn sprite_rel_path(path: &Path, options: &IngestOptions) -> String {
    let strip = |root: &Path| -> Option<String> {
        if !root.is_dir() {
            return None;
        }
        let rel = path.strip_prefix(root).ok()?;
        if rel.as_os_str().is_empty() {
            return None;
        }
        Some(to_posix_stem(rel))
    };

    if let Some(rel) = strip(options.input_directory) {
        return rel;
    }
    for folder in options.extra_inputs {
        if let Some(mut rel) = strip(folder) {
            if options.prepend_folder_name {
                if let Some(name) = folder.file_name().and_then(|n| n.to_str()) {
                    rel = format!("{name}/{rel}");
                }
            }
            return rel;
        }
    }
    file_stem(path).unwrap_or_else(|| "sprite".to_string())
}

/// Join path components with `/` (asset names always use forward slashes),
/// dropping the extension of the last component (like `Path::file_stem`).
fn to_posix_stem(rel: &Path) -> String {
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if let Some(last) = parts.last_mut() {
        if let Some(dot) = last.rfind('.') {
            if dot > 0 {
                last.truncate(dot);
            }
        }
    }
    parts.join("/")
}

fn round_up(value: i32, divisor: i32) -> i32 {
    let d = divisor.max(1);
    let rest = value % d;
    if rest == 0 {
        value
    } else {
        value + (d - rest)
    }
}

/// Extend the trimmed buffer with transparent pixels so both axes are
/// divisible by the common divisor.
fn extend_to_divisor(bounds: Rect, pixels: &[u8], (dx, dy): (i32, i32)) -> (Rect, Vec<u8>) {
    let nw = round_up(bounds.width, dx);
    let nh = round_up(bounds.height, dy);
    if nw == bounds.width && nh == bounds.height {
        return (bounds, pixels.to_vec());
    }
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    let row_bytes = bounds.width as usize * 4;
    for y in 0..bounds.height as usize {
        let src = y * row_bytes;
        let dst = y * nw as usize * 4;
        out[dst..dst + row_bytes].copy_from_slice(&pixels[src..src + row_bytes]);
    }
    (Rect::new(bounds.x, bounds.y, nw, nh), out)
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

/// Candidate 9-patch borders detected on a sprite: `[left, top, right, bottom]`
/// in pixels of the *untrimmed* source image.
pub type DetectedBorders = [i32; 4];

/// True when the scan line `line` (row if `is_row`, else column) is a *solid
/// color bar*: it has at least one visible pixel (alpha > `threshold`) and all
/// of them share the same RGBA value (within `tolerance` per channel).
fn is_solid_line(
    rgba: &[u8],
    width: i32,
    is_row: bool,
    line: i32,
    length: i32,
    threshold: u8,
    tolerance: i32,
) -> bool {
    let px = |j: i32| -> [u8; 4] {
        let (x, y) = if is_row { (j, line) } else { (line, j) };
        let i = ((y * width + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    let mut first: Option<[u8; 4]> = None;
    for j in 0..length {
        let c = px(j);
        if c[3] <= threshold {
            continue; // los píxeles transparentes no rompen la barra
        }
        match first {
            None => first = Some(c),
            Some(f) => {
                let same = f
                    .iter()
                    .zip(c.iter())
                    .all(|(a, b)| (*a as i32 - *b as i32).abs() <= tolerance);
                if !same {
                    return false;
                }
            }
        }
    }
    first.is_some()
}

/// Auto-detect 9-patch borders on a sprite (borders): find the outermost
/// rows/columns of solid color that frame the
/// content. Each side is measured scanning inward while consecutive lines
/// (up to `max_search` per side) qualify as solid bars; transparent margins or
/// non-solid content stop the scan. Sides without a detected bar report 0
/// (3-patch on one axis, or no 9-patch at all if everything is 0).
///
/// Runs on the **untrimmed** RGBA pixels (`load_image_rgba` output).
///
/// For sprites surrounded by a transparent margin use
/// [`detect_borders_auto`], which crops the margin before the scan.
pub fn detect_borders(
    rgba: &[u8],
    width: i32,
    height: i32,
    threshold: u8,
    tolerance: i32,
    max_search: i32,
) -> DetectedBorders {
    let measure = |is_row: bool, from_start: bool| -> i32 {
        let (size, length) = if is_row {
            (height, width)
        } else {
            (width, height)
        };
        if size <= 0 || length <= 0 {
            return 0;
        }
        let limit = max_search.max(0).min(size);
        let mut count = 0i32;
        for step in 0..limit {
            let line = if from_start { step } else { size - 1 - step };
            if !is_solid_line(
                rgba,
                width,
                is_row,
                line,
                length,
                threshold,
                tolerance.max(0),
            ) {
                break;
            }
            count += 1;
        }
        count
    };
    [
        measure(false, true),  // left: columnas desde x=0
        measure(true, true),   // top: filas desde y=0
        measure(false, false), // right: columnas desde el borde derecho
        measure(true, false),  // bottom: filas desde el borde inferior
    ]
}

/// Auto-detect 9-patch borders tolerating a fully transparent margin around
/// the sprite: the visible bounding box (alpha > `threshold`) is cropped with
/// [`trim_rgba`] and the solid-bar scan runs inside it, so sprites whose
/// frame does not touch the image edges are detected too.
///
/// The returned values keep the project convention (borders measured on the
/// **untrimmed** source image, like `borders.json`): each face adds the
/// distance from the image edge to the bounding box. When no solid bar is
/// found the result is `[0, 0, 0, 0]` — margins alone never count as bars.
pub fn detect_borders_auto(
    rgba: &[u8],
    width: i32,
    height: i32,
    threshold: u8,
    tolerance: i32,
    max_search: i32,
) -> DetectedBorders {
    let (bounds, trimmed) = trim_rgba(rgba, width, height, threshold);
    if bounds.width <= 0 || bounds.height <= 0 {
        return [0, 0, 0, 0];
    }
    let mut b = detect_borders(
        &trimmed,
        bounds.width,
        bounds.height,
        threshold,
        tolerance,
        max_search,
    );
    if b != [0, 0, 0, 0] {
        b[0] += bounds.x;
        b[1] += bounds.y;
        b[2] += width - bounds.x - bounds.width;
        b[3] += height - bounds.y - bounds.height;
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_image_file_accepts_every_documented_input_extension() {
        for name in [
            "a.png",
            "a.webp",
            "a.jpg",
            "a.tga",
            "a.bmp",
            "a.dds",
            "a.qoi",
            "a.ppm",
            "a.xbm",
            "a.xpm",
            "a.astc",
            "a.ktx",
            "a.ktx2",
            "a.psd",
            "a.svg",
            "a.svgz",
            "a.basis",
            "a.pkm",
            "a.pvr",
            "a.pvrtc",
            "a.ccz",
            "a.pvr.gz",
            "A.PNG",
            "carpeta/héroe.basis",
        ] {
            assert!(is_image_file(Path::new(name)), "{name} debería ser imagen");
        }
        for name in [
            "a.txt",
            "a.json",
            "a.tpproj",
            "basis",
            "carpeta/",
            "backup.gz",
            "a.png.gz",
        ] {
            assert!(!is_image_file(Path::new(name)), "{name} no es imagen");
        }
    }

    #[test]
    fn detect_borders_finds_solid_frame() {
        // Marco sólido de 3 px alrededor de un contenido de gradiente.
        let mut img = vec![0u8; 10 * 10 * 4];
        for y in 0..10i32 {
            for x in 0..10i32 {
                let i = ((y * 10 + x) * 4) as usize;
                if !(3..7).contains(&x) || !(3..7).contains(&y) {
                    img[i..i + 4].copy_from_slice(&[0, 0, 255, 255]);
                } else {
                    img[i..i + 4].copy_from_slice(&[(x * 20) as u8, (y * 20) as u8, 0, 255]);
                }
            }
        }
        assert_eq!(detect_borders(&img, 10, 10, 0, 0, 64), [3, 3, 3, 3]);
    }

    #[test]
    fn detect_borders_stops_at_nonsolid_line() {
        // Dos filas superiores sólidas; el resto es un gradiente por columna
        // (cada fila no es de color uniforme).
        let mut img = vec![0u8; 8 * 8 * 4];
        for y in 0..8i32 {
            for x in 0..8i32 {
                let i = ((y * 8 + x) * 4) as usize;
                let color: [u8; 4] = if y < 2 {
                    [255, 0, 0, 255]
                } else {
                    [(x * 30) as u8, 0, 0, 255]
                };
                img[i..i + 4].copy_from_slice(&color);
            }
        }
        assert_eq!(detect_borders(&img, 8, 8, 0, 0, 64), [0, 2, 0, 0]);
    }

    #[test]
    fn detect_borders_transparent_margin_stops_the_scan() {
        // Fila 0 transparente y fila 1 sólida: el escaneo se detiene en el
        // margen (resultado conservador, 0).
        let mut img = vec![0u8; 6 * 6 * 4];
        for y in 0..6i32 {
            for x in 0..6i32 {
                let i = ((y * 6 + x) * 4) as usize;
                let color: [u8; 4] = if y == 0 {
                    [0, 0, 0, 0]
                } else if y == 1 {
                    [10, 200, 10, 255]
                } else {
                    [(x * 40) as u8, 0, 0, 255]
                };
                img[i..i + 4].copy_from_slice(&color);
            }
        }
        assert_eq!(detect_borders(&img, 6, 6, 0, 0, 64), [0, 0, 0, 0]);
    }

    #[test]
    fn detect_borders_tolerance_within_line() {
        // Fila 0 con píxeles alternando 100/101 (diferencia 1 por canal):
        // no es barra con tolerancia 0, sí con tolerancia 1. El resto son
        // gradientes por columna (nunca barras).
        let mut img = vec![0u8; 6 * 6 * 4];
        for y in 0..6i32 {
            for x in 0..6i32 {
                let i = ((y * 6 + x) * 4) as usize;
                let color: [u8; 4] = if y == 0 {
                    [100 + (x % 2) as u8, 0, 0, 255]
                } else {
                    [(x * 40) as u8, 0, 0, 255]
                };
                img[i..i + 4].copy_from_slice(&color);
            }
        }
        assert_eq!(detect_borders(&img, 6, 6, 0, 0, 64), [0, 0, 0, 0]);
        assert_eq!(detect_borders(&img, 6, 6, 0, 1, 64), [0, 1, 0, 0]);
    }

    #[test]
    fn detect_borders_max_search_limits_the_scan() {
        // Dos filas superiores uniformes y un gradiente debajo:
        // la búsqueda completa detecta 2, limitada a 1 línea solo 1.
        let mut img = vec![0u8; 6 * 6 * 4];
        for y in 0..6i32 {
            for x in 0..6i32 {
                let i = ((y * 6 + x) * 4) as usize;
                let color: [u8; 4] = if y < 2 {
                    [100, 0, 0, 255]
                } else {
                    [(x * 40) as u8, 0, 0, 255]
                };
                img[i..i + 4].copy_from_slice(&color);
            }
        }
        assert_eq!(detect_borders(&img, 6, 6, 0, 0, 64), [0, 2, 0, 0]);
        assert_eq!(detect_borders(&img, 6, 6, 0, 0, 1), [0, 1, 0, 0]);
    }

    #[test]
    fn detect_borders_auto_tolerates_transparent_margin() {
        // Marco de 1 px (azul) alrededor de un interior de gradiente 6×6, con
        // margen transparente de 3 px arriba/izquierda y 5 px derecha/abajo
        // (lienzo 16×16). El margen no debe detener el análisis y los valores
        // se expresan sobre la imagen sin recortar: [1+3, 1+3, 1+5, 1+5].
        let mut img = vec![0u8; 16 * 16 * 4];
        for y in 0..16i32 {
            for x in 0..16i32 {
                let i = ((y * 16 + x) * 4) as usize;
                if (4..10).contains(&x) && (4..10).contains(&y) {
                    img[i..i + 4].copy_from_slice(&[(x * 20) as u8, (y * 20) as u8, 0, 255]);
                } else if (3..11).contains(&x) && (3..11).contains(&y) {
                    img[i..i + 4].copy_from_slice(&[0, 0, 255, 255]);
                }
            }
        }
        assert_eq!(detect_borders_auto(&img, 16, 16, 0, 0, 64), [4, 4, 6, 6]);
    }

    #[test]
    fn detect_borders_auto_matches_plain_detection_without_margin() {
        // Sin margen: auto y directa coinciden (el recorte no cambia nada).
        let mut img = vec![0u8; 10 * 10 * 4];
        for y in 0..10i32 {
            for x in 0..10i32 {
                let i = ((y * 10 + x) * 4) as usize;
                if (3..7).contains(&x) && (3..7).contains(&y) {
                    // gradiente central (no sólido)
                    img[i..i + 4].copy_from_slice(&[(x * 20) as u8, (y * 20) as u8, 0, 255]);
                } else {
                    img[i..i + 4].copy_from_slice(&[0, 0, 255, 255]);
                }
            }
        }
        assert_eq!(detect_borders(&img, 10, 10, 0, 0, 64), [3, 3, 3, 3]);
        assert_eq!(detect_borders_auto(&img, 10, 10, 0, 0, 64), [3, 3, 3, 3]);
    }

    #[test]
    fn detect_borders_auto_ignores_margins_as_bars() {
        // Solo margen transparente y contenido sin barras: ceros, el margen
        // nunca cuenta como barra (distinto de contar el margen como borde).
        let img = vec![0u8; 6 * 6 * 4];
        assert_eq!(detect_borders_auto(&img, 6, 6, 0, 0, 64), [0, 0, 0, 0]);
    }

    #[test]
    fn detect_borders_fully_transparent_is_zero() {
        let img = vec![0u8; 6 * 6 * 4];
        assert_eq!(detect_borders(&img, 6, 6, 0, 0, 64), [0, 0, 0, 0]);
    }

    #[test]
    fn extra_inputs_and_exclusions_change_the_sprite_set() {
        let root = std::env::temp_dir().join(format!("tp_ingest_{}", std::process::id()));
        let extra = root.join("extra");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&extra).unwrap();
        let write = |path: &Path, rgb: [u8; 3]| {
            let img =
                image::RgbaImage::from_pixel(4, 4, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
            img.save(path).unwrap();
        };
        write(&root.join("a.png"), [255, 0, 0]);
        write(&extra.join("c.png"), [0, 0, 255]);

        let excluded = root.join("a.png");
        let options = IngestOptions {
            input_directory: &root,
            trim_threshold: 1,
            trim_mode: TrimMode::Trim,
            trim_margin: 0,
            enable_normal_maps: false,
            normal_map_suffix: "_normal".into(),
            normal_map_filter: String::new(),
            normal_map_auto_detect: false,
            recursive: true,
            extra_inputs: std::slice::from_ref(&extra),
            excluded_inputs: &[],
            trim_sprite_names: true,
            prepend_folder_name: false,
            common_divisor_x: 1,
            common_divisor_y: 1,
            ignore_patterns: &[],
            name_replacements: &[],
            heuristic_mask: false,
        };

        let all = ingest(&options);
        assert_eq!(all.sprites.len(), 2);

        let mut filtered_options = options.clone();
        filtered_options.excluded_inputs = std::slice::from_ref(&excluded);
        let filtered = ingest(&filtered_options);
        let ids: Vec<&str> = filtered.sprites.iter().map(|s| s.id.as_str()).collect();
        // `extra/c.png` lives inside the input root, so its id carries the
        // sub-folder name (`extra/c`).
        assert_eq!(ids, vec!["extra/c"]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn trim_margin_expands_bounds_with_transparency() {
        // 8x8, visible blob at (2,3)-(4,4) (3x2).
        let buf = rgba(8, 8, |x, y| {
            if (2..5).contains(&x) && (3..5).contains(&y) {
                [255, 0, 0, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let (bounds, trimmed) = trim_rgba(&buf, 8, 8, 1);
        assert_eq!(
            (bounds.x, bounds.y, bounds.width, bounds.height),
            (2, 3, 3, 2)
        );

        let (b2, t2) = apply_trim_margin(&buf, 8, 8, bounds, trimmed, 2);
        assert_eq!((b2.x, b2.y, b2.width, b2.height), (0, 1, 7, 6));
        assert_eq!(t2.len(), 7 * 6 * 4);
        // El pixel expandido en la esquina es transparente.
        assert_eq!(&t2[0..4], &[0, 0, 0, 0]);
        // El píxel original sigue en su sitio relativo (offset 2,2 dentro del recorte).
        let idx = ((2 * 7 + 2) * 4) as usize;
        assert_eq!(&t2[idx..idx + 4], &[255, 0, 0, 255]);
    }

    #[test]
    fn trim_margin_zero_returns_trimmed_buffer() {
        let buf = rgba(4, 4, |x, y| {
            if x == 1 && y == 1 {
                [9, 9, 9, 255]
            } else {
                [0, 0, 0, 0]
            }
        });
        let (bounds, trimmed) = trim_rgba(&buf, 4, 4, 1);
        let (b2, t2) = apply_trim_margin(&buf, 4, 4, bounds, trimmed.clone(), 0);
        assert_eq!(b2, bounds);
        assert_eq!(t2, trimmed);
    }

    #[test]
    fn trim_mode_none_keeps_full_image_and_transparent_sprites() {
        let root = std::env::temp_dir().join(format!("tp_trimnone_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // 4x4 con un píxel visible en el centro: con trim serían 1x1.
        let mut img = image::RgbaImage::new(4, 4);
        img.put_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        img.save(root.join("blob.png")).unwrap();
        // Totalmente transparente: se incluye porque no hay trim.
        image::RgbaImage::new(4, 4)
            .save(root.join("empty.png"))
            .unwrap();

        let options = IngestOptions {
            input_directory: &root,
            trim_threshold: 1,
            trim_mode: TrimMode::None,
            trim_margin: 0,
            enable_normal_maps: false,
            normal_map_suffix: "_normal".into(),
            normal_map_filter: String::new(),
            normal_map_auto_detect: false,
            recursive: true,
            extra_inputs: &[],
            excluded_inputs: &[],
            trim_sprite_names: true,
            prepend_folder_name: false,
            common_divisor_x: 1,
            common_divisor_y: 1,
            ignore_patterns: &[],
            name_replacements: &[],
            heuristic_mask: false,
        };
        let out = ingest(&options);
        assert_eq!(
            out.sprites.len(),
            2,
            "modo None conserva hasta los transparentes: {out:?}"
        );
        for s in &out.sprites {
            assert_eq!((s.trimmed_bounds.x, s.trimmed_bounds.y), (0, 0));
            assert_eq!((s.trimmed_bounds.width, s.trimmed_bounds.height), (4, 4));
            assert_eq!(s.pixels.len(), 4 * 4 * 4);
        }

        // Con trim: el blob queda en 1x1 y el transparente se descarta.
        let mut trimmed_options = options.clone();
        trimmed_options.trim_mode = TrimMode::Trim;
        let out2 = ingest(&trimmed_options);
        assert_eq!(out2.sprites.len(), 1);
        assert_eq!(out2.sprites[0].trimmed_bounds.width, 1);
        assert!(out2
            .warnings
            .iter()
            .any(|w| w.contains("transparentes omitidos")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sprite_ids_carry_folders_and_optional_extension() {
        let root = std::env::temp_dir().join(format!("tp_ids_{}", std::process::id()));
        let smart = std::env::temp_dir().join(format!("tp_ids_smart_{}", std::process::id()));
        let root_smart = std::env::temp_dir().join(format!("tp_ids_root_{}", std::process::id()));
        for d in [&root, &smart, &root_smart] {
            let _ = std::fs::remove_dir_all(d);
        }
        std::fs::create_dir_all(root.join("hero")).unwrap();
        std::fs::create_dir_all(&smart).unwrap();
        std::fs::create_dir_all(root_smart.join("fx")).unwrap();
        let write = |path: &Path| {
            image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
                .save(path)
                .unwrap();
        };
        write(&root.join("hero/idle_00.png"));
        write(&root.join("idle_01.png"));
        write(&smart.join("glow.png"));
        write(&root_smart.join("fx/glow.png"));

        let no_extras: &[PathBuf] = &[];
        let options = IngestOptions {
            input_directory: &root,
            trim_threshold: 1,
            trim_mode: TrimMode::Trim,
            trim_margin: 0,
            enable_normal_maps: false,
            normal_map_suffix: "_normal".into(),
            normal_map_filter: String::new(),
            normal_map_auto_detect: false,
            recursive: true,
            extra_inputs: no_extras,
            excluded_inputs: &[],
            trim_sprite_names: true,
            prepend_folder_name: false,
            common_divisor_x: 1,
            common_divisor_y: 1,
            ignore_patterns: &[],
            name_replacements: &[],
            heuristic_mask: false,
        };
        let ids: Vec<String> = ingest(&options).sprites.into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["hero/idle_00", "idle_01"]);

        // Con extensiones: el nombre conserva `.png` (trim sprite names).
        let mut with_ext = options.clone();
        with_ext.trim_sprite_names = false;
        let ids: Vec<String> = ingest(&with_ext)
            .sprites
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, vec!["hero/idle_00.png", "idle_01.png"]);

        // Smart folder *dentro* de la raíz: siempre lleva su subcarpeta.
        let mut inside_root = options.clone();
        inside_root.input_directory = &root_smart;
        let ids: Vec<String> = ingest(&inside_root)
            .sprites
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, vec!["fx/glow"]);

        // Smart folder *fuera* de la raíz: prefijo solo con *Prepend folder name*.
        let smart_slice = std::slice::from_ref(&smart);
        let mut with_smart = options.clone();
        with_smart.extra_inputs = smart_slice;
        let ids: Vec<String> = ingest(&with_smart)
            .sprites
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert!(ids.contains(&"glow".to_string()), "ids: {ids:?}");
        let mut prefixed = with_smart.clone();
        prefixed.prepend_folder_name = true;
        let ids: Vec<String> = ingest(&prefixed)
            .sprites
            .into_iter()
            .map(|s| s.id)
            .collect();
        let smart_name = smart.file_name().unwrap().to_string_lossy().into_owned();
        assert!(ids.contains(&format!("{smart_name}/glow")), "ids: {ids:?}");

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&smart);
        let _ = std::fs::remove_dir_all(&root_smart);
    }

    #[test]
    fn common_divisor_extends_sizes_with_transparency() {
        let root = std::env::temp_dir().join(format!("tp_divisor_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // 3x5 totalmente opaco: el divisor lo estira a 4x8 con transparencia.
        image::RgbaImage::from_pixel(3, 5, image::Rgba([200, 60, 60, 255]))
            .save(root.join("solid.png"))
            .unwrap();

        let options = IngestOptions {
            input_directory: &root,
            trim_threshold: 1,
            trim_mode: TrimMode::Trim,
            trim_margin: 0,
            enable_normal_maps: false,
            normal_map_suffix: "_normal".into(),
            normal_map_filter: String::new(),
            normal_map_auto_detect: false,
            recursive: true,
            extra_inputs: &[],
            excluded_inputs: &[],
            trim_sprite_names: true,
            prepend_folder_name: false,
            common_divisor_x: 4,
            common_divisor_y: 8,
            ignore_patterns: &[],
            name_replacements: &[],
            heuristic_mask: false,
        };
        let out = ingest(&options);
        assert_eq!(out.sprites.len(), 1);
        let s = &out.sprites[0];
        assert_eq!((s.trimmed_bounds.width, s.trimmed_bounds.height), (4, 8));
        assert_eq!(s.pixels.len(), 4 * 8 * 4);
        // El píxel original sigue en su sitio y el estirado es transparente.
        assert_eq!(&s.pixels[0..4], &[200, 60, 60, 255]);
        assert_eq!(&s.pixels[3 * 4..3 * 4 + 4], &[0, 0, 0, 0]);
        assert_eq!(
            &s.pixels[(7 * 4 + 3) * 4..(7 * 4 + 3) * 4 + 4],
            &[0, 0, 0, 0]
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn normal_maps_pair_by_relative_id() {
        let root = std::env::temp_dir().join(format!("tp_normals_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("hero")).unwrap();
        let write = |path: &Path, rgb: [u8; 3]| {
            image::RgbaImage::from_pixel(2, 2, image::Rgba([rgb[0], rgb[1], rgb[2], 255]))
                .save(path)
                .unwrap();
        };
        write(&root.join("hero/idle.png"), [255, 0, 0]);
        write(&root.join("hero/idle_normal.png"), [0, 0, 255]);
        write(&root.join("idle_normal.png"), [0, 255, 0]); // sin difusa: aviso

        let options = IngestOptions {
            input_directory: &root,
            trim_threshold: 1,
            trim_mode: TrimMode::Trim,
            trim_margin: 0,
            enable_normal_maps: true,
            normal_map_suffix: "_normal".into(),
            normal_map_filter: String::new(),
            normal_map_auto_detect: false,
            recursive: true,
            extra_inputs: &[],
            excluded_inputs: &[],
            trim_sprite_names: false,
            prepend_folder_name: false,
            common_divisor_x: 1,
            common_divisor_y: 1,
            ignore_patterns: &[],
            name_replacements: &[],
            heuristic_mask: false,
        };
        let out = ingest(&options);
        assert_eq!(out.sprites.len(), 1);
        assert_eq!(out.sprites[0].id, "hero/idle.png");
        assert!(
            out.sprites[0].normal_path.is_some(),
            "no emparejó la normal por id relativo"
        );
        assert!(out
            .warnings
            .iter()
            .any(|w| w.contains("sin difusa asociada")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn normal_maps_honor_suffix_filter_and_color() {
        let root = std::env::temp_dir().join(format!("tp_normals_opt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("normals")).unwrap();
        let write = |path: &Path, rgb: [u8; 3]| {
            image::RgbaImage::from_pixel(2, 2, image::Rgba([rgb[0], rgb[1], rgb[2], 255]))
                .save(path)
                .unwrap();
        };
        // Difusas de colores vivos; normales en azul tangente (128,128,255).
        write(&root.join("hero.png"), [255, 0, 0]);
        write(&root.join("hero_n.png"), [128, 128, 255]); // sufijo `_n`
        write(&root.join("box.png"), [10, 200, 10]);
        write(&root.join("normals/box.png"), [128, 128, 255]); // filtro `normals/`
        write(&root.join("orb.png"), [120, 40, 40]);
        write(&root.join("orb-det.png"), [128, 128, 255]); // solo por color

        let options = IngestOptions {
            input_directory: &root,
            trim_threshold: 1,
            trim_mode: TrimMode::Trim,
            trim_margin: 0,
            enable_normal_maps: true,
            normal_map_suffix: "_n".into(),
            normal_map_filter: "normals/".into(),
            normal_map_auto_detect: true,
            recursive: true,
            extra_inputs: &[],
            excluded_inputs: &[],
            trim_sprite_names: false,
            prepend_folder_name: false,
            common_divisor_x: 1,
            common_divisor_y: 1,
            ignore_patterns: &[],
            name_replacements: &[],
            heuristic_mask: false,
        };
        let out = ingest(&options);

        let mut ids: Vec<&str> = out.sprites.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["box.png", "hero.png", "orb.png"]);
        for s in &out.sprites {
            assert!(
                s.normal_path.is_some(),
                "a {} le falta su mapa de normales",
                s.id
            );
        }
        // `normals/box.png` empareja por nombre de fichero y `orb-det.png`
        // (detectada por color) por el último grupo del nombre.
        let normal_of = |id: &str| {
            let s = out.sprites.iter().find(|s| s.id == id).unwrap();
            s.normal_path.as_ref().unwrap().display().to_string()
        };
        assert!(normal_of("box.png").ends_with("normals/box.png"));
        assert!(normal_of("orb.png").ends_with("orb-det.png"));
        assert!(normal_of("hero.png").ends_with("hero_n.png"));
        assert!(
            out.warnings
                .iter()
                .any(|w| w.contains("clasificada(s) como mapa de normales")),
            "falta el aviso del auto-detect: {:?}",
            out.warnings
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn color_heuristic_accepts_normals_and_rejects_art() {
        let solid = |rgb: [u8; 4]| rgb.repeat(4 * 4);
        assert!(looks_like_normal_map(&solid([128, 128, 255, 255])));
        assert!(looks_like_normal_map(&solid([110, 140, 230, 255])));
        assert!(!looks_like_normal_map(&solid([255, 0, 0, 255])), "rojo");
        assert!(!looks_like_normal_map(&solid([0, 200, 0, 255])), "verde");
        assert!(
            !looks_like_normal_map(&solid([135, 196, 244, 255])),
            "cielo"
        );
        assert!(!looks_like_normal_map(&[0u8; 16]), "todo transparente");
    }

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
