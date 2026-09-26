//! Subsystem 3: Empaquetado Espacial (Packing Core).
//!
//! - Algorithms (docs: *Algorithm*): MaxRects, Guillotine, Grid and Basic
//! - MaxRects heuristics: BSSF / BAF / BLSF / BottomLeft / ContactPoint / Best
//! - Size search (docs: *Pack*): Fast / Good / Best
//! - Size constraints (docs: *Size Constraints*): AnySize / POT / MultipleOf4 /
//!   WordAligned, fixed size (`fixed_width`/`fixed_height`) and force-squared
//! - 90° rotation support
//! - Multi-atlas auto-split (new page when nothing fits)
//! - Polygon-aware placement: AABB placement from MaxRects is validated against
//!   a per-page occupancy grid rasterized from the sprite mesh, so polygon
//!   sprites can be packed tighter than their bounding boxes.

use crate::error::{Result, TpError};
use std::time::{Duration, Instant};

use crate::config::{
    BasicSortBy, PackMode, PackingAlgorithm, PackingStrategy, SizeConstraint, SortOrder,
};
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
    /// Gap between neighbouring sprites (docs: *Shape padding*).
    pub padding: i32,
    /// Reserved margin between the sprites and the sheet border
    /// (docs: *Border padding*).
    pub border_padding: i32,
    pub polygon_mode: bool,
    /// Packing algorithm family (docs: *Algorithm*).
    pub algorithm: PackingAlgorithm,
    /// Effort spent searching the minimum atlas size (docs: *Pack*).
    pub pack_mode: PackMode,
    /// Required atlas dimensions (docs: *Size Constraints*).
    pub size_constraints: SizeConstraint,
    /// Force a square atlas (docs: *Force squared*).
    pub force_squared: bool,
    /// Fixed atlas width; `0` = decided by the packer (docs: *Fixed Size*).
    pub fixed_width: i32,
    /// Fixed atlas height; `0` = decided by the packer (docs: *Fixed Size*).
    pub fixed_height: i32,
    /// Sort criterion of the Basic algorithm (docs: *Sort by*).
    pub basic_sort_by: BasicSortBy,
    /// Sort direction of the Basic algorithm (docs: *Order*).
    pub basic_order: SortOrder,
    /// Width alignment in pixels for `WordAligned` (1 = no alignment).
    pub word_align_mod: i32,
}

impl PackerOptions {
    pub fn new(
        strategy: PackingStrategy,
        allow_rotation: bool,
        max_size: i32,
        padding: i32,
        border_padding: i32,
        polygon_mode: bool,
    ) -> Self {
        Self {
            strategy,
            allow_rotation,
            max_size,
            padding,
            border_padding,
            polygon_mode,
            algorithm: PackingAlgorithm::MaxRects,
            pack_mode: PackMode::default(),
            size_constraints: SizeConstraint::AnySize,
            force_squared: false,
            fixed_width: 0,
            fixed_height: 0,
            basic_sort_by: BasicSortBy::default(),
            basic_order: SortOrder::default(),
            word_align_mod: 1,
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
    /// Already placed frames (used by the Contact Point heuristic).
    placed: Vec<Rect>,
    placements: Vec<Placement>,
}

impl PageState {
    fn new(index: usize, width: i32, height: i32, border_padding: i32) -> Self {
        let bp = border_padding.max(0);
        Self {
            index,
            width,
            height,
            free_rects: vec![Rect::new(bp, bp, width - 2 * bp, height - 2 * bp)],
            occupied: Vec::new(),
            placed: Vec::new(),
            placements: Vec::new(),
        }
    }

    /// Content bounding box: right/bottom edge of the placed frames.
    fn content_extents(&self) -> (i32, i32) {
        let mut right = 0;
        let mut bottom = 0;
        for p in &self.placements {
            right = right.max(p.frame.x + p.frame.width);
            bottom = bottom.max(p.frame.y + p.frame.height);
        }
        (right, bottom)
    }
}

/// Resolve legacy settings: `packing_strategy = "Guillotine"` selects the
/// Guillotine algorithm (docs moved the option to *Algorithm*).
fn resolve(opts: &PackerOptions) -> PackerOptions {
    let mut o = opts.clone();
    if o.strategy == PackingStrategy::Guillotine {
        o.algorithm = PackingAlgorithm::Guillotine;
        o.strategy = PackingStrategy::Bssf;
    }
    o
}

/// Canvas used for the first packing pass (fixed size wins over `max_size`).
fn canvas(opts: &PackerOptions) -> (i32, i32) {
    let w = if opts.fixed_width > 0 {
        opts.fixed_width
    } else {
        opts.max_size
    };
    let h = if opts.fixed_height > 0 {
        opts.fixed_height
    } else {
        opts.max_size
    };
    (w, h)
}

/// Pack `items` (sorted internally by area, descending) into one or more
/// pages, cropping/searching the atlas size according to `pack_mode`,
/// `size_constraints`, `fixed_width`/`fixed_height` and `force_squared`.
pub fn pack(items: &[PackItem], opts: &PackerOptions) -> Result<PackOutput> {
    let opts = resolve(opts);
    let (cw, ch) = canvas(&opts);
    validate_setup(items, &opts, cw, ch)?;

    let mut out = place_and_size(items, &opts, cw, ch)?;

    // Size search only applies to a single page with at least one auto axis.
    let fixed_both = opts.fixed_width > 0 && opts.fixed_height > 0;
    if !fixed_both && out.pages.len() == 1 && opts.pack_mode != PackMode::Fast {
        if let Some(found) = search_min(items, &opts, cw, ch)? {
            out = found;
        }
    }
    Ok(out)
}

/// Validate the interior and that every sprite fits the canvas.
fn validate_setup(items: &[PackItem], opts: &PackerOptions, cw: i32, ch: i32) -> Result<()> {
    let pad = opts.padding.max(0);
    let bp = opts.border_padding.max(0);
    if cw - 2 * bp <= 0 || ch - 2 * bp <= 0 {
        return Err(TpError::Pack(format!(
            "border_padding ({bp}) deja el área interior vacía en un atlas de {cw}x{ch}",
        )));
    }
    for it in items {
        let (w, h) = (it.width + 2 * pad, it.height + 2 * pad);
        if w + 2 * bp > cw || h + 2 * bp > ch {
            return Err(TpError::Pack(format!(
                "El sprite '{}' ({}x{}) no cabe en un atlas de {cw}x{ch} \
                 (padding {pad} + borde {bp})",
                it.id, it.width, it.height,
            )));
        }
    }
    Ok(())
}

/// One placement pass at canvas `cw`x`ch` plus the final size of each page.
fn place_and_size(
    items: &[PackItem],
    opts: &PackerOptions,
    cw: i32,
    ch: i32,
) -> Result<PackOutput> {
    let pages = place_all(items, opts, cw, ch)?;
    Ok(finalize(pages, opts, cw, ch))
}

/// Place every item, dispatching on the algorithm. Returns raw pages still
/// sized to the canvas (final sizing happens in [`finalize`]).
fn place_all(items: &[PackItem], opts: &PackerOptions, cw: i32, ch: i32) -> Result<Vec<PageState>> {
    match opts.algorithm {
        PackingAlgorithm::Grid => pack_grid(items, opts, cw, ch),
        PackingAlgorithm::Basic => pack_basic(items, opts, cw, ch),
        // Docs: *Algorithm → Polygon* — the tightest packing for non-rectangular
        // sprites: MaxRects placement with polygon occupancy support.
        PackingAlgorithm::Polygon => pack_maxrects(items, opts, cw, ch),
        PackingAlgorithm::MaxRects | PackingAlgorithm::Guillotine => {
            if opts.strategy == PackingStrategy::Best {
                // Docs: *Best* tries every heuristic and keeps the tightest.
                let mut best: Option<(i64, i64, Vec<PageState>)> = None;
                for strategy in PackingStrategy::all_heuristics() {
                    let mut sub = opts.clone();
                    sub.strategy = strategy;
                    let pages = pack_maxrects(items, &sub, cw, ch)?;
                    let better = match &best {
                        None => true,
                        Some((b_pages, b_area, _)) => {
                            (pages.len() as i64, footprint_metric(&pages)) < (*b_pages, *b_area)
                        }
                    };
                    if better {
                        best = Some((pages.len() as i64, footprint_metric(&pages), pages));
                    }
                }
                Ok(best.map(|(_, _, p)| p).unwrap_or_default())
            } else {
                pack_maxrects(items, opts, cw, ch)
            }
        }
    }
}

/// Σ (content width × content height) — tiebreak to compare candidates.
fn footprint_metric(pages: &[PageState]) -> i64 {
    pages
        .iter()
        .map(|p| {
            let (r, b) = p.content_extents();
            (r as i64) * (b as i64)
        })
        .sum()
}

/// Crop/align the size of each page. Single-page results shrink to their
/// content (plus border padding); multi-page results keep the canvas size.
fn finalize(pages: Vec<PageState>, opts: &PackerOptions, cw: i32, ch: i32) -> PackOutput {
    let bp = opts.border_padding.max(0);
    let single = pages.len() == 1;
    let out_pages = pages
        .into_iter()
        .map(|p| {
            let (mut w, mut h) = if single {
                let (right, bottom) = p.content_extents();
                (right + bp, bottom + bp)
            } else {
                (cw, ch)
            };
            let mut auto_w = single && opts.fixed_width <= 0;
            let mut auto_h = single && opts.fixed_height <= 0;
            if opts.fixed_width > 0 {
                w = opts.fixed_width;
                auto_w = false;
            }
            if opts.fixed_height > 0 {
                h = opts.fixed_height;
                auto_h = false;
            }
            if opts.force_squared {
                let m = w.max(h);
                w = m;
                h = m;
                auto_w = opts.fixed_width <= 0;
                auto_h = opts.fixed_height <= 0;
            }
            if auto_w {
                w = align_dimension(w, opts, true);
            }
            if auto_h {
                h = align_dimension(h, opts, false);
            }
            // Never exceed the configured maximum (constraints round up).
            let max = opts.max_size.max(cw).max(ch);
            if w > max && opts.fixed_width <= 0 {
                w = down_align(max, opts, true);
            }
            if h > max && opts.fixed_height <= 0 {
                h = down_align(max, opts, false);
            }
            PackPage {
                index: p.index,
                width: w.max(1),
                height: h.max(1),
                placements: p.placements,
            }
        })
        .collect();
    PackOutput { pages: out_pages }
}

/// Round a dimension **up** to satisfy the size constraint.
fn align_dimension(v: i32, opts: &PackerOptions, is_width: bool) -> i32 {
    match opts.size_constraints {
        SizeConstraint::AnySize => v,
        SizeConstraint::Pot => next_pot(v),
        SizeConstraint::MultipleOf4 => ceil_to(v, 4),
        SizeConstraint::WordAligned => {
            if is_width {
                ceil_to(v, opts.word_align_mod.max(1))
            } else {
                v
            }
        }
    }
}

/// Round a dimension **down** to satisfy the constraint (used when a
/// constraint would push the atlas past `max_size`).
fn down_align(v: i32, opts: &PackerOptions, is_width: bool) -> i32 {
    match opts.size_constraints {
        SizeConstraint::AnySize => v,
        SizeConstraint::Pot => prev_pot(v),
        SizeConstraint::MultipleOf4 => (v / 4) * 4,
        SizeConstraint::WordAligned => {
            if is_width {
                let m = opts.word_align_mod.max(1);
                (v / m) * m
            } else {
                v
            }
        }
    }
}

fn ceil_to(v: i32, step: i32) -> i32 {
    let step = step.max(1);
    let r = v % step;
    if r == 0 {
        v
    } else {
        v + (step - r)
    }
}

fn next_pot(v: i32) -> i32 {
    let mut p = 1;
    while p < v && p < (1 << 30) {
        p *= 2;
    }
    p
}

fn prev_pot(v: i32) -> i32 {
    let mut p = 1;
    while p * 2 <= v {
        p *= 2;
    }
    p.max(1)
}

/// Time budget shared by the size-search passes.
struct TimeBudget {
    start: Instant,
    budget: Duration,
}

impl TimeBudget {
    fn expired(&self) -> bool {
        self.start.elapsed() > self.budget
    }
}

/// Binary-search the smallest single-page canvas (docs: *Pack*). Returns
/// `None` when the search could not improve the current result.
fn search_min(
    items: &[PackItem],
    opts: &PackerOptions,
    cw: i32,
    ch: i32,
) -> Result<Option<PackOutput>> {
    let budget = match opts.pack_mode {
        PackMode::Fast => return Ok(None),
        PackMode::Good => Duration::from_millis(400),
        PackMode::Best => Duration::from_millis(3_000),
    };
    let tb = TimeBudget {
        start: Instant::now(),
        budget,
    };
    let bp = opts.border_padding.max(0);
    let pad = opts.padding.max(0);

    // Lower bound: the biggest inflated sprite plus both borders.
    let (mut lb_w, mut lb_h) = (1, 1);
    for it in items {
        lb_w = lb_w.max(it.width + 2 * pad + 2 * bp);
        lb_h = lb_h.max(it.height + 2 * pad + 2 * bp);
    }

    let fits = |w: i32, h: i32| -> bool {
        if w < lb_w || h < lb_h || w > cw || h > ch {
            return false;
        }
        if opts.force_squared && w != h {
            return false;
        }
        match place_all(items, opts, w, h) {
            Ok(pages) => pages.len() == 1,
            Err(_) => false,
        }
    };

    let mut cur = (cw, ch);
    // Width then height passes; `Best` repeats until stable.
    let passes = match opts.pack_mode {
        PackMode::Best => 4,
        _ => 1,
    };
    for _ in 0..passes {
        let before = cur;
        let (w0, h0) = (cur.0, cur.1);
        cur.0 = search_axis(w0, h0, lb_w, opts, &tb, true, &fits);
        let (w1, h1) = (cur.0, cur.1);
        cur.1 = search_axis(h1, w1, lb_h, opts, &tb, false, &fits);
        if cur == before {
            break;
        }
    }

    if cur.0 >= cw && cur.1 >= ch {
        return Ok(None); // nothing to improve
    }
    if !fits(cur.0, cur.1) {
        return Ok(None);
    }
    place_and_size(items, opts, cur.0, cur.1).map(Some)
}

/// Binary search one axis for the smallest value that still fits on one page.
fn search_axis(
    cur: i32,
    other: i32,
    lb: i32,
    opts: &PackerOptions,
    tb: &TimeBudget,
    width_axis: bool,
    fits: &dyn Fn(i32, i32) -> bool,
) -> i32 {
    let fits_here = |v: i32| {
        if width_axis {
            fits(v, other)
        } else {
            fits(other, v)
        }
    };
    let step = match (opts.size_constraints, width_axis) {
        (SizeConstraint::MultipleOf4, _) => 4,
        (SizeConstraint::WordAligned, true) => opts.word_align_mod.max(1),
        _ => 1,
    };
    let pot = opts.size_constraints == SizeConstraint::Pot;

    let mut hi = cur;
    let mut lo = lb;
    if !fits_here(hi) {
        return cur;
    }
    while lo < hi {
        if tb.expired() {
            break;
        }
        let mid = if pot {
            next_pot((lo + 1).max(hi / 2)) // a power of two strictly below hi when possible
        } else {
            let m = lo + (hi - lo) / 2;
            let m = ceil_to(m, step);
            if m <= lo {
                lo += step.max(1);
                continue;
            }
            m
        };
        if mid >= hi {
            break;
        }
        if fits_here(mid) {
            hi = mid;
        } else {
            lo = if pot {
                next_pot(mid + 1)
            } else {
                mid + step.max(1)
            };
        }
    }
    if fits_here(hi) {
        hi
    } else {
        cur
    }
}

type Score = (i64, i64, i64);

/// MaxRects placement loop (also used by the Guillotine algorithm).
fn pack_maxrects(
    items: &[PackItem],
    opts: &PackerOptions,
    cw: i32,
    ch: i32,
) -> Result<Vec<PageState>> {
    let pad = opts.padding.max(0);
    let bp = opts.border_padding.max(0);

    let mut sorted: Vec<(usize, i32, i32)> = items
        .iter()
        .enumerate()
        .map(|(i, it)| (i, it.width + 2 * pad, it.height + 2 * pad))
        .collect();
    sorted.sort_by_key(|(_, w, h)| std::cmp::Reverse((w * h) as i64));

    let mut pages: Vec<PageState> = Vec::new();
    for (item_idx, w, h) in sorted {
        let item = &items[item_idx];
        let mut placed: Option<Placement> = None;
        if let Some(page) = pages.last_mut() {
            placed = try_place(page, item, w, h, opts)?;
        }
        if placed.is_none() {
            let mut page = PageState::new(pages.len(), cw, ch, bp);
            placed = try_place(&mut page, item, w, h, opts)
                .map_err(|e| TpError::Pack(format!("{e} (página {})", page.index)))?;
            pages.push(page);
        }
        pages.last_mut().unwrap().placements.push(placed.unwrap());
    }
    Ok(pages)
}

/// Grid placement: the largest sprite defines the cell size (docs: *Grid*).
fn pack_grid(items: &[PackItem], opts: &PackerOptions, cw: i32, ch: i32) -> Result<Vec<PageState>> {
    let pad = opts.padding.max(0);
    let bp = opts.border_padding.max(0);
    let (iw, ih) = (cw - 2 * bp, ch - 2 * bp);
    if iw <= 0 || ih <= 0 {
        return Err(TpError::Pack(format!(
            "border_padding ({bp}) deja el área interior vacía en un atlas de {cw}x{ch}",
        )));
    }

    let mut cell_w = 0;
    let mut cell_h = 0;
    for it in items {
        cell_w = cell_w.max(it.width + 2 * pad);
        cell_h = cell_h.max(it.height + 2 * pad);
    }
    if cell_w == 0 {
        return Ok(Vec::new());
    }
    if cell_w > iw || cell_h > ih {
        let big = items
            .iter()
            .find(|it| it.width + 2 * pad > iw || it.height + 2 * pad > ih)
            .map(|it| it.id.clone())
            .unwrap_or_default();
        return Err(TpError::Pack(format!(
            "El sprite '{big}' no cabe en un atlas de {cw}x{ch} (rejilla {cell_w}x{cell_h}, \
             padding {pad} + borde {bp})",
        )));
    }

    let cols = ((iw / cell_w) as usize).max(1);
    let rows = ((ih / cell_h) as usize).max(1);
    let per_page = cols * rows;

    let mut sorted: Vec<&PackItem> = items.iter().collect();
    sorted.sort_by(|a, b| {
        let ka = (a.width * a.height, a.id.as_str());
        let kb = (b.width * b.height, b.id.as_str());
        kb.cmp(&ka)
    });

    let mut pages: Vec<PageState> = Vec::new();
    for (i, item) in sorted.iter().enumerate() {
        let page_idx = i / per_page;
        while pages.len() <= page_idx {
            pages.push(PageState::new(pages.len(), cw, ch, bp));
        }
        let slot = i % per_page;
        let (col, row) = (slot % cols, slot / cols);
        let frame = Rect::new(
            bp + col as i32 * cell_w,
            bp + row as i32 * cell_h,
            item.width + 2 * pad,
            item.height + 2 * pad,
        );
        let page = &mut pages[page_idx];
        page.placed.push(frame);
        page.placements.push(Placement {
            id: item.id.clone(),
            frame,
            rotated: false,
            page: page_idx,
        });
    }
    Ok(pages)
}

/// Row-based left-to-right placement (docs: *Basic*).
fn pack_basic(
    items: &[PackItem],
    opts: &PackerOptions,
    cw: i32,
    ch: i32,
) -> Result<Vec<PageState>> {
    let pad = opts.padding.max(0);
    let bp = opts.border_padding.max(0);
    let (iw, ih) = (cw - 2 * bp, ch - 2 * bp);
    if iw <= 0 || ih <= 0 {
        return Err(TpError::Pack(format!(
            "border_padding ({bp}) deja el área interior vacía en un atlas de {cw}x{ch}",
        )));
    }

    // Docs: *Best* tests all sorting variants and keeps the tightest one.
    if opts.basic_sort_by == BasicSortBy::Best {
        let mut best: Option<(i64, i64, Vec<PageState>)> = None;
        for sort_by in BasicSortBy::all() {
            for order in SortOrder::all() {
                let mut sub = opts.clone();
                sub.basic_sort_by = sort_by;
                sub.basic_order = order;
                let pages = pack_basic(items, &sub, cw, ch)?;
                let better = match &best {
                    None => true,
                    Some((b_pages, b_area, _)) => {
                        (pages.len() as i64, footprint_metric(&pages)) < (*b_pages, *b_area)
                    }
                };
                if better {
                    best = Some((pages.len() as i64, footprint_metric(&pages), pages));
                }
            }
        }
        return Ok(best.map(|(_, _, p)| p).unwrap_or_default());
    }

    let mut sorted: Vec<&PackItem> = items.iter().collect();
    sorted.sort_by(|a, b| {
        use std::cmp::Ordering;
        let ord = match opts.basic_sort_by {
            BasicSortBy::Name => a.id.cmp(&b.id),
            BasicSortBy::Width => a.width.cmp(&b.width),
            BasicSortBy::Height => a.height.cmp(&b.height),
            BasicSortBy::Area => (a.width * a.height).cmp(&(b.width * b.height)),
            BasicSortBy::Circumference => (a.width + a.height).cmp(&(b.width + b.height)),
            BasicSortBy::Best => Ordering::Equal,
        };
        match opts.basic_order {
            SortOrder::Ascending => ord,
            SortOrder::Descending => ord.reverse(),
        }
    });

    let mut pages: Vec<PageState> = vec![PageState::new(0, cw, ch, bp)];
    let mut page_idx = 0usize;
    let (mut x, mut y, mut row_h) = (bp, bp, 0);

    for item in &sorted {
        let w = item.width + 2 * pad;
        let h = item.height + 2 * pad;
        if w > iw || h > ih {
            return Err(TpError::Pack(format!(
                "El sprite '{}' ({}x{}) no cabe en un atlas de {cw}x{ch} \
                 (padding {pad} + borde {bp})",
                item.id, item.width, item.height
            )));
        }
        if x + w > bp + iw {
            x = bp;
            y += row_h;
            row_h = 0;
        }
        if y + h > bp + ih {
            page_idx += 1;
            pages.push(PageState::new(page_idx, cw, ch, bp));
            x = bp;
            y = bp;
            row_h = 0;
        }
        let frame = Rect::new(x, y, w, h);
        let page = &mut pages[page_idx];
        page.placed.push(frame);
        page.placements.push(Placement {
            id: item.id.clone(),
            frame,
            rotated: false,
            page: page_idx,
        });
        x += w;
        row_h = row_h.max(h);
    }
    Ok(pages)
}

/// Try to place `(w, h)` (inflated) on `page`. Candidates are ranked by the
/// heuristic; in polygon mode, invalid (overlapping) placements are skipped.
fn try_place(
    page: &mut PageState,
    item: &PackItem,
    w: i32,
    h: i32,
    opts: &PackerOptions,
) -> Result<Option<Placement>> {
    // Collect all candidate placements, ranked by score.
    let mut candidates: Vec<(usize, bool, Score)> = Vec::new();
    for (ri, fr) in page.free_rects.iter().enumerate() {
        for rotated in [false, true] {
            if rotated && (!opts.allow_rotation || w == h) {
                continue;
            }
            let (pw, ph) = if rotated { (h, w) } else { (w, h) };
            if fr.can_fit(pw, ph) {
                let score = score_placement(fr, pw, ph, opts, page);
                candidates.push((ri, rotated, score));
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
            if page.occupied.is_empty() {
                page.occupied = vec![false; (page.width * page.height) as usize];
            }
            if let Some(mesh) = &item.mesh {
                let footprint =
                    polygon_footprint(mesh, item.width, item.height, rotated, opts.padding);
                if overlaps(&page.occupied, &footprint, page.width, page.height, frame) {
                    continue;
                }
            }
        }

        // Commit: split free rects and update occupancy.
        split_rects(&mut page.free_rects, frame);
        prune_contained(&mut page.free_rects);
        page.placed.push(frame);

        match &item.mesh {
            Some(mesh) if opts.polygon_mode => {
                let footprint =
                    polygon_footprint(mesh, item.width, item.height, rotated, opts.padding);
                blit_footprint(
                    &mut page.occupied,
                    &footprint,
                    page.width,
                    page.height,
                    frame,
                );
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
fn score_placement(fr: &Rect, w: i32, h: i32, opts: &PackerOptions, page: &PageState) -> Score {
    let right = (fr.width - w) as i64;
    let bottom = (fr.height - h) as i64;
    let short_side = right.min(bottom);
    let long_side = right.max(bottom);
    match opts.strategy {
        PackingStrategy::Bssf | PackingStrategy::Guillotine => (short_side, long_side, 0),
        PackingStrategy::Baf => (right * bottom, short_side, 0),
        PackingStrategy::Blsf => (long_side, short_side, 0),
        PackingStrategy::BottomLeft => ((fr.y + h) as i64, (fr.x + w) as i64, short_side),
        // Negated contact: maximising the contact == minimising the score.
        PackingStrategy::ContactPoint => (-contact_score(fr.x, fr.y, w, h, page), short_side, 0),
        // `Best` is resolved in `place_all` (one run per heuristic).
        PackingStrategy::Best => (short_side, long_side, 0),
    }
}

/// Contact length of a candidate rect with placed frames and the page border
/// (docs: *Contact Point*).
fn contact_score(x: i32, y: i32, w: i32, h: i32, page: &PageState) -> i64 {
    let (x0, y0, x1, y1) = (x, y, x + w, y + h);
    let mut score: i64 = 0;
    for p in &page.placed {
        let (px0, py0, px1, py1) = (p.x, p.y, p.x + p.width, p.y + p.height);
        // Vertical shared edge (horizontal adjacency).
        if px1 == x0 || x1 == px0 {
            let lo = y0.max(py0);
            let hi = y1.min(py1);
            if hi > lo {
                score += (hi - lo) as i64;
            }
        }
        // Horizontal shared edge (vertical adjacency).
        if py1 == y0 || y1 == py0 {
            let lo = x0.max(px0);
            let hi = x1.min(px1);
            if hi > lo {
                score += (hi - lo) as i64;
            }
        }
    }
    // Contact with the sheet borders counts too.
    if x == 0 {
        score += h as i64;
    }
    if y == 0 {
        score += w as i64;
    }
    if x1 == page.width {
        score += h as i64;
    }
    if y1 == page.height {
        score += w as i64;
    }
    score
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
    let (gw, gh) = if rotated {
        (height, width)
    } else {
        (width, height)
    };
    let grid_w = (gw + 2 * padding) as usize;
    let grid_h = (gh + 2 * padding) as usize;
    let local: Vec<Point2D> = mesh
        .vertices
        .iter()
        .map(|p| {
            let q = if rotated {
                rotate_90_cw(*p, height as f32)
            } else {
                *p
            };
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
    if occupied.is_empty() {
        return; // grid only allocated in polygon mode
    }
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
        let opts = PackerOptions::new(PackingStrategy::Bssf, true, 16, 2, 0, false);
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
        let opts = PackerOptions::new(PackingStrategy::Bssf, true, 32, 0, 0, false);
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
        let opts = PackerOptions::new(PackingStrategy::Bssf, true, 8, 0, 0, false);
        let out = pack(&[item("bar1", 8, 4), item("bar2", 4, 8)], &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        assert_eq!(out.pages[0].placements.len(), 2);
        assert!(out.pages[0].placements.iter().any(|p| p.rotated));
        // Without rotation it must open a second page.
        let opts2 = PackerOptions::new(PackingStrategy::Bssf, false, 8, 0, 0, false);
        let out2 = pack(&[item("bar1", 8, 4), item("bar2", 4, 8)], &opts2).unwrap();
        assert_eq!(out2.pages.len(), 2);
    }

    #[test]
    fn oversized_sprite_errors() {
        let opts = PackerOptions::new(PackingStrategy::Bssf, true, 32, 0, 0, false);
        let err = pack(&[item("huge", 64, 64)], &opts).unwrap_err();
        assert!(err.to_string().contains("huge"));
    }

    #[test]
    fn bssf_packs_smaller_into_gaps() {
        let opts = PackerOptions::new(PackingStrategy::Bssf, false, 10, 0, 0, false);
        let out = pack(&[item("a", 6, 6), item("b", 4, 4), item("c", 3, 3)], &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        let frames: Vec<Rect> = out.pages[0].placements.iter().map(|p| p.frame).collect();
        for i in 0..frames.len() {
            for j in (i + 1)..frames.len() {
                assert!(!frames[i].intersects(&frames[j]));
            }
        }
    }

    #[test]
    fn border_padding_keeps_margin_to_the_edge() {
        let opts = PackerOptions::new(PackingStrategy::Bssf, false, 32, 1, 4, false);
        let out = pack(&[item("a", 10, 10), item("b", 10, 10)], &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        let frames: Vec<Rect> = out.pages[0].placements.iter().map(|p| p.frame).collect();
        for f in &frames {
            assert!(f.x >= 4, "frame pegado al borde izquierdo: {f:?}");
            assert!(f.y >= 4, "frame pegado al borde superior: {f:?}");
            assert!(
                f.x + f.width <= 32 - 4,
                "frame cruza el borde derecho: {f:?}"
            );
            assert!(
                f.y + f.height <= 32 - 4,
                "frame cruza el borde inferior: {f:?}"
            );
        }
        for i in 0..frames.len() {
            for j in (i + 1)..frames.len() {
                assert!(!frames[i].intersects(&frames[j]));
            }
        }
    }

    #[test]
    fn border_padding_rejects_sprites_that_no_longer_fit() {
        // 28 + 2*1 de padding = 30, + 2*4 de borde = 38 > 32.
        let opts = PackerOptions::new(PackingStrategy::Bssf, false, 32, 1, 4, false);
        let err = pack(&[item("big", 28, 28)], &opts).unwrap_err();
        assert!(err.to_string().contains("big"), "error: {err}");
        // Sin padding de borde el mismo sprite sí cabe.
        let ok = PackerOptions::new(PackingStrategy::Bssf, false, 32, 1, 0, false);
        assert_eq!(pack(&[item("big", 28, 28)], &ok).unwrap().pages.len(), 1);
        // Borde tan grande que no queda área interior.
        let impossible = PackerOptions::new(PackingStrategy::Bssf, false, 8, 0, 4, false);
        assert!(pack(&[item("a", 1, 1)], &impossible).is_err());
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
        let opts = PackerOptions::new(PackingStrategy::Bssf, true, 8, 0, 0, true);
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

    // ------------------------------------------------------------------
    // Lote 6: tamaño, restricciones y algoritmos
    // ------------------------------------------------------------------

    #[test]
    fn fixed_size_atlas_is_respected() {
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 512, 0, 0, false);
        opts.fixed_width = 64;
        opts.fixed_height = 32;
        let out = pack(&[item("a", 8, 8), item("b", 8, 8)], &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        assert_eq!(out.pages[0].width, 64);
        assert_eq!(out.pages[0].height, 32);
    }

    #[test]
    fn force_squared_keeps_both_axes_equal() {
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 512, 0, 0, false);
        opts.force_squared = true;
        let out = pack(&[item("wide", 40, 8), item("tall", 8, 40)], &opts).unwrap();
        assert_eq!(out.pages[0].width, out.pages[0].height);
        assert!(out.pages[0].width >= 48);
    }

    #[test]
    fn size_constraints_round_the_single_page() {
        // POT: content ~10x10 (padding 2 → 14x14 con borde 0) → 16x16.
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 4096, 2, 0, false);
        opts.size_constraints = SizeConstraint::Pot;
        let out = pack(&[item("a", 6, 6)], &opts).unwrap();
        assert_eq!(out.pages[0].width, 16);
        assert_eq!(out.pages[0].height, 16);

        // MultipleOf4: sin redondear quedaría 6x6 (4 + 2*padding 1... = 6).
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 4096, 1, 0, false);
        opts.size_constraints = SizeConstraint::MultipleOf4;
        let out = pack(&[item("a", 4, 4)], &opts).unwrap();
        assert_eq!(out.pages[0].width % 4, 0);
        assert_eq!(out.pages[0].height % 4, 0);
        assert!(out.pages[0].width <= 8 && out.pages[0].height <= 8);
    }

    #[test]
    fn word_aligned_width_is_a_multiple_of_the_word() {
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 4096, 0, 0, false);
        opts.size_constraints = SizeConstraint::WordAligned;
        opts.word_align_mod = 4; // color_depth RGB888/RGBA8888
        let out = pack(&[item("a", 5, 5)], &opts).unwrap();
        assert_eq!(out.pages[0].width % 4, 0);
        // El alto no se alinea a palabra.
        assert_eq!(out.pages[0].height, 5);
    }

    #[test]
    fn grid_algorithm_uses_uniform_cells() {
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 512, 1, 0, false);
        opts.algorithm = PackingAlgorithm::Grid;
        opts.pack_mode = PackMode::Fast;
        let items = [
            item("a", 10, 10),
            item("b", 4, 4),
            item("c", 7, 3),
            item("d", 4, 4),
        ];
        let out = pack(&items, &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        assert_eq!(out.pages[0].placements.len(), 4);
        // La celda es el mayor sprite inflado (10 + 2 = 12): posiciones en
        // múltiplos de 12 respecto del borde.
        let frames: Vec<Rect> = out.pages[0].placements.iter().map(|p| p.frame).collect();
        for f in &frames {
            assert_eq!(f.x % 12, 0, "x no alineado a celda: {f:?}");
            assert_eq!(f.y % 12, 0, "y no alineado a celda: {f:?}");
        }
        for i in 0..frames.len() {
            for j in (i + 1)..frames.len() {
                assert!(!frames[i].intersects(&frames[j]));
            }
        }
    }

    #[test]
    fn basic_algorithm_fills_rows() {
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 512, 0, 0, false);
        opts.algorithm = PackingAlgorithm::Basic;
        opts.pack_mode = PackMode::Fast;
        opts.basic_sort_by = BasicSortBy::Name;
        opts.basic_order = SortOrder::Descending;
        let items = [item("a", 8, 8), item("b", 8, 8), item("c", 8, 8)];
        let out = pack(&items, &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        let names: Vec<&str> = out.pages[0]
            .placements
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        // Orden descendente por nombre y luego en filas de izquierda a derecha.
        assert_eq!(names, vec!["c", "b", "a"]);
        let xs: Vec<i32> = out.pages[0].placements.iter().map(|p| p.frame.x).collect();
        assert!(
            xs.windows(2).all(|w| w[0] <= w[1]),
            "fila invertida: {xs:?}"
        );
        let (leftmost, rightmost) = (
            xs.iter().copied().min().unwrap(),
            xs.iter().copied().max().unwrap(),
        );
        assert!(leftmost < rightmost);
        let ys: Vec<i32> = out.pages[0].placements.iter().map(|p| p.frame.y).collect();
        assert_eq!(ys[0], *ys.iter().min().unwrap());
    }

    #[test]
    fn pack_mode_good_is_at_least_as_tight_as_fast() {
        let items: Vec<PackItem> = (0..40)
            .map(|i| item(&format!("s{i:02}"), 6 + (i % 5) * 3, 5 + (i % 7) * 2))
            .collect();
        let mut fast = PackerOptions::new(PackingStrategy::Bssf, false, 512, 0, 0, false);
        fast.pack_mode = PackMode::Fast;
        let mut good = fast.clone();
        good.pack_mode = PackMode::Good;
        let mut best = fast.clone();
        best.pack_mode = PackMode::Best;

        let a = pack(&items, &fast).unwrap();
        let b = pack(&items, &good).unwrap();
        let c = pack(&items, &best).unwrap();
        let metric = |o: &PackOutput| -> (usize, i64) {
            (
                o.pages.len(),
                o.pages
                    .iter()
                    .map(|p| (p.width as i64) * (p.height as i64))
                    .sum(),
            )
        };
        assert!(metric(&b) <= metric(&a), "Good peor que Fast");
        assert!(metric(&c) <= metric(&a), "Best peor que Fast");
    }

    #[test]
    fn legacy_guillotine_strategy_selects_the_guillotine_algorithm() {
        let opts = PackerOptions::new(PackingStrategy::Guillotine, false, 64, 0, 0, false);
        let out = pack(&[item("a", 10, 10), item("b", 10, 10)], &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        assert_eq!(out.pages[0].placements.len(), 2);
    }

    #[test]
    fn contact_point_heuristic_packs_without_error() {
        let mut opts = PackerOptions::new(PackingStrategy::ContactPoint, false, 32, 0, 0, false);
        opts.pack_mode = PackMode::Fast;
        let out = pack(&[item("a", 8, 8), item("b", 8, 8), item("c", 8, 8)], &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        let frames: Vec<Rect> = out.pages[0].placements.iter().map(|p| p.frame).collect();
        for i in 0..frames.len() {
            for j in (i + 1)..frames.len() {
                assert!(!frames[i].intersects(&frames[j]));
            }
        }
    }

    #[test]
    fn best_heuristic_runs_all_strategies() {
        let mut opts = PackerOptions::new(PackingStrategy::Best, false, 32, 0, 0, false);
        opts.pack_mode = PackMode::Fast;
        let out = pack(
            &[item("a", 12, 4), item("b", 4, 12), item("c", 8, 8)],
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
}
