//! Floating "Dividir hoja" window: cut an existing sprite sheet into single
//! PNG frames on a regular grid and add them to the project.

use super::{App, LogKind};
use crate::i18n::t;
use eframe::egui;
use std::path::PathBuf;
use tp_core::split::{grid_cells, GridMode, SplitSpec};

/// Session state of the split-sheet window.
pub(super) struct SplitState {
    source: Option<PathBuf>,
    texture: Option<egui::TextureHandle>,
    img_w: i32,
    img_h: i32,
    use_grid: bool,
    cols: i32,
    rows: i32,
    cell_w: i32,
    cell_h: i32,
    margin: i32,
    spacing: i32,
    exclude_source: bool,
    auto_publish: bool,
}

impl Default for SplitState {
    fn default() -> Self {
        Self {
            source: None,
            texture: None,
            img_w: 0,
            img_h: 0,
            use_grid: true,
            cols: 4,
            rows: 4,
            cell_w: 32,
            cell_h: 32,
            margin: 0,
            spacing: 0,
            exclude_source: true,
            auto_publish: true,
        }
    }
}

impl SplitState {
    fn spec(&self) -> Option<SplitSpec> {
        let source = self.source.clone()?;
        let mode = if self.use_grid {
            GridMode::Grid {
                cols: self.cols,
                rows: self.rows,
            }
        } else {
            GridMode::Fixed {
                cell_w: self.cell_w,
                cell_h: self.cell_h,
            }
        };
        Some(SplitSpec {
            source,
            margin: self.margin,
            spacing: self.spacing,
            mode,
        })
    }
}

pub(super) fn split_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_split {
        return;
    }
    let mut open = app.show_split;
    super::ventana_flotante(t!("Dividir hoja"), egui::Id::new("split_sheet"), ctx)
        .open(&mut open)
        .default_size([430.0, 400.0])
        .min_size([330.0, 280.0])
        .resizable(true)
        .show(ctx, |ui| split_ui(app, ctx, ui));
    app.show_split = open;
}

fn split_ui(app: &mut App, ctx: &egui::Context, ui: &mut egui::Ui) {
    // --- source sheet ---------------------------------------------------
    ui.horizontal(|ui| {
        let name = app
            .split
            .source
            .as_ref()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| t!("Ninguna").to_string());
        ui.label(t!("Hoja: {}", name));
        if ui.button(t!("Elegir hoja…")).clicked() {
            pick_sheet(app, ctx);
        }
    });

    let Some(spec) = app.split.spec() else {
        super::separador(ui);
        ui.label(
            egui::RichText::new(t!(
                "Elige una hoja (sprite sheet) para dividirla en sprites individuales."
            ))
            .weak(),
        );
        // egui sizes windows by content; keep the panel tall enough while empty.
        let size = egui::vec2(ui.available_width(), 220.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        // Un visor vacío enseña su suelo (re-review D3: el gris fijo 22
        // sobre un diálogo de 226 dejaba el texto a 4,10:1 en claro).
        let visor = super::visores(ui.visuals().dark_mode);
        painter.rect_filled(rect, 4.0, visor.suelo);
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0_f32, visor.borde),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            t!("Sin hoja seleccionada"),
            crate::ui_prefs::font_id(13.0, ui.ctx()),
            visor.texto,
        );
        return;
    };
    ui.horizontal(|ui| {
        ui.label(format!("{} × {} px", app.split.img_w, app.split.img_h));
        super::separador(ui);
        ui.label(t!(
            "{} celda(s) de {} px",
            grid_cells(app.split.img_w, app.split.img_h, &spec).len(),
            if app.split.use_grid {
                "variable"
            } else {
                t!("fijo")
            }
        ));
    });

    // --- grid settings --------------------------------------------------
    ui.horizontal(|ui| {
        ui.selectable_value(&mut app.split.use_grid, true, t!("Columnas × filas"));
        ui.selectable_value(&mut app.split.use_grid, false, t!("Tamaño fijo"));
    });
    ui.horizontal(|ui| {
        if app.split.use_grid {
            ui.add(
                egui::DragValue::new(&mut app.split.cols)
                    .range(1..=128)
                    .prefix(t!("cols ")),
            );
            ui.add(
                egui::DragValue::new(&mut app.split.rows)
                    .range(1..=128)
                    .prefix(t!("filas ")),
            );
        } else {
            ui.add(
                egui::DragValue::new(&mut app.split.cell_w)
                    .range(1..=4096)
                    .prefix(t!("ancho ")),
            );
            ui.add(
                egui::DragValue::new(&mut app.split.cell_h)
                    .range(1..=4096)
                    .prefix(t!("alto ")),
            );
        }
        ui.add(
            egui::DragValue::new(&mut app.split.margin)
                .range(0..=512)
                .prefix(t!("margen ")),
        );
        ui.add(
            egui::DragValue::new(&mut app.split.spacing)
                .range(0..=128)
                .prefix(t!("espaciado ")),
        );
    });

    // --- preview --------------------------------------------------------
    let cells = grid_cells(app.split.img_w, app.split.img_h, &spec);
    if let Some(texture) = &app.split.texture {
        let zoom = {
            let avail = ui.available_width() - 16.0;
            (avail / app.split.img_w.max(1) as f32).clamp(0.1, 4.0)
        };
        let size = egui::vec2(app.split.img_w as f32 * zoom, app.split.img_h as f32 * zoom);
        egui::ScrollArea::vertical()
            .id_salt("split_preview")
            .auto_shrink([false, false])
            .max_height(260.0)
            .show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, super::visores(ui.visuals().dark_mode).hoja);
                painter.image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                for cell in &cells {
                    let r = egui::Rect::from_min_max(
                        egui::pos2(
                            rect.min.x + cell.rect.x as f32 * zoom,
                            rect.min.y + cell.rect.y as f32 * zoom,
                        ),
                        egui::pos2(
                            rect.min.x + (cell.rect.x + cell.rect.width) as f32 * zoom,
                            rect.min.y + (cell.rect.y + cell.rect.height) as f32 * zoom,
                        ),
                    );
                    painter.rect_stroke(
                        r,
                        0.0,
                        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0, 220, 255)),
                        egui::StrokeKind::Inside,
                    );
                }
            });
    }

    // --- options & action ----------------------------------------------
    ui.checkbox(
        &mut app.split.exclude_source,
        t!("Excluir la hoja original del set de sprites"),
    );
    ui.checkbox(&mut app.split.auto_publish, t!("Publicar al terminar"));
    ui.horizontal(|ui| {
        let can_split = !cells.is_empty() && app.split.source.is_some();
        if ui
            .add_enabled(can_split, egui::Button::new(t!("Dividir")))
            .on_hover_text(t!("Escribe cada celda como PNG y la añade al proyecto"))
            // RB4: con el botón apagado egui no enseña el de arriba, pero
            // la ayuda no puede quedarse en blanco: hay que decir por qué
            // no se puede dividir y qué tocar para que se pueda. Las dos
            // cuadrículas fallan por sitios distintos.
            .on_disabled_hover_text(if app.split.use_grid {
                t!("No hay celdas con la cuadrícula actual: baja el margen o el espaciado, o reduce las columnas y filas")
            } else {
                t!("No hay celdas con la cuadrícula actual: baja el margen, o la celda no cabe en la hoja")
            })
            .clicked()
        {
            run_split(app, &spec, cells.len());
        }
        if can_split {
            ui.label(format!("➡ {}", spec.default_out_dir().display()));
        }
    });
}

fn pick_sheet(app: &mut App, ctx: &egui::Context) {
    let picked = rfd::FileDialog::new()
        .add_filter(
            t!("Imágenes"),
            &[
                "png", "webp", "jpg", "jpeg", "tga", "bmp", "gif", "ico", "tiff", "tif", "dds",
                "qoi", "pbm", "pgm", "ppm", "pnm", "xbm", "xpm", "astc", "ktx", "ktx2", "basis",
                "psd", "svg", "svgz", "pkm", "pvr", "pvrtc", "ccz", "gz",
            ],
        )
        .pick_file();
    let Some(path) = picked else {
        return;
    };
    match tp_core::ingest::load_image_rgba(&path) {
        Ok((w, h, rgba)) => {
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            let name = format!(
                "split_{}_{w}x{h}",
                path.file_name()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default()
            );
            app.split.texture = Some(ctx.load_texture(name, img, egui::TextureOptions::NEAREST));
            app.split.img_w = w;
            app.split.img_h = h;
            app.split.source = Some(path.clone());
            app.aviso(LogKind::Info, t!("Hoja cargada: {}", path.display()));
        }
        Err(e) => app.aviso(
            LogKind::Error,
            t!(
                "No se pudo cargar la hoja: {}",
                crate::i18n::tr(&e.to_string())
            ),
        ),
    }
}

fn run_split(app: &mut App, spec: &SplitSpec, expected: usize) {
    let out_dir = spec.default_out_dir();
    match tp_core::split::slice_to_dir(spec, &out_dir) {
        Ok(files) => {
            debug_assert_eq!(files.len(), expected);
            app.aviso(
                LogKind::Info,
                t!(
                    "{} sprite(s) escrito(s) en {}.",
                    files.len(),
                    out_dir.display()
                ),
            );
            if app.split.exclude_source {
                app.exclude(&spec.source);
                app.log(
                    LogKind::Info,
                    t!("Hoja original excluida: {}", spec.source.display()),
                );
            }
            if app.add_input(out_dir.clone()) {
                app.log(LogKind::Info, t!("Carpeta añadida: {}", out_dir.display()));
            } else {
                app.log(
                    LogKind::Warning,
                    t!("La carpeta de sprites ya estaba añadida.").into(),
                );
            }
            if app.split.auto_publish && app.running.is_none() {
                app.start_pack(false);
            }
        }
        Err(e) => app.aviso(
            LogKind::Error,
            t!(
                "No se pudo dividir la hoja: {}",
                crate::i18n::tr(&e.to_string())
            ),
        ),
    }
}

/// RB4: «Dividir» deshabilitada conservaba la ayuda de la versión activa
/// («Escribe cada celda…») sin decir por qué no se puede usar.
#[cfg(test)]
mod dividir_deshabilitada_tests {
    use super::*;
    use crate::testing::textos_pintados;

    /// Un frame sólo de la ventana de división, con el reloj a mano: la
    /// ayuda tarda medio segundo de puntero quieto en salir, así que es
    /// el test quien mueve el tiempo.
    fn frame(
        app: &mut App,
        ctx: &egui::Context,
        t: f64,
        eventos: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let entrada = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1360.0, 860.0),
            )),
            time: Some(t),
            events: eventos,
            ..egui::RawInput::default()
        };
        ctx.run(entrada, |_| split_window(app, ctx))
    }

    /// Hoja de 64×64 px con `margen` píxeles: con 40 no cabe ninguna
    /// celda y el botón se queda apagado.
    fn hoja(margen: i32) -> (App, egui::Context) {
        let ctx = egui::Context::default();
        let mut app = App::new_for_testing(ctx.clone(), None);
        app.show_split = true;
        app.split.source = Some(PathBuf::from("/tmp/hoja.png"));
        app.split.img_w = 64;
        app.split.img_h = 64;
        app.split.margin = margen;
        (app, ctx)
    }

    /// Punto del rótulo «Dividir». La ventana abre en dos pasadas (la
    /// primera sólo guarda su área), así que se pinta un par de frames
    /// antes de buscarlo.
    fn dividi(app: &mut App, ctx: &egui::Context) -> egui::Pos2 {
        for t in [0.1, 0.2, 0.3] {
            let out = frame(app, ctx, t, Vec::new());
            if let Some(donde) = textos_pintados(&out)
                .iter()
                .filter(|(texto, rect, recorte)| {
                    texto.trim() == "Dividir" && recorte.contains(rect.center())
                })
                .map(|(_, rect, _)| rect.center())
                .next()
            {
                return donde;
            }
        }
        panic!("el botón «Dividir» debe estar en la ventana de división");
    }

    /// Toda la ayuda pintada tras dejar el puntero quieto encima del
    /// botón (egui espera medio segundo de quietud).
    fn ayuda_sobre(app: &mut App, ctx: &egui::Context, donde: egui::Pos2) -> String {
        frame(app, ctx, 0.5, vec![egui::Event::PointerMoved(donde)]);
        frame(app, ctx, 1.0, Vec::new());
        frame(app, ctx, 1.1, Vec::new());
        frame(app, ctx, 1.2, Vec::new());
        let out = frame(app, ctx, 1.3, Vec::new());
        textos_pintados(&out)
            .iter()
            .map(|(texto, _, _)| texto.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn dividir_deshabilitada_explica_el_motivo() {
        // Sin celdas (40 px de margen en una hoja de 64): el botón apagado
        // explica por qué y qué tocar.
        let (mut app, ctx) = hoja(40);
        let spec = app.split.spec().expect("la hoja está elegida");
        assert!(
            grid_cells(app.split.img_w, app.split.img_h, &spec).is_empty(),
            "la primera mitad necesita una cuadrícula sin celdas"
        );
        let donde = dividi(&mut app, &ctx);
        let ayuda = ayuda_sobre(&mut app, &ctx, donde);
        assert!(
            ayuda.contains("No hay celdas con la cuadrícula actual"),
            "el botón apagado debe explicar el motivo: {ayuda}"
        );
        assert!(
            !ayuda.contains("Escribe cada celda"),
            "la ayuda de la versión activa no debe acompañar a la apagada: {ayuda}"
        );

        // Control: con celdas, la misma posición enseña la ayuda de
        // siempre —sin esta mitad, la primera no distinguiría nada—.
        let (mut app, ctx) = hoja(0);
        let donde = dividi(&mut app, &ctx);
        let ayuda = ayuda_sobre(&mut app, &ctx, donde);
        assert!(
            ayuda.contains("Escribe cada celda"),
            "control: con celdas la ayuda es la de siempre: {ayuda}"
        );
        assert!(
            !ayuda.contains("No hay celdas"),
            "con celdas no debe avisarse de que no las hay: {ayuda}"
        );

        // La otra cuadrícula falla por otro sitio —una celda de 128 px no
        // cabe en 64— y el motivo debe nombrar sus propios controles.
        let (mut app, ctx) = hoja(0);
        app.split.use_grid = false;
        app.split.cell_w = 128;
        app.split.cell_h = 32;
        let spec = app.split.spec().expect("la hoja está elegida");
        assert!(
            grid_cells(app.split.img_w, app.split.img_h, &spec).is_empty(),
            "esta mitad necesita una celda más grande que la hoja"
        );
        let donde = dividi(&mut app, &ctx);
        let ayuda = ayuda_sobre(&mut app, &ctx, donde);
        assert!(
            ayuda.contains("la celda no cabe en la hoja"),
            "en tamaño fijo el motivo nombra el tamaño de celda: {ayuda}"
        );
        assert!(
            !ayuda.contains("reduce las columnas y filas"),
            "el motivo de la otra cuadrícula no debe confundir los controles: {ayuda}"
        );
    }
}
