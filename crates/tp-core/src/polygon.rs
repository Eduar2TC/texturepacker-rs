//! Subsystem 2: Motor Geométrico & Mallas.
//!
//! - Marching Squares contour extraction over the alpha mask
//!   (ambiguity resolved by 8-connected component analysis)
//! - Ramer–Douglas–Peucker simplification
//! - Ear-clipping triangulation; hole contours are bridged into their solid
//!   ring (the bridge vertices appear twice) so a donut mesh leaves the hole
//!   open instead of painting it with transparent texels
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
    // solid and produce no segments). Coordinates are translated back by the
    // one-pixel padding, plus the half-pixel corner-sample correction below.
    let pw = w + 2;
    let ph = h + 2;
    let mut padded = vec![0u8; (pw * ph) as usize];
    for y in 0..h {
        padded[((y + 1) * pw + 1) as usize..((y + 1) * pw + 1 + w) as usize]
            .copy_from_slice(&alpha[(y * w) as usize..((y + 1) * w) as usize]);
    }

    let loops = marching_squares(&padded, pw, ph);
    // Marching squares samples alpha at the pixel corners, so its isolines
    // land half a pixel *before* the true pixel edges: the +0.5 correction
    // puts the contours on the real boundaries (a sprite then meshes exactly
    // `[0, w] x [0, h]`, and its UVs sample texel centers).
    let loops: Vec<Vec<Point2D>> = loops
        .into_iter()
        .map(|l| {
            l.into_iter()
                .map(|p| Point2D::new(p.x - 0.5, p.y - 0.5))
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

    // Classify loops into solids (even nesting) and holes (odd), keeping each
    // hole with the solid that contains it directly.
    let (nesting, parent) = classify_nesting(&loops);

    let mut contours: Vec<Contour> = Vec::new();
    let mut vertices: Vec<Point2D> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let solids: Vec<usize> = (0..loops.len()).filter(|&i| nesting[i] % 2 == 0).collect();

    for &outer in &solids {
        contours.push(Contour {
            points: loops[outer].clone(),
            is_hole: false,
        });
        let holes: Vec<&[Point2D]> = (0..loops.len())
            .filter(|&i| nesting[i] % 2 == 1 && parent[i] == Some(outer))
            .map(|i| loops[i].as_slice())
            .collect();
        // El anillo puenteado lleva los vértices de los agujeros, así que la
        // malla los deja abiertos: cada vértice puente aparece dos veces.
        let mut ring = bridge_holes(&loops[outer], &holes);
        let mut tri = earcut(&ring);
        if tri.is_empty() && !holes.is_empty() {
            // Un clip degenerado sobre el anillo puenteado no debe dejar al
            // sólido sin malla: se rellena el agujero (como antes de los
            // puentes) y el contorno sigue enseñando el agujero.
            ring = loops[outer].clone();
            tri = earcut(&ring);
        }
        if tri.is_empty() {
            continue;
        }
        let base = vertices.len() as u32;
        vertices.extend_from_slice(&ring);
        indices.extend(tri.into_iter().map(|t| t + base));
    }

    for (i, loop_pts) in loops.iter().enumerate() {
        if nesting[i] % 2 == 1 {
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
    /// Triangulated mesh: every solid ring with its holes bridged in, so the
    /// mesh leaves holes open (each bridge vertex appears twice).
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
    if w == 0 || h == 0 || vertices.len() < 3 {
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
        // `total_cmp`, no `partial_cmp().unwrap()`: una malla con NaN en
        // las coordenadas (fichero corrupto) revienta el sort.
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks_exact(2) {
            let (x0, x1) = (pair[0], pair[1]);
            // Span NaN o fuera del lienzo (izquierda o derecha): no pinta
            // nada. Antes solo se recortaba la derecha, así que un span a la
            // izquierda acababa pintando el píxel 0.
            if !x0.is_finite() || !x1.is_finite() || x1 <= 0.0 || x0 >= w as f32 {
                continue;
            }
            // Un píxel se pinta si la intersección con el span tiene medida
            // positiva: x recorre [floor(x0), ceil(x1) - 1]. El antiguo
            // ceil(x0)..=floor(x1) pintaba de más con extremos enteros (el
            // span [0, 1] mojaba también el píxel 1) y de menos con
            // extremos fraccionarios (el span [0.5, 2.5] se comía el 0).
            let from = x0.floor().max(0.0) as usize;
            let to = (x1.ceil() as usize)
                .saturating_sub(1)
                .min(w.saturating_sub(1));
            for x in from..=to {
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
                // Three solid corners (single empty corner): the isoline
                // wraps the empty corner, so it crosses exactly the two
                // edges that touch it — an edge lies on the isoline iff its
                // two corners differ, and the other two edges join solid
                // corners (same edges as the mirrored single-solid case).
                7 => segments.push((ml, mt)),  // empty TL
                11 => segments.push((mt, mr)), // empty TR
                13 => segments.push((mr, mb)), // empty BR
                14 => segments.push((mb, ml)), // empty BL
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

/// For each loop, return its nesting depth (0 = outermost) and its immediate
/// container (`None` at depth 0). Even depths are solid regions (silhouettes
/// and islands); odd depths are holes punched into the loop that contains
/// them.
fn classify_nesting(loops: &[Vec<Point2D>]) -> (Vec<usize>, Vec<Option<usize>>) {
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
    // The immediate container of j is its deepest known container: exactly the
    // loop one level up in the nesting that `depth` just measured.
    let parent = (0..n)
        .map(|j| {
            (0..n)
                .filter(|&i| contains[i][j])
                .max_by_key(|&i| depth[i])
                .filter(|&i| depth[i] + 1 == depth[j])
        })
        .collect();
    (depth, parent)
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

/// Triangulate a simple polygon into triangle indices. Holes must already be
/// spliced in by [`bridge_holes`]; the duplicated bridge vertices are
/// absorbed by the collinear cleanup below.
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
// Hole bridging
// ---------------------------------------------------------------------------

/// Splice every hole into the outer ring through the shortest mutually
/// visible bridge.
///
/// Outer and holes are normalized to opposite orientations first, so the
/// combined ring has area `outer - holes` and the plain ear clipper
/// triangulates the donut without any hole-aware logic. Each bridge vertex
/// shows up twice in the result (the classic weakly-simple splice).
fn bridge_holes(outer: &[Point2D], holes: &[&[Point2D]]) -> Vec<Point2D> {
    let mut ring = oriented(outer, true);
    let holes: Vec<Vec<Point2D>> = holes.iter().map(|h| oriented(h, false)).collect();
    for hole in &holes {
        if hole.len() < 3 {
            continue;
        }
        ring = splice_hole(&ring, hole, &holes);
    }
    ring
}

/// Copy of `pts` with positive area when `positive`, negative otherwise
/// (the ear clipper normalizes again, so only the sign pair matters).
fn oriented(pts: &[Point2D], positive: bool) -> Vec<Point2D> {
    if (signed_area(pts) > 0.0) != positive {
        pts.iter().rev().copied().collect()
    } else {
        pts.to_vec()
    }
}

/// Insert `hole` (negative area) into `ring` (positive area) across the
/// shortest segment that crosses no edge of the geometry. When every
/// candidate bridge is blocked the ring is returned unchanged.
fn splice_hole(ring: &[Point2D], hole: &[Point2D], loops: &[Vec<Point2D>]) -> Vec<Point2D> {
    let obstacles: Vec<&[Point2D]> = std::iter::once(ring)
        .chain(loops.iter().map(Vec::as_slice))
        .collect();
    let Some((p, m)) = shortest_visible_bridge(ring, hole, &obstacles) else {
        return ring.to_vec();
    };
    // O0..Op, puente a Hm, la vuelta al agujero desde m+1 hasta m, puente de
    // vuelta a Op y el resto del anillo: A y B quedan duplicados.
    let mut out = Vec::with_capacity(ring.len() + hole.len() + 2);
    out.extend_from_slice(&ring[..=p]);
    out.push(hole[m]);
    out.extend_from_slice(&hole[m + 1..]);
    out.extend_from_slice(&hole[..=m]);
    out.push(ring[p]);
    out.extend_from_slice(&ring[p + 1..]);
    out
}

/// Indices `(ring, hole)` of the shortest segment joining both loops that
/// crosses no edge of the geometry; `None` when every candidate is blocked.
fn shortest_visible_bridge(
    ring: &[Point2D],
    hole: &[Point2D],
    obstacles: &[&[Point2D]],
) -> Option<(usize, usize)> {
    let mut best: Option<(f32, usize, usize)> = None;
    for (m, &b) in hole.iter().enumerate() {
        for (p, &a) in ring.iter().enumerate() {
            let dx = a.x - b.x;
            let dy = a.y - b.y;
            let d2 = dx * dx + dy * dy;
            if d2 <= 1e-12 {
                continue; // misma posición: no hay puente que construir
            }
            if best.is_some_and(|(best_d2, _, _)| d2 >= best_d2) {
                continue;
            }
            if crosses_nothing(b, a, obstacles) {
                best = Some((d2, p, m));
            }
        }
    }
    best.map(|(_, p, m)| (p, m))
}

/// True when the segment runs from `b` to `a` without properly crossing any
/// edge of any loop. Both endpoints sit on the geometry, so the edges they
/// touch never count as crossings — that is how the bridge attaches.
fn crosses_nothing(b: Point2D, a: Point2D, loops: &[&[Point2D]]) -> bool {
    loops.iter().all(|pts| {
        let n = pts.len();
        (0..n).all(|i| !segments_properly_cross(b, a, pts[i], pts[(i + 1) % n]))
    })
}

/// Strict crossing: the segments share interior points. Touching at an
/// endpoint or running along a common line is not a crossing (holes from
/// marching squares never touch the outer loop, so a bridge can only graze
/// an unrelated vertex, and the ear clipper's collinear cleanup absorbs it).
fn segments_properly_cross(p1: Point2D, p2: Point2D, q1: Point2D, q2: Point2D) -> bool {
    let d1 = cross(q1, q2, p1);
    let d2 = cross(q1, q2, p2);
    let d3 = cross(p1, p2, q1);
    let d4 = cross(p1, p2, q2);
    ((d1 > 1e-6 && d2 < -1e-6) || (d1 < -1e-6 && d2 > 1e-6))
        && ((d3 > 1e-6 && d4 < -1e-6) || (d3 < -1e-6 && d4 > 1e-6))
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

    /// Rasterize every triangle of the mesh into one coverage grid: the union
    /// is what a consumer painting the mesh would fill.
    fn rasterize_mesh(vertices: &[Point2D], indices: &[u32], w: i32, h: i32) -> Vec<bool> {
        let mut grid = vec![false; (w * h) as usize];
        for t in indices.chunks_exact(3) {
            let tri = [
                vertices[t[0] as usize],
                vertices[t[1] as usize],
                vertices[t[2] as usize],
            ];
            for (cell, &on) in rasterize_polygon(&tri, w as usize, h as usize)
                .iter()
                .enumerate()
            {
                grid[cell] |= on;
            }
        }
        grid
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

    /// A corrupt mesh can carry NaN coordinates: the scanline sort used to
    /// `unwrap()` a `partial_cmp` that returns `None` for them.
    #[test]
    fn rasterize_survives_nan_vertices() {
        let verts = [
            Point2D::new(0.0, 0.0),
            Point2D::new(f32::NAN, 4.0),
            Point2D::new(8.0, 8.0),
        ];
        let mask = rasterize_polygon(&verts, 8, 8);
        assert_eq!(mask.len(), 64);
    }

    /// A span entirely left of the canvas paints nothing: the old clamp
    /// only trimmed the right side, so `ceil().max(0)` and
    /// `floor().min(w-1)` both landed on pixel 0 and painted it.
    #[test]
    fn rasterize_skips_spans_outside_the_left_edge() {
        let verts = [
            Point2D::new(-10.0, 0.0),
            Point2D::new(-5.0, 0.0),
            Point2D::new(-5.0, 8.0),
            Point2D::new(-10.0, 8.0),
        ];
        let mask = rasterize_polygon(&verts, 8, 8);
        assert!(
            !mask.iter().any(|&b| b),
            "painted pixels from a polygon that is entirely off-canvas"
        );
    }

    #[test]
    fn donut_reports_hole_contour() {
        let a = alpha_from_shape(8, 8, |x, y| x == 0 || y == 0 || x == 7 || y == 7);
        let poly = build_polygons(&a, 8, 8, 0.01).unwrap();
        assert!(poly.contours.iter().any(|c| c.is_hole));
        assert!(!poly.mesh.indices.is_empty());
        let n = poly.mesh.vertices.len() as u32;
        assert!(poly.mesh.indices.iter().all(|&i| i < n));

        // La malla debe pintar el anillo y dejar el agujero al aire: antes
        // del puente el exterior relleno cubría también el centro.
        let mask = rasterize_mesh(&poly.mesh.vertices, &poly.mesh.indices, 8, 8);
        let at = |x: i32, y: i32| mask[(y * 8 + x) as usize];
        assert!(at(0, 3), "borde izquierdo");
        assert!(at(3, 0), "borde superior");
        assert!(!at(3, 3), "el centro del donut debe quedar sin cubrir");
        assert!(!at(4, 4), "el centro del donut debe quedar sin cubrir");
    }

    /// La suma de áreas de los triángulos es exactamente exterior menos
    /// agujeros: ni el relleno viejo (de más) ni un agujero comido (de
    /// menos) pasan esta invariant.
    #[test]
    fn donut_mesh_area_is_outer_minus_hole() {
        let a = alpha_from_shape(9, 9, |x, y| x == 0 || y == 0 || x == 8 || y == 8);
        let poly = build_polygons(&a, 9, 9, 0.01).unwrap();
        let mut expected = 0.0f32;
        for c in &poly.contours {
            let area = signed_area(&c.points).abs();
            expected = if c.is_hole {
                expected - area
            } else {
                expected + area
            };
        }
        let mut got = 0.0f32;
        for t in poly.mesh.indices.chunks_exact(3) {
            let tri = [
                poly.mesh.vertices[t[0] as usize],
                poly.mesh.vertices[t[1] as usize],
                poly.mesh.vertices[t[2] as usize],
            ];
            got += signed_area(&tri).abs();
        }
        assert!(expected > 0.0, "el contorno debe tener área");
        assert!(
            (got - expected).abs() <= 0.01 * expected,
            "malla {got} frente a contornos {expected}"
        );
    }

    /// Una isla dentro de un agujero es sólida por paridad (profundidad par)
    /// y malla con el anillo que la rodea: dos sólidos y un agujero.
    #[test]
    fn island_inside_a_hole_is_meshed_as_a_solid() {
        let a = alpha_from_shape(8, 8, |x, y| {
            let borde = x == 0 || y == 0 || x == 7 || y == 7;
            let isla = (3..=4).contains(&x) && (3..=4).contains(&y);
            borde || isla
        });
        let poly = build_polygons(&a, 8, 8, 0.01).unwrap();
        let solids = poly.contours.iter().filter(|c| !c.is_hole).count();
        let holes = poly.contours.iter().filter(|c| c.is_hole).count();
        assert_eq!(solids, 2, "anillo e isla");
        assert_eq!(holes, 1, "el hueco que rodea la isla");

        let mask = rasterize_mesh(&poly.mesh.vertices, &poly.mesh.indices, 8, 8);
        let at = |x: i32, y: i32| mask[(y * 8 + x) as usize];
        assert!(at(0, 3), "el anillo sí se malla");
        assert!(at(3, 3), "la isla sí se malla");
        assert!(!at(1, 1), "el hueco entre anillo e isla queda al aire");
        assert!(!at(6, 6), "el hueco entre anillo e isla queda al aire");
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
