//! Right settings panel: basic options always visible, advanced behind a toggle.

use super::App;
use eframe::egui;
use std::path::PathBuf;
use tp_core::config::{
    AlphaHandling, BasicSortBy, ColorDepth, DitheringAlgorithm, DxtMode, GdxFilter, GpuFormat,
    PackMode, PackingAlgorithm, PackingStrategy, PixelFormat, PngDither, ProjectConfig, ScaleMode,
    SizeConstraint, SortOrder, TemplateFormat, TrimMode, VariantOptions,
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
    // Bandera por-frame: cualquier combo cambia la config y dispara el
    // repack (los controles directos lo hacen inline con .changed()).
    ui.ctx().data_mut(|d| {
        d.insert_temp(egui::Id::new(SETTINGS_CHANGED_FLAG), false);
    });
    egui::ScrollArea::vertical()
        .id_salt("settings_scroll")
        .show(ui, |ui| {
            interface_section(app, ui);
            data_section(app, ui);
            layout_section(app, ui);
            processing_section(app, ui);
            warnings_section(app, ui);
        });
    if ui.ctx().data(|d| {
        d.get_temp::<bool>(egui::Id::new(SETTINGS_CHANGED_FLAG))
            .unwrap_or(false)
    }) {
        app.on_config_changed();
    }
}

/// Preferencias del usuario: idioma y tema de la ventana. Se guardan en
/// `ui.toml` (al lado de `keys.toml`), no en el proyecto, porque acompañan a
/// la app en cualquier `.tpproj`.
fn interface_section(app: &mut App, ui: &mut egui::Ui) {
    use crate::i18n::{t, LangChoice};
    use crate::ui_prefs::Theme;

    egui::CollapsingHeader::new(t!("Interfaz"))
        .default_open(true)
        .show(ui, |ui| {
            ui.label(t!("Idioma"));
            let mut lang = app.prefs().lang_choice();
            egui::ComboBox::from_id_salt("ui_lang")
                .selected_text(lang_label(lang))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut lang,
                        LangChoice::System,
                        t!("Sistema (idioma del equipo)"),
                    );
                    ui.selectable_value(&mut lang, LangChoice::Es, "Español");
                    ui.selectable_value(&mut lang, LangChoice::En, "English");
                });
            if lang != app.prefs().lang_choice() {
                app.set_lang_choice(lang);
            }

            ui.label(t!("Tema"));
            let mut theme = app.prefs().theme();
            egui::ComboBox::from_id_salt("ui_theme")
                .selected_text(theme_label(theme))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut theme, Theme::System, t!("Sistema"));
                    ui.selectable_value(&mut theme, Theme::Light, t!("Claro"));
                    ui.selectable_value(&mut theme, Theme::Dark, t!("Oscuro"));
                });
            if theme != app.prefs().theme() {
                app.set_theme(theme);
            }

            ui.label(egui::RichText::new(t!("Se guarda en tu equipo, no en el proyecto.")).weak());
        });
}

/// Nombre del idioma en el combo: el nombre propio nunca se traduce.
fn lang_label(choice: crate::i18n::LangChoice) -> &'static str {
    match choice {
        crate::i18n::LangChoice::System => crate::i18n::t!("Sistema (idioma del equipo)"),
        crate::i18n::LangChoice::Es => "Español",
        crate::i18n::LangChoice::En => "English",
    }
}

/// Nombre del tema en el combo (sí se traduce: es etiqueta, no nombre propio).
fn theme_label(theme: crate::ui_prefs::Theme) -> &'static str {
    match theme {
        crate::ui_prefs::Theme::System => crate::i18n::t!("Sistema"),
        crate::ui_prefs::Theme::Light => crate::i18n::t!("Claro"),
        crate::ui_prefs::Theme::Dark => crate::i18n::t!("Oscuro"),
    }
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
                let out = ui
                    .add(egui::TextEdit::singleline(&mut app.output_dir_text).desired_width(190.0));
                if out.changed() {
                    app.on_config_changed();
                }
                if ui.button("…").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        app.output_dir_text = dir.display().to_string();
                        app.config.output_directory = dir;
                        app.on_config_changed();
                    }
                }
            });
            ui.label("Nombre base de los archivos");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut app.config.base_file_name).desired_width(190.0),
                )
                .changed()
            {
                app.on_config_changed();
            }
            ui.label(
                egui::RichText::new("Placeholders: {n} {n1} {v}  (p. ej. hoja{n1}{v})").weak(),
            );
            data_format_combo(app, ui);

            if !advanced {
                return;
            }
            // Ficheros de datos extra, como los --class-file/--header-file/…
            // del original (se escriben junto a los metadatos).
            let mut extras_changed = false;
            let extra_fields: [(&str, &mut String); 4] = [
                ("Class file (Swift)", &mut app.config.class_file),
                ("Header file (C++/ObjC)", &mut app.config.header_file),
                ("Source file (C++)", &mut app.config.source_file),
                ("Sprite ids file", &mut app.config.spriteids_file),
            ];
            egui::CollapsingHeader::new("Ficheros extra por framework")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Vacío = no escribir. Alias CLI: --class-file, --header-file, \
                             --source-file y --spriteids-file.",
                        )
                        .weak(),
                    );
                    for (label, value) in extra_fields {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            if ui
                                .add(egui::TextEdit::singleline(value).desired_width(200.0))
                                .changed()
                            {
                                extras_changed = true;
                            }
                        });
                    }
                });
            if extras_changed {
                app.on_config_changed();
            }
            // Extras de data format: cache busting (Pixi/Phaser), filtro
            // (LibGDX) y shape debug (contorno dibujado en la hoja).
            let mut data_extras_changed = false;
            egui::CollapsingHeader::new("Extras del data format")
                .default_open(false)
                .show(ui, |ui| {
                    data_extras_changed |= ui
                        .checkbox(
                            &mut app.config.cache_busting,
                            "Cache busting (?v= en la textura citada)",
                        )
                        .on_hover_text(
                            "Añade ?v=<hash del fichero> a la imagen que los metadatos \
                             referencian, como los data formats de Pixi/Phaser.",
                        )
                        .changed();
                    enum_combo(
                        ui,
                        "Filtro (LibGDX)",
                        app.config.gdx_filter.as_str(),
                        |ui, v| {
                            ui.selectable_value(v, GdxFilter::Linear, "Linear");
                            ui.selectable_value(v, GdxFilter::Nearest, "Nearest");
                        },
                        &mut app.config.gdx_filter,
                    );
                    data_extras_changed |= ui
                        .checkbox(
                            &mut app.config.shape_debug,
                            "Shape debug (contornos en la hoja)",
                        )
                        .on_hover_text(
                            "Dibuja el rectángulo visible y los polígonos de cada sprite \
                             sobre la hoja, en magenta.",
                        )
                        .changed();
                });
            if data_extras_changed {
                app.on_config_changed();
            }
            if ui
                .checkbox(&mut app.config.recursive, "Buscar en subdirectorios")
                .changed()
            {
                app.on_config_changed();
            }
            if ui
                .checkbox(
                    &mut app.config.trim_sprite_names,
                    "Quitar la extensión de los nombres",
                )
                .on_hover_text("hero/idle_00.png pasa a llamarse hero/idle_00")
                .changed()
            {
                app.on_config_changed();
            }
            if ui
                .checkbox(
                    &mut app.config.prepend_folder_name,
                    "Anteponer el nombre de la carpeta inteligente",
                )
                .on_hover_text("Solo aplica a carpetas añadidas fuera del directorio de entrada")
                .changed()
            {
                app.on_config_changed();
            }
            if ui
                .checkbox(
                    &mut app.config.enable_auto_detect_animations,
                    "Auto-detectar animaciones",
                )
                .on_hover_text(
                    "Agrupa sprites como walk_001..walk_003 en una animación walk \
                     y la expone en los metadatos (auto-detectar animaciones)",
                )
                .changed()
            {
                app.on_config_changed();
            }
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
                app.on_config_changed();
            }
            ui.label("Escalado de variantes (p. ej. 2, 0.5 ➡ @2x, -hd)");
            if ui
                .add(egui::TextEdit::singleline(&mut app.variants_text).desired_width(190.0))
                .changed()
            {
                app.on_config_changed();
            }
            variant_presets_ui(app, ui);
            variant_options_ui(app, ui);
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
            custom_exporters_ui(app, ui);
            template_properties_ui(app, ui);
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
            // Clave global: se guarda una sola vez y se reutiliza en
            // cualquier proyecto, como en el original.
            ui.label("Clave global (guardada una vez y reutilizable)");
            let names = tp_core::keys::list();
            let mut name = app.config.encryption_key_name.clone().unwrap_or_default();
            ui.horizontal(|ui| {
                let selected = if name.is_empty() {
                    "— ninguna —".to_string()
                } else {
                    name.clone()
                };
                egui::ComboBox::from_id_salt("global_key_name")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(&mut name, String::new(), "— ninguna —")
                            .clicked()
                        {
                            app.config.encryption_key_name = None;
                            app.on_config_changed();
                        }
                        for n in &names {
                            if ui
                                .selectable_value(&mut name, n.clone(), n)
                                .on_hover_text("Usa esta clave en el proyecto")
                                .clicked()
                            {
                                app.config.encryption_key_name = Some(n.clone());
                                // La clave escrita a mano tiene prioridad:
                                // se limpia para que la global surta efecto.
                                app.config.encryption_key = None;
                                app.on_config_changed();
                            }
                        }
                    });
                if ui
                    .button("Guardar")
                    .on_hover_text("Guarda la clave escrita arriba con este nombre")
                    .clicked()
                {
                    let key = app.config.encryption_key.clone().unwrap_or_default();
                    match tp_core::keys::put(&name, &key) {
                        Ok(()) => {
                            app.config.encryption_key_name = Some(name.clone());
                            app.log(
                                super::LogKind::Info,
                                format!("Clave global «{name}» guardada."),
                            );
                        }
                        Err(e) => app.log(super::LogKind::Warning, e.to_string()),
                    }
                }
                if ui
                    .button("Borrar")
                    .on_hover_text("Borra la clave global seleccionada")
                    .clicked()
                    && !name.is_empty()
                {
                    match tp_core::keys::remove(&name) {
                        Ok(true) => {
                            app.config.encryption_key_name = None;
                            app.log(
                                super::LogKind::Info,
                                format!("Clave global «{name}» borrada."),
                            );
                        }
                        Ok(false) => {}
                        Err(e) => app.log(super::LogKind::Warning, e.to_string()),
                    }
                }
            });
            if ui
                .add(
                    egui::TextEdit::singleline(&mut name)
                        .desired_width(190.0)
                        .hint_text("nombre de la clave global"),
                )
                .changed()
            {
                app.config.encryption_key_name = if name.trim().is_empty() {
                    None
                } else {
                    Some(name)
                };
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
                        if ui
                            .selectable_value(&mut app.config.max_texture_size, s, s.to_string())
                            .clicked()
                        {
                            app.on_config_changed();
                        }
                    }
                });
            if ui
                .checkbox(&mut app.config.multipack, "Multipack (varias hojas)")
                .on_hover_text(
                    "Si los sprites no caben en una hoja se generan varias; con la opción \
                     desactivada el empaquetado falla",
                )
                .changed()
            {
                app.on_config_changed();
            }
            if ui
                .add(
                    egui::Slider::new(&mut app.config.padding, 0..=16)
                        .text("Shape padding (px)"),
                )
                .changed()
            {
                app.on_config_changed();
            }
            if ui
                .add(egui::Slider::new(&mut app.config.extrude, 0..=16).text("Extrude (px)"))
                .changed()
            {
                app.on_config_changed();
            }
            if ui
                .checkbox(&mut app.config.allow_rotation, "Permitir rotación 90°")
                .changed()
            {
                app.on_config_changed();
            }
            if ui
                .checkbox(&mut app.config.flip_vertical, "Voltear verticalmente (flip Y)")
                .on_hover_text(
                    "Solo formatos de hardware (ASTC/ETC2/ETC1/PVRTC); las coordenadas \
                     de los frames no cambian",
                )
                .changed()
            {
                app.on_config_changed();
            }
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
            // Un cambio de algoritmo reacciona al instante (vía selectable_value
            // dentro del combo, cableado en enum_combo).
            if matches!(
                app.config.algorithm,
                PackingAlgorithm::MaxRects | PackingAlgorithm::Guillotine
            ) {
                // Las heurísticas de colocación son comunes; con Guillotine
                // no van prefijadas por "MaxRects" y desaparece la entrada
                // legacy (que `resolve()` traduciría en BSSF).
                let guillotine = app.config.algorithm == PackingAlgorithm::Guillotine;
                let prefix = if guillotine { "" } else { "MaxRects " };
                let shown = strategy_display(app.config.packing_strategy, guillotine);
                enum_combo(
                    ui,
                    "Heurística",
                    &shown,
                    |ui, v| {
                        ui.selectable_value(v, PackingStrategy::Bssf, format!("{prefix}BSSF"));
                        ui.selectable_value(v, PackingStrategy::Baf, format!("{prefix}BAF"));
                        ui.selectable_value(v, PackingStrategy::Blsf, format!("{prefix}BLSF"));
                        ui.selectable_value(v, PackingStrategy::Best, "Best (probar todas)");
                        ui.selectable_value(v, PackingStrategy::BottomLeft, "BottomLeft");
                        ui.selectable_value(v, PackingStrategy::ContactPoint, "ContactPoint");
                        if !guillotine {
                            ui.selectable_value(
                                v,
                                PackingStrategy::Guillotine,
                                "Guillotine (legacy)",
                            );
                        }
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
            if ui
                .checkbox(&mut app.config.enable_trim, "Trim (recortar transparencia)")
                .changed()
            {
                app.on_config_changed();
            }

            if !advanced {
                return;
            }
            if ui
                .add(
                    egui::Slider::new(&mut app.config.border_padding, 0..=64)
                        .text("Border padding (px)"),
                )
                .on_hover_text("Margen transparente entre los sprites y el borde del atlas")
                .changed()
            {
                app.on_config_changed();
            }
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
            if ui
                .checkbox(&mut app.config.force_squared, "Atlas cuadrado (force squared)")
                .changed()
            {
                app.on_config_changed();
            }
            ui.horizontal(|ui| {
                ui.label("Tamaño fijo (0 = automático)");
                if ui
                    .add(
                        egui::DragValue::new(&mut app.config.fixed_width)
                            .range(0..=8192)
                            .speed(1),
                    )
                    .changed()
                {
                    app.on_config_changed();
                }
                ui.label(egui::RichText::new("x").weak());
                if ui
                    .add(
                        egui::DragValue::new(&mut app.config.fixed_height)
                            .range(0..=8192)
                            .speed(1),
                    )
                    .changed()
                {
                    app.on_config_changed();
                }
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
                if ui
                    .add(
                        egui::DragValue::new(&mut app.config.common_divisor_x)
                            .range(1..=2048)
                            .speed(1),
                    )
                    .changed()
                {
                    app.on_config_changed();
                }
                ui.label(egui::RichText::new("y").weak());
                if ui
                    .add(
                        egui::DragValue::new(&mut app.config.common_divisor_y)
                            .range(1..=2048)
                            .speed(1),
                    )
                    .changed()
                {
                    app.on_config_changed();
                }
            })
            .response
            .on_hover_text("Estira los sprites con transparencia hasta ser divisibles");
            if ui
                .add(
                    egui::Slider::new(&mut app.config.align_to_grid, 0..=64)
                        .text("Alinear a rejilla (0 = off)"),
                )
                .on_hover_text("Coloca las esquinas de los sprites en coordenadas múltiplos")
                .changed()
            {
                app.on_config_changed();
            }
            if ui
                .add_enabled(
                    app.config.enable_trim,
                    egui::Slider::new(&mut app.config.trim_threshold, 1..=255)
                        .text("Trim threshold (1-255)"),
                )
                .changed()
            {
                app.on_config_changed();
            }
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
            if app.config.enable_trim
                && app.config.trim_mode.trims()
                && ui
                    .add(
                        egui::Slider::new(&mut app.config.trim_margin, 0..=16)
                            .text("Margen de recorte (px)"),
                    )
                    .changed()
            {
                app.on_config_changed();
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
            if app.config.enable_normal_maps {
                ui.horizontal(|ui| {
                    ui.label("Sufijo");
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut app.config.normal_map_suffix)
                                .desired_width(110.0),
                        )
                        .changed()
                    {
                        app.on_config_changed();
                    }
                    ui.label("Filtro de ruta");
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut app.config.normal_map_filter)
                                .desired_width(130.0),
                        )
                        .changed()
                    {
                        app.on_config_changed();
                    }
                });
                if ui
                    .checkbox(
                        &mut app.config.normal_map_auto_detect,
                        "Detectar por color (auto-detect)",
                    )
                    .changed()
                {
                    app.on_config_changed();
                }
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Hoja de normales (vacío = <imagen>_normal)")
                            .weak(),
                    );
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut app.config.normal_map_sheet)
                                .desired_width(150.0),
                        )
                        .changed()
                    {
                        app.on_config_changed();
                    }
                });
            }
            ui.add_enabled_ui(!polygon_auto, |ui| {
                ui.checkbox(&mut app.config.enable_polygon, "Modo polígono (mallas)");
            });
            if ui
                .add_enabled(
                    polygon_auto,
                    egui::Slider::new(&mut app.config.polygon_tolerance, 0.0..=10.0)
                        .text("Tolerancia (RDP)"),
                )
                .changed()
            {
                app.on_config_changed();
            }
            if polygon_auto {
                ui.label(
                    egui::RichText::new(
                        "Empaqueta sprites por su contorno (Marching Squares ➡ RDP ➡ Earcut). Activo por el modo de recorte Polígono.",
                    )
                    .weak(),
                );
            }
            ui.label("Pivot por defecto (normalizado 0..1)");
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::DragValue::new(&mut app.config.default_pivot_x)
                            .range(0.0..=1.0)
                            .speed(0.01),
                    )
                    .changed()
                {
                    app.on_config_changed();
                }
                if ui
                    .add(
                        egui::DragValue::new(&mut app.config.default_pivot_y)
                            .range(0.0..=1.0)
                            .speed(0.01),
                    )
                    .changed()
                {
                    app.on_config_changed();
                }
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
                    ui.selectable_value(
                        v,
                        DitheringAlgorithm::NearestNeighbour,
                        "Nearest Neighbour",
                    );
                    ui.selectable_value(v, DitheringAlgorithm::Linear, "Linear");
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
                    ui.selectable_value(v, ScaleMode::Scale2x, "Scale2x (2x)");
                    ui.selectable_value(v, ScaleMode::Scale3x, "Scale3x (3x)");
                    ui.selectable_value(v, ScaleMode::Scale4x, "Scale4x (4x)");
                    ui.selectable_value(v, ScaleMode::Eagle, "Eagle (2x)");
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
                    ui.selectable_value(v, GpuFormat::Bmp, "BMP");
                    ui.selectable_value(v, GpuFormat::Tga, "TGA");
                    ui.selectable_value(v, GpuFormat::Tiff, "TIFF");
                    ui.selectable_value(v, GpuFormat::Dds, "DDS");
                    ui.separator();
                    ui.selectable_value(v, GpuFormat::Astc4x4, "ASTC 4x4");
                    ui.selectable_value(v, GpuFormat::Etc2Rgba, "ETC2 RGBA (ktx)");
                    ui.selectable_value(v, GpuFormat::Etc1, "ETC1 (pkm)");
                    ui.selectable_value(v, GpuFormat::Etc1Ktx, "ETC1 en KTX (ktx)");
                    ui.selectable_value(v, GpuFormat::Pvrtc4Bpp, "PVRTC 4BPP (pvr)");
                    ui.selectable_value(v, GpuFormat::Basis, "Basis (basis)");
                    ui.separator();
                    ui.selectable_value(v, GpuFormat::Zktx, "KTX con zlib (zktx)");
                    ui.selectable_value(v, GpuFormat::Ktx2, "KTX2 sin comprimir (ktx2)");
                    ui.selectable_value(v, GpuFormat::Pvr3Gz, "PVR3 en gzip (pvr.gz)");
                    ui.selectable_value(v, GpuFormat::Pvr3Ccz, "PVR3 en CCZ (pvr.ccz)");
                },
                &mut app.config.gpu_format,
            );
            // El formato de píxel debe encajar con el formato de textura
            // elegido: si el usuario cambia de formato y deja un pixel format
            // insoportable, se vuelve al RGBA8888.
            if !app
                .config
                .pixel_format
                .is_compatible_with(app.config.gpu_format)
            {
                app.config.pixel_format = PixelFormat::Rgba8888;
            }
            let gpu = app.config.gpu_format;
            enum_combo(
                ui,
                "Formato de píxel",
                app.config.pixel_format.as_str(),
                |ui, v| {
                    for &(fmt, label) in SOFT_PIXEL_FORMATS {
                        ui.selectable_value(v, fmt, label);
                    }
                    if GPU_PIXEL_FORMATS
                        .iter()
                        .any(|(f, _)| f.is_compatible_with(gpu))
                    {
                        ui.separator();
                        for &(fmt, label) in GPU_PIXEL_FORMATS {
                            if fmt.is_compatible_with(gpu) {
                                ui.selectable_value(v, fmt, label);
                            }
                        }
                    }
                },
                &mut app.config.pixel_format,
            );
            match app.config.gpu_format {
                GpuFormat::Png | GpuFormat::Png8 => {
                    ui.horizontal(|ui| {
                        ui.label("Optimización PNG (0-7)");
                        if ui
                            .add(egui::DragValue::new(&mut app.config.png_opt_level).range(0..=7))
                            .changed()
                        {
                            // No reempaqueta: solo afecta a la exportación.
                            app.log(
                                super::LogKind::Info,
                                "Se aplicará al Publicar (nivel de optimización PNG).".into(),
                            );
                        }
                    });
                }
                GpuFormat::Jpg => {
                    ui.horizontal(|ui| {
                        ui.label("Calidad JPG (0-100)");
                        if ui
                            .add(egui::DragValue::new(&mut app.config.jpg_quality).range(0..=100))
                            .changed()
                        {
                            app.log(
                                super::LogKind::Info,
                                "Se aplicará al Publicar (calidad JPG).".into(),
                            );
                        }
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
                            if ui
                                .add(
                                    egui::DragValue::new(&mut app.config.webp_quality)
                                        .range(0..=100),
                                )
                                .changed()
                            {
                                app.log(
                                    super::LogKind::Info,
                                    "Se aplicará al Publicar (calidad WebP).".into(),
                                );
                            }
                        });
                    }
                }
                _ => {}
            }
            // Calidades por formato de textura (mismos rangos que el original).
            match app.config.gpu_format {
                GpuFormat::Pvrtc4Bpp | GpuFormat::Pvr3Gz | GpuFormat::Pvr3Ccz => {
                    ui.horizontal(|ui| {
                        ui.label("Calidad PVRTC (0-7)");
                        ui.add(egui::DragValue::new(&mut app.config.pvr_quality).range(0..=7));
                    });
                }
                GpuFormat::Etc1 | GpuFormat::Etc1Ktx => {
                    ui.horizontal(|ui| {
                        ui.label("Calidad ETC1 (0-100)");
                        ui.add(egui::DragValue::new(&mut app.config.etc1_quality).range(0..=100));
                    });
                }
                GpuFormat::Etc2Rgba => {
                    ui.horizontal(|ui| {
                        ui.label("Calidad ETC2 (0-100)");
                        ui.add(egui::DragValue::new(&mut app.config.etc2_quality).range(0..=100));
                    });
                }
                GpuFormat::Astc4x4 => {
                    ui.horizontal(|ui| {
                        ui.label("Calidad ASTC (0-4, 4 = exhaustivo)");
                        ui.add(egui::DragValue::new(&mut app.config.astc_quality).range(0..=4));
                    });
                }
                GpuFormat::Basis => {
                    ui.horizontal(|ui| {
                        ui.label("Calidad Basis ETC1S (0-100)");
                        ui.add(egui::DragValue::new(&mut app.config.basis_quality).range(0..=100));
                    });
                }
                _ => {}
            }
            if app.config.gpu_format == GpuFormat::Dds
                && matches!(
                    app.config.pixel_format,
                    PixelFormat::Dxt1 | PixelFormat::Dxt5
                )
            {
                enum_combo(
                    ui,
                    "Modo DXT",
                    app.config.dxt_mode.as_str(),
                    |ui, v| {
                        ui.selectable_value(v, DxtMode::Linear, "DXT_LINEAR (error uniforme)");
                        ui.selectable_value(
                            v,
                            DxtMode::Perceptual,
                            "DXT_PERCEPTUAL (pondera la luminancia)",
                        );
                    },
                    &mut app.config.dxt_mode,
                );
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
    if let Some(path) = app.config.export_template.as_deref() {
        if !path.is_file() {
            warnings.push(format!(
                "La plantilla {} no existe: el empaquetado fallará.",
                path.display()
            ));
        }
    }
    if let Some(dir) = app.config.custom_exporters_directory.as_deref() {
        if tp_core::dataformats::custom_exporter_ids(dir).is_empty() {
            warnings.push(format!(
                "No hay <id>.hbs en {}: no se podrá elegir un exportador propio.",
                dir.display()
            ));
        }
    }
    if app.config.template_format != TemplateFormat::Css
        && (app.config.css_sprite_prefix.is_some() || app.config.css_media_query_2x.is_some())
    {
        warnings.push(
            "El prefijo de clase y la media query 2× sólo aplican al formato CSS; se ignorarán."
                .into(),
        );
    }
    if !app.config.gpu_format.is_supported() {
        warnings.push(
            "ASTC requiere compilar con --features gpu-formats; el publicado fallará.".into(),
        );
    }
    if app.config.flip_vertical && !app.config.gpu_format.is_hardware() {
        warnings.push(
            "Voltear verticalmente (flip Y) solo aplica a formatos de hardware \
             (ASTC/ETC2/ETC1/PVRTC); se ignorará."
                .into(),
        );
    }
    if !app
        .config
        .pixel_format
        .is_compatible_with(app.config.gpu_format)
    {
        warnings.push(format!(
            "El formato de píxel {} no está soportado por el formato de textura {};              se usará RGBA8888 al publicar.",
            app.config.pixel_format.as_str(),
            app.config.gpu_format.as_str()
        ));
    }
    if app.config.scale_variants.iter().any(|s| s.fract() != 0.0) {
        warnings.push(
            "Hay variantes de escala fraccionarias; las coordenadas se redondearán a píxeles."
                .into(),
        );
    }
    if let Some(want) = app.config.scale_mode.required_factor() {
        let want = want as f32;
        let other: Vec<String> = app
            .config
            .scale_variants
            .iter()
            .copied()
            .filter(|s| (s - 1.0).abs() > 1e-6 && (*s - want).abs() > 1e-6)
            .map(|s| format!("{s}x"))
            .collect();
        if !other.is_empty() {
            warnings.push(format!(
                "{} solo se aplica a la escala exacta {want}x; {} se reescalarán con Smooth.",
                app.config.scale_mode.as_str(),
                other.join(", ")
            ));
        }
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

/// Id de la bandera por-frame "algún combo cambió la configuración".
const SETTINGS_CHANGED_FLAG: &str = "tp_settings_changed";

/// Opciones de cada escala listada en «Scaling variants»: filtro de sprites,
/// tamaño máximo de textura y si la variante reutiliza la hoja base.
/// Presets del diálogo de variantes del original: se eligen y se aplican de
/// una vez, sobrescribiendo escalas, sufijos y opciones de cada variante.
fn variant_presets_ui(app: &mut App, ui: &mut egui::Ui) {
    let id = egui::Id::new("variant_preset_selected");
    let mut selected = ui
        .ctx()
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| tp_core::config::VARIANT_PRESETS[0].name.to_string());
    ui.horizontal(|ui| {
        ui.label("Presets de variantes");
        egui::ComboBox::from_id_salt("variant_preset")
            .selected_text(selected.clone())
            .show_ui(ui, |ui| {
                for preset in tp_core::config::VARIANT_PRESETS {
                    let detail = preset
                        .variants
                        .iter()
                        .map(|(scale, suffix)| format!("{scale}{suffix}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    ui.selectable_value(&mut selected, preset.name.to_string(), preset.name)
                        .on_hover_text(detail);
                }
            });
        if ui
            .button("Aplicar")
            .on_hover_text(
                "Sobrescribe las variantes actuales por las del preset, igual que el \
                 botón Apply del original.",
            )
            .clicked()
            && app.config.apply_variant_preset(&selected)
        {
            app.sync_variants();
            app.on_config_changed();
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, selected));
}

fn variant_options_ui(app: &mut App, ui: &mut egui::Ui) {
    let scales = app.config.scale_variants.clone();
    if scales.len() < 2 && app.config.variant_options.is_empty() {
        return;
    }
    ui.add_space(2.0);
    ui.collapsing("Opciones por variante", |ui| {
        ui.label(
            egui::RichText::new(
                "«Reutiliza la base» = la hoja empaquetada a escala 1.0 llevada a esta escala \
                 (rápido, mismo layout y mismos frames). Con filtro o tope, esa variante se \
                 empaqueta sola y sus archivos pueden diferir.",
            )
            .weak(),
        );
        ui.add_space(3.0);
        egui::Grid::new("variant_options_grid")
            .num_columns(6)
            .spacing([8.0, 4.0])
            .striped(true)
            .min_col_width(56.0)
            .show(ui, |ui| {
                ui.strong("escala");
                ui.strong("filtro de sprites");
                ui.strong("máx. px");
                ui.strong("idéntico");
                ui.strong("fracc.");
                ui.strong("qué hace");
                ui.end_row();
                for scale in scales {
                    let default = VariantOptions {
                        scale,
                        ..VariantOptions::default()
                    };
                    let mut opts = app
                        .config
                        .variant_options_for(scale)
                        .cloned()
                        .unwrap_or(default.clone());
                    let mut changed = false;

                    ui.label(format!(
                        "{scale} ➡ {}",
                        tp_core::pipeline::variant_suffix(scale)
                    ))
                    .on_hover_text(
                        "Sufijo que llevarán los archivos de esta variante ({v} = escala)",
                    );
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut opts.sprite_filter)
                                .desired_width(150.0)
                                .hint_text("vacío = todos"),
                        )
                        .on_hover_text(
                            "Patrones separados por comas con comodines * y ? sobre el nombre \
                             del sprite (p. ej. hero*, coin). Solo lo que coincide entra en esta \
                             variante.",
                        )
                        .changed();
                    let mut max = opts.max_texture_size.unwrap_or(0);
                    if ui
                        .add(
                            egui::DragValue::new(&mut max)
                                .range(0..=16384)
                                .suffix(" px"),
                        )
                        .on_hover_text(
                            "0 = el tamaño máximo del proyecto. Con un valor distinto, esta \
                             variante se empaqueta sola respetando ese tope.",
                        )
                        .changed()
                    {
                        opts.max_texture_size = if max == 0 { None } else { Some(max) };
                        changed = true;
                    }
                    changed |= ui
                        .checkbox(&mut opts.force_identical_layout, "")
                        .on_hover_text(
                            "Sin marcar, la variante se empaqueta de nuevo con su escala en \
                             vez de reutilizar la hoja base.",
                        )
                        .changed();
                    changed |= ui
                        .checkbox(&mut opts.accept_fractional, "")
                        .on_hover_text(
                            "«Accept fractional values»: la variante queda fuera del común \
                             divisor, así que su hoja idéntica se redondea al píxel y no \
                             obliga a estirar las demás para caber en su denominador.",
                        )
                        .changed();

                    let (estado, color, motivo) = variant_state(&opts);
                    ui.colored_label(color, estado).on_hover_text(motivo);

                    if changed {
                        upsert_variant_option(&mut app.config.variant_options, opts, &default);
                        app.on_config_changed();
                    }
                    ui.end_row();
                }
            });
    });
}

/// Qué hará realmente la variante en la publicación, para poder decirlo en
/// la tabla sin que el usuario tenga que adivinarlo.
fn variant_state(opts: &VariantOptions) -> (&'static str, egui::Color32, String) {
    const SOLO: egui::Color32 = egui::Color32::from_rgb(255, 200, 80);
    const BASE: egui::Color32 = egui::Color32::from_rgb(130, 200, 130);
    let filtered = !opts.sprite_filter.trim().is_empty();
    let capped = opts.max_texture_size.is_some();
    if filtered || capped {
        let mut why = String::from("Se empaqueta por su cuenta porque ");
        match (filtered, capped) {
            (true, true) => why.push_str("tiene filtro y tamaño máximo"),
            (true, false) => why.push_str("tiene filtro"),
            (false, true) => why.push_str("tiene tamaño máximo"),
            (false, false) => unreachable!(),
        }
        why.push('.');
        return ("empaqueta sola", SOLO, why);
    }
    if opts.force_identical_layout {
        return (
            "reutiliza la base",
            BASE,
            "Toma la hoja base y la escala: mismo layout y mismos frames.".into(),
        );
    }
    (
        "empaqueta sola",
        SOLO,
        "Layout no idéntico: se empaqueta de nuevo con su escala.".into(),
    )
}

/// Guarda las opciones de una escala, o las borra si vuelven a los valores
/// por defecto (así el proyecto no arrastra entradas vacías).
fn upsert_variant_option(
    list: &mut Vec<VariantOptions>,
    opts: VariantOptions,
    default: &VariantOptions,
) {
    list.retain(|o| (o.scale - opts.scale).abs() > 1e-6);
    if &opts != default {
        list.push(opts);
    }
}

fn enum_combo<T: PartialEq + Clone>(
    ui: &mut egui::Ui,
    label: &str,
    selected_text: &str,
    items: impl FnOnce(&mut egui::Ui, &mut T),
    value: &mut T,
) {
    egui::ComboBox::from_label(label)
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
            let before = value.clone();
            items(ui, value);
            if *value != before {
                ui.ctx().data_mut(|d| {
                    d.insert_temp(egui::Id::new(SETTINGS_CHANGED_FLAG), true);
                });
            }
        });
}

fn strategy_display(s: PackingStrategy, guillotine: bool) -> String {
    if guillotine && s == PackingStrategy::Guillotine {
        // `resolve()` traduce la entrada legacy en la heurística BSSF.
        return "BSSF".to_string();
    }
    let prefix = if guillotine { "" } else { "MaxRects " };
    match s {
        PackingStrategy::Bssf => format!("{prefix}BSSF"),
        PackingStrategy::Baf => format!("{prefix}BAF"),
        PackingStrategy::Blsf => format!("{prefix}BLSF"),
        PackingStrategy::Guillotine => "Guillotine (legacy)".to_string(),
        PackingStrategy::Best => "Best (probar todas)".to_string(),
        PackingStrategy::BottomLeft => "BottomLeft".to_string(),
        PackingStrategy::ContactPoint => "ContactPoint".to_string(),
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
        DitheringAlgorithm::NearestNeighbour => "Nearest Neighbour",
        DitheringAlgorithm::Linear => "Linear",
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

/// Combo de formato de datos con todos los presets del original, agrupados
/// por categoría. Elegir uno convierte el proyecto (familia + extensión) y
/// aplica sus valores recomendados, como el diálogo «Data format…».
fn data_format_combo(app: &mut App, ui: &mut egui::Ui) {
    let selected = match app.config.data_format_preset() {
        Some(preset) => preset.label,
        None => template_name(app.config.template_format),
    };
    egui::ComboBox::from_label("Formato de metadatos")
        .selected_text(selected)
        .show_ui(ui, |ui| {
            let mut sel = app.config.data_format.clone();
            egui::ScrollArea::vertical()
                .id_salt("data_format_list")
                .max_height(280.0)
                .show(ui, |ui| {
                    for &category in tp_core::dataformats::CATEGORIES {
                        ui.separator();
                        ui.strong(category);
                        for preset in tp_core::dataformats::data_formats_in_category(category) {
                            ui.selectable_value(&mut sel, preset.id.to_string(), preset.label);
                        }
                    }
                });
            if sel != app.config.data_format && app.config.apply_data_format(&sel) {
                mark_settings_changed(ui);
            }
        });
    // «Update to recommended values» del diálogo de conversión: re-aplica la
    // rotación, el algoritmo y la auto-detección del preset elegido.
    let recommended = app.config.data_format_preset().is_some();
    if ui
        .add_enabled(recommended, egui::Button::new("Valores recomendados"))
        .on_hover_text(
            "Aplica la rotación, el algoritmo y la auto-detección de animaciones \
             recomendados para el formato seleccionado.",
        )
        .clicked()
        && app.config.apply_data_format_defaults()
    {
        mark_settings_changed(ui);
    }
}

/// «Exportadores propios»: una carpeta de `<id>.hbs` elegible como plantilla
/// de salida. La familia y la extensión las sigue mandando el combo
/// «Formato de metadatos»: el exportador propio sólo aporta el texto, igual
/// que en el original (y `validate()` sólo acepta ids de formatos oficiales).
fn custom_exporters_ui(app: &mut App, ui: &mut egui::Ui) {
    let mut changed = false;
    egui::CollapsingHeader::new("Exportadores propios")
        .default_open(false)
        .show(ui, |ui| changed = custom_exporters_body(app, ui));
    if changed {
        app.on_config_changed();
    }
}

/// Contenido de «Exportadores propios». Devuelve `true` si tocó la config;
/// el wrapper decide si avisa. Se le puede llamar a mano desde un `Ui` de
/// test, sin abrir la cabecera.
fn custom_exporters_body(app: &mut App, ui: &mut egui::Ui) -> bool {
    let mut changed = false;
    ui.label(
        egui::RichText::new(
            "Carpeta con plantillas <id>.hbs propias; elegir una no cambia la familia \
             ni la extensión del fichero de datos.",
        )
        .weak(),
    );
    ui.label("Directorio de exportadores");
    ui.horizontal(|ui| {
        let mut dir = app
            .config
            .custom_exporters_directory
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if ui
            .add(
                egui::TextEdit::singleline(&mut dir)
                    .desired_width(160.0)
                    .hint_text("vacío = ninguno"),
            )
            .changed()
        {
            app.config.custom_exporters_directory = if dir.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(dir))
            };
            changed = true;
        }
        if ui.button("…").clicked() {
            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                app.config.custom_exporters_directory = Some(d);
                changed = true;
            }
        }
    });

    let ids = match app.config.custom_exporters_directory.as_deref() {
        Some(dir) => tp_core::dataformats::custom_exporter_ids(dir),
        None => Vec::new(),
    };
    let active = active_custom_exporter_id(&app.config).unwrap_or_default();
    let mut selected = active.clone();
    egui::ComboBox::from_id_salt("custom_exporter_id")
        .selected_text(if active.is_empty() {
            "— ninguno —".to_string()
        } else {
            active.clone()
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut selected, String::new(), "— ninguno —");
            for id in &ids {
                ui.selectable_value(&mut selected, id.clone(), id);
            }
        });
    if selected != active {
        let applied = if selected.is_empty() {
            app.config.clear_custom_exporter()
        } else {
            app.config.select_custom_exporter(&selected)
        };
        changed |= applied;
    }
    if ids.is_empty() {
        ui.label(
            egui::RichText::new("Sin <id>.hbs seleccionables: revisa el directorio de arriba.")
                .weak(),
        );
    }
    changed
}

/// «Propiedades de la plantilla»: lo que sólo consume el texto escrito — el
/// prefijo y la media query del exportador CSS, y las propiedades
/// `exporterProperties.*` que citan la plantilla de texto plano y los `.hbs`
/// propios.
fn template_properties_ui(app: &mut App, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("Propiedades de la plantilla")
        .default_open(false)
        .show(ui, |ui| template_properties_body(app, ui));
}

/// Contenido de «Propiedades de la plantilla» (ver
/// [`custom_exporters_body`] para por qué está separado de la cabecera).
fn template_properties_body(app: &mut App, ui: &mut egui::Ui) {
    if app.config.template_format == TemplateFormat::Css {
        ui.label("Prefijo de clase CSS (--css-sprite-prefix)");
        let mut prefix = app.config.css_sprite_prefix.clone().unwrap_or_default();
        if ui
            .add(
                egui::TextEdit::singleline(&mut prefix)
                    .desired_width(190.0)
                    .hint_text("vacío = ninguno (p. ej. icon-)"),
            )
            .changed()
        {
            app.config.css_sprite_prefix = none_if_empty(prefix);
            app.on_config_changed();
        }
        ui.label("Media query de la variante 2× (--css-media-query-2x)");
        let mut query = app.config.css_media_query_2x.clone().unwrap_or_default();
        if ui
            .add(
                egui::TextEdit::singleline(&mut query)
                    .desired_width(190.0)
                    .hint_text("sólo envuelve la hoja de las variantes >1×"),
            )
            .changed()
        {
            app.config.css_media_query_2x = none_if_empty(query);
            app.on_config_changed();
        }
    } else {
        ui.label(
            egui::RichText::new(
                "El prefijo de clase y la media query 2× sólo aplican al formato CSS.",
            )
            .weak(),
        );
    }

    ui.label("string_property de la plantilla (--plain-string-property)");
    let mut text = app.config.plain_string_property.clone().unwrap_or_default();
    if ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .desired_width(190.0)
                .hint_text("vacío = no escribir"),
        )
        .changed()
    {
        app.config.plain_string_property = none_if_empty(text);
        app.on_config_changed();
    }

    ui.label("bool_property de la plantilla (--plain-bool-property)");
    let before = plain_bool_choice(app.config.plain_bool_property);
    let mut choice = before;
    egui::ComboBox::from_id_salt("plain_bool_property")
        .selected_text(PLAIN_BOOL_LABELS[choice])
        .show_ui(ui, |ui| {
            for (i, label) in PLAIN_BOOL_LABELS.iter().enumerate() {
                ui.selectable_value(&mut choice, i, *label);
            }
        });
    if choice != before {
        app.config.plain_bool_property = plain_bool_value(choice);
        app.on_config_changed();
    }
}

/// Etiquetas del tri-estado de `plain_bool_property` (índice =
/// [`plain_bool_choice`]).
const PLAIN_BOOL_LABELS: [&str; 3] = ["— no escribir", "true", "false"];

/// `Some("")` no es un valor: vacío y en blanco vuelven a `None`, como en
/// `texture_path`.
fn none_if_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

/// El tri-estado de `plain_bool_property` como índice de
/// [`PLAIN_BOOL_LABELS`] (0 = `None`).
fn plain_bool_choice(value: Option<bool>) -> usize {
    match value {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    }
}

fn plain_bool_value(choice: usize) -> Option<bool> {
    match choice {
        0 => None,
        1 => Some(true),
        _ => Some(false),
    }
}

/// El id del exportador propio activo: el nombre de `export_template` sin
/// `.hbs`, sólo cuando esa plantilla vive dentro del directorio de
/// exportadores (una plantilla elegida aparte no es «propia» del combo).
fn active_custom_exporter_id(config: &ProjectConfig) -> Option<String> {
    let dir = config.custom_exporters_directory.as_deref()?;
    let path = config.export_template.as_deref()?;
    if path.parent() != Some(dir) {
        return None;
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".hbs"))
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// Marca que la config cambió en este frame (el panel llama a
/// `on_config_changed` al terminar de pintar).
fn mark_settings_changed(ui: &egui::Ui) {
    ui.ctx().data_mut(|d| {
        d.insert_temp(egui::Id::new(SETTINGS_CHANGED_FLAG), true);
    });
}

fn template_name(t: TemplateFormat) -> &'static str {
    match t {
        TemplateFormat::Json => "JSON (lista)",
        TemplateFormat::JsonHash => "JSON (hash)",
        TemplateFormat::Phaser => "Phaser 3",
        TemplateFormat::PixiJson => "PixiJS",
        TemplateFormat::Xml => "XML (libgdx)",
        TemplateFormat::Starling => "Starling",
        TemplateFormat::Plist => "Plist (cocos2d)",
        TemplateFormat::UIKitPlist => "Plist (UIKit)",
        TemplateFormat::LibgdxAtlas => "Atlas libGDX",
        TemplateFormat::SpineAtlas => "Atlas Spine",
        TemplateFormat::Css => "CSS (sprite)",
        TemplateFormat::CppHeader => "Cabecera C++",
        TemplateFormat::Tsv => "TSV",
        TemplateFormat::PlainText => "Texto plano",
        TemplateFormat::SpriteSheetOnly => "Solo hoja de sprites",
    }
}

/// Pixel formats de software: siempre disponibles.
const SOFT_PIXEL_FORMATS: &[(PixelFormat, &str)] = &[
    (PixelFormat::Rgba8888, "RGBA8888"),
    (PixelFormat::Rgb888, "RGB888 (sobre negro)"),
    (PixelFormat::Alpha8, "ALPHA8"),
    (PixelFormat::Intensity8, "INTENSITY8"),
    (PixelFormat::AlphaIntensity8, "Alpha+Intensity"),
    (PixelFormat::Rgba5551, "RGBA5551 (16 bits)"),
    (PixelFormat::Rgba5555, "RGBA5555 (20 bits)"),
    (PixelFormat::Bgra8888, "BGRA8888"),
    (PixelFormat::Rgba4444, "RGBA4444 (16 bits)"),
    (PixelFormat::Rgb565, "RGB565 (16 bits)"),
];

/// Pixel formats de hardware; la GUI solo muestra los que soporta el formato
/// de textura seleccionado («Only pixel formats supported by the selected
/// Texture Format can be chosen»).
const GPU_PIXEL_FORMATS: &[(PixelFormat, &str)] = &[
    (PixelFormat::Pvrtc2BppRgba, "PVRTCI_2BPP_RGBA"),
    (PixelFormat::Pvrtc4BppRgba, "PVRTCI_4BPP_RGBA"),
    (PixelFormat::Pvrtc2BppRgb, "PVRTCI_2BPP_RGB"),
    (PixelFormat::Pvrtc4BppRgb, "PVRTCI_4BPP_RGB"),
    (PixelFormat::Etc1Rgb, "ETC1_RGB"),
    (PixelFormat::Etc2Rgb, "ETC2_RGB"),
    (PixelFormat::Etc2Rgba, "ETC2_RGBA"),
    (PixelFormat::Dxt1, "DXT1"),
    (PixelFormat::Dxt5, "DXT5"),
    (PixelFormat::Astc4x4, "ASTC_4x4"),
    (PixelFormat::Astc5x4, "ASTC_5x4"),
    (PixelFormat::Astc5x5, "ASTC_5x5"),
    (PixelFormat::Astc6x5, "ASTC_6x5"),
    (PixelFormat::Astc6x6, "ASTC_6x6"),
    (PixelFormat::Astc8x5, "ASTC_8x5"),
    (PixelFormat::Astc8x6, "ASTC_8x6"),
    (PixelFormat::Astc8x8, "ASTC_8x8"),
    (PixelFormat::Astc10x5, "ASTC_10x5"),
    (PixelFormat::Astc10x6, "ASTC_10x6"),
    (PixelFormat::Astc10x8, "ASTC_10x8"),
    (PixelFormat::Astc10x10, "ASTC_10x10"),
    (PixelFormat::Astc12x10, "ASTC_12x10"),
    (PixelFormat::Astc12x12, "ASTC_12x12"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_custom_exporter_id_only_reads_ids_inside_the_directory() {
        let dir = PathBuf::from("mis-exportadores");
        let mut cfg = ProjectConfig {
            custom_exporters_directory: Some(dir.clone()),
            ..ProjectConfig::default()
        };
        assert_eq!(active_custom_exporter_id(&cfg), None, "sin plantilla");

        cfg.export_template = Some(dir.join("mi.hbs"));
        assert_eq!(active_custom_exporter_id(&cfg).as_deref(), Some("mi"));

        cfg.export_template = Some(dir.join("otro.json.hbs"));
        assert_eq!(
            active_custom_exporter_id(&cfg).as_deref(),
            Some("otro.json"),
            "el id conserva sus puntos"
        );

        cfg.export_template = Some(PathBuf::from("de-fuera/hbs"));
        assert_eq!(
            active_custom_exporter_id(&cfg),
            None,
            "una plantilla de fuera del directorio no es «propia»"
        );

        cfg.export_template = Some(dir.join("sin_extension"));
        assert_eq!(active_custom_exporter_id(&cfg), None, "sin .hbs");

        cfg.custom_exporters_directory = None;
        cfg.export_template = Some(PathBuf::from("x.hbs"));
        assert_eq!(active_custom_exporter_id(&cfg), None, "sin directorio");
    }

    #[test]
    fn plain_bool_choice_is_a_faithful_tri_state() {
        for (value, choice) in [(None, 0), (Some(true), 1), (Some(false), 2)] {
            assert_eq!(plain_bool_choice(value), choice);
            assert_eq!(plain_bool_value(choice), value);
        }
        assert_eq!(PLAIN_BOOL_LABELS.len(), 3);
        assert_eq!(PLAIN_BOOL_LABELS[plain_bool_choice(Some(false))], "false");
    }

    #[test]
    fn empty_strings_become_none_like_texture_path() {
        assert_eq!(none_if_empty(String::new()), None);
        assert_eq!(none_if_empty("   ".into()), None);
        assert_eq!(none_if_empty(" icon- ".into()).as_deref(), Some(" icon- "));
    }

    #[test]
    fn the_new_sections_render_in_every_state() {
        let ctx = egui::Context::default();
        let screen = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1360.0, 860.0),
            )),
            ..egui::RawInput::default()
        };
        let cases = [
            (TemplateFormat::Css, true, Some(false)),
            (TemplateFormat::Json, false, None),
            (TemplateFormat::PlainText, true, Some(true)),
        ];
        for (template, with_dir, flag) in cases {
            let mut app = App::new_for_testing(ctx.clone(), None);
            app.advanced_settings = true;
            app.config.template_format = template;
            app.config.custom_exporters_directory = with_dir.then(|| PathBuf::from("exportadores"));
            app.config.css_sprite_prefix = Some("icon-".into());
            app.config.plain_bool_property = flag;

            // El panel entero con las avanzadas activadas y, aparte, el
            // contenido de los dos colapsables nuevos (cerrados por
            // defecto: aquí se les llama a mano).
            let _ = ctx.run(screen.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    settings_ui(&mut app, ui);
                    custom_exporters_body(&mut app, ui);
                    template_properties_body(&mut app, ui);
                });
            });
        }
    }
}
