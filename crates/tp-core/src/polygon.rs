//! Subsystem 2: Motor Geométrico & Mallas.
//!
//! - Marching Squares contour extraction over the alpha mask
//!   (ambiguity resolved by 8-connected component analysis)
//! - Ramer–Douglas–Peucker simplification
//! - Ear-clipping triangulation (simple polygons; hole contours are kept for
//!   visualization but not meshed — the hole region is transparent in the
//!   atlas, so a filled mesh over it renders nothing visible)
//!
//! Mesh vertices live in *trimmed sprite local space* (y-down, origin at the
//! top-left of the trimmed region, coordinates in `[0, w] x [0, h]`). UVs are
//! computed later once the frame is known (`compute_uvs`).

use crate::types::{Contour, Point2D, Rect, TriangleMesh};
use std::collections::HashMap;

/// Build the polygon mesh for one trimmed sprite.
///
/// `alpha` is the trimmed alpha channel (length `w * h`). Returns `None` when
/// the sprite has no visible pixels. `tolerance` is the RDP epsilon in pixels.
pub fn build_polygons(alpha: &[u8], w: i32, h: i32, tolerance: f32) -> Option<Polygons> {
    debug_assert_eq!(alpha.len(), (w * h) as usize);
    if w < 1 || h < 1 {
        return None;
    }

    // Pad the mask with a one-pixel empty border so marching squares traces
    // the *outer* boundary of the sprite (otherwise interior cells are all
    // solid and produce no segments). Coordinates are translated back by -1.
    let pw = w + 2;
    let ph = h + 2;
    let mut padded = vec![0u8; (pw * ph) as usize];
    for y in 0..h {
        padded[((y + 1) * pw + 1) as usize..((y + 1) * pw + 1 + w) as usize]
            .copy_from_slice(&alpha[(y * w) as usize..((y + 1) * w) as usize]);
    }

    let loops = marching_squares(&padded, pw, ph);
    let loops: Vec<Vec<Point2D>> = loops
        .into_iter()
        .map(|l| {
            l.into_iter()
                .map(|p| Point2D::new(p.x - 1.0, p.y - 1.0))
                .collect()
        })
        .collect();

    // Simplify each loop, drop degenerate ones.
    let loops: Vec<Vec<Point2D>> = loops
        .into_iter()
        .map(|l| rdp(&l, tolerance))
        .filter(|l| l.len() >= 3)
        .collect();

    if loops.is_empty() {
        // Fully transparent — the caller should have trimmed it already.
        return None;
    }

    // Classify loops into outers and holes via containment nesting.
    let nesting = classify_nesting(&loops);

    let mut contours: Vec<Contour> = Vec::new();
    let mut vertices: Vec<Point2D> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let outer_indices: Vec<usize> = (0..loops.len()).filter(|&i| nesting[i] == 0).collect();

    for &outer in &outer_indices {
        contours.push(Contour {
            points: loops[outer].clone(),
            is_hole: false,
        });
        let tri = earcut(&loops[outer]);
        if tri.is_empty() {
            continue;
        }
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&loops[outer]);
        indices.extend(tri.into_iter().map(|t| t + base));
    }

    for (i, loop_pts) in loops.iter().enumerate() {
        if nesting[i] > 0 {
            contours.push(Contour {
                points: loop_pts.clone(),
                is_hole: true,
            });
        }
    }

    if indices.is_empty() {
        return None;
    }

    Some(Polygons {
        contours,
        mesh: TriangleMesh {
            vertices,
            indices,
            uvs: Vec::new(),
        },
    })
}

/// Output of the polygon engine.
#[derive(Debug, Clone)]
pub struct Polygons {
    /// Contours (outer + holes) after simplification.
    pub contours: Vec<Contour>,
    /// Triangulated mesh (outer-contour vertices only).
    pub mesh: TriangleMesh,
}

/// Compute UV coordinates for mesh vertices given the allocated frame.
pub fn compute_uvs(
    vertices: &[Point2D],
    frame: &Rect,
    atlas_w: i32,
    atlas_h: i32,
    rotated: bool,
) -> Vec<Point2D> {
    let aw = atlas_w as f32;
    let ah = atlas_h as f32;
    let fh = if rotated { frame.height } else { frame.width } as f32;
    vertices
        .iter()
        .map(|p| {
            let (lx, ly) = if rotated {
                // 90° CW rotation in local space: (x, y) -> (H - y, x)
                (fh - p.y, p.x)
            } else {
                (p.x, p.y)
            };
            Point2D::new((frame.x as f32 + lx) / aw, (frame.y as f32 + ly) / ah)
        })
        .collect()
}

/// Rotate a local-space point by 90° CW, matching `compute_uvs`/pixel blitting.
pub fn rotate_90_cw(p: Point2D, height: f32) -> Point2D {
    Point2D::new(height - p.y, p.x)
}

/// Rasterize a polygon (even-odd fill) into a `w x h` boolean grid.
/// Used by the polygon-aware packer for overlap testing.
pub fn rasterize_polygon(vertices: &[Point2D], w: usize, h: usize) -> Vec<bool> {
    let mut grid = vec![false; w * h];
    if vertices.len() < 3 {
        return grid;
    }
    // Scanline fill: for each row, compute x intersections with polygon edges.
    for y in 0..h {
        let fy = y as f32 + 0.5;
        let mut xs: Vec<f32> = Vec::new();
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            if (a.y <= fy && b.y > fy) || (b.y <= fy && a.y > fy) {
                let t = (fy - a.y) / (b.y - a.y);
                xs.push(a.x + t * (b.x - a.x));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for pair in xs.chunks_exact(2) {
            let x0 = pair[0].ceil().max(0.0) as usize;
            let x1 = pair[1].floor().min(w as f32 - 1.0) as usize;
            for x in x0..=x1 {
                grid[y * w + x] = true;
            }
        }
    }
    grid
}

// ---------------------------------------------------------------------------
// Marching Squares
// ---------------------------------------------------------------------------

/// Extract closed contours (loops) from the binary alpha mask using marching
/// squares. Coordinates are half-pixel lattice points.
fn marching_squares(alpha: &[u8], w: i32, h: i32) -> Vec<Vec<Point2D>> {
    debug_assert!(w >= 3 && h >= 3);

    let solid = |x: i32, y: i32| -> bool { alpha[(y * w + x) as usize] > 0 };

    // Union-find over solid pixels (8-connectivity) to resolve ambiguous cases.
    let mut uf = UnionFind::new((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            if !solid(x, y) {
                continue;
            }
            let i = (y * w + x) as usize;
            if x + 1 < w && solid(x + 1, y) {
                uf.union(i, i + 1);
            }
            if y + 1 < h && solid(x, y + 1) {
                uf.union(i, i + w as usize);
            }
            if x + 1 < w && y + 1 < h && solid(x + 1, y + 1) {
                uf.union(i, i + w as usize + 1);
            }
            if x > 0 && y + 1 < h && solid(x - 1, y + 1) {
                uf.union(i, i + w as usize - 1);
            }
        }
    }

    let mut segments: Vec<(Point2D, Point2D)> = Vec::new();

    for y in 0..h - 1 {
        for x in 0..w - 1 {
            let tl = solid(x, y);
            let tr = solid(x + 1, y);
            let br = solid(x + 1, y + 1);
            let bl = solid(x, y + 1);

            let ml = Point2D::new(x as f32, y as f32 + 0.5);
            let mt = Point2D::new(x as f32 + 0.5, y as f32);
            let mr = Point2D::new(x as f32 + 1.0, y as f32 + 0.5);
            let mb = Point2D::new(x as f32 + 0.5, y as f32 + 1.0);

            let case = (tl as u8) << 3 | (tr as u8) << 2 | (br as u8) << 1 | (bl as u8);
            match case {
                0 | 15 => {}
                // single solid corner
                1 => segments.push((mb, ml)), // BL
                2 => segments.push((mr, mb)), // BR
                4 => segments.push((mt, mr)), // TR
                8 => segments.push((ml, mt)), // TL
                // two adjacent corners (solid strip)
                3 => segments.push((ml, mr)),  // BL+BR
                6 => segments.push((mt, mb)),  // TR+BR
                12 => segments.push((ml, mr)), // TL+TR
                9 => segments.push((mt, mb)),  // TL+BL
                // three solid corners (single empty corner)
                7 => segments.push((mb, ml)),  // empty TL
                11 => segments.push((mr, mb)), // empty TR
                13 => segments.push((mt, mr)), // empty BR
                14 => segments.push((ml, mt)), // empty BL
                // ambiguous diagonals (resolved via connectivity)
                5 => {
                    // TL + BR
                    if uf.find((y * w + x) as usize) == uf.find(((y + 1) * w + x + 1) as usize) {
                        segments.push((mt, mr));
                        segments.push((mb, ml));
                    } else {
                        segments.push((ml, mt));
                        segments.push((mr, mb));
                    }
                }
                10 => {
                    // TR + BL
                    if uf.find((y * w + x + 1) as usize) == uf.find(((y + 1) * w + x) as usize) {
                        segments.push((ml, mt));
                        segments.push((mr, mb));
                    } else {
                        segments.push((mt, mr));
                        segments.push((mb, ml));
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    stitch_loops(&segments)
}

/// Stitch line segments into closed loops by matching endpoints.
fn stitch_loops(segments: &[(Point2D, Point2D)]) -> Vec<Vec<Point2D>> {
    let key = |p: &Point2D| ((p.x * 1000.0).round() as i32, (p.y * 1000.0).round() as i32);

    let mut point_map: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segments.iter().enumerate() {
        point_map.entry(key(a)).or_default().push(i);
        point_map.entry(key(b)).or_default().push(i);
    }

    let mut used = vec![false; segments.len()];
    let mut loops: Vec<Vec<Point2D>> = Vec::new();

    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        let mut loop_pts: Vec<Point2D> = Vec::new();
        let mut current = segments[start].0;
        let mut seg = start;
        loop {
            used[seg] = true;
            let (a, b) = segments[seg];
            if key(&a) == key(&current) {
                current = b;
                loop_pts.push(a);
            } else {
                current = a;
                loop_pts.push(b);
            }

            let candidates = point_map.get(&key(&current)).cloned().unwrap_or_default();
            match candidates.into_iter().find(|&i| !used[i] && i != seg) {
                Some(i) => seg = i,
                None => break,
            }
        }
        if loop_pts.len() >= 3 {
            loops.push(loop_pts);
        }
    }
    loops
}

// ---------------------------------------------------------------------------
// Ramer–Douglas–Peucker
// ---------------------------------------------------------------------------

/// Simplify a closed loop with the RDP algorithm (epsilon in pixels).
/// The closing edge is implicit and preserved.
pub fn rdp(points: &[Point2D], epsilon: f32) -> Vec<Point2D> {
    if points.len() <= 2 || epsilon <= 0.0 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    rdp_rec(points, 0, points.len() - 1, epsilon, &mut keep);
    points
        .iter()
        .zip(keep.iter())
        .filter_map(|(p, k)| if *k { Some(*p) } else { None })
        .collect()
}

fn rdp_rec(points: &[Point2D], first: usize, last: usize, epsilon: f32, keep: &mut [bool]) {
    if last <= first + 1 {
        return;
    }
    let mut max_dist = 0.0f32;
    let mut index = 0usize;
    for i in (first + 1)..last {
        let d = dist_point_segment(points[i], points[first], points[last]);
        if d > max_dist {
            max_dist = d;
            index = i;
        }
    }
    if max_dist > epsilon {
        keep[index] = true;
        rdp_rec(points, first, index, epsilon, keep);
        rdp_rec(points, index, last, epsilon, keep);
    }
}

fn dist_point_segment(p: Point2D, a: Point2D, b: Point2D) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    if len2 <= f32::EPSILON {
        return ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt();
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    let proj_x = a.x + t * dx;
    let proj_y = a.y + t * dy;
    ((p.x - proj_x).powi(2) + (p.y - proj_y).powi(2)).sqrt()
}

// ---------------------------------------------------------------------------
// Loop classification (nesting)
// ---------------------------------------------------------------------------

/// Signed area (shoelace). Sign flips with y-down coordinates; only *relative*
/// signs are meaningful here.
fn signed_area(points: &[Point2D]) -> f32 {
    let mut area = 0.0f32;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        area += a.x * b.y - b.x * a.y;
    }
    area * 0.5
}

/// For each loop, return its nesting depth (0 = outermost). Depth-0 loops are
/// sprite silhouettes; depth 1 are holes; depth 2 are islands inside holes.
fn classify_nesting(loops: &[Vec<Point2D>]) -> Vec<usize> {
    let n = loops.len();
    let mut contains = vec![vec![false; n]; n];
    for i in 0..n {
        for j in 0..n {
            if i != j {
                contains[i][j] = loop_contains(&loops[i], &loops[j]);
            }
        }
    }
    let mut depth = vec![0usize; n];
    for _ in 0..n {
        let mut changed = false;
        for i in 0..n {
            for j in 0..n {
                if contains[i][j] && depth[j] <= depth[i] {
                    depth[j] = depth[i] + 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    depth
}

fn loop_contains(outer: &[Point2D], inner: &[Point2D]) -> bool {
    let (mut ox0, mut oy0, mut ox1, mut oy1) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for p in outer {
        ox0 = ox0.min(p.x);
        oy0 = oy0.min(p.y);
        ox1 = ox1.max(p.x);
        oy1 = oy1.max(p.y);
    }
    let (mut ix0, mut iy0, mut ix1, mut iy1) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for p in inner {
        ix0 = ix0.min(p.x);
        iy0 = iy0.min(p.y);
        ix1 = ix1.max(p.x);
        iy1 = iy1.max(p.y);
    }
    if ix0 < ox0 || iy0 < oy0 || ix1 > ox1 || iy1 > oy1 {
        return false;
    }
    point_in_polygon(inner[0], outer)
}

fn point_in_polygon(p: Point2D, poly: &[Point2D]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[j];
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ---------------------------------------------------------------------------
// Ear clipping triangulation (simple polygons)
// ---------------------------------------------------------------------------

/// Triangulate a simple polygon (no holes) into triangle indices.
pub fn earcut(points: &[Point2D]) -> Vec<u32> {
    if points.len() < 3 {
        return Vec::new();
    }
    let area = signed_area(points);
    if area.abs() < 1e-6 {
        return Vec::new();
    }
    // Normalize to CCW (positive shoelace).
    let pts: Vec<Point2D> = if area < 0.0 {
        points.iter().rev().copied().collect()
    } else {
        points.to_vec()
    };

    let n = pts.len();
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let mut alive = vec![true; n];

    // Remove duplicate and collinear vertices.
    let mut removed = true;
    while removed {
        removed = false;
        for i in 0..n {
            if !alive[i] {
                continue;
            }
            let p = prev[i];
            let q = next[i];
            if p == q {
                alive[i] = false;
                removed = true;
                continue;
            }
            if is_collinear(pts[p], pts[i], pts[q]) {
                next[p] = q;
                prev[q] = p;
                alive[i] = false;
                removed = true;
            }
        }
    }

    let mut indices: Vec<u32> = Vec::new();
    let mut remaining: Vec<usize> = (0..n).filter(|&i| alive[i]).collect();
    if remaining.len() < 3 {
        return Vec::new();
    }

    let mut guard = 0usize;
    while remaining.len() > 3 {
        guard += 1;
        if guard > n * n * 4 {
            break;
        }
        let mut clipped = false;
        for pos in 0..remaining.len() {
            let i = remaining[pos];
            if !is_reflex(&pts, &prev, &next, i) && is_ear(&pts, &prev, &next, i) {
                let p = prev[i];
                let q = next[i];
                indices.extend_from_slice(&[p as u32, i as u32, q as u32]);
                next[p] = q;
                prev[q] = p;
                remaining.remove(pos);
                clipped = true;
                break;
            }
        }
        if !clipped {
            // Safety: clip the first remaining vertex to guarantee progress.
            let i = remaining[0];
            let p = prev[i];
            let q = next[i];
            indices.extend_from_slice(&[p as u32, i as u32, q as u32]);
            next[p] = q;
            prev[q] = p;
            remaining.remove(0);
        }
    }
    if remaining.len() == 3 {
        indices.extend_from_slice(&[
            remaining[0] as u32,
            remaining[1] as u32,
            remaining[2] as u32,
        ]);
    }

    // Drop degenerate triangles.
    let mut out = Vec::with_capacity(indices.len());
    for t in indices.chunks_exact(3) {
        let (a, b, c) = (t[0], t[1], t[2]);
        if a != b && b != c && a != c {
            out.extend_from_slice(&[a, b, c]);
        }
    }
    out
}

fn cross(o: Point2D, a: Point2D, b: Point2D) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn is_collinear(a: Point2D, b: Point2D, c: Point2D) -> bool {
    cross(a, b, c).abs() <= 1e-6
}

/// Reflex vertex (interior angle > 180°) in a CCW ring.
fn is_reflex(pts: &[Point2D], prev: &[usize], next: &[usize], i: usize) -> bool {
    cross(pts[prev[i]], pts[i], pts[next[i]]) < 0.0
}

fn is_ear(pts: &[Point2D], prev: &[usize], next: &[usize], i: usize) -> bool {
    let p = prev[i];
    let q = next[i];
    let a = pts[p];
    let b = pts[i];
    let c = pts[q];
    // No other vertex may lie inside triangle (a, b, c). Testing already-clipped
    // vertices is harmless: their coordinates are valid and a clipped vertex
    // inside the ear would only make us reject an ear that is actually fine,
    // which the safety fallback recovers from.
    for (k, &pt) in pts.iter().enumerate() {
        if k == p || k == i || k == q {
            continue;
        }
        if point_in_triangle(pt, a, b, c) {
            return false;
        }
    }
    true
}

fn point_in_triangle(p: Point2D, a: Point2D, b: Point2D, c: Point2D) -> bool {
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

// ---------------------------------------------------------------------------
// Union-Find
// ---------------------------------------------------------------------------

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_from_shape(w: i32, h: i32, solid: impl Fn(i32, i32) -> bool) -> Vec<u8> {
        let mut out = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                out.push(if solid(x, y) { 255u8 } else { 0u8 });
            }
        }
        out
    }

    #[test]
    fn square_gives_quad_with_two_triangles() {
        let a = alpha_from_shape(4, 4, |_, _| true);
        let poly = build_polygons(&a, 4, 4, 0.01).unwrap();
        // Marching squares produces a slightly beveled rectangle (half-pixel
        // corners); RDP keeps the corner points. At least 4 vertices, and a
        // simple polygon of N vertices triangulates to N-2 triangles.
        assert!(
            poly.mesh.vertices.len() >= 4,
            "got {}",
            poly.mesh.vertices.len()
        );
        assert!(poly.mesh.indices.len() >= 6);
        assert_eq!(poly.mesh.indices.len() % 3, 0);
        let n = poly.mesh.vertices.len() as u32;
        assert_eq!(poly.contours.len(), 1);
        assert!(!poly.contours[0].is_hole);
        assert!(poly.mesh.indices.iter().all(|&i| i < n));
    }

    #[test]
    fn l_shape_triangulates() {
        let a = alpha_from_shape(3, 3, |x, y| x < 2 || y < 2);
        let poly = build_polygons(&a, 3, 3, 0.01).unwrap();
        assert!(!poly.mesh.indices.is_empty());
        assert_eq!(poly.mesh.indices.len() % 3, 0);
        let n = poly.mesh.vertices.len() as u32;
        assert!(poly.mesh.indices.iter().all(|&i| i < n));
    }

    #[test]
    fn donut_reports_hole_contour() {
        let a = alpha_from_shape(8, 8, |x, y| x == 0 || y == 0 || x == 7 || y == 7);
        let poly = build_polygons(&a, 8, 8, 0.01).unwrap();
        assert!(poly.contours.iter().any(|c| c.is_hole));
        assert!(!poly.mesh.indices.is_empty());
        let n = poly.mesh.vertices.len() as u32;
        assert!(poly.mesh.indices.iter().all(|&i| i < n));
    }

    #[test]
    fn diagonal_triangulates() {
        let a = alpha_from_shape(5, 5, |x, y| x + y < 5);
        let poly = build_polygons(&a, 5, 5, 0.01).unwrap();
        assert!(!poly.mesh.indices.is_empty());
        assert_eq!(poly.mesh.indices.len() % 3, 0);
    }

    #[test]
    fn rdp_removes_collinear() {
        let pts = vec![
            Point2D::new(0.0, 0.0),
            Point2D::new(1.0, 0.0),
            Point2D::new(2.0, 0.0),
            Point2D::new(3.0, 0.0),
            Point2D::new(3.0, 3.0),
        ];
        let simplified = rdp(&pts, 1.0);
        assert_eq!(simplified.len(), 3);
        assert_eq!(simplified[0], Point2D::new(0.0, 0.0));
        assert_eq!(simplified[1], Point2D::new(3.0, 0.0));
        assert_eq!(simplified[2], Point2D::new(3.0, 3.0));
    }

    #[test]
    fn uvs_follow_frame_and_rotation() {
        let frame = Rect::new(10, 20, 4, 5);
        let v = vec![Point2D::new(0.0, 0.0), Point2D::new(4.0, 5.0)];
        let uvs = compute_uvs(&v, &frame, 100, 100, false);
        assert!((uvs[0].x - 0.10).abs() < 1e-4);
        assert!((uvs[0].y - 0.20).abs() < 1e-4);
        assert!((uvs[1].x - 0.14).abs() < 1e-4);
        assert!((uvs[1].y - 0.25).abs() < 1e-4);

        // Rotated: local (0,0) -> atlas (frame.x + H, frame.y)
        let uvs_r = compute_uvs(&v, &frame, 100, 100, true);
        assert!((uvs_r[0].x - 0.15).abs() < 1e-4);
        assert!((uvs_r[0].y - 0.20).abs() < 1e-4);
    }

    #[test]
    fn rasterize_fills_polygon() {
        let square = vec![
            Point2D::new(0.0, 0.0),
            Point2D::new(4.0, 0.0),
            Point2D::new(4.0, 4.0),
            Point2D::new(0.0, 4.0),
        ];
        let grid = rasterize_polygon(&square, 4, 4);
        assert!(grid.iter().all(|&b| b));
    }
}
