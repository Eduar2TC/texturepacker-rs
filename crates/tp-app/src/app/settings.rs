//! Right settings panel: basic options always visible, advanced behind a toggle.

use super::App;
use crate::i18n::t;
use eframe::egui;
use std::path::PathBuf;
use tp_core::config::{
    AlphaHandling, BasicSortBy, ColorDepth, DitheringAlgorithm, DxtMode, GdxFilter, GpuFormat,
    PackMode, PackingAlgorithm, PackingStrategy, PixelFormat, PngDither, ProjectConfig, ScaleMode,
    SizeConstraint, SortOrder, TemplateFormat, TrimMode, VariantOptions,
};

/// Panel derecho de Ajustes. Los controles no avisan uno a uno: mutan
/// `config` y el sondeo por frame (`poll_changes`) detecta el cambio por
/// huella, repinta y reempaqueta en el frame siguiente. Para notificar en
/// el mismo frame sigue existiendo [`App::on_config_changed`].
pub(super) fn settings_ui(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.heading(t!("Ajustes"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut app.advanced_settings, t!("Avanzados"))
                .on_hover_text(t!("Mostrar todas las opciones"));
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("settings_scroll")
        .show(ui, |ui| {
            interface_section(app, ui);
            data_section(app, ui);
            layout_section(app, ui);
            processing_section(app, ui);
            warnings_section(app, ui);
        });
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
    egui::CollapsingHeader::new(t!("Datos"))
        .default_open(true)
        .show(ui, |ui| {
            ui.label(t!("Directorio de entrada"));
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
            ui.label(t!("Directorio de salida"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut app.output_dir_text).desired_width(190.0));
                if ui.button("…").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        app.output_dir_text = dir.display().to_string();
                        app.config.output_directory = dir;
                        app.on_paths_edited();
                    }
                }
            });
            ui.label(t!("Nombre base de los archivos"));
            ui.add(egui::TextEdit::singleline(&mut app.config.base_file_name).desired_width(190.0));
            ui.label(
                egui::RichText::new(t!("Placeholders: {n} {n1} {v}  (p. ej. hoja{n1}{v})")).weak(),
            );
            data_format_combo(app, ui);

            if !advanced {
                return;
            }
            // Ficheros de datos extra (--class-file/--header-file/…), que se
            // escriben junto a los metadatos.
            let extra_fields: [(&str, &mut String); 4] = [
                ("Class file (Swift)", &mut app.config.class_file),
                ("Header file (C++/ObjC)", &mut app.config.header_file),
                ("Source file (C++)", &mut app.config.source_file),
                ("Sprite ids file", &mut app.config.spriteids_file),
            ];
            egui::CollapsingHeader::new(t!("Ficheros extra por framework"))
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(t!(
                            "Vacío = no escribir. Alias CLI: --class-file, --header-file, \
                             --source-file y --spriteids-file."
                        ))
                        .weak(),
                    );
                    for (label, value) in extra_fields {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            ui.add(egui::TextEdit::singleline(value).desired_width(200.0));
                        });
                    }
                });
            // Extras de data format: cache busting (Pixi/Phaser), filtro
            // (LibGDX) y shape debug (contorno dibujado en la hoja).
            egui::CollapsingHeader::new(t!("Extras del data format"))
                .default_open(false)
                .show(ui, |ui| {
                    ui.checkbox(
                        &mut app.config.cache_busting,
                        t!("Cache busting (?v= en la textura citada)"),
                    )
                    .on_hover_text(t!(
                        "Añade ?v=<hash del fichero> a la imagen que los metadatos \
                         referencian, como los data formats de Pixi/Phaser."
                    ));
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
                    ui.checkbox(
                        &mut app.config.shape_debug,
                        t!("Shape debug (contornos en la hoja)"),
                    )
                    .on_hover_text(t!(
                        "Dibuja el rectángulo visible y los polígonos de cada sprite \
                         sobre la hoja, en magenta."
                    ));
                });
            ui.checkbox(&mut app.config.recursive, t!("Buscar en subdirectorios"));
            ui.checkbox(
                &mut app.config.trim_sprite_names,
                t!("Quitar la extensión de los nombres"),
            )
            .on_hover_text(t!("hero/idle_00.png pasa a llamarse hero/idle_00"));
            ui.checkbox(
                &mut app.config.prepend_folder_name,
                t!("Anteponer el nombre de la carpeta inteligente"),
            )
            .on_hover_text(t!(
                "Solo aplica a carpetas añadidas fuera del directorio de entrada"
            ));
            ui.checkbox(
                &mut app.config.enable_auto_detect_animations,
                t!("Auto-detectar animaciones"),
            )
            .on_hover_text(t!(
                "Agrupa sprites como walk_001..walk_003 en una animación walk \
                     y la expone en los metadatos (auto-detectar animaciones)"
            ));
            ui.label(t!("Ruta de la textura en los metadatos (p. ej. /assets)"));
            let mut texture_path = app.config.texture_path.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut texture_path)
                        .desired_width(190.0)
                        .hint_text(t!("vacío = sin prefijo")),
                )
                .changed()
            {
                app.config.texture_path = if texture_path.trim().is_empty() {
                    None
                } else {
                    Some(texture_path)
                };
            }
            ui.label(t!("Escalado de variantes (p. ej. 2, 0.5 ➡ @2x, -hd)"));
            ui.add(egui::TextEdit::singleline(&mut app.variants_text).desired_width(190.0));
            variant_presets_ui(app, ui);
            variant_options_ui(app, ui);
            ui.label(t!("Plantilla Mustache personalizada (opcional)"));
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
            ui.label(t!("Clave de cifrado AES-256-GCM (opcional)"));
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
            // cualquier proyecto.
            ui.label(t!("Clave global (guardada una vez y reutilizable)"));
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
                            .selectable_value(&mut name, String::new(), t!("— ninguna —"))
                            .clicked()
                        {
                            app.config.encryption_key_name = None;
                        }
                        for n in &names {
                            if ui
                                .selectable_value(&mut name, n.clone(), n)
                                .on_hover_text(t!("Usa esta clave en el proyecto"))
                                .clicked()
                            {
                                app.config.encryption_key_name = Some(n.clone());
                                // La clave escrita a mano tiene prioridad:
                                // se limpia para que la global surta efecto.
                                app.config.encryption_key = None;
                            }
                        }
                    });
                if ui
                    .button(t!("Guardar"))
                    .on_hover_text(t!("Guarda la clave escrita arriba con este nombre"))
                    .clicked()
                {
                    let key = app.config.encryption_key.clone().unwrap_or_default();
                    match tp_core::keys::put(&name, &key) {
                        Ok(()) => {
                            app.config.encryption_key_name = Some(name.clone());
                            app.log(
                                super::LogKind::Info,
                                t!("Clave global «{}» guardada.", name),
                            );
                        }
                        Err(e) => app.log(super::LogKind::Warning, e.to_string()),
                    }
                }
                if ui
                    .button(t!("Borrar"))
                    .on_hover_text(t!("Borra la clave global seleccionada"))
                    .clicked()
                    && !name.is_empty()
                {
                    match tp_core::keys::remove(&name) {
                        Ok(true) => {
                            app.config.encryption_key_name = None;
                            app.log(super::LogKind::Info, t!("Clave global «{}» borrada.", name));
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
                        .hint_text(t!("nombre de la clave global")),
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
    egui::CollapsingHeader::new(t!("Composición"))
        .default_open(true)
        .show(ui, |ui| {
            let sizes = [256i32, 512, 1024, 2048, 4096, 8192, 16384];
            egui::ComboBox::from_label(t!("Tamaño máximo"))
                .selected_text(app.config.max_texture_size.to_string())
                .show_ui(ui, |ui| {
                    for s in sizes {
                        ui
                            .selectable_value(&mut app.config.max_texture_size, s, s.to_string());
                    }
                });
            ui
                .checkbox(&mut app.config.multipack, t!("Multipack (varias hojas)"))
                .on_hover_text(
                    t!("Si los sprites no caben en una hoja se generan varias; con la opción \
                     desactivada el empaquetado falla"),
                );
            ui
                .add(
                    egui::Slider::new(&mut app.config.padding, 0..=16)
                        .text(t!("Separación entre sprites (px)")),
                );
            ui
                .add(egui::Slider::new(&mut app.config.extrude, 0..=16).text(t!("Extrusión (px)")));
            ui
                .checkbox(&mut app.config.allow_rotation, t!("Permitir rotación 90°"));
            ui
                .checkbox(&mut app.config.flip_vertical, t!("Voltear verticalmente (flip Y)"))
                .on_hover_text(
                    t!("Solo formatos de hardware (ASTC/ETC2/ETC1/PVRTC); las coordenadas \
                     de los frames no cambian"),
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
                    t!("Algoritmo"),
                    &algorithm_label,
                    |ui, v| {
                        ui.selectable_value(v, PackingAlgorithm::MaxRects, "MaxRects");
                        ui.selectable_value(v, PackingAlgorithm::Guillotine, "Guillotine");
                        ui.selectable_value(v, PackingAlgorithm::Grid, t!("Rejilla (Grid)"));
                        ui.selectable_value(v, PackingAlgorithm::Basic, t!("Básico (Basic)"));
                        ui.selectable_value(
                            v,
                            PackingAlgorithm::Manual,
                            t!("Manual (arrastrar en la vista)"),
                        )
                        .on_hover_text(
                            t!("Arrastra los sprites en la vista previa para fijar su posición"),
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
                    t!("Heurística"),
                    &shown,
                    |ui, v| {
                        ui.selectable_value(v, PackingStrategy::Bssf, format!("{prefix}BSSF"));
                        ui.selectable_value(v, PackingStrategy::Baf, format!("{prefix}BAF"));
                        ui.selectable_value(v, PackingStrategy::Blsf, format!("{prefix}BLSF"));
                        ui.selectable_value(v, PackingStrategy::Best, t!("Best (probar todas)"));
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
                t!("Modo de empaquetado"),
                app.config.pack_mode.as_str(),
                |ui, v| {
                    ui.selectable_value(v, PackMode::Fast, t!("Fast (recorte simple)"));
                    ui.selectable_value(v, PackMode::Good, t!("Good (búsqueda rápida)"));
                    ui.selectable_value(v, PackMode::Best, t!("Best (búsqueda intensiva)"));
                },
                &mut app.config.pack_mode,
            );
            ui
                .checkbox(&mut app.config.enable_trim, t!("Trim (recortar transparencia)"));

            if !advanced {
                return;
            }
            ui
                .add(
                    egui::Slider::new(&mut app.config.border_padding, 0..=64)
                        .text(t!("Margen de borde (px)")),
                )
                .on_hover_text(t!("Margen transparente entre los sprites y el borde del atlas"));
            enum_combo(
                ui,
                t!("Restricción de tamaño"),
                app.config.size_constraints.as_str(),
                |ui, v| {
                    ui.selectable_value(v, SizeConstraint::AnySize, t!("Cualquiera"));
                    ui.selectable_value(v, SizeConstraint::Pot, t!("POT (potencia de 2)"));
                    ui.selectable_value(v, SizeConstraint::MultipleOf4, t!("Múltiplo de 4"));
                    ui.selectable_value(v, SizeConstraint::WordAligned, t!("Alineado a palabra"));
                },
                &mut app.config.size_constraints,
            );
            ui
                .checkbox(&mut app.config.force_squared, t!("Atlas cuadrado (force squared)"));
            ui.horizontal(|ui| {
                ui.label(t!("Tamaño fijo (0 = automático)"));
                ui
                    .add(
                        egui::DragValue::new(&mut app.config.fixed_width)
                            .range(0..=8192)
                            .speed(1),
                    );
                ui.label(egui::RichText::new("x").weak());
                ui
                    .add(
                        egui::DragValue::new(&mut app.config.fixed_height)
                            .range(0..=8192)
                            .speed(1),
                    );
            })
            .response
            .on_hover_text(t!("Fija las dimensiones del atlas (tamaño fijo)"));
            if app.config.algorithm == PackingAlgorithm::Basic {
                enum_combo(
                    ui,
                    t!("Ordenar por (Basic)"),
                    app.config.basic_sort_by.as_str(),
                    |ui, v| {
                        ui.selectable_value(v, BasicSortBy::Best, t!("Best (probar todas)"));
                        ui.selectable_value(v, BasicSortBy::Name, t!("Nombre"));
                        ui.selectable_value(v, BasicSortBy::Width, t!("Ancho"));
                        ui.selectable_value(v, BasicSortBy::Height, t!("Alto"));
                        ui.selectable_value(v, BasicSortBy::Area, t!("Área"));
                        ui.selectable_value(
                            v,
                            BasicSortBy::Circumference,
                            t!("Perímetro (circumference)"),
                        );
                    },
                    &mut app.config.basic_sort_by,
                );
                enum_combo(
                    ui,
                    t!("Orden (Basic)"),
                    app.config.basic_order.as_str(),
                    |ui, v| {
                        ui.selectable_value(v, SortOrder::Ascending, t!("Ascendente"));
                        ui.selectable_value(v, SortOrder::Descending, t!("Descendente"));
                    },
                    &mut app.config.basic_order,
                );
            }
            ui.horizontal(|ui| {
                ui.label(t!("Divisor común"));
                ui.label(egui::RichText::new("x").weak());
                ui
                    .add(
                        egui::DragValue::new(&mut app.config.common_divisor_x)
                            .range(1..=2048)
                            .speed(1),
                    );
                ui.label(egui::RichText::new("y").weak());
                ui
                    .add(
                        egui::DragValue::new(&mut app.config.common_divisor_y)
                            .range(1..=2048)
                            .speed(1),
                    );
            })
            .response
            .on_hover_text(t!("Estira los sprites con transparencia hasta ser divisibles"));
            ui
                .add(
                    egui::Slider::new(&mut app.config.align_to_grid, 0..=64)
                        .text(t!("Alinear a rejilla (0 = off)")),
                )
                .on_hover_text(t!("Coloca las esquinas de los sprites en coordenadas múltiplos"));
            ui
                .add_enabled(
                    app.config.enable_trim,
                    egui::Slider::new(&mut app.config.trim_threshold, 1..=255)
                        .text(t!("Umbral de recorte (1-255)")),
                );
            ui.add_enabled_ui(app.config.enable_trim, |ui| {
                egui::ComboBox::from_label(t!("Modo de recorte"))
                    .selected_text(trim_mode_name(app.config.trim_mode))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.config.trim_mode,
                            TrimMode::None,
                            t!("None (sin recorte)"),
                        );
                        ui.selectable_value(&mut app.config.trim_mode, TrimMode::Trim, "Trim");
                        ui.selectable_value(
                            &mut app.config.trim_mode,
                            TrimMode::CropKeepPos,
                            t!("Crop, conservar posición"),
                        );
                        ui.selectable_value(
                            &mut app.config.trim_mode,
                            TrimMode::Crop,
                            t!("Crop, fijar en 0/0"),
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
                        .text(t!("Margen de recorte (px)")),
                );
            }
            if app.config.trim_mode == TrimMode::Polygon {
                ui.label(
                    egui::RichText::new(
                        t!("Polygon activa el empaquetado por contorno y exporta la malla."),
                    )
                    .weak(),
                );
            }
            ui.checkbox(
                &mut app.config.enable_aliasing,
                t!("Detección de duplicados (alias)"),
            );
            ui.checkbox(
                &mut app.config.enable_normal_maps,
                t!("Empaquetar mapas de normales"),
            );
            if app.config.enable_normal_maps {
                ui.horizontal(|ui| {
                    ui.label(t!("Sufijo"));
                    ui
                        .add(
                            egui::TextEdit::singleline(&mut app.config.normal_map_suffix)
                                .desired_width(110.0),
                        );
                    ui.label(t!("Filtro de ruta"));
                    ui
                        .add(
                            egui::TextEdit::singleline(&mut app.config.normal_map_filter)
                                .desired_width(130.0),
                        );
                });
                ui
                    .checkbox(
                        &mut app.config.normal_map_auto_detect,
                        t!("Detectar por color (auto-detect)"),
                    );
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(t!("Hoja de normales (vacío = <imagen>_normal)"))
                            .weak(),
                    );
                    ui
                        .add(
                            egui::TextEdit::singleline(&mut app.config.normal_map_sheet)
                                .desired_width(150.0),
                        );
                });
            }
            ui.add_enabled_ui(!polygon_auto, |ui| {
                ui.checkbox(&mut app.config.enable_polygon, t!("Modo polígono (mallas)"));
            });
            ui
                .add_enabled(
                    polygon_auto,
                    egui::Slider::new(&mut app.config.polygon_tolerance, 0.0..=10.0)
                        .text(t!("Tolerancia (RDP)")),
                );
            if polygon_auto {
                ui.label(
                    egui::RichText::new(
                        t!("Empaqueta sprites por su contorno (Marching Squares ➡ RDP ➡ Earcut). Activo por el modo de recorte Polígono."),
                    )
                    .weak(),
                );
            }
            ui.label(t!("Pivot por defecto (normalizado 0..1)"));
            ui.horizontal(|ui| {
                ui
                    .add(
                        egui::DragValue::new(&mut app.config.default_pivot_x)
                            .range(0.0..=1.0)
                            .speed(0.01),
                    );
                ui
                    .add(
                        egui::DragValue::new(&mut app.config.default_pivot_y)
                            .range(0.0..=1.0)
                            .speed(0.01),
                    );
            });
        });
}

fn processing_section(app: &mut App, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new(t!("Procesamiento"))
        .default_open(true)
        .show(ui, |ui| {
            enum_combo(
                ui,
                t!("Profundidad de color"),
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
                    ui.selectable_value(v, DitheringAlgorithm::None, t!("Ninguno"));
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
                        t!("Conservar píxeles"),
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
                    ui.selectable_value(
                        v,
                        AlphaHandling::PremultiplyAlpha,
                        t!("Premultiplicar alpha"),
                    );
                },
                &mut app.config.alpha_handling,
            );
            enum_combo(
                ui,
                t!("Escalado de variantes"),
                app.config.scale_mode.as_str(),
                |ui, v| {
                    ui.selectable_value(v, ScaleMode::Smooth, t!("Suave (bilineal)"));
                    ui.selectable_value(v, ScaleMode::Fast, t!("Rápido (vecino más cercano)"));
                    ui.selectable_value(v, ScaleMode::Scale2x, "Scale2x (2x)");
                    ui.selectable_value(v, ScaleMode::Scale3x, "Scale3x (3x)");
                    ui.selectable_value(v, ScaleMode::Scale4x, "Scale4x (4x)");
                    ui.selectable_value(v, ScaleMode::Eagle, "Eagle (2x)");
                },
                &mut app.config.scale_mode,
            );
            enum_combo(
                ui,
                t!("Formato de publicación"),
                app.config.gpu_format.as_str(),
                |ui, v| {
                    ui.selectable_value(v, GpuFormat::Png, "PNG");
                    ui.selectable_value(v, GpuFormat::Png8, t!("PNG-8 (indexado)"));
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
                    ui.selectable_value(v, GpuFormat::Etc1Ktx, t!("ETC1 en KTX (ktx)"));
                    ui.selectable_value(v, GpuFormat::Pvrtc4Bpp, "PVRTC 4BPP (pvr)");
                    ui.selectable_value(v, GpuFormat::Basis, "Basis (basis)");
                    ui.separator();
                    ui.selectable_value(v, GpuFormat::Zktx, t!("KTX con zlib (zktx)"));
                    ui.selectable_value(v, GpuFormat::Ktx2, t!("KTX2 sin comprimir (ktx2)"));
                    ui.selectable_value(v, GpuFormat::Pvr3Gz, t!("PVR3 en gzip (pvr.gz)"));
                    ui.selectable_value(v, GpuFormat::Pvr3Ccz, t!("PVR3 en CCZ (pvr.ccz)"));
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
                t!("Formato de píxel"),
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
                        ui.label(t!("Optimización PNG (0-7)"));
                        if ui
                            .add(egui::DragValue::new(&mut app.config.png_opt_level).range(0..=7))
                            .changed()
                        {
                            // No reempaqueta: solo afecta a la exportación.
                            app.log(
                                super::LogKind::Info,
                                t!("Se aplicará al Publicar (nivel de optimización PNG).").into(),
                            );
                        }
                    });
                }
                GpuFormat::Jpg => {
                    ui.horizontal(|ui| {
                        ui.label(t!("Calidad JPG (0-100)"));
                        if ui
                            .add(egui::DragValue::new(&mut app.config.jpg_quality).range(0..=100))
                            .changed()
                        {
                            app.log(
                                super::LogKind::Info,
                                t!("Se aplicará al Publicar (calidad JPG).").into(),
                            );
                        }
                    });
                }
                GpuFormat::WebP => {
                    let mut lossless = app.config.webp_quality > 100;
                    if ui
                        .checkbox(&mut lossless, t!("WebP sin pérdidas"))
                        .changed()
                    {
                        app.config.webp_quality = if lossless { 101 } else { 100 };
                    }
                    if app.config.webp_quality <= 100 {
                        ui.horizontal(|ui| {
                            ui.label(t!("Calidad WebP (0-100)"));
                            if ui
                                .add(
                                    egui::DragValue::new(&mut app.config.webp_quality)
                                        .range(0..=100),
                                )
                                .changed()
                            {
                                app.log(
                                    super::LogKind::Info,
                                    t!("Se aplicará al Publicar (calidad WebP).").into(),
                                );
                            }
                        });
                    }
                }
                _ => {}
            }
            // Calidades por formato de textura.
            match app.config.gpu_format {
                GpuFormat::Pvrtc4Bpp | GpuFormat::Pvr3Gz | GpuFormat::Pvr3Ccz => {
                    ui.horizontal(|ui| {
                        ui.label(t!("Calidad PVRTC (0-7)"));
                        ui.add(egui::DragValue::new(&mut app.config.pvr_quality).range(0..=7));
                    });
                }
                GpuFormat::Etc1 | GpuFormat::Etc1Ktx => {
                    ui.horizontal(|ui| {
                        ui.label(t!("Calidad ETC1 (0-100)"));
                        ui.add(egui::DragValue::new(&mut app.config.etc1_quality).range(0..=100));
                    });
                }
                GpuFormat::Etc2Rgba => {
                    ui.horizontal(|ui| {
                        ui.label(t!("Calidad ETC2 (0-100)"));
                        ui.add(egui::DragValue::new(&mut app.config.etc2_quality).range(0..=100));
                    });
                }
                GpuFormat::Astc4x4 => {
                    ui.horizontal(|ui| {
                        ui.label(t!("Calidad ASTC (0-4, 4 = exhaustivo)"));
                        ui.add(egui::DragValue::new(&mut app.config.astc_quality).range(0..=4));
                    });
                }
                GpuFormat::Basis => {
                    ui.horizontal(|ui| {
                        ui.label(t!("Calidad Basis ETC1S (0-100)"));
                        ui.add(egui::DragValue::new(&mut app.config.basis_quality).range(0..=100));
                    });
                }
                _ => {}
            }
            if app.config.gpu_format == GpuFormat::Dds
                && matches!(
                    app.config.pixel_format,
                    PixelFormat::Dxt1 | PixelFormat::Dxt3 | PixelFormat::Dxt5
                )
            {
                enum_combo(
                    ui,
                    "Modo DXT",
                    app.config.dxt_mode.as_str(),
                    |ui, v| {
                        ui.selectable_value(v, DxtMode::Linear, t!("DXT_LINEAR (error uniforme)"));
                        ui.selectable_value(
                            v,
                            DxtMode::Perceptual,
                            t!("DXT_PERCEPTUAL (pondera la luminancia)"),
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
        warnings.push(t!("El dithering no se aplica con RGBA8888; usa RGBA4444 o RGB565.").into());
    }
    if app.config.extrude > app.config.padding {
        warnings.push(t!(
            "El extrude ({}) se limita internamente al padding ({}).",
            app.config.extrude,
            app.config.padding
        ));
    }
    if let Some(path) = app.config.export_template.as_deref() {
        if !path.is_file() {
            warnings.push(t!(
                "La plantilla {} no existe: el empaquetado fallará.",
                path.display()
            ));
        }
    }
    if let Some(dir) = app.config.custom_exporters_directory.as_deref() {
        if tp_core::dataformats::custom_exporter_ids(dir).is_empty() {
            warnings.push(t!(
                "No hay <id>.hbs en {}: no se podrá elegir un exportador propio.",
                dir.display()
            ));
        }
    }
    if app.config.template_format != TemplateFormat::Css
        && (app.config.css_sprite_prefix.is_some() || app.config.css_media_query_2x.is_some())
    {
        warnings.push(
            t!("El prefijo de clase y la media query 2× sólo aplican al formato CSS; se ignorarán.")
                .into(),
        );
    }
    if !app.config.gpu_format.is_supported() {
        warnings.push(
            t!("ASTC requiere compilar con --features gpu-formats; el publicado fallará.").into(),
        );
    }
    if app.config.flip_vertical && !app.config.gpu_format.is_hardware() {
        warnings.push(
            t!(
                "Voltear verticalmente (flip Y) solo aplica a formatos de hardware \
             (ASTC/ETC2/ETC1/PVRTC); se ignorará."
            )
            .into(),
        );
    }
    if !app
        .config
        .pixel_format
        .is_compatible_with(app.config.gpu_format)
    {
        warnings.push(t!(
            "El formato de píxel {} no está soportado por el formato de textura {};              se usará RGBA8888 al publicar.",
            app.config.pixel_format.as_str(),
            app.config.gpu_format.as_str()
        ));
    }
    if app.config.scale_variants.iter().any(|s| s.fract() != 0.0) {
        warnings.push(
            t!("Hay variantes de escala fraccionarias; las coordenadas se redondearán a píxeles.")
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
            warnings.push(t!(
                "{} solo se aplica a la escala exacta {}x; {} se reescalarán con Smooth.",
                app.config.scale_mode.as_str(),
                want,
                other.join(", ")
            ));
        }
    }
    let align = app.config.align_to_grid;
    if align > 0 && (app.config.padding % align != 0 || app.config.border_padding % align != 0) {
        warnings.push(t!(
            "Para respetar la rejilla de {} px, el padding se redondeará al múltiplo de {} al publicar.",
            align,
            align
        ));
    }
    if app.config.border_padding * 2 >= app.config.max_texture_size {
        warnings
            .push(t!("El padding de borde deja el atlas interior vacío; publicar fallará.").into());
    }
    if app.config.gpu_format == GpuFormat::Pvrtc4Bpp
        && app.config.size_constraints != SizeConstraint::Pot
    {
        warnings.push(
            t!("PVRTC exige dimensiones potencia de dos; activa la restricción «POT» o fija el tamaño.")
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
                t!("El tamaño fijo no deja área interior con el padding de borde actual; publicar fallará.")
                    .into(),
            );
        }
    }
    let (dx, dy) = app.config.effective_divisors();
    if dx > 1 || dy > 1 {
        warnings.push(t!(
            "Los sprites se estirarán con transparencia hasta ser divisibles entre {}x{}.",
            dx,
            dy
        ));
    }
    if warnings.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new(t!("Avisos")).strong());
    for w in warnings {
        ui.label(egui::RichText::new(t!("- {}", w)).color(ui.visuals().warn_fg_color));
    }
}

/// Opciones de cada escala listada en «Scaling variants»: filtro de sprites,
/// tamaño máximo de textura y si la variante reutiliza la hoja base.
/// Presets del diálogo de variantes: se eligen y se aplican de
/// una vez, sobrescribiendo escalas, sufijos y opciones de cada variante.
fn variant_presets_ui(app: &mut App, ui: &mut egui::Ui) {
    let id = egui::Id::new("variant_preset_selected");
    let mut selected = ui
        .ctx()
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| tp_core::config::VARIANT_PRESETS[0].name.to_string());
    ui.horizontal(|ui| {
        ui.label(t!("Presets de variantes"));
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
            .button(t!("Aplicar"))
            .on_hover_text(t!(
                "Sobrescribe las variantes actuales por las del preset, igual que el \
                 botón Apply del original."
            ))
            .clicked()
            && app.config.apply_variant_preset(&selected)
        {
            app.sync_variants();
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
    ui.collapsing(t!("Opciones por variante"), |ui| {
        ui.label(
            egui::RichText::new(t!(
                "«Reutiliza la base» = la hoja empaquetada a escala 1.0 llevada a esta escala \
                     (rápido, mismo layout y mismos frames). Con filtro o tope, esa variante se \
                     empaqueta sola y sus archivos pueden diferir."
            ))
            .weak(),
        );
        ui.add_space(3.0);
        egui::Grid::new("variant_options_grid")
            .num_columns(6)
            .spacing([8.0, 4.0])
            .striped(true)
            .min_col_width(56.0)
            .show(ui, |ui| {
                ui.strong(t!("escala"));
                ui.strong(t!("filtro de sprites"));
                ui.strong(t!("máx. px"));
                ui.strong(t!("idéntico"));
                ui.strong(t!("fracc."));
                ui.strong(t!("qué hace"));
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
                    .on_hover_text(t!(
                        "Sufijo que llevarán los archivos de esta variante ({v} = escala)",
                    ));
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut opts.sprite_filter)
                                .desired_width(150.0)
                                .hint_text(t!("vacío = todos")),
                        )
                        .on_hover_text(t!(
                            "Patrones separados por comas con comodines * y ? sobre el nombre \
                             del sprite (p. ej. hero*, coin). Solo lo que coincide entra en esta \
                             variante."
                        ))
                        .changed();
                    let mut max = opts.max_texture_size.unwrap_or(0);
                    if ui
                        .add(
                            egui::DragValue::new(&mut max)
                                .range(0..=16384)
                                .suffix(" px"),
                        )
                        .on_hover_text(t!(
                            "0 = el tamaño máximo del proyecto. Con un valor distinto, esta \
                                 variante se empaqueta sola respetando ese tope."
                        ))
                        .changed()
                    {
                        opts.max_texture_size = if max == 0 { None } else { Some(max) };
                        changed = true;
                    }
                    changed |= ui
                        .checkbox(&mut opts.force_identical_layout, "")
                        .on_hover_text(t!(
                            "Sin marcar, la variante se empaqueta de nuevo con su escala en \
                             vez de reutilizar la hoja base."
                        ))
                        .changed();
                    changed |= ui
                        .checkbox(&mut opts.accept_fractional, "")
                        .on_hover_text(t!(
                            "«Accept fractional values»: la variante queda fuera del común \
                             divisor, así que su hoja idéntica se redondea al píxel y no \
                             obliga a estirar las demás para caber en su denominador."
                        ))
                        .changed();

                    let (estado, color, motivo) = variant_state(&opts, ui.visuals());
                    ui.colored_label(color, estado).on_hover_text(motivo);

                    if changed {
                        upsert_variant_option(&mut app.config.variant_options, opts, &default);
                    }
                    ui.end_row();
                }
            });
    });
}

/// Qué hará realmente la variante en la publicación, para poder decirlo en
/// la tabla sin que el usuario tenga que adivinarlo.
fn variant_state(
    opts: &VariantOptions,
    v: &egui::Visuals,
) -> (&'static str, egui::Color32, String) {
    let solo = super::amber_color(v);
    let base = if v.dark_mode {
        egui::Color32::from_rgb(130, 200, 130)
    } else {
        egui::Color32::from_rgb(0, 130, 60)
    };
    let filtered = !opts.sprite_filter.trim().is_empty();
    let capped = opts.max_texture_size.is_some();
    if filtered || capped {
        let mut why = String::from(t!("Se empaqueta por su cuenta porque "));
        match (filtered, capped) {
            (true, true) => why.push_str(t!("tiene filtro y tamaño máximo")),
            (true, false) => why.push_str(t!("tiene filtro")),
            (false, true) => why.push_str(t!("tiene tamaño máximo")),
            (false, false) => unreachable!(),
        }
        why.push('.');
        return (t!("empaqueta sola"), solo, why);
    }
    if opts.force_identical_layout {
        return (
            t!("reutiliza la base"),
            base,
            t!("Toma la hoja base y la escala: mismo layout y mismos frames.").into(),
        );
    }
    (
        t!("empaqueta sola"),
        solo,
        t!("Layout no idéntico: se empaqueta de nuevo con su escala.").into(),
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

fn enum_combo<T>(
    ui: &mut egui::Ui,
    label: &str,
    selected_text: &str,
    items: impl FnOnce(&mut egui::Ui, &mut T),
    value: &mut T,
) {
    // No se comprueba aquí si cambió: mutar `config` basta para que el
    // sondeo por frame lo detecte y notifique (ver `settings_ui`).
    egui::ComboBox::from_label(label)
        .selected_text(selected_text)
        .show_ui(ui, |ui| items(ui, value));
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
        PackingStrategy::Best => t!("Best (probar todas)").to_string(),
        PackingStrategy::BottomLeft => "BottomLeft".to_string(),
        PackingStrategy::ContactPoint => "ContactPoint".to_string(),
    }
}

fn trim_mode_name(t: TrimMode) -> &'static str {
    match t {
        TrimMode::None => "None",
        TrimMode::Trim => "Trim",
        TrimMode::CropKeepPos => t!("Crop, conservar posición"),
        TrimMode::Crop => t!("Crop, fijar en 0/0"),
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
        AlphaHandling::KeepTransparentPixels => t!("Conservar píxeles"),
        AlphaHandling::ClearTransparentPixels => "Limpiar transparentes",
        AlphaHandling::ReduceBorderArtifacts => "Reducir bordes (bleeding)",
        AlphaHandling::PremultiplyAlpha => "Premultiplicar alpha",
    }
}

/// Combo de formato de datos con todos los presets, agrupados
/// por categoría. Elegir uno convierte el proyecto (familia + extensión) y
/// aplica sus valores recomendados, como el diálogo «Data format…».
fn data_format_combo(app: &mut App, ui: &mut egui::Ui) {
    let selected = match app.config.data_format_preset() {
        Some(preset) => preset.label,
        None => template_name(app.config.template_format),
    };
    egui::ComboBox::from_label(t!("Formato de metadatos"))
        .selected_text(crate::i18n::tr(selected))
        .show_ui(ui, |ui| {
            let mut sel = app.config.data_format.clone();
            egui::ScrollArea::vertical()
                .id_salt("data_format_list")
                .max_height(280.0)
                .show(ui, |ui| {
                    for &category in tp_core::dataformats::CATEGORIES {
                        ui.separator();
                        ui.strong(crate::i18n::tr(category));
                        for preset in tp_core::dataformats::data_formats_in_category(category) {
                            ui.selectable_value(
                                &mut sel,
                                preset.id.to_string(),
                                crate::i18n::tr(preset.label),
                            );
                        }
                    }
                });
            if sel != app.config.data_format {
                app.config.apply_data_format(&sel);
            }
        });
    // «Update to recommended values» del diálogo de conversión: re-aplica la
    // rotación, el algoritmo y la auto-detección del preset elegido.
    let recommended = app.config.data_format_preset().is_some();
    if ui
        .add_enabled(recommended, egui::Button::new(t!("Valores recomendados")))
        .on_hover_text(t!(
            "Aplica la rotación, el algoritmo y la auto-detección de animaciones \
             recomendados para el formato seleccionado."
        ))
        .clicked()
    {
        app.config.apply_data_format_defaults();
    }
}

/// «Exportadores propios»: una carpeta de `<id>.hbs` elegible como plantilla
/// de salida. La familia y la extensión las sigue mandando el combo
/// «Formato de metadatos»: el exportador propio sólo aporta el texto (y
/// `validate()` sólo acepta ids de formatos oficiales).
fn custom_exporters_ui(app: &mut App, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new(t!("Exportadores propios"))
        .default_open(false)
        .show(ui, |ui| custom_exporters_body(app, ui));
}

/// Contenido de «Exportadores propios». Devuelve `true` si tocó la config
/// (informativo: de avisar se encarga el sondeo por frame). Se le puede
/// llamar a mano desde un `Ui` de test, sin abrir la cabecera.
fn custom_exporters_body(app: &mut App, ui: &mut egui::Ui) -> bool {
    let mut changed = false;
    ui.label(
        egui::RichText::new(t!(
            "Carpeta con plantillas <id>.hbs propias; elegir una no cambia la familia \
             ni la extensión del fichero de datos."
        ))
        .weak(),
    );
    ui.label(t!("Directorio de exportadores"));
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
                    .hint_text(t!("vacío = ninguno")),
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
            ui.selectable_value(&mut selected, String::new(), t!("— ninguno —"));
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
            egui::RichText::new(t!(
                "Sin <id>.hbs seleccionables: revisa el directorio de arriba."
            ))
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
    egui::CollapsingHeader::new(t!("Propiedades de la plantilla"))
        .default_open(false)
        .show(ui, |ui| template_properties_body(app, ui));
}

/// Contenido de «Propiedades de la plantilla» (ver
/// [`custom_exporters_body`] para por qué está separado de la cabecera).
fn template_properties_body(app: &mut App, ui: &mut egui::Ui) {
    if app.config.template_format == TemplateFormat::Css {
        ui.label(t!("Prefijo de clase CSS (--css-sprite-prefix)"));
        let mut prefix = app.config.css_sprite_prefix.clone().unwrap_or_default();
        if ui
            .add(
                egui::TextEdit::singleline(&mut prefix)
                    .desired_width(190.0)
                    .hint_text(t!("vacío = ninguno (p. ej. icon-)")),
            )
            .changed()
        {
            app.config.css_sprite_prefix = none_if_empty(prefix);
        }
        ui.label(t!("Media query de la variante 2× (--css-media-query-2x)"));
        let mut query = app.config.css_media_query_2x.clone().unwrap_or_default();
        if ui
            .add(
                egui::TextEdit::singleline(&mut query)
                    .desired_width(190.0)
                    .hint_text(t!("sólo envuelve la hoja de las variantes >1×")),
            )
            .changed()
        {
            app.config.css_media_query_2x = none_if_empty(query);
        }
    } else {
        ui.label(
            egui::RichText::new(t!(
                "El prefijo de clase y la media query 2× sólo aplican al formato CSS."
            ))
            .weak(),
        );
    }

    ui.label(t!(
        "string_property de la plantilla (--plain-string-property)"
    ));
    let mut text = app.config.plain_string_property.clone().unwrap_or_default();
    if ui
        .add(
            egui::TextEdit::singleline(&mut text)
                .desired_width(190.0)
                .hint_text(t!("vacío = no escribir")),
        )
        .changed()
    {
        app.config.plain_string_property = none_if_empty(text);
    }

    ui.label(t!("bool_property de la plantilla (--plain-bool-property)"));
    let before = plain_bool_choice(app.config.plain_bool_property);
    let mut choice = before;
    egui::ComboBox::from_id_salt("plain_bool_property")
        .selected_text(plain_bool_labels()[choice])
        .show_ui(ui, |ui| {
            for (i, label) in plain_bool_labels().iter().enumerate() {
                ui.selectable_value(&mut choice, i, *label);
            }
        });
    if choice != before {
        app.config.plain_bool_property = plain_bool_value(choice);
    }
}

/// Etiquetas del tri-estado de `plain_bool_property` (índice =
/// [`plain_bool_choice`]). Fn y no `const` para poder pasar cada etiqueta
/// por `t!()`.
fn plain_bool_labels() -> [&'static str; 3] {
    [t!("— no escribir"), "true", "false"]
}

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
/// [`plain_bool_labels`] (0 = `None`).
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

fn template_name(t: TemplateFormat) -> &'static str {
    match t {
        TemplateFormat::Json => t!("JSON (lista)"),
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
        TemplateFormat::CppHeader => t!("Cabecera C++"),
        TemplateFormat::Tsv => "TSV",
        TemplateFormat::PlainText => t!("Texto plano"),
        TemplateFormat::SpriteSheetOnly => t!("Solo hoja de sprites"),
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
    (PixelFormat::Dxt3, "DXT3"),
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
        assert_eq!(plain_bool_labels().len(), 3);
        assert_eq!(plain_bool_labels()[plain_bool_choice(Some(false))], "false");
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
