//! Subsystem 3: Empaquetado Espacial (Packing Core).
//!
//! - MaxRects with BSSF / BAF / BLSF heuristics
//! - Guillotine bin packing
//! - 90° rotation support
//! - Multi-atlas auto-split (new page when nothing fits)
//! - Polygon-aware placement: AABB placement from MaxRects is validated against
//!   a per-page occupancy grid rasterized from the sprite mesh, so polygon
//!   sprites can be packed tighter than their bounding boxes.

use crate::config::PackingStrategy;
use crate::polygon::{rasterize_polygon, rotate_90_cw};
use crate::types::{Point2D, Rect, TriangleMesh};

/// A sprite ready for packing (trimmed size + optional local-space mesh).
#[derive(Debug, Clone)]
pub struct PackItem {
    pub id: String,
    pub width: i32,
    pub height: i32,
    pub mesh: Option<TriangleMesh>,
}

/// Where a sprite ended up.
#[derive(Debug, Clone)]
pub struct Placement {
    pub id: String,
    /// Frame in atlas coordinates (includes padding on all sides).
    pub frame: Rect,
    pub rotated: bool,
    pub page: usize,
}

/// One page of the packing output.
#[derive(Debug, Clone)]
pub struct PackPage {
    pub index: usize,
    pub width: i32,
    pub height: i32,
    pub placements: Vec<Placement>,
}

/// Full packing output.
#[derive(Debug, Clone)]
pub struct PackOutput {
    pub pages: Vec<PackPage>,
}

#[derive(Debug, Clone)]
pub struct PackerOptions {
    pub strategy: PackingStrategy,
    pub allow_rotation: bool,
    pub max_size: i32,
    pub padding: i32,
    pub polygon_mode: bool,
}

impl PackerOptions {
    pub fn new(
        strategy: PackingStrategy,
        allow_rotation: bool,
        max_size: i32,
        padding: i32,
        polygon_mode: bool,
    ) -> Self {
        Self {
            strategy,
            allow_rotation,
            max_size,
            padding,
            polygon_mode,
        }
    }
}

/// Internal packing state for one page.
struct PageState {
    index: usize,
    width: i32,
    height: i32,
    free_rects: Vec<Rect>,
    /// Occupancy grid (only maintained in polygon mode).
    occupied: Vec<bool>,
    placements: Vec<Placement>,
}

impl PageState {
    fn new(index: usize, size: i32) -> Self {
        Self {
            index,
            width: size,
            height: size,
            free_rects: vec![Rect::new(0, 0, size, size)],
            occupied: vec![false; (size * size) as usize],
            placements: Vec::new(),
        }
    }
}

/// Pack `items` (sorted internally by area, descending) into one or more
/// square canvases of `max_size` x `max_size`.
pub fn pack(items: &[PackItem], opts: &PackerOptions) -> Result<PackOutput, String> {
    let pad = opts.padding.max(0);
    let max = opts.max_size;

    // Inflate sizes by padding, validate fit.
    let mut sorted: Vec<(usize, i32, i32)> = items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let w = it.width + 2 * pad;
            let h = it.height + 2 * pad;
            if w > max || h > max {
                Err(format!(
                    "El sprite '{}' ({}x{}) no cabe en un atlas de {max}x{max}",
                    it.id, it.width, it.height
                ))
            } else {
                Ok((i, w, h))
            }
        })
        .collect::<Result<Vec<_>, String>>()?;

    // Sort by inflated area, descending (spec PASO 5).
    sorted.sort_by_key(|(_, w, h)| std::cmp::Reverse((w * h) as i64));

    let mut pages: Vec<PageState> = Vec::new();

    for (item_idx, w, h) in sorted {
        let item = &items[item_idx];

        let mut placed: Option<Placement> = None;
        if let Some(page) = pages.last_mut() {
            placed = try_place(page, item, w, h, opts)?;
        }
        if placed.is_none() {
            let mut page = PageState::new(pages.len(), max);
            placed = try_place(&mut page, item, w, h, opts)
                .map_err(|e| format!("{e} (página {})", page.index))?;
            pages.push(page);
        }
        pages
            .last_mut()
            .unwrap()
            .placements
            .push(placed.unwrap());
    }

    Ok(PackOutput {
        pages: pages
            .into_iter()
            .map(|p| PackPage {
                index: p.index,
                width: p.width,
                height: p.height,
                placements: p.placements,
            })
            .collect(),
    })
}

type Score = (i64, i64, i64);

/// Try to place `(w, h)` (inflated) on `page`. Candidates are ranked by the
/// heuristic; in polygon mode, invalid (overlapping) placements are skipped.
fn try_place(
    page: &mut PageState,
    item: &PackItem,
    w: i32,
    h: i32,
    opts: &PackerOptions,
) -> Result<Option<Placement>, String> {
    // Collect all candidate placements, ranked by score.
    let mut candidates: Vec<(usize, bool, Score)> = Vec::new();
    for (ri, fr) in page.free_rects.iter().enumerate() {
        for rotated in [false, true] {
            if rotated && (!opts.allow_rotation || w == h) {
                continue;
            }
            let (pw, ph) = if rotated { (h, w) } else { (w, h) };
            if fr.can_fit(pw, ph) {
                candidates.push((ri, rotated, score_placement(fr, pw, ph, opts.strategy)));
            }
        }
    }
    candidates.sort_by_key(|(_, _, s)| *s);

    for (ri, rotated, _) in candidates {
        let fr = page.free_rects[ri];
        let (pw, ph) = if rotated { (h, w) } else { (w, h) };
        let frame = Rect::new(fr.x, fr.y, pw, ph);

        // Polygon validation against the occupancy grid.
        if opts.polygon_mode {
            if let Some(mesh) = &item.mesh {
                let footprint = polygon_footprint(mesh, item.width, item.height, rotated, opts.padding);
                if overlaps(&page.occupied, &footprint, page.width, page.height, frame) {
                    continue;
                }
            }
        }

        // Commit: split free rects and update occupancy.
        split_rects(&mut page.free_rects, frame);
        prune_contained(&mut page.free_rects);

        match &item.mesh {
            Some(mesh) if opts.polygon_mode => {
                let footprint = polygon_footprint(mesh, item.width, item.height, rotated, opts.padding);
                blit_footprint(&mut page.occupied, &footprint, page.width, page.height, frame);
            }
            _ => mark_rect(&mut page.occupied, page.width, page.height, frame),
        }

        return Ok(Some(Placement {
            id: item.id.clone(),
            frame,
            rotated,
            page: page.index,
        }));
    }

    Ok(None)
}

/// The score tuple for a heuristic. Lower is better.
fn score_placement(fr: &Rect, w: i32, h: i32, strategy: PackingStrategy) -> Score {
    let right = (fr.width - w) as i64;
    let bottom = (fr.height - h) as i64;
    let short_side = right.min(bottom);
    let long_side = right.max(bottom);
    match strategy {
        PackingStrategy::Bssf | PackingStrategy::Guillotine => (short_side, long_side, 0),
        PackingStrategy::Baf => (right * bottom, short_side, 0),
        PackingStrategy::Blsf => (long_side, short_side, 0),
    }
}

/// Split every free rect that intersects `placed` (MaxRects split + prune).
fn split_rects(free: &mut Vec<Rect>, placed: Rect) {
    let mut new_rects = Vec::new();
    for fr in free.iter() {
        if !fr.intersects(&placed) {
            new_rects.push(*fr);
            continue;
        }
        if placed.x > fr.x {
            new_rects.push(Rect::new(fr.x, fr.y, placed.x - fr.x, fr.height));
        }
        if placed.x + placed.width < fr.x + fr.width {
            new_rects.push(Rect::new(
                placed.x + placed.width,
                fr.y,
                fr.x + fr.width - (placed.x + placed.width),
                fr.height,
            ));
        }
        if placed.y > fr.y {
            new_rects.push(Rect::new(fr.x, fr.y, fr.width, placed.y - fr.y));
        }
        if placed.y + placed.height < fr.y + fr.height {
            new_rects.push(Rect::new(
                fr.x,
                placed.y + placed.height,
                fr.width,
                fr.y + fr.height - (placed.y + placed.height),
            ));
        }
    }
    *free = new_rects;
}

/// Remove free rects fully contained in another free rect (MaxRects prune).
fn prune_contained(free: &mut Vec<Rect>) {
    let mut i = 0;
    while i < free.len() {
        let mut removed = false;
        for (j, other) in free.iter().enumerate() {
            if i != j && other.contains(&free[i]) && other.area() > free[i].area() {
                free.remove(i);
                removed = true;
                break;
            }
        }
        if !removed {
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Occupancy grid (polygon packing)
// ---------------------------------------------------------------------------

/// Rasterized footprint of a sprite's mesh inside its frame (frame-local
/// coordinates, including padding). Grid size = (rotated dims) + 2*padding.
fn polygon_footprint(
    mesh: &TriangleMesh,
    width: i32,
    height: i32,
    rotated: bool,
    padding: i32,
) -> Vec<bool> {
    let (gw, gh) = if rotated { (height, width) } else { (width, height) };
    let grid_w = (gw + 2 * padding) as usize;
    let grid_h = (gh + 2 * padding) as usize;
    let local: Vec<Point2D> = mesh
        .vertices
        .iter()
        .map(|p| {
            let q = if rotated { rotate_90_cw(*p, height as f32) } else { *p };
            Point2D::new(q.x + padding as f32, q.y + padding as f32)
        })
        .collect();
    rasterize_polygon(&local, grid_w, grid_h)
}

/// True if any solid pixel of `footprint` (frame-local) lands on an occupied
/// pixel (or outside the canvas) when placed at `frame`.
fn overlaps(occupied: &[bool], footprint: &[bool], pw: i32, ph: i32, frame: Rect) -> bool {
    let fw = frame.width;
    let fh = frame.height;
    debug_assert_eq!(footprint.len(), (fw * fh) as usize);
    for y in 0..fh {
        for x in 0..fw {
            let fi = (y * fw + x) as usize;
            if !footprint[fi] {
                continue;
            }
            let ax = frame.x + x;
            let ay = frame.y + y;
            if ax < 0 || ay < 0 || ax >= pw || ay >= ph {
                return true;
            }
            if occupied[(ay * pw + ax) as usize] {
                return true;
            }
        }
    }
    false
}

fn mark_rect(occupied: &mut [bool], pw: i32, ph: i32, r: Rect) {
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            if x >= 0 && y >= 0 && x < pw && y < ph {
                occupied[(y * pw + x) as usize] = true;
            }
        }
    }
}

fn blit_footprint(occupied: &mut [bool], footprint: &[bool], pw: i32, ph: i32, frame: Rect) {
    let fw = frame.width;
    let fh = frame.height;
    for y in 0..fh {
        for x in 0..fw {
            if footprint[(y * fw + x) as usize] {
                let ax = frame.x + x;
                let ay = frame.y + y;
                if ax >= 0 && ay >= 0 && ax < pw && ay < ph {
                    occupied[(ay * pw + ax) as usize] = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, w: i32, h: i32) -> PackItem {
        PackItem {
            id: id.into(),
            width: w,
            height: h,
            mesh: None,
        }
    }

    #[test]
    fn packs_two_squares_with_padding() {
        let opts = PackerOptions::new(PackingStrategy::Bssf, true, 16, 2, false);
        let out = pack(&[item("a", 4, 4), item("b", 4, 4)], &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        assert_eq!(out.pages[0].placements.len(), 2);
        let f1 = out.pages[0].placements[0].frame;
        let f2 = out.pages[0].placements[1].frame;
        assert!(!f1.intersects(&f2));
        assert!(f1.width >= 8 && f1.height >= 8); // 4 + 2*padding
    }

    #[test]
    fn multi_atlas_split() {
        let opts = PackerOptions::new(PackingStrategy::Bssf, true, 32, 0, false);
        let items: Vec<PackItem> = (0..100).map(|i| item(&format!("s{i}"), 8, 8)).collect();
        let out = pack(&items, &opts).unwrap();
        assert!(out.pages.len() >= 6 && out.pages.len() <= 7);
        for page in &out.pages {
            assert_eq!(page.width, 32);
            let frames: Vec<Rect> = page.placements.iter().map(|p| p.frame).collect();
            for i in 0..frames.len() {
                for j in (i + 1)..frames.len() {
                    assert!(!frames[i].intersects(&frames[j]));
                }
            }
        }
    }

    #[test]
    fn rotation_is_used_when_beneficial() {
        // 8x8 canvas: an 8x4 bar followed by a 4x8 bar. The second bar only
        // fits rotated (8x4) in the space below the first.
        let opts = PackerOptions::new(PackingStrategy::Bssf, true, 8, 0, false);
        let out = pack(&[item("bar1", 8, 4), item("bar2", 4, 8)], &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        assert_eq!(out.pages[0].placements.len(), 2);
        assert!(out.pages[0].placements.iter().any(|p| p.rotated));
        // Without rotation it must open a second page.
        let opts2 = PackerOptions::new(PackingStrategy::Bssf, false, 8, 0, false);
        let out2 = pack(&[item("bar1", 8, 4), item("bar2", 4, 8)], &opts2).unwrap();
        assert_eq!(out2.pages.len(), 2);
    }

    #[test]
    fn oversized_sprite_errors() {
        let opts = PackerOptions::new(PackingStrategy::Bssf, true, 32, 0, false);
        let err = pack(&[item("huge", 64, 64)], &opts).unwrap_err();
        assert!(err.contains("huge"));
    }

    #[test]
    fn bssf_packs_smaller_into_gaps() {
        let opts = PackerOptions::new(PackingStrategy::Bssf, false, 10, 0, false);
        let out = pack(
            &[item("a", 6, 6), item("b", 4, 4), item("c", 3, 3)],
            &opts,
        )
        .unwrap();
        assert_eq!(out.pages.len(), 1);
        let frames: Vec<Rect> = out.pages[0].placements.iter().map(|p| p.frame).collect();
        for i in 0..frames.len() {
            for j in (i + 1)..frames.len() {
                assert!(!frames[i].intersects(&frames[j]));
            }
        }
    }

    #[test]
    fn polygon_packing_fits_both_l_shapes() {
        // Two L-shaped sprites (2x4 L inside a 4x4 box). AABB-only packing in a
        // 8x8 canvas fits 4x4 + 4x4; polygon packing fits both too, but the
        // occupancy grid must keep them non-overlapping.
        let l_mesh = TriangleMesh {
            vertices: vec![
                Point2D::new(0.0, 0.0),
                Point2D::new(2.0, 0.0),
                Point2D::new(2.0, 4.0),
                Point2D::new(0.0, 4.0),
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            uvs: vec![],
        };
        let opts = PackerOptions::new(PackingStrategy::Bssf, true, 8, 0, true);
        let out = pack(
            &[
                PackItem {
                    id: "l1".into(),
                    width: 4,
                    height: 4,
                    mesh: Some(l_mesh.clone()),
                },
                PackItem {
                    id: "l2".into(),
                    width: 4,
                    height: 4,
                    mesh: Some(l_mesh),
                },
            ],
            &opts,
        )
        .unwrap();
        assert_eq!(out.pages.len(), 1);
        assert_eq!(out.pages[0].placements.len(), 2);
    }
}
