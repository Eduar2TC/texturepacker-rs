//! Subsystem 3: Empaquetado Espacial (Packing Core).
//!
//! - Algorithms: MaxRects, Guillotine, Grid and Basic
//! - MaxRects heuristics: BSSF / BAF / BLSF / BottomLeft / ContactPoint / Best
//! - Size search: Fast / Good / Best
//! - Size constraints: AnySize / POT / MultipleOf4 /
//!   WordAligned, fixed size (`fixed_width`/`fixed_height`) and force-squared
//! - 90° rotation support
//! - Multi-atlas auto-split (new page when nothing fits)
//! - Polygon-aware placement: AABB placement from MaxRects is validated against
//!   a per-page occupancy grid rasterized from the sprite mesh, so polygon
//!   sprites can be packed tighter than their bounding boxes.

use crate::error::{Result, TpError};

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

/// Manual algorithm: atlas position per sprite id, hand-set in the GUI.
/// The stored position refers to the **trimmed** sprite (no padding); the
/// placement expands it with the shape padding exactly like the other
/// algorithms.
pub type ManualPositions = std::collections::HashMap<String, (i32, i32)>;

#[derive(Debug, Clone)]
pub struct PackerOptions {
    pub strategy: PackingStrategy,
    pub allow_rotation: bool,
    pub max_size: i32,
    /// Width cap for the sheet (`--max-width`); initialised to `max_size`.
    pub max_width: i32,
    /// Height cap for the sheet (`--max-height`); initialised to `max_size`.
    pub max_height: i32,
    /// Gap between neighbouring sprites (shape padding).
    pub padding: i32,
    /// Reserved margin between the sprites and the sheet border
    /// (border padding).
    pub border_padding: i32,
    pub polygon_mode: bool,
    /// Packing algorithm family.
    pub algorithm: PackingAlgorithm,
    /// Effort spent searching the minimum atlas size.
    pub pack_mode: PackMode,
    /// Required atlas dimensions.
    pub size_constraints: SizeConstraint,
    /// Force a square atlas.
    pub force_squared: bool,
    /// Fixed atlas width; `0` = decided by the packer.
    pub fixed_width: i32,
    /// Fixed atlas height; `0` = decided by the packer.
    pub fixed_height: i32,
    /// Sort criterion of the Basic algorithm.
    pub basic_sort_by: BasicSortBy,
    /// Sort direction of the Basic algorithm.
    pub basic_order: SortOrder,
    /// Width alignment in pixels for `WordAligned` (1 = no alignment).
    pub word_align_mod: i32,
    /// Manual algorithm: hand-set position per sprite id (trimmed coords).
    pub manual_positions: ManualPositions,
    /// Manual algorithm: optional snap grid (also snaps the free row-flow).
    pub manual_grid: Option<crate::config::ManualGrid>,
    /// Align to grid: every placed frame starts on a coordinate divisible by
    /// this value. `0` disables it.
    pub align_grid: i32,
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
            max_width: max_size,
            max_height: max_size,
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
            manual_positions: ManualPositions::new(),
            manual_grid: None,
            align_grid: 0,
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

/// Canvas used for the first packing pass (fixed size wins over the
/// per-axis caps).
fn canvas(opts: &PackerOptions) -> (i32, i32) {
    let w = if opts.fixed_width > 0 {
        opts.fixed_width
    } else {
        opts.max_width
    };
    let h = if opts.fixed_height > 0 {
        opts.fixed_height
    } else {
        opts.max_height
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

/// Signature shared by the MaxRects and Guillotine placement runners.
type PlaceFn = fn(&[PackItem], &PackerOptions, i32, i32) -> Result<Vec<PageState>>;

/// Place every item, dispatching on the algorithm. Returns raw pages still
/// sized to the canvas (final sizing happens in [`finalize`]).
fn place_all(items: &[PackItem], opts: &PackerOptions, cw: i32, ch: i32) -> Result<Vec<PageState>> {
    match opts.algorithm {
        PackingAlgorithm::Grid => pack_grid(items, opts, cw, ch),
        PackingAlgorithm::Basic => pack_basic(items, opts, cw, ch),
        PackingAlgorithm::Manual => pack_manual(items, opts, cw, ch),
        // Trim mode Polygon — the tightest packing for non-rectangular
        // sprites: MaxRects placement with polygon occupancy support.
        PackingAlgorithm::Polygon => pack_maxrects(items, opts, cw, ch),
        PackingAlgorithm::MaxRects | PackingAlgorithm::Guillotine => {
            let runner: PlaceFn = if opts.algorithm == PackingAlgorithm::Guillotine {
                pack_guillotine
            } else {
                pack_maxrects
            };
            if opts.strategy == PackingStrategy::Best {
                // `Best` tries every heuristic and keeps the tightest.
                let mut best: Option<(i64, i64, Vec<PageState>)> = None;
                for strategy in PackingStrategy::all_heuristics() {
                    let mut sub = opts.clone();
                    sub.strategy = strategy;
                    let pages = runner(items, &sub, cw, ch)?;
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
                runner(items, opts, cw, ch)
            }
        }
    }
}

/// Área de un item para ordenar de mayor a menor. Amplía **antes** de
/// multiplicar: `(w * h) as i64` multiplica en `i32` y con un lienzo que
/// no pase por `ProjectConfig::validate` (que lo topa a 16384) se pasa de
/// `i32::MAX`, que en debug es un panic y en release un orden erróneo.
fn area_orden(w: i32, h: i32) -> i64 {
    (w as i64) * (h as i64)
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
            // Content size before alignment: the clamp below may not go
            // under it, or the blit would crop sprites in silence.
            let raw_w = w;
            let raw_h = h;
            if auto_w {
                w = align_dimension(w, opts, true);
            }
            if auto_h {
                h = align_dimension(h, opts, false);
            }
            // Never exceed the configured maximum (constraints round up).
            let max_w = opts.max_width.max(cw);
            let max_h = opts.max_height.max(ch);
            // Rounding the maximum down can land *under* the content when
            // the alignment above has just rounded it up (16383 → 16384 →
            // 16380): keeping the raw size gives up the constraint but
            // saves the pixels. A texture a few px wide is preferable to a
            // silently amputated atlas.
            if w > max_w && opts.fixed_width <= 0 {
                w = down_align(max_w, opts, true).max(raw_w);
            }
            if h > max_h && opts.fixed_height <= 0 {
                h = down_align(max_h, opts, false).max(raw_h);
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

/// Largest coordinate divisible by `step` that is not greater than `v`
/// (counterpoint of [`ceil_to`]; `v` is never negative here).
fn floor_to(v: i32, step: i32) -> i32 {
    let step = step.max(1);
    v - v.rem_euclid(step)
}

/// Move a frame origin up to the next coordinate divisible by `align`
/// (*Align to grid*): the top-left corners of the sprites land on the
/// requested grid. `align <= 1` leaves the position untouched.
fn snap_pos(x: i32, y: i32, align: i32) -> (i32, i32) {
    if align <= 1 {
        (x, y)
    } else {
        (ceil_to(x.max(0), align), ceil_to(y.max(0), align))
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

/// Presupuesto de trabajo de la búsqueda de tamaño.
///
/// El presupuesto **no es de reloj**: se mide en unidades de trabajo
/// (candidatos examinados en `try_place` y celdas del footprint
/// escaneadas en `overlaps`), que son la misma magnitud en todas las
/// máquinas. Con el reloj de antes (400 ms en `Good`, 3 s en `Best`) la
/// búsqueda comprobaba más tamaños cuanto más rápida fuera la CPU, así
/// que la misma entrada daba atlas de distinto tamaño según dónde se
/// empaquetara — mientras el resto del packer es cuidadosamente
/// determinista (sorts estables, tiebreak por orden de entrada).
///
/// La magnitud de trabajo además reparte el presupuesto en función del
/// coste real: un empaquetado de rectángulos sale barato y la búsqueda
/// llega a converger, mientras que uno de polígonos escanea celdas y
/// consume el presupuesto en pocas comprobaciones, como hacía el reloj.
struct WorkBudget {
    left: std::cell::Cell<u64>,
}

impl WorkBudget {
    fn new(unidades: u64) -> Self {
        Self {
            left: std::cell::Cell::new(unidades),
        }
    }

    fn expired(&self) -> bool {
        self.left.get() == 0
    }

    /// Descuenta el trabajo que costó la última comprobación. Nunca se
    /// pasa de cero: si una sola comprobación ya se come el presupuesto,
    /// la siguiente comprobación ve el saldo agotado y se corta.
    fn gastar(&self, unidades: u64) {
        #[cfg(test)]
        COBRO.with(|c| c.set(c.get().saturating_add(unidades)));
        self.left.set(self.left.get().saturating_sub(unidades));
    }
}

thread_local! {
    /// Trabajo consumido por el empaquetado en curso **en este hilo**: la
    /// búsqueda lo pone a cero antes de cada comprobación y lee lo que
    /// gastó. Thread-local y no global para que paquetes simultáneos (la
    /// app empaqueta en un hilo de fondo, los tests corren en paralelo)
    /// no se mezclen y rompan el determinismo.
    static TRABAJO: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Apunta trabajo consumido al contador del hilo.
fn gastar_trabajo(unidades: u64) {
    TRABAJO.with(|t| t.set(t.get().saturating_add(unidades)));
}

/// Trabajo consumido desde la última [`reiniciar_trabajo`].
fn trabajo_consumido() -> u64 {
    TRABAJO.with(|t| t.get())
}

/// Pone el contador a cero: se llama antes de cada comprobación de la
/// búsqueda de tamaño.
fn reiniciar_trabajo() {
    TRABAJO.with(|t| t.set(0))
}

#[cfg(test)]
thread_local! {
    /// Trabajo cobrado por la búsqueda de tamaño desde la última
    /// [`reiniciar_cobro`]. Es la comprobación de que el presupuesto de M3
    /// se gasta en trabajo: con un presupuesto de reloj nadie llama a
    /// [`WorkBudget::gastar`] y esto se queda a cero, que es el bug.
    static COBRO: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Apunta el cobro de la búsqueda al contador de prueba de este hilo.
#[cfg(test)]
fn reiniciar_cobro() {
    COBRO.with(|c| c.set(0))
}

/// Trabajo que la búsqueda cobró desde la última [`reiniciar_cobro`].
#[cfg(test)]
fn cobrado() -> u64 {
    COBRO.with(|c| c.get())
}

/// Unidades de trabajo que `Good` (una pasada de búsqueda) puede gastar.
///
/// La unidad cuenta trabajo real de la búsqueda —candidatos examinados en
/// `try_place`, celdas del footprint en `overlaps`, rectángulos libres en
/// `split_rects`/`prune_contained`— y no reloj, para que la misma entrada
/// dé el mismo atlas en todas las máquinas. Medida en esta máquina, la
/// tasa queda entre 12 k y 28 k unidades/ms según la entrada (dentro de un
/// factor ~2,3), así que un presupuesto en unidades acota también el
/// tiempo. Conviene recordar que el reloj de antes tampoco cortaba a los
/// 400 ms: solo se comprobaba entre evaluaciones, y una evaluación de
/// atlas pesado llega a costar 400 ms por sí sola, de ahí que "400 ms de
/// reloj" fueran en la práctica 3 s de paquete.
///
/// El valor se fijo contra ese reloj, con tres corridas por celda:
///
/// | entrada | reloj de 400 ms | 13 M (elegido) | 10 M (descartado) |
/// |---|---|---|---|
/// | 600 sprites 64×64 | 1088×3487 en 3,1 s | 1088×3487 en 3,6–4,3 s | 2091×2094 en 1,8 s (+16 % de área) |
/// | 60 polígonos 256×192 | 1064×3008 en 1,4 s | 936×4072 en 2,7 s | 1064×3008 en 1,6 s |
///
/// 13 M es el mínimo que reproduce el atlas de antes en rectángulos, el
/// caso por defecto. En polígonos el recorte de contenido no es monótono
/// con el presupuesto (lienzo más estrecho, pila más alta), así que ahí la
/// comparación no admite preferencia clara y manda el caso de rectángulos.
/// Entradas ligeras (21,5 k unidades para 10 sprites) convergen con
/// cualquiera de estos valores, igual que convergían con el reloj.
const GOOD_WORK_UNITS: u64 = 13_000_000;
/// Unidades de trabajo de `Best` (cuatro pasadas hasta estabilizar).
///
/// El reloj de antes era de 3 s; en la misma entrada de rectángulos la
/// primera pasada llegaba a 3,4 s ≈ 50 M unidades, así que este valor
/// reproduce `Best` de antes y sigue siendo ~3,8× `Good`, que es lo que
/// separa los dos modos.
const BEST_WORK_UNITS: u64 = 50_000_000;

/// Binary-search the smallest single-page canvas. Returns
/// `None` when the search could not improve the current result.
fn search_min(
    items: &[PackItem],
    opts: &PackerOptions,
    cw: i32,
    ch: i32,
) -> Result<Option<PackOutput>> {
    let unidades = match opts.pack_mode {
        PackMode::Fast => return Ok(None),
        PackMode::Good => GOOD_WORK_UNITS,
        PackMode::Best => BEST_WORK_UNITS,
    };
    let tb = WorkBudget::new(unidades);
    let bp = opts.border_padding.max(0);
    let pad = opts.padding.max(0);

    // Lower bound: the biggest inflated sprite plus both borders.
    let (mut lb_w, mut lb_h) = (1, 1);
    for it in items {
        lb_w = lb_w.max(it.width + 2 * pad + 2 * bp);
        lb_h = lb_h.max(it.height + 2 * pad + 2 * bp);
    }

    // Cada comprobación se apunta al presupuesto por lo que costó de
    // verdad, no por lo que tardó: el resultado es el mismo en todas las
    // máquinas y el gasto, acotado.
    let fits = |w: i32, h: i32| -> bool {
        if w < lb_w || h < lb_h || w > cw || h > ch {
            return false;
        }
        if opts.force_squared && w != h {
            return false;
        }
        reiniciar_trabajo();
        let cabe = cabe_en_una_hoja(items, opts, w, h, bp, pad);
        tb.gastar(trabajo_consumido());
        cabe
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

/// ¿Cabe todo el conjunto en **una** hoja de `w x h` sin recortar
/// posiciones fijadas por el usuario? Es lo que comprueba cada paso de la
/// búsqueda de tamaño; se separa de `search_min` para que el presupuesto
/// de trabajo se pueda cobrar alrededor entero de la comprobación,
/// incluidos los cortes por la mitad.
fn cabe_en_una_hoja(
    items: &[PackItem],
    opts: &PackerOptions,
    w: i32,
    h: i32,
    bp: i32,
    pad: i32,
) -> bool {
    match place_all(items, opts, w, h) {
        Ok(pages) => {
            if pages.len() != 1 {
                return false;
            }
            // Manual: el lienzo solo sirve si ninguna posición fijada
            // por el usuario quedó recortada al encogerlo.
            if opts.algorithm == PackingAlgorithm::Manual {
                let (iw, ih) = (w - 2 * bp, h - 2 * bp);
                for it in items {
                    if let Some((px, py)) = opts.manual_positions.get(&it.id).copied() {
                        let fw = it.width + 2 * pad;
                        let fh = it.height + 2 * pad;
                        let fx = px.max(0).min(iw - fw);
                        let fy = py.max(0).min(ih - fh);
                        if bp + fx != px || bp + fy != py {
                            return false;
                        }
                    }
                }
            }
            true
        }
        Err(_) => false,
    }
}

/// Binary search one axis for the smallest value that still fits on one page.
fn search_axis(
    cur: i32,
    other: i32,
    lb: i32,
    opts: &PackerOptions,
    tb: &WorkBudget,
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
    // Clave calculada en `area_orden` (i64): ver su doc por qué.
    sorted.sort_by_key(|(_, w, h)| std::cmp::Reverse(area_orden(*w, *h)));

    let mut pages: Vec<PageState> = Vec::new();
    for (item_idx, w, h) in sorted {
        let item = &items[item_idx];
        let mut placed: Option<Placement> = None;
        if let Some(page) = pages.last_mut() {
            placed = try_place(page, item, w, h, opts, false)?;
        }
        // Página nueva: si el sprite tampoco cabe ahí, es un error de
        // configuración (el snap a la rejilla se come el interior cuando
        // `border_padding` no es múltiplo de `align_grid`), no un panic en
        // `placed.unwrap()`. La rama de Guillotine ya lo trataba así.
        let placed = match placed {
            Some(p) => p,
            None => {
                let mut page = PageState::new(pages.len(), cw, ch, bp);
                let p = try_place(&mut page, item, w, h, opts, false)
                    .map_err(|e| TpError::Pack(format!("{e} (página {})", page.index)))?
                    .ok_or_else(|| {
                        let align = opts.align_grid.max(0);
                        let grid = if align > 1 {
                            format!(", con rejilla de {align} px")
                        } else {
                            String::new()
                        };
                        TpError::Pack(format!(
                            "El sprite '{}' ({}x{}) no cabe en un atlas de {cw}x{ch} \
                             (padding {pad} + borde {bp}{grid})",
                            item.id, item.width, item.height,
                        ))
                    })?;
                pages.push(page);
                p
            }
        };
        pages
            .last_mut()
            .expect("la página acaba de crearse o de recibir la colocación")
            .placements
            .push(placed);
    }
    Ok(pages)
}

/// Guillotine placement: the free list is a **disjoint partition** of the
/// canvas (one rectangle at first; every cut replaces one rectangle with two
/// children that tile it exactly). Each sprite takes the top-left of the free
/// rectangle chosen by the heuristic, so unlike MaxRects no free rectangle is
/// ever split against a sprite placed in a *different* one — that is what
/// gives the algorithm its name and its characteristic long strips.
///
/// As in MaxRects, only the axis-aligned bounding box is packed (polygon
/// mode validates the mesh against the occupancy grid afterwards).
fn pack_guillotine(
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
    // Clave calculada en `area_orden` (i64): ver su doc por qué.
    sorted.sort_by_key(|(_, w, h)| std::cmp::Reverse(area_orden(*w, *h)));

    let mut pages: Vec<PageState> = Vec::new();
    for (item_idx, w, h) in sorted {
        let item = &items[item_idx];
        let mut placed = None;
        if let Some(page) = pages.last_mut() {
            placed = try_place(page, item, w, h, opts, true)?;
        }
        let placed = match placed {
            Some(p) => p,
            None => {
                let mut page = PageState::new(pages.len(), cw, ch, bp);
                let p = try_place(&mut page, item, w, h, opts, true)?.ok_or_else(|| {
                    TpError::Pack(format!(
                        "El sprite '{}' ({}x{}) no cabe en un atlas de {cw}x{ch} \
                         (padding {pad} + borde {bp})",
                        item.id, item.width, item.height,
                    ))
                })?;
                pages.push(page);
                p
            }
        };
        pages.last_mut().unwrap().placements.push(placed);
    }
    Ok(pages)
}

/// Guillotine cut: only the chosen free rectangle is split into children
/// that tile it exactly (so the free list stays a disjoint partition and no
/// space is wasted by the split itself).
///
/// With *Align to grid* the frame origin is snapped up inside the free
/// rectangle, so `placed` does **not** necessarily sit at its top-left
/// corner. The split is therefore computed from the *snapped* origin: the
/// four children below cover `fr` exactly whatever the offset, which is what
/// keeps the partition disjoint (a cut from `fr`'s corner would leave the
/// offset strip unassigned *and* hand out space already taken by the sprite).
///
/// When the origin is the corner itself (no alignment, or already aligned
/// children) the classic guillotine cut applies: two children, along the
/// axis that keeps the larger one — that is what preserves usable rectangles
/// for the next sprites.
fn split_guillotine(free: &mut Vec<Rect>, ri: usize, placed: Rect) {
    let fr = free[ri];
    free.remove(ri);

    let ox = (placed.x - fr.x).clamp(0, fr.width);
    let oy = (placed.y - fr.y).clamp(0, fr.height);
    let w = placed.width.max(0).min(fr.width - ox);
    let h = placed.height.max(0).min(fr.height - oy);

    let push = |free: &mut Vec<Rect>, r: Rect| {
        if r.width > 0 && r.height > 0 {
            free.push(r);
        }
    };

    // El sprite arranca dentro del rectángulo: la región sobrante es en
    // forma de L y se trocea en cuatro bandas que tilinguean `fr` sin
    // solapes — izquierda y derecha a altura completa, más la superior y la
    // inferior justo del ancho del sprite. Con `ox == oy == 0` tres de ellas
    // quedan vacías y sobra la pareja del corte clásico.
    if ox > 0 || oy > 0 {
        push(free, Rect::new(fr.x, fr.y, ox, fr.height));
        push(
            free,
            Rect::new(fr.x + ox + w, fr.y, fr.width - ox - w, fr.height),
        );
        push(free, Rect::new(fr.x + ox, fr.y, w, oy));
        push(
            free,
            Rect::new(fr.x + ox, fr.y + oy + h, w, fr.height - oy - h),
        );
        return;
    }

    // El sprite ocupa la esquina superior izquierda del rectángulo, así que
    // los dos hijos que tilingean el resto son siempre el «derecha»
    // (a la derecha del sprite) y el «debajo» (bajo el sprite); el corte
    // decide cuánto ancho/alto lleva cada uno:
    //   · vertical (corte a lo ancho): la derecha gana toda la altura y lo
    //     de debajo solo la columna del sprite;
    //   · horizontal (corte a lo alto): lo de debajo gana todo el ancho y la
    //     derecha solo la fila del sprite.
    let right = (fr.width - w).max(0);
    let bottom = (fr.height - h).max(0);

    // Se elige el corte que deja el hijo de mayor área (más rectángulo
    // usable para los próximos sprites).
    let vertical = (w * bottom).max(right * fr.height) >= (fr.width * bottom).max(right * h);

    if vertical {
        push(free, Rect::new(fr.x, fr.y + h, w, bottom));
        push(free, Rect::new(fr.x + w, fr.y, right, fr.height));
    } else {
        push(free, Rect::new(fr.x, fr.y + h, fr.width, bottom));
        push(free, Rect::new(fr.x + w, fr.y, right, h));
    }
}

/// Grid placement: the largest sprite defines the cell size.
fn pack_grid(items: &[PackItem], opts: &PackerOptions, cw: i32, ch: i32) -> Result<Vec<PageState>> {
    let pad = opts.padding.max(0);
    let bp = opts.border_padding.max(0);
    let (iw, ih) = (cw - 2 * bp, ch - 2 * bp);
    if iw <= 0 || ih <= 0 {
        return Err(TpError::Pack(format!(
            "border_padding ({bp}) deja el área interior vacía en un atlas de {cw}x{ch}",
        )));
    }

    let align = opts.align_grid.max(0);
    let mut cell_w = 0;
    let mut cell_h = 0;
    for it in items {
        cell_w = cell_w.max(it.width + 2 * pad);
        cell_h = cell_h.max(it.height + 2 * pad);
    }
    if cell_w == 0 {
        return Ok(Vec::new());
    }
    // *Align to grid*: la celda crece hasta el múltiplo, así todos los
    // orígenes de celda caen en la rejilla sin comprobarlos uno a uno.
    cell_w = ceil_to(cell_w, align);
    cell_h = ceil_to(cell_h, align);
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

    let (ox, oy) = snap_pos(bp, bp, align);
    if ox + cell_w > bp + iw || oy + cell_h > bp + ih {
        return Err(TpError::Pack(format!(
            "La rejilla de {align} px no cabe en el área interior de {cw}x{ch} \
             (celda {cell_w}x{cell_h}, borde {bp})"
        )));
    }
    let cols = (((bp + iw - ox) / cell_w) as usize).max(1);
    let rows = (((bp + ih - oy) / cell_h) as usize).max(1);
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
            ox + col as i32 * cell_w,
            oy + row as i32 * cell_h,
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

/// Manual placement: each sprite goes exactly where the user dragged it in
/// the GUI (`manual_positions`), no rotation. Sprites without a position fall
/// back to the Basic row flow. Multi-page is supported: sprites that do not
/// fit the (fixed) canvas overflow to a new page keeping their position.
fn pack_manual(
    items: &[PackItem],
    opts: &PackerOptions,
    cw: i32,
    ch: i32,
) -> Result<Vec<PageState>> {
    let pad = opts.padding.max(0);
    let bp = opts.border_padding.max(0);
    let align = opts.align_grid.max(0);
    let (iw, ih) = (cw - 2 * bp, ch - 2 * bp);
    if iw <= 0 || ih <= 0 {
        return Err(TpError::Pack(format!(
            "border_padding ({bp}) deja el área interior vacía en un atlas de {cw}x{ch}",
        )));
    }

    let pos = |it: &PackItem| -> Option<(i32, i32)> { opts.manual_positions.get(&it.id).copied() };

    // Con tamaños automáticos, el área necesaria fija el lienzo: recalcular
    // no cambia las posiciones porque son relativas al borde del atlas.
    let mut sorted: Vec<&PackItem> = items.iter().collect();
    sorted.sort_by(|a, b| {
        let ka = (a.width * a.height, a.id.as_str());
        let kb = (b.width * b.height, b.id.as_str());
        kb.cmp(&ka)
    });

    let mut pages: Vec<PageState> = vec![PageState::new(0, cw, ch, bp)];
    let mut page_idx = 0usize;
    // Rejilla opcional: además de imantar el arrastre en la GUI, el flujo
    // de los libres respeta la rejilla (origen y alturas de fila).
    let flow = opts.manual_grid.filter(|g| g.snap_flow && g.step > 0);

    // Pasada 1: sprites con posición manual (van exactamente ahí).
    // El flujo de los libres empezará por debajo del más bajo de estos
    // frames para no solaparlos.
    let mut flow_y = bp;
    for item in &sorted {
        let Some((px, py)) = pos(item) else {
            continue;
        };
        let w = item.width + 2 * pad;
        let h = item.height + 2 * pad;
        if w > iw || h > ih {
            return Err(TpError::Pack(format!(
                "El sprite '{}' ({}x{}) no cabe en un atlas de {cw}x{ch} \
                 (padding {pad} + borde {bp})",
                item.id, item.width, item.height
            )));
        }
        // Ajustar al interior y, con *Align to grid*, a la rejilla: se
        // sube el origen y, si eso lo sacaría del lienzo, se baja (la
        // posición ya ajustada siempre cabe, así que alguna de las dos
        // alternativas mantiene el sprite dentro).
        let fx = px.max(0).min(iw - w);
        let fy = py.max(0).min(ih - h);
        let (sx, sy) = snap_pos(bp + fx, bp + fy, align);
        let (sx, sy) = if sx + w <= bp + iw && sy + h <= bp + ih {
            (sx, sy)
        } else {
            (bp + floor_to(fx, align), bp + floor_to(fy, align))
        };
        let frame = Rect::new(sx, sy, w, h);
        let page = &mut pages[page_idx];
        page.placed.push(frame);
        page.placements.push(Placement {
            id: item.id.clone(),
            frame,
            rotated: false,
            page: page_idx,
        });
        flow_y = flow_y.max(frame.y + frame.height);
    }

    // Pasada 2: sprites sin posición manual → flujo Basic (filas).
    // Con rejilla (snap_flow), origen, avance e inicios de fila se imantan
    // a múltiplos del paso; sin ella, flujo Basic exacto. El imán siempre
    // sube (nunca redondea a la baja): redondear hacia atrás metería el
    // sprite en el hueco que acaba de dejar el anterior, y el origen de fila
    // tendría que empezar por debajo del borde inferior real de la fila.
    let snapv = |v: i32| match flow {
        Some(g) => ceil_to(v, g.step.max(1)),
        None => v,
    };
    // `row_bottom` es el borde inferior real de la fila en curso (el origen
    // de cada sprite sube a la rejilla, así que la fila no termina en
    // `y + max(alto)` sino en `max(sy + alto)`).
    let (mut x, mut y, mut row_bottom) = (bp, snapv(flow_y), snapv(flow_y));
    for item in &sorted {
        if pos(item).is_some() {
            continue;
        }
        let w = item.width + 2 * pad;
        let h = item.height + 2 * pad;
        if w > iw || h > ih {
            return Err(TpError::Pack(format!(
                "El sprite '{}' ({}x{}) no cabe en un atlas de {cw}x{ch} \
                 (padding {pad} + borde {bp})",
                item.id, item.width, item.height
            )));
        }
        let (mut sx, mut sy) = snap_pos(snapv(x), snapv(y), align);
        if sx + w > bp + iw {
            x = bp;
            y = snapv(row_bottom);
            row_bottom = y;
            (sx, sy) = snap_pos(snapv(x), snapv(y), align);
        }
        if sy + h > bp + ih {
            page_idx += 1;
            pages.push(PageState::new(page_idx, cw, ch, bp));
            x = bp;
            y = snapv(bp);
            row_bottom = y;
            (sx, sy) = snap_pos(snapv(x), snapv(y), align);
        }
        if sx + w > bp + iw || sy + h > bp + ih {
            return Err(TpError::Pack(format!(
                "El sprite '{}' no cabe alineado a la rejilla de {align} px \
                 en un atlas de {cw}x{ch}",
                item.id
            )));
        }
        let frame = Rect::new(sx, sy, w, h);
        let page = &mut pages[page_idx];
        page.placed.push(frame);
        page.placements.push(Placement {
            id: item.id.clone(),
            frame,
            rotated: false,
            page: page_idx,
        });
        x = sx + w;
        row_bottom = row_bottom.max(sy + h);
    }
    Ok(pages)
}

/// Row-based left-to-right placement (Basic algorithm).
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

    // `Best` tests all sorting variants and keeps the tightest one.
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

    let align = opts.align_grid.max(0);
    let mut pages: Vec<PageState> = vec![PageState::new(0, cw, ch, bp)];
    let mut page_idx = 0usize;
    // `row_bottom` es el borde inferior real de la fila en curso: como el
    // origen de cada sprite puede subir a la rejilla, la fila no termina en
    // `y + max(alto)` sino en `max(sy + alto)` — usar lo primero hacía que la
    // fila siguiente empezara dentro de la anterior.
    let (mut x, mut y, mut row_bottom) = (bp, bp, bp);

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
        // Cada sprite arranca en un múltiplo de `align` (*Align to grid*):
        // si al subir el origen no cabe, pasa de fila y, si tampoco, de hoja.
        let (mut sx, mut sy) = snap_pos(x, y, align);
        if sx + w > bp + iw {
            // La nueva fila empieza justo donde acababa la anterior
            // (`row_bottom` ya contiene ese valor).
            x = bp;
            y = row_bottom;
            (sx, sy) = snap_pos(x, y, align);
        }
        if sy + h > bp + ih {
            page_idx += 1;
            pages.push(PageState::new(page_idx, cw, ch, bp));
            x = bp;
            y = bp;
            row_bottom = bp;
            (sx, sy) = snap_pos(x, y, align);
        }
        if sx + w > bp + iw || sy + h > bp + ih {
            return Err(TpError::Pack(format!(
                "El sprite '{}' no cabe alineado a la rejilla de {align} px \
                 en un atlas de {cw}x{ch}",
                item.id
            )));
        }
        let frame = Rect::new(sx, sy, w, h);
        let page = &mut pages[page_idx];
        page.placed.push(frame);
        page.placements.push(Placement {
            id: item.id.clone(),
            frame,
            rotated: false,
            page: page_idx,
        });
        x = sx + w;
        row_bottom = row_bottom.max(sy + h);
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
    guillotine: bool,
) -> Result<Option<Placement>> {
    let align = opts.align_grid.max(0);
    // Collect all candidate placements, ranked by score. With *Align to grid*
    // the origin is snapped up first, so only free rects that still hold the
    // snapped frame become candidates.
    let mut candidates: Vec<(usize, bool, Score)> = Vec::new();
    for (ri, fr) in page.free_rects.iter().enumerate() {
        for rotated in [false, true] {
            if rotated && (!opts.allow_rotation || w == h) {
                continue;
            }
            // Un candidato = una comprobación de solape + una puntuación:
            // es la unidad con la que la búsqueda de tamaño paga su
            // presupuesto (M3).
            gastar_trabajo(1);
            let (pw, ph) = if rotated { (h, w) } else { (w, h) };
            let (sx, sy) = snap_pos(fr.x, fr.y, align);
            if sx - fr.x + pw <= fr.width && sy - fr.y + ph <= fr.height {
                let room = Rect::new(sx, sy, fr.x + fr.width - sx, fr.y + fr.height - sy);
                let score = score_placement(&room, pw, ph, opts, page);
                candidates.push((ri, rotated, score));
            }
        }
    }
    candidates.sort_by_key(|(_, _, s)| *s);

    // El footprint rasterizado depende solo del mesh, del tamaño, de la
    // orientación y del padding, no del candidato en el que se coloque:
    // se calcula una vez por orientación (como mucho dos) y se reutiliza
    // tanto para validar el solape como para marcar la celda. Rasterizarlo
    // dentro del bucle costaba O(free_rects × área) allocations por sprite
    // (M8). El orden se mantiene: se puntúa antes de validar porque
    // puntuar es aritmética pura, mientras que el barrido de solape se
    // corta en cuanto un candidato sirve.
    let mut footprints: [Option<Vec<bool>>; 2] = [None, None];

    for (ri, rotated, _) in candidates {
        let fr = page.free_rects[ri];
        let (pw, ph) = if rotated { (h, w) } else { (w, h) };
        let (sx, sy) = snap_pos(fr.x, fr.y, align);
        if sx - fr.x + pw > fr.width || sy - fr.y + ph > fr.height {
            continue;
        }
        let frame = Rect::new(sx, sy, pw, ph);

        // Polygon validation against the occupancy grid.
        if opts.polygon_mode {
            if page.occupied.is_empty() {
                page.occupied = vec![false; (page.width * page.height) as usize];
            }
            if let Some(mesh) = &item.mesh {
                let footprint = footprints[rotated as usize].get_or_insert_with(|| {
                    polygon_footprint(mesh, item.width, item.height, rotated, opts.padding)
                });
                if overlaps(
                    &page.occupied,
                    footprint.as_slice(),
                    page.width,
                    page.height,
                    frame,
                ) {
                    continue;
                }
            }
        }

        // Commit: split free rects and update occupancy.
        if guillotine {
            split_guillotine(&mut page.free_rects, ri, frame);
        } else {
            split_rects(&mut page.free_rects, frame);
        }
        prune_contained(&mut page.free_rects);
        page.placed.push(frame);

        match &item.mesh {
            Some(mesh) if opts.polygon_mode => {
                let footprint = footprints[rotated as usize].get_or_insert_with(|| {
                    polygon_footprint(mesh, item.width, item.height, rotated, opts.padding)
                });
                blit_footprint(
                    &mut page.occupied,
                    footprint.as_slice(),
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
/// (contact point heuristic).
fn contact_score(x: i32, y: i32, w: i32, h: i32, page: &PageState) -> i64 {
    // Recorre todos los ya colocados: es el coste real de puntuar un
    // candidato con esta estrategia y entra en el presupuesto (M3).
    gastar_trabajo(page.placed.len() as u64);
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
    // Un examen por rectángulo libre: es parte del coste real de una
    // colocación y, por tanto, de lo que la búsqueda de tamaño gasta (M3).
    let trabajo = free.len() as u64;
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
    gastar_trabajo(trabajo);
}

/// Remove free rects fully contained in another free rect (MaxRects prune).
fn prune_contained(free: &mut Vec<Rect>) {
    // O(free²) comparaciones, con un `remove` de O(free) por borrado: es el
    // término dominante de una colocación en atlas grandes, así que entra en
    // el presupuesto de trabajo de la búsqueda de tamaño (M3).
    let mut trabajo = 0u64;
    let mut i = 0;
    while i < free.len() {
        let mut removed = false;
        for (j, other) in free.iter().enumerate() {
            trabajo += 1;
            if i != j && other.contains(&free[i]) && other.area() > free[i].area() {
                free.remove(i);
                trabajo += free.len() as u64;
                removed = true;
                break;
            }
        }
        if !removed {
            i += 1;
        }
    }
    gastar_trabajo(trabajo);
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
    // Las celdas recorridas se apuntan una sola vez al final: es lo que
    // hace caro a un candidato de polígono (una comprobación puede
    // barrer decenas de miles de celdas) y lo que reparte el presupuesto
    // de la búsqueda de tamaño entre comprobaciones caras y baratas (M3).
    let mut visitadas = 0u64;
    let mut solapa = false;
    'barrido: for y in 0..fh {
        for x in 0..fw {
            visitadas += 1;
            let fi = (y * fw + x) as usize;
            if !footprint[fi] {
                continue;
            }
            let ax = frame.x + x;
            let ay = frame.y + y;
            if ax < 0 || ay < 0 || ax >= pw || ay >= ph {
                solapa = true;
                break 'barrido;
            }
            if occupied[(ay * pw + ax) as usize] {
                solapa = true;
                break 'barrido;
            }
        }
    }
    gastar_trabajo(visitadas);
    solapa
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

    /// El orden por área hacía `(w * h) as i64`, que multiplica en `i32`
    /// y desborda con un lienzo que no pase por `ProjectConfig::validate`
    /// (que topa el lienzo a 16384): panic en debug, orden erróneo en
    /// release. 46 341² ya se pasa de `i32::MAX`.
    #[test]
    fn el_orden_por_area_no_desborda_en_i32() {
        assert_eq!(
            area_orden(46_341, 46_341),
            2_147_488_281,
            "en i32 envolvería a un valor negativo y ordenaría al revés"
        );
        assert_eq!(area_orden(50_000, 50_000), 2_500_000_000);
        // Dos ítems, no uno: con uno la clave no se llega a evaluar.
        let items = [item("g", 46_341, 46_341), item("s", 100, 100)];
        for algorithm in [PackingAlgorithm::MaxRects, PackingAlgorithm::Guillotine] {
            let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 50_000, 0, 0, false);
            opts.algorithm = algorithm;
            opts.fixed_width = 50_000;
            opts.fixed_height = 50_000;
            opts.pack_mode = PackMode::Fast;
            let out = pack(&items, &opts).unwrap_or_else(|e| panic!("{algorithm:?}: {e}"));
            assert_eq!(out.pages.len(), 1, "{algorithm:?}");
            assert_eq!(out.pages[0].placements.len(), 2, "{algorithm:?}");
        }
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

    /// El snap a la rejilla puede comerse el interior de una página recién
    /// creada (borde 1, rejilla 8: el origen sube de 1 a 8 y el sprite ya no
    /// cabe). Eso es un error de configuración, no un `unwrap()` en la ruta
    /// de página nueva.
    #[test]
    fn maxrects_errors_when_the_snap_leaves_no_room_on_a_fresh_page() {
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 32, 0, 1, false);
        opts.align_grid = 8;
        let err = pack(&[item("a", 30, 30)], &opts).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains('a'), "cita al sprite: {msg}");
        assert!(msg.contains("no cabe"), "explica el fallo: {msg}");
        // Con la rejilla desactivada el mismo sprite sí cabe en 32 con borde 1.
        let mut plain = PackerOptions::new(PackingStrategy::Bssf, false, 32, 0, 1, false);
        plain.align_grid = 0;
        assert_eq!(pack(&[item("a", 30, 30)], &plain).unwrap().pages.len(), 1);
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

    /// M4: ninguna página puede quedar más estrecha que lo que lleva
    /// dentro. El contenido que llena el lienzo sube a la siguiente cuota
    /// de la restricción (1001 → 1004 en `MultipleOf4`, 1500 → 2048 en
    /// `POT`), esa cuota ya no cabe en el máximo configurado, y el
    /// redondeo hacia abajo caía por debajo del contenido (1000, 1024):
    /// los píxeles de más se perdían en el blit sin que nadie lo dijera.
    #[test]
    fn ninguna_pagina_queda_mas_estrecha_que_sus_placements() {
        for (constraint, max) in [
            (SizeConstraint::MultipleOf4, 1001),
            (SizeConstraint::Pot, 1500),
            (SizeConstraint::WordAligned, 1001),
            (SizeConstraint::AnySize, 1001), // caso de control
        ] {
            let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, max, 0, 0, false);
            opts.size_constraints = constraint;
            opts.word_align_mod = 4;
            let out = pack(&[item("a", max, max)], &opts).unwrap();
            assert_eq!(out.pages.len(), 1, "{constraint:?}");
            for p in &out.pages[0].placements {
                assert!(
                    p.frame.x + p.frame.width <= out.pages[0].width,
                    "{constraint:?}: frame {:?} más ancho que la página {}",
                    p.frame,
                    out.pages[0].width
                );
                assert!(
                    p.frame.y + p.frame.height <= out.pages[0].height,
                    "{constraint:?}: frame {:?} más alto que la página {}",
                    p.frame,
                    out.pages[0].height
                );
            }
            assert!(out.pages[0].width <= max, "{constraint:?}");
            assert!(out.pages[0].height <= max, "{constraint:?}");
        }
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
    fn manual_algorithm_places_where_told() {
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 128, 1, 2, false);
        opts.algorithm = PackingAlgorithm::Manual;
        opts.pack_mode = PackMode::Fast;
        opts.manual_positions.insert("a".into(), (10, 5));
        opts.manual_positions.insert("b".into(), (30, 40));
        // "c" sin posición: cae en el flujo Basic.
        let out = pack(&[item("a", 8, 8), item("b", 6, 6), item("c", 5, 5)], &opts).unwrap();
        assert_eq!(out.pages.len(), 1);
        let p = |id: &str| {
            out.pages[0]
                .placements
                .iter()
                .find(|pl| pl.id == id)
                .unwrap()
                .frame
        };
        // manual (10,5) → frame (border 2 + 10, border 2 + 5) con padding ya
        // influido: frame = pos + border, tamaño + 2*padding.
        let fa = p("a");
        assert_eq!((fa.x, fa.y), (12, 7));
        assert_eq!((fa.width, fa.height), (10, 10));
        let fb = p("b");
        assert_eq!((fb.x, fb.y), (32, 42));
        assert_eq!((fb.width, fb.height), (8, 8));
        // "c" fluye: dentro del interior y sin solaparse con "a" ni "b".
        let fc = p("c");
        assert!(fc.x >= 2 && fc.y >= 2);
        assert!(!fa.intersects(&fc) && !fb.intersects(&fc));
    }

    #[test]
    fn manual_positions_are_clamped_inside_the_interior() {
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 32, 0, 4, false);
        opts.algorithm = PackingAlgorithm::Manual;
        opts.pack_mode = PackMode::Fast;
        opts.manual_positions.insert("x".into(), (-10, 999));
        let out = pack(&[item("x", 8, 8)], &opts).unwrap();
        let f = out.pages[0].placements[0].frame;
        // x=-10 satura a 0; y=999 satura al máximo (24-8=16). +borde 4.
        assert_eq!((f.x, f.y), (4, 20));
        assert_eq!((f.x + f.width, f.y + f.height), (12, 28));
        assert!(f.x + f.width <= 32 - 4 && f.y + f.height <= 32 - 4);
    }

    #[test]
    fn manual_snap_grid_aligns_free_flow() {
        // snap_flow: el flujo de los libres también imanta a la rejilla.
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, false, 128, 0, 0, false);
        opts.algorithm = PackingAlgorithm::Manual;
        opts.pack_mode = PackMode::Fast;
        opts.manual_grid = Some(crate::config::ManualGrid::new(16, true));
        opts.manual_positions.insert("a".into(), (32, 48));
        // "b" y "c" sin posición: el flujo imanta a múltiplos de 16.
        let out = pack(
            &[item("a", 8, 8), item("b", 10, 10), item("c", 12, 12)],
            &opts,
        )
        .unwrap();
        assert_eq!(out.pages.len(), 1);
        let p = |id: &str| {
            out.pages[0]
                .placements
                .iter()
                .find(|pl| pl.id == id)
                .unwrap()
                .frame
        };
        // Fijado: exactamente donde se pidió.
        assert_eq!((p("a").x, p("a").y), (32, 48));
        // Libres: origen y avance en múltiplos de 16.
        let fb = p("b");
        let fc = p("c");
        assert_eq!(fb.x % 16, 0, "x de b fuera de rejilla: {}", fb.x);
        assert_eq!(fb.y % 16, 0, "y de b fuera de rejilla: {}", fb.y);
        assert_eq!(fc.x % 16, 0, "x de c fuera de rejilla: {}", fc.x);
        assert_eq!(fc.y % 16, 0, "y de c fuera de rejilla: {}", fc.y);
        assert!(!fb.intersects(&fc) && !p("a").intersects(&fb) && !p("a").intersects(&fc));
        // La primera fila arranca por debajo del fijado (48+8=56 → snap 64).
        assert!(fb.y >= 64, "b debería empezar en y>=64, fue {}", fb.y);
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

    // -------------------------------------------------------------------
    // Guillotine
    // -------------------------------------------------------------------

    /// Resultado válido: cada frame dentro de su página y sin solapes.
    fn assert_valid(out: &PackOutput) {
        for p in &out.pages {
            for (i, a) in p.placements.iter().enumerate() {
                assert!(
                    a.frame.x >= 0
                        && a.frame.y >= 0
                        && a.frame.x + a.frame.width <= p.width
                        && a.frame.y + a.frame.height <= p.height,
                    "{} sale de la página {}x{}: {:?}",
                    a.id,
                    p.width,
                    p.height,
                    a.frame
                );
                for b in p.placements.iter().skip(i + 1) {
                    assert!(!a.frame.intersects(&b.frame), "{} solapa a {}", a.id, b.id);
                }
            }
        }
    }

    /// (páginas, área total) — para comparar heurísticas.
    fn size_metric(out: &PackOutput) -> (usize, i64) {
        (
            out.pages.len(),
            out.pages
                .iter()
                .map(|p| (p.width as i64) * (p.height as i64))
                .sum(),
        )
    }

    fn guillotine_opts(strategy: PackingStrategy, max: i32) -> PackerOptions {
        let mut opts = PackerOptions::new(strategy, false, max, 4, 2, false);
        opts.algorithm = PackingAlgorithm::Guillotine;
        opts.pack_mode = PackMode::Fast;
        opts
    }

    fn mixed_items() -> Vec<PackItem> {
        [
            ("a", 40, 40),
            ("b", 70, 20),
            ("c", 20, 70),
            ("d", 30, 30),
            ("e", 15, 45),
            ("f", 45, 15),
            ("g", 10, 10),
            ("h", 60, 60),
            ("i", 33, 17),
            ("j", 17, 33),
        ]
        .iter()
        .map(|(id, w, h)| item(id, *w, *h))
        .collect()
    }

    #[test]
    fn guillotine_places_every_sprite_without_overlaps() {
        let items = mixed_items();
        let out = pack(&items, &guillotine_opts(PackingStrategy::Bssf, 256)).unwrap();
        assert_eq!(out.pages.len(), 1, "todo cabe en 256");
        assert_eq!(out.pages[0].placements.len(), items.len());
        assert_valid(&out);
        // El borde de 2 px se respeta en los cuatro lados.
        for p in &out.pages {
            for s in &p.placements {
                assert!(
                    s.frame.x >= 2 && s.frame.y >= 2,
                    "sin margen: {:?}",
                    s.frame
                );
                assert!(
                    s.frame.x + s.frame.width <= p.width - 2
                        && s.frame.y + s.frame.height <= p.height - 2,
                    "sin margen inferior/derecho: {:?}",
                    s.frame
                );
            }
        }
    }

    /// *Align to grid* mueve el origen del sprite dentro del rectángulo
    /// libre, así que el corte tiene que calcularse desde el origen ya
    /// recortado: si no, la franja que sobra queda sin cubrir y el siguiente
    /// sprite se coloca encima del anterior.
    #[test]
    fn guillotine_align_grid_keeps_placements_disjoint() {
        for (w, h, align) in [(5, 5, 4), (7, 3, 8), (11, 9, 4), (3, 7, 16)] {
            let items = vec![
                item("a", w, h),
                item("b", w, h),
                item("c", w, h),
                item("d", w, h),
            ];
            let mut opts = guillotine_opts(PackingStrategy::Bssf, 32);
            opts.align_grid = align;
            opts.padding = 0;
            opts.border_padding = 0;
            let out = pack(&items, &opts).unwrap_or_else(|e| panic!("[{w}x{h} align {align}] {e}"));
            assert_valid(&out);
            for p in &out.pages {
                for s in &p.placements {
                    assert_eq!(
                        s.frame.x % align,
                        0,
                        "[{w}x{h} align {align}] {} x={} fuera de rejilla",
                        s.id,
                        s.frame.x
                    );
                    assert_eq!(
                        s.frame.y % align,
                        0,
                        "[{w}x{h} align {align}] {} y={} fuera de rejilla",
                        s.id,
                        s.frame.y
                    );
                }
            }
        }
    }

    #[test]
    fn guillotine_opens_a_second_page_when_nothing_fits() {
        let mut items = mixed_items();
        items.push(item("huge", 200, 200));
        let out = pack(&items, &guillotine_opts(PackingStrategy::Baf, 256)).unwrap();
        assert!(out.pages.len() >= 2, "el sprite grande fuerza otra página");
        assert_valid(&out);
        let total: usize = out.pages.iter().map(|p| p.placements.len()).sum();
        assert_eq!(
            total,
            items.len(),
            "todos los sprites acaban en alguna página"
        );
    }

    #[test]
    fn guillotine_best_is_never_worse_than_a_single_heuristic() {
        let items = mixed_items();
        let best = pack(&items, &guillotine_opts(PackingStrategy::Best, 256)).unwrap();
        assert_valid(&best);
        for strategy in PackingStrategy::all_heuristics() {
            let one = pack(&items, &guillotine_opts(strategy, 256)).unwrap();
            assert_valid(&one);
            assert!(
                size_metric(&best) <= size_metric(&one),
                "Best {:?} peor que {:?}: {:?} vs {:?}",
                PackingStrategy::Best,
                strategy,
                size_metric(&best),
                size_metric(&one)
            );
        }
    }

    #[test]
    fn guillotine_honours_rotation_when_allowed() {
        // `a` deja una banda de 44 px: `b` (20x60) solo cabe girada.
        let items = vec![item("a", 60, 20), item("b", 20, 60)];
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, true, 64, 0, 0, false);
        opts.algorithm = PackingAlgorithm::Guillotine;
        opts.pack_mode = PackMode::Fast;
        let out = pack(&items, &opts).unwrap();
        assert_valid(&out);
        assert_eq!(out.pages.len(), 1, "girando, todo cabe en una página");
        let b = out.pages[0]
            .placements
            .iter()
            .find(|p| p.id == "b")
            .unwrap();
        assert!(b.rotated, "b debería haberse girado 90°");

        // Sin rotación la misma pareja necesita dos páginas.
        opts.allow_rotation = false;
        let out2 = pack(&items, &opts).unwrap();
        assert_valid(&out2);
        assert_eq!(out2.pages.len(), 2, "sin rotación, b desborda la página");
    }

    #[test]
    fn guillotine_beats_the_grid_layout_on_a_mixed_set() {
        let items = mixed_items();
        let mut grid = PackerOptions::new(PackingStrategy::Bssf, false, 256, 4, 2, false);
        grid.algorithm = PackingAlgorithm::Grid;
        grid.pack_mode = PackMode::Fast;
        let g = pack(&items, &grid).unwrap();
        let u = pack(&items, &guillotine_opts(PackingStrategy::Best, 256)).unwrap();
        assert_valid(&g);
        assert!(
            size_metric(&u) <= size_metric(&g),
            "guillotine: {:?} vs rejilla {:?}",
            size_metric(&u),
            size_metric(&g)
        );
    }

    #[test]
    fn guillotine_free_list_stays_a_partition() {
        // El corte guillotina parte SOLO el rectángulo elegido en dos hijos:
        // entre el sprite y ellos tilingean el padre, sin solapes.
        let mut free = vec![Rect::new(0, 0, 100, 100)];
        let placed = Rect::new(0, 0, 60, 10);
        split_guillotine(&mut free, 0, placed);
        let area: i64 = free.iter().map(|r| r.area()).sum();
        assert_eq!(
            area + placed.area(),
            (100 * 100) as i64,
            "sprite + hijos deben cubrir el padre al completo"
        );
        assert_eq!(free.len(), 2);
        for i in 0..free.len() {
            assert!(!free[i].intersects(&placed), "ningún hijo pisa al sprite");
            for j in (i + 1)..free.len() {
                assert!(
                    !free[i].intersects(&free[j]),
                    "los hijos deben ser disjuntos"
                );
            }
        }
    }
}

#[cfg(test)]
mod presupuesto_m3_tests {
    use super::*;

    /// Huella de un empaquetado: tamaño de cada hoja y dónde cae cada
    /// sprite. Dos corridas iguales deben dar la misma huella.
    fn firma(out: &PackOutput) -> String {
        let mut s = String::new();
        for p in &out.pages {
            s.push_str(&format!("{}x{}:", p.width, p.height));
            for pl in &p.placements {
                s.push_str(&format!(
                    " {}@{},{}{}",
                    pl.id,
                    pl.frame.x,
                    pl.frame.y,
                    if pl.rotated { "r" } else { "" }
                ));
            }
            s.push('\n');
        }
        s
    }

    /// Rectángulos deterministas para alimentar la búsqueda.
    fn rects(n: i32, w: i32, h: i32) -> Vec<PackItem> {
        (0..n)
            .map(|i| PackItem {
                id: format!("s{i}"),
                width: w + i % 7,
                height: h + i % 5,
                mesh: None,
            })
            .collect()
    }

    /// M3: el presupuesto se gasta en **trabajo**. Cada comprobación de la
    /// búsqueda cobra lo que le costó, así que la misma entrada cobra
    /// exactamente lo mismo corrida tras corrida y en cualquier máquina.
    ///
    /// Con un presupuesto de reloj (el bug) nadie llama a
    /// `WorkBudget::gastar`: el contador se queda a cero y este test falla
    /// siempre, sin depender de la carga de la máquina ni del azar.
    #[test]
    fn el_presupuesto_se_gasta_en_trabajo() {
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, true, 4096, 4, 4, false);
        opts.pack_mode = PackMode::Good;

        // Entrada ligera: converge antes de agotar el presupuesto, pero
        // sigue cobrando lo que cuesta cada paso.
        let ligeros = rects(10, 32, 32);
        reiniciar_cobro();
        assert!(pack(&ligeros, &opts).is_ok());
        let gasto1 = cobrado();
        reiniciar_cobro();
        assert!(pack(&ligeros, &opts).is_ok());
        let gasto2 = cobrado();
        assert!(
            gasto1 > 0,
            "la búsqueda no cobró nada por comprobar tamaños: {gasto1}"
        );
        assert!(
            gasto1 < GOOD_WORK_UNITS,
            "una entrada que converge no debe gastar todo el presupuesto: {gasto1}"
        );
        assert_eq!(
            gasto1, gasto2,
            "la misma entrada debe costar siempre el mismo trabajo"
        );

        // Entrada pesada (17,2 M de trabajo convergido): no llega a
        // estabilizarse y se corta porque se agota el presupuesto.
        let pesados = rects(200, 64, 64);
        reiniciar_cobro();
        assert!(pack(&pesados, &opts).is_ok());
        let gasto_pesado = cobrado();
        assert!(
            gasto_pesado >= GOOD_WORK_UNITS,
            "esta entrada no converge: debe cortarse por presupuesto ({gasto_pesado})"
        );
    }

    /// M3: la búsqueda de tamaño se corta por **trabajo**, no por reloj, así
    /// que la misma entrada da el mismo atlas corrida tras corrida y en
    /// cualquier máquina.
    ///
    /// Antes el presupuesto era de reloj (400 ms en `Good`, 3 s en `Best`):
    /// la búsqueda comprobaba tantos tamaños como le diera la CPU y se
    /// cortaba a mitad según la carga. Medido antes de este cambio, una
    /// misma binaria con esta misma entrada daba **dos atlas distintos en
    /// cinco corridas** (los dos resultados que este test atrapa).
    #[test]
    fn la_busqueda_de_tamano_es_determinista() {
        // Entrada cara bastante cara para que el presupuesto se note: la
        // búsqueda convergida cuesta más del doble de lo que `Good` gasta.
        let mesh = TriangleMesh {
            vertices: vec![
                Point2D::new(0.0, 0.0),
                Point2D::new(64.0, 0.0),
                Point2D::new(64.0, 72.0),
                Point2D::new(160.0, 72.0),
                Point2D::new(160.0, 120.0),
                Point2D::new(0.0, 120.0),
            ],
            indices: vec![0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 5],
            uvs: vec![],
        };
        let items: Vec<PackItem> = (0..40)
            .map(|i| PackItem {
                id: format!("p{i}"),
                width: 160,
                height: 120,
                mesh: Some(mesh.clone()),
            })
            .collect();
        let mut opts = PackerOptions::new(PackingStrategy::Bssf, true, 4096, 4, 4, true);
        opts.pack_mode = PackMode::Good;

        let base = firma(&pack(&items, &opts).unwrap());
        assert!(!base.is_empty(), "la prueba empaqueta de verdad");
        for corrida in 1..=4 {
            let otra = firma(&pack(&items, &opts).unwrap());
            assert_eq!(
                otra, base,
                "corrida {corrida}: el atlas debe salir idéntico, no al azar del reloj"
            );
        }
    }
}
