//! Right settings panel: basic options always visible, advanced behind a toggle.

use super::App;
use eframe::egui;
use tp_core::config::{
    AlphaHandling, BasicSortBy, ColorDepth, DitheringAlgorithm, GpuFormat, PackMode,
    PackingAlgorithm, PackingStrategy, PixelFormat, PngDither, ScaleMode, SizeConstraint,
    SortOrder, TemplateFormat, TrimMode,
};

pub(super) fn settings_ui(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.heading("Ajustes");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut app.advanced_settings, "Avanzados")
                .on_hover_text("Mostrar todas las opciones");
        });
    });
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("settings_scroll")
        .show(ui, |ui| {
            data_section(app, ui);
            layout_section(app, ui);
            processing_section(app, ui);
            warnings_section(app, ui);

            ui.add_space(8.0);
            if ui
                .add_enabled(app.running.is_none(), egui::Button::new("Publicar ahora"))
                .clicked()
            {
                app.start_pack();
            }
        });
}

fn data_section(app: &mut App, ui: &mut egui::Ui) {
    let advanced = app.advanced_settings;
    egui::CollapsingHeader::new("Datos")
        .default_open(true)
        .show(ui, |ui| {
            ui.label("Directorio de entrada");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut app.input_dir_text).desired_width(190.0));
                if ui.button("…").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        app.input_dir_text = dir.display().to_string();
                        app.config.input_directory = dir;
                        app.on_paths_edited();
                    }
                }
            });
            ui.label("Directorio de salida");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut app.output_dir_text).desired_width(190.0));
                if ui.button("…").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        app.output_dir_text = dir.display().to_string();
                        app.config.output_directory = dir;
                    }
                }
            });
            ui.label("Nombre base de los archivos");
            ui.add(egui::TextEdit::singleline(&mut app.config.base_file_name).desired_width(190.0));
            ui.label(
                egui::RichText::new("Placeholders: {n} {n1} {v}  (p. ej. hoja{n1}{v})").weak(),
            );
            enum_combo(
                ui,
                "Formato de metadatos",
                template_name(app.config.template_format),
                |ui, v| {
                    ui.selectable_value(v, TemplateFormat::Json, "JSON");
                    ui.selectable_value(v, TemplateFormat::Xml, "XML (libgdx)");
                    ui.selectable_value(v, TemplateFormat::Plist, "Plist (cocos2d)");
                    ui.selectable_value(v, TemplateFormat::CppHeader, "Cabecera C++");
                    ui.selectable_value(v, TemplateFormat::Tsv, "TSV");
                    ui.selectable_value(v, TemplateFormat::PlainText, "Texto plano");
                },
                &mut app.config.template_format,
            );

            if !advanced {
                return;
            }
            ui.checkbox(&mut app.config.recursive, "Buscar en subdirectorios");
            ui.checkbox(
                &mut app.config.trim_sprite_names,
                "Quitar la extensión de los nombres",
            )
            .on_hover_text("hero/idle_00.png pasa a llamarse hero/idle_00");
            ui.checkbox(
                &mut app.config.prepend_folder_name,
                "Anteponer el nombre de la carpeta inteligente",
            )
            .on_hover_text("Solo aplica a carpetas añadidas fuera del directorio de entrada");
            ui.checkbox(
                &mut app.config.enable_auto_detect_animations,
                "Auto-detectar animaciones",
            )
            .on_hover_text(
                "Agrupa sprites como walk_001..walk_003 en una animación walk \
                 y la expone en los metadatos (auto-detectar animaciones)",
            );
            ui.label("Ruta de la textura en los metadatos (p. ej. /assets)");
            let mut texture_path = app.config.texture_path.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut texture_path)
                        .desired_width(190.0)
                        .hint_text("vacío = sin prefijo"),
                )
                .changed()
            {
                app.config.texture_path = if texture_path.trim().is_empty() {
                    None
                } else {
                    Some(texture_path)
                };
            }
            ui.label("Scaling variants (p. ej. 2, 0.5 → @2x, -hd)");
            ui.add(egui::TextEdit::singleline(&mut app.variants_text).desired_width(190.0));
            ui.label("Plantilla Mustache personalizada (opcional)");
            ui.horizontal(|ui| {
                let mut path = app
                    .config
                    .export_template
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                ui.add(egui::TextEdit::singleline(&mut path).desired_width(160.0));
                if ui.button("…").clicked() {
                    if let Some(f) = rfd::FileDialog::new().pick_file() {
                        app.config.export_template = Some(f);
                    }
                } else {
                    app.config.export_template = if path.is_empty() {
                        None
                    } else {
                        Some(std::path::PathBuf::from(path))
                    };
                }
            });
            ui.label("Clave de cifrado AES-256-GCM (opcional)");
            let mut key = app.config.encryption_key.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut key)
                        .desired_width(190.0)
                        .password(true),
                )
                .changed()
            {
                app.config.encryption_key = if key.is_empty() { None } else { Some(key) };
            }
        });
}

fn layout_section(app: &mut App, ui: &mut egui::Ui) {
    let advanced = app.advanced_settings;
    egui::CollapsingHeader::new("Composición")
        .default_open(true)
        .show(ui, |ui| {
            let sizes = [256i32, 512, 1024, 2048, 4096, 8192, 16384];
            egui::ComboBox::from_label("Tamaño máximo")
                .selected_text(app.config.max_texture_size.to_string())
                .show_ui(ui, |ui| {
                    for s in sizes {
                        ui.selectable_value(&mut app.config.max_texture_size, s, s.to_string());
                    }
                });
            ui.checkbox(&mut app.config.multipack, "Multipack (varias hojas)")
                .on_hover_text(
                    "Si los sprites no caben en una hoja se generan varias; con la opción \
                     desactivada el empaquetado falla",
                );
            ui.add(egui::Slider::new(&mut app.config.padding, 0..=16).text("Shape padding (px)"));
            ui.add(egui::Slider::new(&mut app.config.extrude, 0..=16).text("Extrude (px)"));
            ui.checkbox(&mut app.config.allow_rotation, "Permitir rotación 90°");
            ui.checkbox(
                &mut app.config.flip_vertical,
                "Voltear verticalmente (flip Y)",
            )
            .on_hover_text(
                "Solo formatos de hardware (ASTC/ETC2/PVRTC); las coordenadas \
                     de los frames no cambian",
            );
            // Trim mode Polygon cambia el algoritmo a Polygon
            // automáticamente; se muestra y no se puede elegir a mano.
            let polygon_auto =
                app.config.effective_trim_mode() == TrimMode::Polygon || app.config.enable_polygon;
            let algorithm_label = if polygon_auto {
                "Polygon (auto: trim mode Polygon)".to_string()
            } else {
                app.config.algorithm.as_str().to_string()
            };
            ui.add_enabled_ui(!polygon_auto, |ui| {
                enum_combo(
                    ui,
                    "Algoritmo",
                    &algorithm_label,
                    |ui, v| {
                        ui.selectable_value(v, PackingAlgorithm::MaxRects, "MaxRects");
                        ui.selectable_value(v, PackingAlgorithm::Guillotine, "Guillotine");
                        ui.selectable_value(v, PackingAlgorithm::Grid, "Rejilla (Grid)");
                        ui.selectable_value(v, PackingAlgorithm::Basic, "Básico (Basic)");
                        ui.selectable_value(
                            v,
                            PackingAlgorithm::Manual,
                            "Manual (arrastrar en la vista)",
                        )
                        .on_hover_text(
                            "Arrastra los sprites en la vista previa para fijar su posición",
                        );
                    },
                    &mut app.config.algorithm,
                );
            });
            if matches!(
                app.config.algorithm,
                PackingAlgorithm::MaxRects | PackingAlgorithm::Guillotine
            ) {
                enum_combo(
                    ui,
                    "Heurística",
                    strategy_display(app.config.packing_strategy),
                    |ui, v| {
                        ui.selectable_value(v, PackingStrategy::Bssf, "MaxRects BSSF");
                        ui.selectable_value(v, PackingStrategy::Baf, "MaxRects BAF");
                        ui.selectable_value(v, PackingStrategy::Blsf, "MaxRects BLSF");
                        ui.selectable_value(v, PackingStrategy::Best, "Best (probar todas)");
                        ui.selectable_value(v, PackingStrategy::BottomLeft, "BottomLeft");
                        ui.selectable_value(v, PackingStrategy::ContactPoint, "ContactPoint");
                        ui.selectable_value(v, PackingStrategy::Guillotine, "Guillotine (legacy)");
                    },
                    &mut app.config.packing_strategy,
                );
            }
            enum_combo(
                ui,
                "Modo de empaquetado",
                app.config.pack_mode.as_str(),
                |ui, v| {
                    ui.selectable_value(v, PackMode::Fast, "Fast (recorte simple)");
                    ui.selectable_value(v, PackMode::Good, "Good (búsqueda rápida)");
                    ui.selectable_value(v, PackMode::Best, "Best (búsqueda intensiva)");
                },
                &mut app.config.pack_mode,
            );
            ui.checkbox(&mut app.config.enable_trim, "Trim (recortar transparencia)");

            if !advanced {
                return;
            }
            ui.add(
                egui::Slider::new(&mut app.config.border_padding, 0..=64)
                    .text("Border padding (px)"),
            )
            .on_hover_text("Margen transparente entre los sprites y el borde del atlas");
            enum_combo(
                ui,
                "Restricción de tamaño",
                app.config.size_constraints.as_str(),
                |ui, v| {
                    ui.selectable_value(v, SizeConstraint::AnySize, "Cualquiera");
                    ui.selectable_value(v, SizeConstraint::Pot, "POT (potencia de 2)");
                    ui.selectable_value(v, SizeConstraint::MultipleOf4, "Múltiplo de 4");
                    ui.selectable_value(v, SizeConstraint::WordAligned, "Alineado a palabra");
                },
                &mut app.config.size_constraints,
            );
            ui.checkbox(
                &mut app.config.force_squared,
                "Atlas cuadrado (force squared)",
            );
            ui.horizontal(|ui| {
                ui.label("Tamaño fijo (0 = automático)");
                ui.add(
                    egui::DragValue::new(&mut app.config.fixed_width)
                        .range(0..=8192)
                        .speed(1),
                );
                ui.label(egui::RichText::new("x").weak());
                ui.add(
                    egui::DragValue::new(&mut app.config.fixed_height)
                        .range(0..=8192)
                        .speed(1),
                );
            })
            .response
            .on_hover_text("Fija las dimensiones del atlas (tamaño fijo)");
            if app.config.algorithm == PackingAlgorithm::Basic {
                enum_combo(
                    ui,
                    "Ordenar por (Basic)",
                    app.config.basic_sort_by.as_str(),
                    |ui, v| {
                        ui.selectable_value(v, BasicSortBy::Best, "Best (probar todas)");
                        ui.selectable_value(v, BasicSortBy::Name, "Nombre");
                        ui.selectable_value(v, BasicSortBy::Width, "Ancho");
                        ui.selectable_value(v, BasicSortBy::Height, "Alto");
                        ui.selectable_value(v, BasicSortBy::Area, "Área");
                        ui.selectable_value(
                            v,
                            BasicSortBy::Circumference,
                            "Perímetro (circumference)",
                        );
                    },
                    &mut app.config.basic_sort_by,
                );
                enum_combo(
                    ui,
                    "Orden (Basic)",
                    app.config.basic_order.as_str(),
                    |ui, v| {
                        ui.selectable_value(v, SortOrder::Ascending, "Ascendente");
                        ui.selectable_value(v, SortOrder::Descending, "Descendente");
                    },
                    &mut app.config.basic_order,
                );
            }
            ui.horizontal(|ui| {
                ui.label("Divisor común");
                ui.label(egui::RichText::new("x").weak());
                ui.add(
                    egui::DragValue::new(&mut app.config.common_divisor_x)
                        .range(1..=2048)
                        .speed(1),
                );
                ui.label(egui::RichText::new("y").weak());
                ui.add(
                    egui::DragValue::new(&mut app.config.common_divisor_y)
                        .range(1..=2048)
                        .speed(1),
                );
            })
            .response
            .on_hover_text("Estira los sprites con transparencia hasta ser divisibles");
            ui.add(
                egui::Slider::new(&mut app.config.align_to_grid, 0..=64)
                    .text("Alinear a rejilla (0 = off)"),
            )
            .on_hover_text("Coloca las esquinas de los sprites en coordenadas múltiplos");
            ui.add_enabled(
                app.config.enable_trim,
                egui::Slider::new(&mut app.config.trim_threshold, 1..=255)
                    .text("Trim threshold (1-255)"),
            );
            ui.add_enabled_ui(app.config.enable_trim, |ui| {
                egui::ComboBox::from_label("Modo de recorte")
                    .selected_text(trim_mode_name(app.config.trim_mode))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.config.trim_mode,
                            TrimMode::None,
                            "None (sin recorte)",
                        );
                        ui.selectable_value(&mut app.config.trim_mode, TrimMode::Trim, "Trim");
                        ui.selectable_value(
                            &mut app.config.trim_mode,
                            TrimMode::CropKeepPos,
                            "Crop, conservar posición",
                        );
                        ui.selectable_value(
                            &mut app.config.trim_mode,
                            TrimMode::Crop,
                            "Crop, fijar en 0/0",
                        );
                        ui.selectable_value(
                            &mut app.config.trim_mode,
                            TrimMode::Polygon,
                            "Polygon (mallas)",
                        );
                    });
            });
            if app.config.enable_trim && app.config.trim_mode.trims() {
                ui.add(
                    egui::Slider::new(&mut app.config.trim_margin, 0..=16)
                        .text("Margen de recorte (px)"),
                );
            }
            if app.config.trim_mode == TrimMode::Polygon {
                ui.label(
                    egui::RichText::new(
                        "Polygon activa el empaquetado por contorno y exporta la malla.",
                    )
                    .weak(),
                );
            }
            ui.checkbox(
                &mut app.config.enable_aliasing,
                "Detección de duplicados (alias)",
            );
            ui.checkbox(
                &mut app.config.enable_normal_maps,
                "Empaquetar mapas de normales",
            );
            ui.add_enabled_ui(!polygon_auto, |ui| {
                ui.checkbox(&mut app.config.enable_polygon, "Modo polígono (mallas)");
            });
            ui.add_enabled(
                polygon_auto,
                egui::Slider::new(&mut app.config.polygon_tolerance, 0.0..=10.0)
                    .text("Tolerancia (RDP)"),
            );
            if polygon_auto {
                ui.label(
                    egui::RichText::new(
                        "Empaqueta sprites por su contorno (Marching Squares → RDP → Earcut).                          Activo por el Trim mode Polygon.",
                    )
                    .weak(),
                );
            }
            ui.label("Pivot por defecto (normalizado 0..1)");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut app.config.default_pivot_x)
                        .range(0.0..=1.0)
                        .speed(0.01),
                );
                ui.add(
                    egui::DragValue::new(&mut app.config.default_pivot_y)
                        .range(0.0..=1.0)
                        .speed(0.01),
                );
            });
        });
}

fn processing_section(app: &mut App, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("Procesamiento")
        .default_open(true)
        .show(ui, |ui| {
            enum_combo(
                ui,
                "Profundidad de color",
                app.config.color_depth.as_str(),
                |ui, v| {
                    ui.selectable_value(v, ColorDepth::Rgba8888, "RGBA8888");
                    ui.selectable_value(v, ColorDepth::Rgba4444, "RGBA4444");
                    ui.selectable_value(v, ColorDepth::Rgb565, "RGB565");
                },
                &mut app.config.color_depth,
            );
            enum_combo(
                ui,
                "Dithering",
                dither_name(app.config.dithering_algorithm),
                |ui, v| {
                    ui.selectable_value(v, DitheringAlgorithm::None, "Ninguno");
                    ui.selectable_value(v, DitheringAlgorithm::FloydSteinberg, "Floyd–Steinberg");
                    ui.selectable_value(
                        v,
                        DitheringAlgorithm::FloydSteinbergAlpha,
                        "Floyd–Steinberg + alpha",
                    );
                    ui.selectable_value(v, DitheringAlgorithm::Atkinson, "Atkinson");
                    ui.selectable_value(v, DitheringAlgorithm::AtkinsonAlpha, "Atkinson + alpha");
                },
                &mut app.config.dithering_algorithm,
            );
            enum_combo(
                ui,
                "Transparencia",
                alpha_handling_name(app.config.alpha_handling),
                |ui, v| {
                    ui.selectable_value(
                        v,
                        AlphaHandling::KeepTransparentPixels,
                        "Conservar píxeles",
                    );
                    ui.selectable_value(
                        v,
                        AlphaHandling::ClearTransparentPixels,
                        "Limpiar transparentes",
                    );
                    ui.selectable_value(
                        v,
                        AlphaHandling::ReduceBorderArtifacts,
                        "Reducir bordes (bleeding)",
                    );
                    ui.selectable_value(v, AlphaHandling::PremultiplyAlpha, "Premultiplicar alpha");
                },
                &mut app.config.alpha_handling,
            );
            enum_combo(
                ui,
                "Escalado de variantes",
                app.config.scale_mode.as_str(),
                |ui, v| {
                    ui.selectable_value(v, ScaleMode::Smooth, "Suave (bilineal)");
                    ui.selectable_value(v, ScaleMode::Fast, "Rápido (vecino más cercano)");
                },
                &mut app.config.scale_mode,
            );
            enum_combo(
                ui,
                "Formato de publicación",
                app.config.gpu_format.as_str(),
                |ui, v| {
                    ui.selectable_value(v, GpuFormat::Png, "PNG");
                    ui.selectable_value(v, GpuFormat::Png8, "PNG-8 (indexado)");
                    ui.selectable_value(v, GpuFormat::Jpg, "JPG");
                    ui.selectable_value(v, GpuFormat::WebP, "WebP");
                    ui.selectable_value(v, GpuFormat::Astc4x4, "ASTC 4x4");
                    ui.selectable_value(v, GpuFormat::Etc2Rgba, "ETC2 RGBA");
                    ui.selectable_value(v, GpuFormat::Pvrtc4Bpp, "PVRTC 4BPP");
                },
                &mut app.config.gpu_format,
            );
            enum_combo(
                ui,
                "Formato de píxel",
                app.config.pixel_format.as_str(),
                |ui, v| {
                    ui.selectable_value(v, PixelFormat::Rgba8888, "RGBA8888");
                    ui.selectable_value(v, PixelFormat::Rgb888, "RGB888 (sobre negro)");
                    ui.selectable_value(v, PixelFormat::Alpha8, "ALPHA8");
                    ui.selectable_value(v, PixelFormat::Intensity8, "INTENSITY8");
                    ui.selectable_value(v, PixelFormat::AlphaIntensity8, "Alpha+Intensity");
                    ui.selectable_value(v, PixelFormat::Rgba5551, "RGBA5551 (16 bits)");
                    ui.selectable_value(v, PixelFormat::Rgba5555, "RGBA5555 (20 bits)");
                    ui.selectable_value(v, PixelFormat::Bgra8888, "BGRA8888");
                },
                &mut app.config.pixel_format,
            );
            match app.config.gpu_format {
                GpuFormat::Png | GpuFormat::Png8 => {
                    ui.horizontal(|ui| {
                        ui.label("Optimización PNG (0-7)");
                        ui.add(egui::DragValue::new(&mut app.config.png_opt_level).range(0..=7));
                    });
                }
                GpuFormat::Jpg => {
                    ui.horizontal(|ui| {
                        ui.label("Calidad JPG (0-100)");
                        ui.add(egui::DragValue::new(&mut app.config.jpg_quality).range(0..=100));
                    });
                }
                GpuFormat::WebP => {
                    let mut lossless = app.config.webp_quality > 100;
                    if ui.checkbox(&mut lossless, "WebP sin pérdidas").changed() {
                        app.config.webp_quality = if lossless { 101 } else { 100 };
                    }
                    if app.config.webp_quality <= 100 {
                        ui.horizontal(|ui| {
                            ui.label("Calidad WebP (0-100)");
                            ui.add(
                                egui::DragValue::new(&mut app.config.webp_quality).range(0..=100),
                            );
                        });
                    }
                }
                _ => {}
            }
            if app.config.gpu_format == GpuFormat::Png8 {
                enum_combo(
                    ui,
                    "Dithering PNG-8",
                    app.config.png8_dither.as_str(),
                    |ui, v| {
                        ui.selectable_value(v, PngDither::Low, "PngQuant Low");
                        ui.selectable_value(v, PngDither::Medium, "PngQuant Medium");
                        ui.selectable_value(v, PngDither::High, "PngQuant High");
                    },
                    &mut app.config.png8_dither,
                );
            }
        });
}

/// Avisos de configuración que solo se ven en la GUI (antes de publicar).
fn warnings_section(app: &App, ui: &mut egui::Ui) {
    let mut warnings: Vec<String> = Vec::new();
    if app.config.dithering_algorithm != DitheringAlgorithm::None
        && app.config.color_depth == ColorDepth::Rgba8888
    {
        warnings.push("El dithering no se aplica con RGBA8888; usa RGBA4444 o RGB565.".into());
    }
    if app.config.extrude > app.config.padding {
        warnings.push(format!(
            "El extrude ({}) se limita internamente al padding ({}).",
            app.config.extrude, app.config.padding
        ));
    }
    if app.config.export_template.is_some() && app.config.template_format == TemplateFormat::Json {
        warnings.push(
            "La plantilla Mustache se ignora con el formato JSON (usa XML/Plist/TSV/...).".into(),
        );
    }
    if !app.config.gpu_format.is_supported() {
        warnings.push(
            "ASTC 4x4 requiere compilar con --features gpu-formats; el publicado fallará.".into(),
        );
    }
    if app.config.flip_vertical && !app.config.gpu_format.is_hardware() {
        warnings.push(
            "Voltear verticalmente (flip Y) solo aplica a formatos de hardware \
             (ASTC/ETC2/PVRTC); se ignorará."
                .into(),
        );
    }
    if app.config.pixel_format != PixelFormat::Rgba8888 && app.config.gpu_format.is_hardware() {
        warnings.push(
            "El formato de píxel solo aplica a PNG/PNG8/JPG/WebP; los formatos de \
             hardware usan RGBA."
                .into(),
        );
    }
    if app.config.scale_variants.iter().any(|s| s.fract() != 0.0) {
        warnings.push(
            "Hay variantes de escala fraccionarias; las coordenadas se redondearán a píxeles."
                .into(),
        );
    }
    let align = app.config.align_to_grid;
    if align > 0 && (app.config.padding % align != 0 || app.config.border_padding % align != 0) {
        warnings.push(format!(
            "Para respetar la rejilla de {align} px, el padding se redondeará al múltiplo de {align} al publicar."
        ));
    }
    if app.config.border_padding * 2 >= app.config.max_texture_size {
        warnings.push("El padding de borde deja el atlas interior vacío; publicar fallará.".into());
    }
    if app.config.gpu_format == GpuFormat::Pvrtc4Bpp
        && app.config.size_constraints != SizeConstraint::Pot
    {
        warnings.push(
            "PVRTC exige dimensiones potencia de dos; activa la restricción «POT» o fija el tamaño."
                .into(),
        );
    }
    let fixed_min = [app.config.fixed_width, app.config.fixed_height]
        .iter()
        .filter(|v| **v > 0)
        .copied()
        .min();
    if let Some(m) = fixed_min {
        if m <= app.config.border_padding * 2 {
            warnings.push(
                "El tamaño fijo no deja área interior con el padding de borde actual; publicar fallará."
                    .into(),
            );
        }
    }
    let (dx, dy) = app.config.effective_divisors();
    if dx > 1 || dy > 1 {
        warnings.push(format!(
            "Los sprites se estirarán con transparencia hasta ser divisibles entre {dx}x{dy}."
        ));
    }
    if warnings.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Avisos").strong());
    for w in warnings {
        ui.label(
            egui::RichText::new(format!("- {w}")).color(egui::Color32::from_rgb(255, 200, 80)),
        );
    }
}

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

fn strategy_display(s: PackingStrategy) -> &'static str {
    match s {
        PackingStrategy::Bssf => "MaxRects BSSF",
        PackingStrategy::Baf => "MaxRects BAF",
        PackingStrategy::Blsf => "MaxRects BLSF",
        PackingStrategy::Guillotine => "Guillotine (legacy)",
        PackingStrategy::Best => "Best (probar todas)",
        PackingStrategy::BottomLeft => "BottomLeft",
        PackingStrategy::ContactPoint => "ContactPoint",
    }
}

fn trim_mode_name(t: TrimMode) -> &'static str {
    match t {
        TrimMode::None => "None",
        TrimMode::Trim => "Trim",
        TrimMode::CropKeepPos => "Crop, conservar posición",
        TrimMode::Crop => "Crop, fijar en 0/0",
        TrimMode::Polygon => "Polygon",
    }
}

fn dither_name(d: DitheringAlgorithm) -> &'static str {
    match d {
        DitheringAlgorithm::None => "Ninguno",
        DitheringAlgorithm::FloydSteinberg => "Floyd–Steinberg",
        DitheringAlgorithm::Atkinson => "Atkinson",
        DitheringAlgorithm::FloydSteinbergAlpha => "Floyd–Steinberg + alpha",
        DitheringAlgorithm::AtkinsonAlpha => "Atkinson + alpha",
    }
}

fn alpha_handling_name(a: AlphaHandling) -> &'static str {
    match a {
        AlphaHandling::KeepTransparentPixels => "Conservar píxeles",
        AlphaHandling::ClearTransparentPixels => "Limpiar transparentes",
        AlphaHandling::ReduceBorderArtifacts => "Reducir bordes (bleeding)",
        AlphaHandling::PremultiplyAlpha => "Premultiplicar alpha",
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
