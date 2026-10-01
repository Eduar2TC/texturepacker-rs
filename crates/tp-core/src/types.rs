//! Domain data structures defined in the technical specification
//! (`Point2D`, `Rect`, `TriangleMesh`, `SpriteAsset`, plus atlas output types).

use serde::{Deserialize, Serialize};

/// A 2D point in floating-point space (mesh vertices, pivots, UVs).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point2D {
    pub x: f32,
    pub y: f32,
}

impl Point2D {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Integer axis-aligned rectangle (pixel coordinates, y grows downward).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn area(&self) -> i64 {
        self.width as i64 * self.height as i64
    }

    /// True when `other` is fully contained inside `self`.
    pub fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.width <= self.x + self.width
            && other.y + other.height <= self.y + self.height
    }

    /// True when the two rects share at least one pixel (edge contact does not count).
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    /// True when `w x h` fits inside `self` (in either orientation is decided by the caller).
    pub fn can_fit(&self, w: i32, h: i32) -> bool {
        w <= self.width && h <= self.height
    }
}

/// A triangle mesh: vertices, triangle indices (into `vertices`) and UVs (same order as vertices).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriangleMesh {
    pub vertices: Vec<Point2D>,
    pub indices: Vec<u32>,
    pub uvs: Vec<Point2D>,
}

/// A polygon contour: ordered list of 2D points (closed; last point != first).
/// `is_hole` marks inner contours of a sprite silhouette.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Contour {
    pub points: Vec<Point2D>,
    pub is_hole: bool,
}

/// One sprite processed by the pipeline. Mirrors the spec's `SpriteAsset`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpriteAsset {
    pub id: String,
    pub source_path: String,
    /// Original (untrimmed) dimensions.
    pub raw_width: i32,
    pub raw_height: i32,
    /// Bounding box of visible pixels within the original image.
    pub trimmed_bounds: Rect,
    /// Offset of the trimmed region inside the original image (top-left).
    pub offset_x: i32,
    pub offset_y: i32,
    /// Hash of the trimmed RGBA pixels (hex). Used for alias detection.
    pub pixel_hash: String,
    pub is_alias: bool,
    pub alias_target_id: Option<String>,
    /// Normalized pivot (0..1 relative to the *original* sprite size).
    pub pivot: Point2D,
    /// 9-patch borders in pixels `[left, top, right, bottom]` measured on the
    /// *untrimmed* source image (9-patch borders). `None` when the sprite is
    /// not a 9-patch / 3-patch.
    pub border: Option<[i32; 4]>,
    /// Local-space mesh (trimmed coordinates) when polygon mode is enabled.
    pub mesh: Option<TriangleMesh>,
    /// Frame allocated inside the atlas page (includes padding).
    pub allocated_frame: Rect,
    /// Frame of the visible (trimmed) pixels inside the atlas, without padding.
    pub visible_frame: Rect,
    pub is_rotated: bool,
    pub atlas_page_index: i32,
    /// Companion normal map (co-packed in the same frame) when present.
    pub normal_source_path: Option<String>,
    /// Whether this sprite is the target that aliases point to.
    pub is_alias_target: bool,
    /// Mesh contours (before triangulation) kept for debugging/visualization.
    pub contours: Vec<Contour>,
}

impl SpriteAsset {
    /// Position of the trimmed sprite within the original image.
    pub fn offset(&self) -> Point2D {
        Point2D::new(self.offset_x as f32, self.offset_y as f32)
    }

    /// Final size of the sprite in the atlas (frame minus padding on both sides).
    pub fn atlas_size(&self) -> (i32, i32) {
        if self.is_rotated {
            (self.allocated_frame.height, self.allocated_frame.width)
        } else {
            (self.allocated_frame.width, self.allocated_frame.height)
        }
    }
}

/// One output atlas page: a canvas plus (optionally) a co-packed normal-map canvas.
#[derive(Debug, Clone)]
pub struct AtlasPage {
    pub index: usize,
    pub width: i32,
    pub height: i32,
    /// RGBA8 pixel buffer, length `width * height * 4`.
    pub pixels: Vec<u8>,
    /// RGBA8 normal-map canvas when normal maps are present (same dimensions).
    pub normal_pixels: Option<Vec<u8>>,
    /// True if any sprite on this page has a normal-map companion.
    pub has_normals: bool,
    /// Fraction of the page covered by sprites, measured right after the
    /// blit and *before* an optional background fill, so the metric keeps
    /// reporting sprite coverage.
    pub fill_ratio: f32,
}

impl AtlasPage {
    pub fn new(index: usize, width: i32, height: i32) -> Self {
        let len = (width * height * 4) as usize;
        Self {
            index,
            width,
            height,
            pixels: vec![0u8; len],
            normal_pixels: None,
            has_normals: false,
            fill_ratio: 0.0,
        }
    }
}

/// Full result of a packing run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackResult {
    pub config: crate::config::ProjectConfig,
    pub sprites: Vec<SpriteAsset>,
    pub pages: Vec<PageInfo>,
    pub warnings: Vec<String>,
    /// Seconds spent in each pipeline stage (for diagnostics).
    pub stage_times_ms: Vec<(String, u64)>,
    pub total_sprites: usize,
    pub alias_count: usize,
    pub output_files: Vec<String>,
}

/// Serializable summary of a page (used by metadata templates).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    pub index: usize,
    pub width: i32,
    pub height: i32,
    pub file_name: String,
    pub format: String,
    pub has_normals: bool,
    pub normal_file_name: Option<String>,
    pub encrypted: bool,
    pub fill_ratio: f32,
    /// Hash corto del fichero de imagen para el cache busting del data
    /// format (`?v=<hash>`); vacío cuando la opción está apagada.
    pub cache_version: String,
}
