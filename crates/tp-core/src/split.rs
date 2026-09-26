//! Sprite sheet slicing ("Dividir hoja"): cut an existing sheet into single
//! PNG frames laid out on a regular grid.

use crate::error::{Result, TpError};
use crate::types::Rect;
use crate::{export, ingest};
use std::path::{Path, PathBuf};

/// How the grid over the sheet is defined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridMode {
    /// Split the usable area into `cols` × `rows` equal cells.
    Grid { cols: i32, rows: i32 },
    /// Cut fixed-size `cell_w` × `cell_h` cells (the last row/column may be
    /// left out if it does not fit completely).
    Fixed { cell_w: i32, cell_h: i32 },
}

/// Description of a grid split over one sheet image.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitSpec {
    pub source: PathBuf,
    /// Pixels kept free on every border of the sheet.
    pub margin: i32,
    /// Pixels between neighbouring cells.
    pub spacing: i32,
    pub mode: GridMode,
}

impl Default for SplitSpec {
    fn default() -> Self {
        Self {
            source: PathBuf::new(),
            margin: 0,
            spacing: 0,
            mode: GridMode::Grid { cols: 4, rows: 1 },
        }
    }
}

/// One cell of the computed grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
    pub rect: Rect,
}

impl SplitSpec {
    /// Grid for an image of `img_w` × `img_h` pixels.
    pub fn cells(&self, img_w: i32, img_h: i32) -> Vec<Cell> {
        grid_cells(img_w, img_h, self)
    }

    /// Default output directory next to the sheet: `<stem>_slices/`.
    pub fn default_out_dir(&self) -> PathBuf {
        let parent = self.source.parent().unwrap_or_else(|| Path::new("."));
        let stem = self
            .source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sheet".to_string());
        parent.join(format!("{stem}_slices"))
    }
}

/// Compute the cells of `spec` for an image of `img_w` × `img_h` pixels.
///
/// Cells that would fall outside the usable area (image minus `margin` on both
/// sides) are dropped, so incomplete trailing rows/columns in [`GridMode::Fixed`]
/// are simply ignored.
pub fn grid_cells(img_w: i32, img_h: i32, spec: &SplitSpec) -> Vec<Cell> {
    let margin = spec.margin.max(0);
    let spacing = spec.spacing.max(0);
    let usable_w = (img_w - 2 * margin).max(0);
    let usable_h = (img_h - 2 * margin).max(0);
    if usable_w == 0 || usable_h == 0 {
        return Vec::new();
    }

    let (cols, rows, cell_w, cell_h) = match spec.mode {
        GridMode::Grid { cols, rows } => {
            let cols = cols.max(1);
            let rows = rows.max(1);
            let w = (usable_w - spacing * (cols - 1)) / cols;
            let h = (usable_h - spacing * (rows - 1)) / rows;
            (cols, rows, w, h)
        }
        GridMode::Fixed { cell_w, cell_h } => {
            if cell_w <= 0 || cell_h <= 0 {
                return Vec::new();
            }
            let cols = (usable_w + spacing) / (cell_w + spacing);
            let rows = (usable_h + spacing) / (cell_h + spacing);
            (cols, rows, cell_w, cell_h)
        }
    };
    if cols <= 0 || rows <= 0 || cell_w <= 0 || cell_h <= 0 {
        return Vec::new();
    }

    let mut cells = Vec::with_capacity((cols * rows) as usize);
    for row in 0..rows {
        for col in 0..cols {
            let x = margin + col * (cell_w + spacing);
            let y = margin + row * (cell_h + spacing);
            if x + cell_w > img_w || y + cell_h > img_h {
                continue;
            }
            cells.push(Cell {
                row: row as usize,
                col: col as usize,
                rect: Rect::new(x, y, cell_w, cell_h),
            });
        }
    }
    cells
}

/// Cut `spec.source` into single PNG files inside `out_dir`.
///
/// Files are named `<stem>_<row:02>_<col:02>.png` and overwritten on every
/// call, so slicing is idempotent. Returns the paths that were written.
pub fn slice_to_dir(spec: &SplitSpec, out_dir: &Path) -> Result<Vec<PathBuf>> {
    if spec.source.as_os_str().is_empty() {
        return Err("No se ha seleccionado ninguna hoja.".into());
    }
    let (img_w, img_h, rgba) = ingest::load_image_rgba(&spec.source)?;
    let cells = grid_cells(img_w, img_h, spec);
    if cells.is_empty() {
        return Err("La rejilla no produce ninguna celda válida.".into());
    }
    std::fs::create_dir_all(out_dir)
        .map_err(|e| TpError::Other(format!("No se pudo crear {}: {e}", out_dir.display())))?;

    let stem = spec
        .source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sheet".to_string());
    let mut written = Vec::with_capacity(cells.len());
    for cell in &cells {
        let buf = crop_rect(&rgba, img_w, &cell.rect);
        let bytes = export::encode_to_bytes(
            &buf,
            cell.rect.width as usize,
            cell.rect.height as usize,
            &export::EncodeOptions::default(),
        )?;
        let path = out_dir.join(format!("{stem}_{:02}_{:02}.png", cell.row, cell.col));
        std::fs::write(&path, bytes)
            .map_err(|e| TpError::Other(format!("No se pudo escribir {}: {e}", path.display())))?;
        written.push(path);
    }
    Ok(written)
}

/// Copy `rect` out of an RGBA buffer of an image `img_w` pixels wide.
fn crop_rect(rgba: &[u8], img_w: i32, rect: &Rect) -> Vec<u8> {
    let mut out = Vec::with_capacity((rect.width * rect.height * 4) as usize);
    for y in 0..rect.height {
        let src = (((rect.y + y) * img_w + rect.x) * 4) as usize;
        let len = (rect.width * 4) as usize;
        out.extend_from_slice(&rgba[src..src + len]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn spec(mode: GridMode, margin: i32, spacing: i32) -> SplitSpec {
        SplitSpec {
            source: PathBuf::from("sheet.png"),
            margin,
            spacing,
            mode,
        }
    }

    #[test]
    fn fixed_grid_respects_margin_and_spacing() {
        // 68x36 sheet, 4px margin, 4px spacing, 16x16 cells:
        // usable = 60x28 → cols = (60+4)/(16+4) = 3, rows = (28+4)/20 = 1
        let cells = grid_cells(
            68,
            36,
            &spec(
                GridMode::Fixed {
                    cell_w: 16,
                    cell_h: 16,
                },
                4,
                4,
            ),
        );
        assert_eq!(cells.len(), 3);
        let xs: Vec<i32> = cells.iter().map(|c| c.rect.x).collect();
        assert_eq!(xs, vec![4, 24, 44]);
        assert!(cells
            .iter()
            .all(|c| c.rect.width == 16 && c.rect.height == 16));
        assert!(cells
            .iter()
            .all(|c| c.rect.x + 16 <= 64 && c.rect.y + 16 <= 32));
    }

    #[test]
    fn fixed_grid_skips_cells_outside_the_image() {
        // Only one full 30px cell fits into 64px with no margin/spacing.
        let cells = grid_cells(
            64,
            32,
            &spec(
                GridMode::Fixed {
                    cell_w: 30,
                    cell_h: 30,
                },
                0,
                0,
            ),
        );
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].rect, Rect::new(0, 0, 30, 30));
        assert_eq!(cells[1].rect, Rect::new(30, 0, 30, 30));
    }

    #[test]
    fn grid_mode_splits_usable_area_equally() {
        // usable = 100x50 → 4 cols x 2 rows, cell 25x25
        let cells = grid_cells(100, 50, &spec(GridMode::Grid { cols: 4, rows: 2 }, 0, 0));
        assert_eq!(cells.len(), 8);
        assert_eq!(cells[0].rect, Rect::new(0, 0, 25, 25));
        assert_eq!(cells[7].rect, Rect::new(75, 25, 25, 25));
        assert_eq!(cells[3].col, 3);
        assert_eq!(cells[4].row, 1);
    }

    #[test]
    fn invalid_specs_produce_no_cells() {
        assert!(grid_cells(
            50,
            50,
            &spec(
                GridMode::Fixed {
                    cell_w: 0,
                    cell_h: 8
                },
                0,
                0
            )
        )
        .is_empty());
        assert!(grid_cells(
            10,
            10,
            &spec(
                GridMode::Fixed {
                    cell_w: 32,
                    cell_h: 32
                },
                0,
                0
            )
        )
        .is_empty());
        assert!(grid_cells(10, 10, &spec(GridMode::Grid { cols: 4, rows: 1 }, 6, 0)).is_empty());
    }

    #[test]
    fn slice_writes_one_png_per_cell() {
        let root = std::env::temp_dir().join(format!("tp_split_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let sheet = root.join("hoja.png");
        let mut img = image::RgbaImage::new(32, 16);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgba([(x % 255) as u8, (y % 255) as u8, 7, 255]);
        }
        img.save(&sheet).unwrap();

        let spec = SplitSpec {
            source: sheet,
            margin: 0,
            spacing: 0,
            mode: GridMode::Grid { cols: 4, rows: 2 },
        };
        let out = root.join("out");
        let written = slice_to_dir(&spec, &out).unwrap();
        assert_eq!(written.len(), 8);
        for path in &written {
            let (w, h, _) = ingest::load_image_rgba(path).unwrap();
            assert_eq!((w, h), (8, 8), "cell size for {}", path.display());
        }
        // Idempotent: a second run overwrites instead of failing.
        let again = slice_to_dir(&spec, &out).unwrap();
        assert_eq!(again.len(), 8);
        assert_eq!(written, again);
        let _ = std::fs::remove_dir_all(&root);
    }
}
