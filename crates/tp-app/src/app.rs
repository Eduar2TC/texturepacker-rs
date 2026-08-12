//! Main application state and views.

use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use tp_core::config::{
    ColorDepth, DitheringAlgorithm, GpuFormat, PackingStrategy, ProjectConfig, TemplateFormat,
};
use tp_core::pipeline::PipelineOutput;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Atlas,
    Sprites,
    Mesh,
    Pivots,
    Output,
    Log,
}

#[derive(Clone)]
enum LogKind {
    Info,
    Warning,
    Error,
}

struct LogEntry {
    kind: LogKind,
    text: String,
}

struct RunMessage {
    elapsed_ms: u128,
    result: Result<PipelineOutput, String>,
}

pub struct App {
    config: ProjectConfig,
    input_dir_text: String,
    output_dir_text: String,
    variants_text: String,
    result: Option<PipelineOutput>,
    textures: Vec<egui::TextureHandle>,
    running: Option<Receiver<RunMessage>>,
    tab: Tab,
    selected_sprite: Option<String>,
    selected_page: usize,
    zoom: f32,
    show_frames: bool,
    show_pivots: bool,
    logs: Vec<LogEntry>,
    project_path: Option<PathBuf>,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            config: ProjectConfig::default(),
            input_dir_text: String::new(),
            output_dir_text: String::new(),
            variants_text: "1.0".to_string(),
            result: None,
            textures: Vec::new(),
            running: None,
            tab: Tab::Atlas,
            selected_sprite: None,
            selected_page: 0,
            zoom: 1.0,
            show_frames: true,
            show_pivots: true,
            logs: vec![LogEntry {
                kind: LogKind::Info,
                text: "Bienvenido a TexturePacker-RS. Elige un directorio de entrada y pulsa «Empaquetar».".into(),
            }],
            project_path: None,
        }
    }

    fn log(&mut self, kind: LogKind, text: String) {
        self.logs.push(LogEntry { kind, text });
        if self.logs.len() > 2000 {
            self.logs.drain(0..self.logs.len() - 2000);
        }
    }

    fn sync_variants(&mut self) {
        self.variants_text = self
            .config
            .scale_variants
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ");
    }

    /// Keep the text fields in sync with the config before running/saving.
    fn commit_paths(&mut self) {
        self.config.input_directory = PathBuf::from(self.input_dir_text.trim());
        self.config.output_directory = PathBuf::from(self.output_dir_text.trim());
    }

    fn parse_variants(&mut self) {
        let parsed: Vec<f32> = self
            .variants_text
            .split([',', ';', ' '])
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .filter(|v| *v > 0.0 && *v <= 1.0)
            .collect();
        if !parsed.is_empty() {
            self.config.scale_variants = parsed;
        }
    }

    fn start_pack(&mut self) {
        self.commit_paths();
        self.parse_variants();
        if let Err(e) = self.config.validate() {
            self.log(LogKind::Error, e);
            return;
        }
        if self.config.input_directory.as_os_str().is_empty() {
            self.log(LogKind::Error, "Selecciona un directorio de entrada.".into());
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let cfg = self.config.clone();
        let started = std::time::Instant::now();
        std::thread::spawn(move || {
            let result = tp_core::pipeline::run(&cfg);
            let _ = tx.send(RunMessage {
                elapsed_ms: started.elapsed().as_millis(),
                result,
            });
        });
        self.running = Some(rx);
        self.log(
            LogKind::Info,
            format!("Empaquetando desde {} ...", self.config.input_directory.display()),
        );
    }

    fn rebuild_textures(&mut self, ctx: &egui::Context) {
        self.textures.clear();
        let Some(out) = &self.result else {
            return;
        };
        for page in &out.pages {
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [page.width as usize, page.height as usize],
                &page.pixels,
            );
            let name = format!("page_{}", page.index);
            self.textures.push(
                ctx.load_texture(name, img, egui::TextureOptions::NEAREST),
            );
        }
    }

    fn handle_run_result(&mut self, ctx: &egui::Context, msg: RunMessage) {
        self.running = None;
        match msg.result {
            Ok(out) => {
                let total = out.result.total_sprites;
                let aliases = out.result.alias_count;
                let pages = out.pages.len();
                self.log(
                    LogKind::Info,
                    format!(
                        "Empaquetado completo en {} ms: {total} sprites ({aliases} aliases), {pages} página(s).",
                        msg.elapsed_ms
                    ),
                );
                for w in &out.result.warnings {
                    self.log(LogKind::Warning, w.clone());
                }
                for (stage, ms) in &out.result.stage_times_ms {
                    self.log(LogKind::Info, format!("  [{stage}] {ms} ms"));
                }
                for f in &out.result.output_files {
                    self.log(LogKind::Info, format!("  → {f}"));
                }
                self.selected_page = 0;
                self.selected_sprite = None;
                self.result = Some(out);
                self.rebuild_textures(ctx);
            }
            Err(e) => {
                self.log(LogKind::Error, format!("Empaquetado fallido: {e}"));
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll the background packing thread.
        if let Some(rx) = &self.running {
            match rx.try_recv() {
                Ok(msg) => self.handle_run_result(ctx, msg),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.running = None;
                    self.log(LogKind::Error, "El hilo de empaquetado terminó inesperadamente.".into());
                }
            }
        }

        egui::TopBottomPanel::top("topbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("🧩 TexturePacker-RS");
                ui.separator();
                if ui.button("📦 Empaquetar").clicked() {
                    self.start_pack();
                }
                if ui.button("💾 Guardar proyecto").clicked() {
                    self.save_project();
                }
                if ui.button("📂 Cargar proyecto").clicked() {
                    self.load_project();
                }
                if ui.button("↺ Valores por defecto").clicked() {
                    self.config = ProjectConfig::default();
                    self.sync_variants();
                }
                if let Some(path) = &self.project_path {
                    ui.separator();
                    ui.label(egui::RichText::new(path.display().to_string()).weak());
                }
                if self.running.is_some() {
                    ui.separator();
                    ui.spinner();
                    ui.label("Empaquetando…");
                }
            });
        });

        egui::SidePanel::left("config")
            .resizable(true)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.heading("Configuración");
                ui.add_space(4.0);
                egui::ScrollArea::vertical().show(ui, |ui| self.config_ui(ui));
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Atlas, "Atlas");
                ui.selectable_value(&mut self.tab, Tab::Sprites, "Sprites");
                ui.selectable_value(&mut self.tab, Tab::Mesh, "Malla");
                ui.selectable_value(&mut self.tab, Tab::Pivots, "Pivots");
                ui.selectable_value(&mut self.tab, Tab::Output, "Salida");
                ui.selectable_value(&mut self.tab, Tab::Log, "Log");
            });
            ui.separator();
            match self.tab {
                Tab::Atlas => self.view_atlas(ui),
                Tab::Sprites => self.view_sprites(ui),
                Tab::Mesh => self.view_mesh(ui),
                Tab::Pivots => self.view_pivots(ui),
                Tab::Output => self.view_output(ui),
                Tab::Log => self.view_log(ui),
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Config form
// ---------------------------------------------------------------------------

impl App {
    fn config_ui(&mut self, ui: &mut egui::Ui) {
        self.input_dir_text = self.config.input_directory.display().to_string();
        self.output_dir_text = self.config.output_directory.display().to_string();
        egui::CollapsingHeader::new("Entrada / Salida")
            .default_open(true)
            .show(ui, |ui| {
                ui.label("Directorio de entrada");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.input_dir_text).desired_width(170.0));
                    if ui.button("…").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.input_dir_text = dir.display().to_string();
                            self.config.input_directory = dir;
                        }
                    }
                });
                ui.label("Directorio de salida");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.output_dir_text).desired_width(170.0));
                    if ui.button("…").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            self.output_dir_text = dir.display().to_string();
                            self.config.output_directory = dir;
                        }
                    }
                });
                ui.checkbox(&mut self.config.recursive, "Buscar en subdirectorios");
                ui.label("Nombre base de los archivos");
                ui.add(egui::TextEdit::singleline(&mut self.config.base_file_name).desired_width(170.0));
            });

        egui::CollapsingHeader::new("Atlas")
            .default_open(true)
            .show(ui, |ui| {
                let sizes = [512i32, 1024, 2048, 4096, 8192];
                egui::ComboBox::from_label("Tamaño máximo")
                    .selected_text(self.config.max_texture_size.to_string())
                    .show_ui(ui, |ui| {
                        for s in sizes {
                            ui.selectable_value(&mut self.config.max_texture_size, s, s.to_string());
                        }
                    });
                ui.add(
                    egui::Slider::new(&mut self.config.padding, 0..=16).text("Padding (px)"),
                );
                ui.add(
                    egui::Slider::new(&mut self.config.extrude, 0..=16).text("Extrude (px)"),
                );
                ui.checkbox(&mut self.config.allow_rotation, "Permitir rotación 90°");
                enum_combo(
                    ui,
                    "Estrategia",
                    self.config.packing_strategy.as_str(),
                    |ui, v| {
                        ui.selectable_value(v, PackingStrategy::Bssf, "MaxRects BSSF");
                        ui.selectable_value(v, PackingStrategy::Baf, "MaxRects BAF");
                        ui.selectable_value(v, PackingStrategy::Blsf, "MaxRects BLSF");
                        ui.selectable_value(v, PackingStrategy::Guillotine, "Guillotine");
                    },
                    &mut self.config.packing_strategy,
                );
                ui.label("Variantes de escala (p. ej. 1.0, 0.5)");
                ui.add(
                    egui::TextEdit::singleline(&mut self.variants_text).desired_width(170.0),
                );
            });

        egui::CollapsingHeader::new("Recorte / Aliasing")
            .default_open(false)
            .show(ui, |ui| {
                ui.checkbox(&mut self.config.enable_trim, "Recortar bordes transparentes");
                ui.add_enabled(
                    self.config.enable_trim,
                    egui::Slider::new(&mut self.config.trim_threshold, 0..=255)
                        .text("Umbral de alpha"),
                );
                ui.checkbox(&mut self.config.enable_aliasing, "Detección de duplicados (alias)");
                ui.checkbox(&mut self.config.enable_normal_maps, "Empaquetar mapas de normales");
            });

        egui::CollapsingHeader::new("Polígonos")
            .default_open(false)
            .show(ui, |ui| {
                ui.checkbox(&mut self.config.enable_polygon, "Modo polígono (mallas)");
                ui.add_enabled(
                    self.config.enable_polygon,
                    egui::Slider::new(&mut self.config.polygon_tolerance, 0.0..=10.0)
                        .text("Tolerancia (RDP)"),
                );
                if self.config.enable_polygon {
                    ui.label(egui::RichText::new(
                        "Empaqueta sprites por su contorno (Marching Squares → RDP → Earcut).",
                    ).weak());
                }
            });

        egui::CollapsingHeader::new("Color / VRAM")
            .default_open(false)
            .show(ui, |ui| {
                enum_combo(
                    ui,
                    "Profundidad de color",
                    self.config.color_depth.as_str(),
                    |ui, v| {
                        ui.selectable_value(v, ColorDepth::Rgba8888, "RGBA8888");
                        ui.selectable_value(v, ColorDepth::Rgba4444, "RGBA4444");
                        ui.selectable_value(v, ColorDepth::Rgb565, "RGB565");
                    },
                    &mut self.config.color_depth,
                );
                enum_combo(
                    ui,
                    "Dithering",
                    dither_name(self.config.dithering_algorithm),
                    |ui, v| {
                        ui.selectable_value(v, DitheringAlgorithm::None, "Ninguno");
                        ui.selectable_value(v, DitheringAlgorithm::FloydSteinberg, "Floyd–Steinberg");
                        ui.selectable_value(v, DitheringAlgorithm::Atkinson, "Atkinson");
                    },
                    &mut self.config.dithering_algorithm,
                );
                enum_combo(
                    ui,
                    "Formato GPU",
                    self.config.gpu_format.as_str(),
                    |ui, v| {
                        ui.selectable_value(v, GpuFormat::Png, "PNG");
                        ui.selectable_value(v, GpuFormat::WebP, "WebP (lossless)");
                        ui.selectable_value(v, GpuFormat::Astc4x4, "ASTC 4x4");
                        ui.selectable_value(v, GpuFormat::Etc2Rgba, "ETC2 RGBA");
                        ui.selectable_value(v, GpuFormat::Pvrtc4Bpp, "PVRTC 4BPP");
                    },
                    &mut self.config.gpu_format,
                );
            });

        egui::CollapsingHeader::new("Exportación")
            .default_open(false)
            .show(ui, |ui| {
                enum_combo(
                    ui,
                    "Formato de metadatos",
                    template_name(self.config.template_format),
                    |ui, v| {
                        ui.selectable_value(v, TemplateFormat::Json, "JSON");
                        ui.selectable_value(v, TemplateFormat::Xml, "XML (libgdx)");
                        ui.selectable_value(v, TemplateFormat::Plist, "Plist (cocos2d)");
                        ui.selectable_value(v, TemplateFormat::CppHeader, "Cabecera C++");
                        ui.selectable_value(v, TemplateFormat::Tsv, "TSV");
                        ui.selectable_value(v, TemplateFormat::PlainText, "Texto plano");
                    },
                    &mut self.config.template_format,
                );
                ui.label("Plantilla Mustache personalizada (opcional)");
                ui.horizontal(|ui| {
                    let mut path = self
                        .config
                        .export_template
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    ui.add(egui::TextEdit::singleline(&mut path).desired_width(170.0));
                    if ui.button("…").clicked() {
                        if let Some(f) = rfd::FileDialog::new().pick_file() {
                            self.config.export_template = Some(f);
                        }
                    } else {
                        self.config.export_template = if path.is_empty() {
                            None
                        } else {
                            Some(PathBuf::from(path))
                        };
                    }
                });
                ui.label("Clave de cifrado AES-256-GCM (opcional)");
                let mut key = self.config.encryption_key.clone().unwrap_or_default();
                if ui
                    .add(egui::TextEdit::singleline(&mut key).desired_width(170.0).password(true))
                    .changed()
                {
                    self.config.encryption_key = if key.is_empty() { None } else { Some(key) };
                }
            });

        egui::CollapsingHeader::new("Pivots")
            .default_open(false)
            .show(ui, |ui| {
                ui.label("Pivot por defecto (normalizado 0..1)");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.config.default_pivot_x).range(0.0..=1.0).speed(0.01));
                    ui.add(egui::DragValue::new(&mut self.config.default_pivot_y).range(0.0..=1.0).speed(0.01));
                });
                ui.label(egui::RichText::new(
                    "También puedes cargar `pivots.json` en el directorio de entrada.",
                ).weak());
            });

        ui.add_space(8.0);
        if ui.button("📦 Empaquetar ahora").clicked() {
            self.start_pack();
        }
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

impl App {
    fn view_atlas(&mut self, ui: &mut egui::Ui) {
        let Some(out) = &self.result else {
            ui.centered_and_justified(|ui| {
                ui.label("Aún no hay atlas. Configura el proyecto y pulsa «Empaquetar».");
            });
            return;
        };
        if out.pages.is_empty() {
            ui.label("El resultado no tiene páginas.");
            return;
        }
        if self.selected_page >= out.pages.len() {
            self.selected_page = 0;
        }

        ui.horizontal(|ui| {
            if out.pages.len() > 1 {
                egui::ComboBox::from_label("Página")
                    .selected_text(format!("{}", self.selected_page))
                    .show_ui(ui, |ui| {
                        for i in 0..out.pages.len() {
                            ui.selectable_value(&mut self.selected_page, i, format!("{i}"));
                        }
                    });
            }
            ui.add(egui::Slider::new(&mut self.zoom, 0.05..=8.0).logarithmic(true).text("Zoom"));
            ui.checkbox(&mut self.show_frames, "Marcos");
            ui.checkbox(&mut self.show_pivots, "Pivots");
            if let Some(name) = &self.selected_sprite {
                ui.separator();
                ui.label(egui::RichText::new(format!("Seleccionado: {name}")).strong());
            }
        });

        let page = &out.pages[self.selected_page];
        let tex = &self.textures[self.selected_page];
        let size = egui::vec2(
            page.width as f32 * self.zoom,
            page.height as f32 * self.zoom,
        );
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.painter().image(
                    tex.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                let to_screen = |x: i32, y: i32| {
                    egui::pos2(
                        rect.min.x + x as f32 * self.zoom,
                        rect.min.y + y as f32 * self.zoom,
                    )
                };
                if self.show_frames {
                    for sprite in out
                        .result
                        .sprites
                        .iter()
                        .filter(|s| s.atlas_page_index as usize == self.selected_page)
                    {
                        let f = sprite.allocated_frame;
                        let color = if Some(&sprite.id) == self.selected_sprite.as_ref() {
                            egui::Color32::YELLOW
                        } else if sprite.is_alias {
                            egui::Color32::GRAY
                        } else {
                            egui::Color32::from_rgb(0, 220, 255)
                        };
                        let r = egui::Rect::from_min_max(to_screen(f.x, f.y), to_screen(f.x + f.width, f.y + f.height));
                        ui.painter().rect_stroke(
                            r,
                            0.0,
                            egui::Stroke::new(1.0, color),
                            egui::StrokeKind::Inside,
                        );
                        if self.zoom > 1.5 {
                            ui.painter().text(
                                to_screen(f.x + 1, f.y + 10),
                                egui::Align2::LEFT_TOP,
                                &sprite.id,
                                egui::FontId::proportional(9.0),
                                color,
                            );
                        }
                    }
                }
                if self.show_pivots {
                    for sprite in out
                        .result
                        .sprites
                        .iter()
                        .filter(|s| s.atlas_page_index as usize == self.selected_page && !s.is_alias)
                    {
                        let v = sprite.visible_frame;
                        let px = v.x as f32 + sprite.pivot.x * v.width as f32;
                        let py = v.y as f32 + sprite.pivot.y * v.height as f32;
                        let p = to_screen(px as i32, py as i32);
                        ui.painter().circle_filled(p, 3.0, egui::Color32::RED);
                    }
                }
                // Hover -> select sprite under the cursor.
                if let Some(pos) = response.hover_pos() {
                    let px = ((pos.x - rect.min.x) / self.zoom) as i32;
                    let py = ((pos.y - rect.min.y) / self.zoom) as i32;
                    if let Some(sprite) = out
                        .result
                        .sprites
                        .iter()
                        .find(|s| {
                            s.atlas_page_index as usize == self.selected_page
                                && s.allocated_frame.contains(&tp_core::types::Rect::new(px, py, 1, 1))
                        })
                    {
                        self.selected_sprite = Some(sprite.id.clone());
                    }
                }
            });
    }

    fn view_sprites(&mut self, ui: &mut egui::Ui) {
        let Some(out) = &self.result else {
            ui.centered_and_justified(|ui| ui.label("Ejecuta un empaquetado primero."));
            return;
        };
        let mut selected: Option<String> = None;
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("sprites")
                .striped(true)
                .min_col_width(70.0)
                .show(ui, |ui| {
                    ui.strong("id");
                    ui.strong("tamaño");
                    ui.strong("recortado");
                    ui.strong("frame (x,y,w,h)");
                    ui.strong("rot");
                    ui.strong("página");
                    ui.strong("alias");
                    ui.strong("pivot");
                    ui.strong("malla");
                    ui.end_row();
                    for s in &out.result.sprites {
                        let is_sel = self.selected_sprite.as_ref() == Some(&s.id);
                        if ui.selectable_label(is_sel, &s.id).clicked() {
                            selected = Some(s.id.clone());
                            self.selected_page = s.atlas_page_index.max(0) as usize;
                        }
                        ui.label(format!("{}x{}", s.raw_width, s.raw_height));
                        ui.label(format!(
                            "{}x{}@({},{})",
                            s.trimmed_bounds.width,
                            s.trimmed_bounds.height,
                            s.offset_x,
                            s.offset_y
                        ));
                        let f = s.allocated_frame;
                        ui.label(format!("({},{},{},{})", f.x, f.y, f.width, f.height));
                        ui.label(if s.is_rotated { "90°" } else { "—" });
                        ui.label(s.atlas_page_index.to_string());
                        ui.label(if s.is_alias {
                            format!("→ {}", s.alias_target_id.as_deref().unwrap_or("?"))
                        } else {
                            "—".into()
                        });
                        ui.label(format!("({:.2},{:.2})", s.pivot.x, s.pivot.y));
                        ui.label(if s.mesh.is_some() { "✓" } else { "—" });
                        ui.end_row();
                    }
                });
        });
        if let Some(id) = selected {
            self.selected_sprite = Some(id);
        }
    }

    fn view_mesh(&mut self, ui: &mut egui::Ui) {
        let Some(out) = &self.result else {
            ui.centered_and_justified(|ui| ui.label("Ejecuta un empaquetado primero."));
            return;
        };
        let Some(sel) = &self.selected_sprite else {
            ui.centered_and_justified(|ui| ui.label("Selecciona un sprite en la pestaña Sprites o Atlas."));
            return;
        };
        let Some(sprite) = out.result.sprites.iter().find(|s| &s.id == sel) else {
            return;
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Malla de «{sel}»")).strong());
            if sprite.is_alias {
                ui.label(egui::RichText::new("(alias — reutiliza el frame de su objetivo)").weak());
            }
        });
        match &sprite.mesh {
            None => {
                ui.label("Este sprite no tiene malla (modo polígono desactivado o sprite alias).");
            }
            Some(mesh) => {
                ui.label(format!(
                    "Vértices: {} · Triángulos: {}",
                    mesh.vertices.len(),
                    mesh.indices.len() / 3
                ));
                ui.label(format!(
                    "Contornos: {}",
                    sprite.contours.len()
                ));

                // Draw the contours in a small canvas.
                let tw = sprite.trimmed_bounds.width.max(1) as f32;
                let th = sprite.trimmed_bounds.height.max(1) as f32;
                let scale = 220.0 / tw.max(th);
                let size = egui::vec2(tw * scale, th * scale);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let painter = ui.painter();
                painter.rect_filled(rect, 2.0, egui::Color32::from_gray(20));
                for contour in &sprite.contours {
                    let pts: Vec<egui::Pos2> = contour
                        .points
                        .iter()
                        .map(|p| {
                            egui::pos2(
                                rect.min.x + p.x * scale,
                                rect.min.y + p.y * scale,
                            )
                        })
                        .collect();
                    if pts.len() >= 2 {
                        let color = if contour.is_hole {
                            egui::Color32::from_rgb(255, 120, 120)
                        } else {
                            egui::Color32::from_rgb(120, 220, 255)
                        };
                        for w in pts.windows(2) {
                            painter.line_segment([w[0], w[1]], egui::Stroke::new(1.5, color));
                        }
                        if pts.len() >= 3 {
                            painter.line_segment(
                                [*pts.last().unwrap(), pts[0]],
                                egui::Stroke::new(1.5, color),
                            );
                        }
                    }
                }
                // Triangle wireframe.
                for tri in mesh.indices.chunks_exact(3) {
                    let a = mesh.vertices[tri[0] as usize];
                    let b = mesh.vertices[tri[1] as usize];
                    let c = mesh.vertices[tri[2] as usize];
                    let pa = egui::pos2(rect.min.x + a.x * scale, rect.min.y + a.y * scale);
                    let pb = egui::pos2(rect.min.x + b.x * scale, rect.min.y + b.y * scale);
                    let pc = egui::pos2(rect.min.x + c.x * scale, rect.min.y + c.y * scale);
                    painter.line_segment([pa, pb], egui::Stroke::new(0.5, egui::Color32::from_rgb(90, 90, 90)));
                    painter.line_segment([pb, pc], egui::Stroke::new(0.5, egui::Color32::from_rgb(90, 90, 90)));
                    painter.line_segment([pc, pa], egui::Stroke::new(0.5, egui::Color32::from_rgb(90, 90, 90)));
                }
                ui.label(egui::RichText::new("Azul: contorno exterior · Rojo: agujeros · Gris: triángulos").weak());
            }
        }
    }

    fn view_pivots(&mut self, ui: &mut egui::Ui) {
        let Some(out) = &mut self.result else {
            ui.centered_and_justified(|ui| ui.label("Ejecuta un empaquetado primero."));
            return;
        };
        ui.label("Ajusta los pivots (normalizados 0..1). Los cambios se aplican en la siguiente exportación.");
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("pivots")
                .striped(true)
                .min_col_width(90.0)
                .show(ui, |ui| {
                    ui.strong("id");
                    ui.strong("pivot X");
                    ui.strong("pivot Y");
                    ui.end_row();
                    let sprites = &mut out.result.sprites;
                    for s in sprites.iter_mut() {
                        ui.label(&s.id);
                        ui.add(egui::DragValue::new(&mut s.pivot.x).range(0.0..=1.0).speed(0.01));
                        ui.add(egui::DragValue::new(&mut s.pivot.y).range(0.0..=1.0).speed(0.01));
                        ui.end_row();
                    }
                });
        });
    }

    fn view_output(&mut self, ui: &mut egui::Ui) {
        let Some(out) = &self.result else {
            ui.centered_and_justified(|ui| ui.label("Ejecuta un empaquetado primero."));
            return;
        };
        let result = &out.result;
        ui.heading("Archivos generados");
        for f in &result.output_files {
            ui.monospace(f);
        }
        ui.add_space(8.0);
        ui.heading("Tiempos por etapa");
        for (stage, ms) in &result.stage_times_ms {
            ui.horizontal(|ui| {
                ui.monospace(format!("{ms:>6} ms"));
                ui.label(stage);
            });
        }
        ui.add_space(8.0);
        ui.heading("Avisos");
        if result.warnings.is_empty() {
            ui.label("(ninguno)");
        }
        for w in &result.warnings {
            ui.colored_label(egui::Color32::from_rgb(255, 200, 80), w);
        }
        ui.add_space(8.0);
        ui.label(format!(
            "Total: {} sprites · {} aliases · {} páginas",
            result.total_sprites,
            result.alias_count,
            result.pages.len()
        ));
        for p in &result.pages {
            ui.label(format!(
                "Página {}: {}x{} · relleno {:.1}% · {}",
                p.index,
                p.width,
                p.height,
                p.fill_ratio * 100.0,
                p.file_name
            ));
        }
    }

    fn view_log(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            for entry in &self.logs {
                let color = match entry.kind {
                    LogKind::Info => egui::Color32::from_gray(200),
                    LogKind::Warning => egui::Color32::from_rgb(255, 200, 80),
                    LogKind::Error => egui::Color32::from_rgb(255, 100, 100),
                };
                ui.colored_label(color, &entry.text);
            }
        });
    }

    fn save_project(&mut self) {
        let path = self
            .project_path
            .clone()
            .or_else(|| rfd::FileDialog::new().add_filter("Proyecto", &["tpproj"]).save_file());
        let Some(path) = path else { return };
        self.parse_variants();
        match self.config.to_toml() {
            Ok(text) => match std::fs::write(&path, text) {
                Ok(_) => {
                    self.project_path = Some(path.clone());
                    self.log(LogKind::Info, format!("Proyecto guardado en {}", path.display()));
                }
                Err(e) => self.log(LogKind::Error, format!("No se pudo guardar: {e}")),
            },
            Err(e) => self.log(LogKind::Error, format!("Config inválida: {e}")),
        }
    }

    fn load_project(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Proyecto", &["tpproj"]).pick_file() else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match ProjectConfig::from_toml(&text) {
                Ok(cfg) => {
                    self.config = cfg;
                    self.sync_variants();
                    self.project_path = Some(path.clone());
                    self.log(LogKind::Info, format!("Proyecto cargado: {}", path.display()));
                }
                Err(e) => self.log(LogKind::Error, format!("Proyecto inválido: {e}")),
            },
            Err(e) => self.log(LogKind::Error, format!("No se pudo leer: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn enum_combo<T: PartialEq>(
    ui: &mut egui::Ui,
    label: &str,
    selected_text: &str,
    items: impl FnOnce(&mut egui::Ui, &mut T),
    value: &mut T,
) {
    egui::ComboBox::from_label(label)
        .selected_text(selected_text)
        .show_ui(ui, |ui| items(ui, value));
}

fn dither_name(d: DitheringAlgorithm) -> &'static str {
    match d {
        DitheringAlgorithm::None => "Ninguno",
        DitheringAlgorithm::FloydSteinberg => "Floyd–Steinberg",
        DitheringAlgorithm::Atkinson => "Atkinson",
    }
}

fn template_name(t: TemplateFormat) -> &'static str {
    match t {
        TemplateFormat::Json => "JSON",
        TemplateFormat::Xml => "XML (libgdx)",
        TemplateFormat::Plist => "Plist (cocos2d)",
        TemplateFormat::CppHeader => "Cabecera C++",
        TemplateFormat::Tsv => "TSV",
        TemplateFormat::PlainText => "Texto plano",
    }
}
