use std::path::{Path, PathBuf};

use super::args::{
    apply_flag_options, apply_format, apply_output_paths, apply_parity_layout_options,
    apply_phase_c_options, apply_scale_factor, apply_template_format, apply_texture_format,
    apply_variant_flags, check_data_extension, check_export_flags, check_exporter_only_options,
    check_unknown_options, classify_positional, parse_args, parse_pixel_format, Positional,
    PACK_FLAGS, PACK_VALUES,
};
use super::help::{exporter_list_text, usage, version_line};
use super::CmdResult;
use tp_core::config::{
    AlphaHandling, BasicSortBy, ColorDepth, DitheringAlgorithm, DxtMode, GdxFilter, PackMode,
    PackingAlgorithm, PackingStrategy, PngDither, ProjectConfig, ScaleMode, SizeConstraint,
    SortOrder, TrimMode,
};

/// Escribe el proyecto como TOML y devuelve el mensaje de resumen. La
/// extensión distinta de `.tpproj` se avisa pero no se rechaza: se guarda
/// el archivo que se le pida.
fn save_project(cfg: &ProjectConfig, path: &Path) -> CmdResult<String> {
    let is_tps = path.extension().and_then(|e| e.to_str()) == Some("tps");
    // El formato lo decide la extensión: `.tps` escribe el XML del original
    // (el subconjunto que aquí se entiende), cualquier otra cosa TOML.
    let text = if is_tps {
        tp_core::tps::write_tps(cfg)
    } else {
        cfg.to_toml()
            .map_err(|e| format!("No se pudo serializar el proyecto: {e}"))?
    };
    std::fs::write(path, text)
        .map_err(|e| format!("No se pudo escribir {}: {e}", path.display()))?;
    let mut msg = format!("✔ Proyecto guardado en {}", path.display());
    if !is_tps && path.extension().and_then(|e| e.to_str()) != Some("tpproj") {
        msg.push_str(" (aviso: la extensión no es .tpproj)");
    }
    Ok(msg)
}

/// Carga el positional de proyecto: `.tpproj`/`.toml` es TOML propio y
/// `.tps` es el XML del original (con sus avisos de ajustes no soportados).
fn load_project_arg(path: &Path, quiet: bool) -> CmdResult<ProjectConfig> {
    let is_tps = path.extension().and_then(|e| e.to_str()) == Some("tps");
    let mut config = if is_tps {
        let project = tp_core::tps::load_tps(path)
            .map_err(|e| format!("Proyecto inválido {}: {e}", path.display()))?;
        if !quiet {
            for warning in &project.warnings {
                eprintln!("{}", tp_i18n::tr(&format!("aviso: {warning}")));
            }
        }
        project.config
    } else {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("No se pudo leer {}: {e}", path.display()))?;
        ProjectConfig::from_toml(&text)
            .map_err(|e| format!("Proyecto inválido {}: {e}", path.display()))?
    };
    // Rutas relativas resueltas contra el propio proyecto, igual que hace la
    // GUI: el resultado no depende de desde dónde se lance el comando (C3).
    config.resolve_relative_paths(path.parent());
    Ok(config)
}

/// `--convert-texture FICHERO`: convierte una sola imagen al formato
/// pedido, sin empaquetar. Aplican el formato y sus calidades (`--format`,
/// `--texture-format`, `--pixel-format`), el `--dpi` de los PNG y el `--scale`
/// de la primera variante; el recorte no tiene sentido sin hoja, así que no
/// se aplica. Escribe junto a la entrada, o en `--output` si se dio.
fn convert_texture(cfg: &ProjectConfig, input: &Path) -> CmdResult<String> {
    if !input.is_file() {
        return Err(format!("--convert-texture: no existe {}", input.display()));
    }
    let (w, h, rgba) =
        tp_core::reader::load_image_rgba(input).map_err(|e| format!("--convert-texture: {e}"))?;
    let factor = cfg.scale_variants.first().copied().unwrap_or(1.0);
    let (pixels, width, height) = if (factor - 1.0).abs() < 1e-6 {
        (rgba, w as usize, h as usize)
    } else {
        tp_core::export::scale_rgba(&rgba, w as usize, h as usize, factor, cfg.scale_mode)
    };
    let opts = tp_core::export::EncodeOptions::from_config(cfg);
    let mut bytes = tp_core::export::encode_to_bytes(&pixels, width, height, &opts)
        .map_err(|e| format!("--convert-texture: {e}"))?;
    bytes = tp_core::export::apply_dpi(bytes, cfg.gpu_format, cfg.dpi);

    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let file_name = format!("{stem}.{}", cfg.gpu_format.file_extension());
    let dir = if cfg.output_directory.as_os_str().is_empty() {
        input
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf()
    } else {
        cfg.output_directory.clone()
    };
    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    }
    let out = dir.join(file_name);
    std::fs::write(&out, &bytes)
        .map_err(|e| format!("No se pudo escribir {}: {e}", out.display()))?;
    Ok(format!(
        "✔ Convertido {} → {} ({width}×{height}, {})",
        input.display(),
        out.display(),
        cfg.gpu_format.as_str()
    ))
}

/// `pack`: monta la configuración, la valida y empaqueta. Los errores se
/// devuelven en vez de abortar: `main` los imprime con `fail` y sale con 1.
pub(crate) fn cmd_pack(args: &[String]) -> CmdResult<()> {
    let (positionals, values, flags) = parse_args(args);
    check_unknown_options(&values, &flags, PACK_VALUES, PACK_FLAGS)?;
    check_exporter_only_options(&values, &flags)?;
    let has = |k: &str| flags.iter().any(|f| f == k);
    let quiet = has("quiet");

    if has("help") {
        usage();
    }
    if has("version") {
        println!("{}", version_line());
        return Ok(());
    }
    if has("exporter-list") {
        print_exporter_list(&values);
        return Ok(());
    }

    let setup = build_pack_config(&positionals, &values, &flags, quiet)?;
    announce_path_warning(setup.path_warning.as_deref(), quiet);
    if run_no_pack_actions(&setup.cfg, &values, quiet)? {
        return Ok(());
    }
    validate_pack_setup(&setup.cfg, setup.data_path.as_ref())?;
    announce_pack_plan(&setup.cfg, &flags, quiet);
    run_pack_pipeline(&setup.cfg, &flags, quiet)
}

/// Imprime la lista de ids de data formats y, si se pidió, los `<id>.hbs` de
/// --custom-exporters-directory.
fn print_exporter_list(values: &[(String, String)]) {
    let val = |k: &str| values.iter().rfind(|(v, _)| v == k).map(|(_, v)| v.clone());
    print!("{}", exporter_list_text());
    // Los `<id>.hbs` de --custom-exporters-directory también son formatos.
    if let Some(dir) = val("custom-exporters-directory") {
        let ids = tp_core::dataformats::custom_exporter_ids(Path::new(&dir));
        if ids.is_empty() {
            eprintln!(
                "{}",
                tp_i18n::tr(&format!(
                    "⚠ --custom-exporters-directory: no hay <id>.hbs en {dir}"
                ))
            );
        } else {
            println!(
                "{}",
                tp_i18n::tr(&format!("Exportadores propios de {dir}:"))
            );
            for id in ids {
                println!("  {id}");
            }
        }
    }
}

/// Configuración ya montada a partir de la línea de comandos.
struct PackSetup {
    cfg: ProjectConfig,
    /// Ruta de `--data`, si se pidió.
    data_path: Option<PathBuf>,
    /// Aviso de la numeración `{n}` de la hoja, si procede.
    path_warning: Option<String>,
}

/// Construcción de la configuración: posicionales, proyecto y todas las
/// opciones de valor, en el mismo orden en que se aplican siempre.
fn build_pack_config(
    positionals: &[String],
    values: &[(String, String)],
    flags: &[String],
    quiet: bool,
) -> CmdResult<PackSetup> {
    let val = |k: &str| values.iter().rfind(|(v, _)| v == k).map(|(_, v)| v.clone());

    let mut cfg = ProjectConfig::default();

    // Posicionales: un `.tpproj` y el resto son sprites, tal y como se
    // pasan en la línea de comandos.
    let mut project: Option<PathBuf> = None;
    let mut inputs: Vec<PathBuf> = Vec::new();
    for raw in positionals {
        match classify_positional(Path::new(raw)) {
            Positional::Project => {
                if project.is_some() {
                    return Err("sólo se admite un proyecto en la línea de comandos".into());
                }
                project = Some(PathBuf::from(raw));
            }
            Positional::Input => {
                if !Path::new(raw).exists() {
                    return Err(format!("no existe: {raw}"));
                }
                if Path::new(raw).is_file() && !tp_core::ingest::is_image_file(Path::new(raw)) {
                    return Err(format!("no es una imagen: {raw}"));
                }
                inputs.push(PathBuf::from(raw));
            }
        }
    }

    if let Some(p) = &project {
        cfg = load_project_arg(p, quiet)?;
    }
    if let Some(v) = val("input") {
        cfg.input_directory = PathBuf::from(v);
    }
    if let Some(v) = val("output") {
        cfg.output_directory = PathBuf::from(v);
    }
    for path in inputs {
        if cfg.input_directory.as_os_str().is_empty() {
            cfg.input_directory = path;
        } else {
            cfg.extra_inputs.push(path);
        }
    }
    // `--sheet`/`--data`: fijan carpeta, nombre base y formato.
    let (data_path, path_warning) = apply_output_paths(&mut cfg, values)?;
    if let Some(v) = val("base-name") {
        cfg.base_file_name = v;
    }
    if let Some(v) = val("max-size") {
        cfg.max_texture_size = v.parse().map_err(|_| "--max-size inválido".to_string())?;
    }
    // `--shape-padding` es el nombre canónico; `--padding` queda
    // como alias corto.
    if let Some(v) = val("shape-padding").or_else(|| val("padding")) {
        cfg.padding = v
            .parse()
            .map_err(|_| "--shape-padding inválido".to_string())?;
    }
    if let Some(v) = val("extrude") {
        cfg.extrude = v.parse().map_err(|_| "--extrude inválido".to_string())?;
    }
    if let Some(v) = val("border-padding") {
        cfg.border_padding = v
            .parse()
            .map_err(|_| "--border-padding inválido".to_string())?;
    }
    if let Some(v) = val("common-divisor") {
        let d: i32 = v
            .parse()
            .map_err(|_| "--common-divisor inválido".to_string())?;
        cfg.common_divisor_x = d;
        cfg.common_divisor_y = d;
    }
    // Ejes sueltos y pivot por defecto: van después de `--common-divisor` para
    // que el eje gane sobre el valor conjunto.
    apply_parity_layout_options(&mut cfg, values)?;
    // Fase C: topes por eje, fondo, dpi, filtros de entrada y renombrado.
    apply_phase_c_options(&mut cfg, values)?;
    if let Some(v) = val("align-to-grid").or_else(|| val("align")) {
        cfg.align_to_grid = v
            .parse()
            .map_err(|_| "--align-to-grid inválido".to_string())?;
    }
    if let Some(v) = val("alpha-handling") {
        cfg.alpha_handling = AlphaHandling::parse(v.as_str())
            .ok_or_else(|| format!("--alpha-handling inválido: {v}"))?;
    }
    if let Some(v) = val("scale-mode") {
        cfg.scale_mode =
            ScaleMode::parse(v.as_str()).ok_or_else(|| format!("--scale-mode inválido: {v}"))?;
    }
    if let Some(v) = val("texture-path").or_else(|| val("texturepath")) {
        cfg.texture_path = Some(v);
    }
    if let Some(v) = val("trim-threshold") {
        cfg.trim_threshold = v
            .parse()
            .map_err(|_| "--trim-threshold inválido".to_string())?;
    }
    if let Some(v) = val("trim-mode") {
        cfg.trim_mode =
            TrimMode::parse(v.as_str()).ok_or_else(|| format!("--trim-mode inválido: {v}"))?;
        cfg.enable_trim = cfg.trim_mode != TrimMode::None;
    }
    if let Some(v) = val("trim-margin") {
        cfg.trim_margin = v
            .parse()
            .map_err(|_| "--trim-margin inválido".to_string())?;
    }
    if let Some(v) = val("tolerance") {
        cfg.polygon_tolerance = v.parse().map_err(|_| "--tolerance inválido".to_string())?;
    }

    if let Some(v) = val("color-depth") {
        cfg.color_depth = match v.to_ascii_lowercase().as_str() {
            "rgba8888" => ColorDepth::Rgba8888,
            "rgba4444" => ColorDepth::Rgba4444,
            "rgb565" => ColorDepth::Rgb565,
            _ => return Err(format!("--color-depth inválido: {v}")),
        };
    }
    if let Some(v) = val("dither").or_else(|| val("dither-type")) {
        cfg.dithering_algorithm = match v.to_ascii_lowercase().as_str() {
            "none" => DitheringAlgorithm::None,
            "floyd" | "floydsteinberg" => DitheringAlgorithm::FloydSteinberg,
            "floyd-alpha" | "floydsteinbergalpha" => DitheringAlgorithm::FloydSteinbergAlpha,
            "atkinson" => DitheringAlgorithm::Atkinson,
            "atkinson-alpha" | "atkinsonalpha" => DitheringAlgorithm::AtkinsonAlpha,
            "nn" | "nearestneighbour" | "nearest-neighbor" | "nearest" => {
                DitheringAlgorithm::NearestNeighbour
            }
            "linear" => DitheringAlgorithm::Linear,
            _ => return Err(format!(
                "--dither inválido: {v} (usa none | NearestNeighbour | Linear | FloydSteinberg | FloydSteinbergAlpha | Atkinson | AtkinsonAlpha; alias nn, floyd, atkinson…)"
            )),
        };
    }
    if let Some(v) = val("format") {
        apply_format(&mut cfg, &v)?;
    }
    if let Some(v) = val("texture-format") {
        apply_texture_format(&mut cfg, &v)?;
    }
    if let Some(v) = val("png-opt-level") {
        cfg.png_opt_level = v
            .parse()
            .map_err(|_| "--png-opt-level inválido".to_string())?;
    }
    if let Some(v) = val("png8-dither") {
        cfg.png8_dither = match v.to_ascii_lowercase().as_str() {
            "low" => PngDither::Low,
            "medium" => PngDither::Medium,
            "high" => PngDither::High,
            _ => return Err(format!("--png8-dither inválido: {v}")),
        };
    }
    if let Some(v) = val("jpg-quality") {
        cfg.jpg_quality = v
            .parse()
            .map_err(|_| "--jpg-quality inválido".to_string())?;
    }
    if let Some(v) = val("webp-quality") {
        cfg.webp_quality = v
            .parse()
            .map_err(|_| "--webp-quality inválido".to_string())?;
    }
    if let Some(v) = val("pixel-format").or_else(|| val("opt")) {
        cfg.pixel_format = parse_pixel_format(&v)?;
    }
    if let Some(v) = val("pvr-quality") {
        cfg.pvr_quality = v
            .parse()
            .map_err(|_| "--pvr-quality inválido".to_string())?;
    }
    if let Some(v) = val("etc1-quality") {
        cfg.etc1_quality = v
            .parse()
            .map_err(|_| "--etc1-quality inválido".to_string())?;
    }
    if let Some(v) = val("etc2-quality") {
        cfg.etc2_quality = v
            .parse()
            .map_err(|_| "--etc2-quality inválido".to_string())?;
    }
    if let Some(v) = val("astc-quality") {
        cfg.astc_quality = v
            .parse()
            .map_err(|_| "--astc-quality inválido".to_string())?;
    }
    if let Some(v) = val("basis-quality").or_else(|| val("basisu-quality")) {
        cfg.basis_quality = v
            .parse()
            .map_err(|_| "--basis-quality inválido".to_string())?;
    }
    if let Some(v) = val("dxt-mode") {
        cfg.dxt_mode = DxtMode::parse(&v)
            .ok_or_else(|| format!("--dxt-mode inválido: {v} (DXT_LINEAR | DXT_PERCEPTUAL)"))?;
    }
    check_export_flags(&cfg)?;
    if let Some(v) = val("strategy").or_else(|| val("maxrects-heuristics")) {
        cfg.packing_strategy = PackingStrategy::parse(v.as_str())
            .ok_or_else(|| format!("--strategy inválido: {v}"))?;
    }
    if let Some(v) = val("algorithm") {
        cfg.algorithm = PackingAlgorithm::parse(v.as_str())
            .ok_or_else(|| format!("--algorithm inválido: {v}"))?;
    }
    if let Some(v) = val("basic-sort-by") {
        cfg.basic_sort_by = BasicSortBy::parse(v.as_str())
            .ok_or_else(|| format!("--basic-sort-by inválido: {v}"))?;
    }
    if let Some(v) = val("basic-order") {
        cfg.basic_order =
            SortOrder::parse(v.as_str()).ok_or_else(|| format!("--basic-order inválido: {v}"))?;
    }
    if let Some(v) = val("pack-mode") {
        cfg.pack_mode =
            PackMode::parse(v.as_str()).ok_or_else(|| format!("--pack-mode inválido: {v}"))?;
    }
    if let Some(v) = val("size-constraints") {
        cfg.size_constraints = SizeConstraint::parse(v.as_str())
            .ok_or_else(|| format!("--size-constraints inválido: {v}"))?;
    }
    if let Some(v) = val("width") {
        cfg.fixed_width = v.parse().map_err(|_| "--width inválido".to_string())?;
    }
    if let Some(v) = val("height") {
        cfg.fixed_height = v.parse().map_err(|_| "--height inválido".to_string())?;
    }
    apply_variant_flags(values, &mut cfg)?;

    if let Some(v) = val("variants") {
        let parsed: Vec<f32> = v
            .split([',', ';', ' '])
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| s.trim().parse().ok())
            .filter(|x| *x > 0.0 && *x <= 8.0)
            .collect();
        if !parsed.is_empty() {
            cfg.scale_variants = parsed;
        }
    }
    // `--scale` va después de --variants/--variant: multiplica lo que haya.
    if let Some(v) = val("scale") {
        let factor: f32 = v
            .parse()
            .map_err(|_| format!("--scale inválido: {v} (número en (0, 8])"))?;
        if !(factor > 0.0 && factor <= 8.0) {
            return Err(format!("--scale inválido: {v} (número en (0, 8])"));
        }
        apply_scale_factor(&mut cfg, factor);
    }
    if let Some(v) = val("template-format") {
        apply_template_format(&mut cfg, &v)?;
    }
    if let Some(v) = val("template") {
        cfg.export_template = Some(PathBuf::from(v));
    }
    if let Some(v) = val("class-file") {
        cfg.class_file = v;
    }
    if let Some(v) = val("header-file") {
        cfg.header_file = v;
    }
    if let Some(v) = val("source-file") {
        cfg.source_file = v;
    }
    if let Some(v) = val("spriteids-file") {
        cfg.spriteids_file = v;
    }
    if let Some(v) = val("gdx-filter") {
        cfg.gdx_filter = GdxFilter::parse(&v)
            .ok_or_else(|| format!("--gdx-filter inválido: {v} (linear | nearest)"))?;
    }
    if let Some(v) = val("key") {
        cfg.encryption_key = if v.is_empty() { None } else { Some(v) };
    }
    if let Some(v) = val("key-name") {
        cfg.encryption_key_name = if v.trim().is_empty() { None } else { Some(v) };
    }
    if let Some(v) = val("save-key") {
        let key = cfg
            .encryption_key
            .clone()
            .ok_or_else(|| "--save-key requiere --key CLAVE".to_string())?;
        tp_core::keys::put(&v, &key).map_err(|e| format!("No se pudo guardar la clave: {e}"))?;
        cfg.encryption_key_name = Some(v);
    }
    if let Some(v) = val("normalmap-suffix") {
        cfg.normal_map_suffix = v;
    }
    if let Some(v) = val("normalmap-filter") {
        cfg.normal_map_filter = v;
    }
    if let Some(v) = val("normalmap-sheet") {
        cfg.normal_map_sheet = v;
    }

    // Opciones booleanas.
    apply_flag_options(&mut cfg, flags);

    Ok(PackSetup {
        cfg,
        data_path,
        path_warning,
    })
}

/// Aviso de la numeración `{n}` que pide `--sheet` cuando `--data` fija el
/// nombre base.
fn announce_path_warning(path_warning: Option<&str>, quiet: bool) {
    if let Some(msg) = path_warning {
        if !quiet {
            println!("⚠ {}", tp_i18n::tr(msg));
        }
    }
}

/// Acciones que terminan sin empaquetar: `--convert-texture` y `--save`.
/// Devuelve `true` si alguna de las dos ha actuado (o ha fallado).
fn run_no_pack_actions(
    cfg: &ProjectConfig,
    values: &[(String, String)],
    quiet: bool,
) -> CmdResult<bool> {
    let val = |k: &str| values.iter().rfind(|(v, _)| v == k).map(|(_, v)| v.clone());
    // `--convert-texture` convierte una imagen y termina, sin empaquetar.
    if let Some(v) = val("convert-texture") {
        let msg = convert_texture(cfg, Path::new(&v))?;
        if !quiet {
            println!("{}", tp_i18n::tr(&msg));
        }
        return Ok(true);
    }
    // `--save FICHERO`: vuelca la configuración ya montada a un .tpproj y
    // termina, para poder guardar una línea de comandos y reutilizarla.
    if let Some(v) = val("save") {
        let msg = save_project(cfg, Path::new(&v))?;
        if !quiet {
            println!("{}", tp_i18n::tr(&msg));
        }
        return Ok(true);
    }

    Ok(false)
}

/// Validaciones finales antes de empaquetar: la extensión de `--data` y las
/// carpetas obligatorias.
fn validate_pack_setup(cfg: &ProjectConfig, data_path: Option<&PathBuf>) -> CmdResult<()> {
    if let Some(dp) = data_path {
        check_data_extension(cfg, dp)?;
    }

    if cfg.input_directory.as_os_str().is_empty() {
        return Err(
            "Falta --input DIR (o pasa los sprites en posiciónles o un .tpproj/.tps)".into(),
        );
    }
    if cfg.output_directory.as_os_str().is_empty() {
        return Err("Falta --output DIR, --sheet/--data (o un .tpproj/.tps)".into());
    }

    Ok(())
}

/// Resumen de `--verbose` con lo que se va a empaquetar (lo calla `--quiet`).
fn announce_pack_plan(cfg: &ProjectConfig, flags: &[String], quiet: bool) {
    let has = |k: &str| flags.iter().any(|f| f == k);
    if has("verbose") && !quiet {
        let data_desc = if cfg.data_format.is_empty() {
            format!("{:?}", cfg.template_format)
        } else {
            cfg.data_format.clone()
        };
        println!(
            "{}",
            tp_i18n::tr(&format!(
                "· entrada: {} · salida: {} · nombre base: {}",
                cfg.input_directory.display(),
                cfg.output_directory.display(),
                cfg.base_file_name
            ))
        );
        println!(
            "{}",
            tp_i18n::tr(&format!(
                "· textura: {} · píxeles: {} · datos: {} · hasta {} px · padding {} + {}",
                cfg.gpu_format.as_str(),
                cfg.pixel_format.as_str(),
                data_desc,
                cfg.max_texture_size,
                cfg.padding,
                cfg.border_padding
            ))
        );
    }
}

/// Corrida del pipeline y resumen en stdout (más `--print-json`).
fn run_pack_pipeline(cfg: &ProjectConfig, flags: &[String], quiet: bool) -> CmdResult<()> {
    let has = |k: &str| flags.iter().any(|f| f == k);
    let started = std::time::Instant::now();
    // Pack por carpetas: si hay grupos con nombre y sprites, cada grupo
    // escribe su hoja en `<output>/<grupo>/`.
    let grouped = cfg
        .folder_groups
        .iter()
        .any(|g| !g.name.is_empty() && !g.sprites.is_empty());
    let out = if grouped {
        tp_core::pipeline::run_grouped(cfg)
    } else {
        tp_core::pipeline::run(cfg)
    }
    .map_err(|e| format!("Empaquetado fallido: {e}"))?;
    let result = &out.result;

    if !quiet {
        println!(
            "{}",
            tp_i18n::tr(&format!(
                "✔ Empaquetado en {} ms: {} sprites ({} aliases), {} página(s)",
                started.elapsed().as_millis(),
                result.total_sprites,
                result.alias_count,
                result.pages.len()
            ))
        );
        for w in &result.warnings {
            println!("⚠ {}", tp_i18n::tr(w));
        }
        for f in &result.output_files {
            println!("  → {f}");
        }
        for (stage, ms) in &result.stage_times_ms {
            println!("  [{stage}] {ms} ms");
        }
    }
    if has("print-json") {
        // Render the base-scale JSON metadata and print it.
        let ctx = tp_core::templates::build_context(
            result,
            &result.pages,
            &result
                .pages
                .iter()
                .map(|p| p.file_name.clone())
                .collect::<Vec<_>>(),
            1.0,
            result.config.effective_trim_mode(),
        );
        println!("{}", serde_json::to_string_pretty(&ctx).unwrap());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::apply_scale_factor;
    use tp_core::config::GpuFormat;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// M22: `--input a --input b` se quedaba con `a` (first-wins silencioso).
    /// Los lookups de valor leen ahora la última aparición, como en
    /// cualquier otro CLI.
    #[test]
    fn con_input_o_output_repetido_gana_la_ultima() {
        let (_, values, _) = parse_args(&args(&[
            "--input", "a", "--output", "b", "--input", "c", "--output", "d",
        ]));
        let setup = build_pack_config(&[], &values, &[], true).expect("config");
        assert_eq!(setup.cfg.input_directory, PathBuf::from("c"));
        assert_eq!(setup.cfg.output_directory, PathBuf::from("d"));
    }

    #[test]
    fn un_tps_del_original_se_carga_como_proyecto() {
        let dir = std::env::temp_dir().join(format!("tpcli_tps_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp");
        let path = dir.join("proj.tps");
        let cfg = ProjectConfig {
            input_directory: PathBuf::from("sprites"),
            ..ProjectConfig::default()
        };
        tp_core::tps::save_tps(&cfg, &path).expect("escribir");
        let loaded = load_project_arg(&path, true).expect("cargar");
        // Las rutas relativas del .tps se resuelven contra su carpeta.
        assert_eq!(loaded.input_directory, dir.join("sprites"));
        // Y se puede volver a escribir en el mismo formato.
        let saved = save_project(&loaded, &dir.join("guardado.tps")).expect("guardar");
        assert!(saved.contains("guardado.tps"), "mensaje: {saved}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scale_flag_multiplies_every_variant() {
        // Sin variantes declaradas: una única escala y sufijo vacío, así
        // que los nombres de los ficheros no cambian.
        let mut cfg = ProjectConfig::default();
        apply_scale_factor(&mut cfg, 0.5);
        assert_eq!(cfg.scale_variants, vec![0.5]);
        assert_eq!(cfg.variant_names, vec![(0.5, String::new())]);
        cfg.validate().unwrap();

        // Con variantes y nombres: las escalas se multiplican y los
        // nombres siguen pegados a su variante.
        let mut cfg = ProjectConfig {
            scale_variants: vec![1.0, 0.5],
            variant_names: vec![(1.0, "-ipadhd".to_string()), (0.5, "-hd".to_string())],
            ..ProjectConfig::default()
        };
        apply_scale_factor(&mut cfg, 2.0);
        assert_eq!(cfg.scale_variants, vec![2.0, 1.0]);
        assert_eq!(
            cfg.variant_names,
            vec![(2.0, "-ipadhd".to_string()), (1.0, "-hd".to_string())]
        );
        cfg.validate().unwrap();

        // El factor llega desde la línea de comandos: --scale sólo
        // multiplica, y la escala resultante sigue siendo válida.
        let out = std::env::temp_dir().join("tpcli_save_scale.tpproj");
        let _ = std::fs::remove_file(&out);
        cmd_pack(&args(&[
            "--input",
            "sprites",
            "--output",
            "build",
            "--variants",
            "1,0.5",
            "--scale",
            "0.5",
            "--save",
            out.to_str().unwrap(),
        ]))
        .unwrap();
        let cfg = ProjectConfig::from_toml(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(cfg.scale_variants, vec![0.5, 0.25]);
        cfg.validate().unwrap();
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn save_writes_a_project_and_skips_the_packing() {
        let out = std::env::temp_dir().join("tpcli_save_phase_c.tpproj");
        let _ = std::fs::remove_file(&out);
        cmd_pack(&args(&[
            "--input",
            "sprites",
            "--output",
            "build",
            "--max-width",
            "2048",
            "--background-color",
            "112233",
            "--ignore-files",
            "*.psd",
            "--save",
            out.to_str().unwrap(),
        ]))
        .unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        let cfg = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(cfg.max_width, 2048);
        assert_eq!(cfg.background_color, Some([0x11, 0x22, 0x33, 0xff]));
        assert_eq!(cfg.input_directory, PathBuf::from("sprites"));
        assert_eq!(cfg.output_directory, PathBuf::from("build"));
        assert_eq!(cfg.ignore_patterns, vec!["*.psd"]);
        let _ = std::fs::remove_file(&out);

        // La extensión distinta de .tpproj se guarda igual, con aviso.
        let odd = std::env::temp_dir().join("tpcli_save_phase_c.toml");
        let _ = std::fs::remove_file(&odd);
        cmd_pack(&args(&[
            "--input",
            "sprites",
            "--output",
            "build",
            "--save",
            odd.to_str().unwrap(),
        ]))
        .unwrap();
        assert!(odd.exists());
        let _ = std::fs::remove_file(&odd);
    }

    #[test]
    fn convert_texture_writes_the_requested_format() {
        use image::ImageEncoder;

        let src = std::env::temp_dir().join("tpcli_convert_src.png");
        let rgba = [255u8, 0, 0, 255, 0, 0, 255, 255];
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(
            &mut png,
            image::codecs::png::CompressionType::Default,
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(&rgba, 2, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
        std::fs::write(&src, &png).unwrap();

        // Mismo directorio que la entrada y otro formato.
        let cfg = ProjectConfig {
            gpu_format: GpuFormat::Jpg,
            ..ProjectConfig::default()
        };
        let msg = convert_texture(&cfg, &src).unwrap();
        assert!(msg.contains("Convertido"), "{msg}");
        let jpg = src.with_extension("jpg");
        let bytes = std::fs::read(&jpg).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "no es un JPEG");

        // Con --scale se amplía antes de codificar, y --output dirige dónde.
        let out_dir = std::env::temp_dir().join("tpcli_convert_out");
        let _ = std::fs::remove_dir_all(&out_dir);
        let cfg = ProjectConfig {
            gpu_format: GpuFormat::Png,
            scale_variants: vec![2.0],
            output_directory: out_dir.clone(),
            ..ProjectConfig::default()
        };
        convert_texture(&cfg, &src).unwrap();
        let scaled = out_dir.join("tpcli_convert_src.png");
        let img = image::load_from_memory(&std::fs::read(&scaled).unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (4, 2));

        // Un fichero que no existe se rechaza con el nombre de la opción.
        let err = convert_texture(&cfg, &std::env::temp_dir().join("no_esta.png")).unwrap_err();
        assert!(err.contains("--convert-texture"), "{err}");

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&jpg);
        let _ = std::fs::remove_dir_all(&out_dir);
    }

    /// C3: la CLI debe resolver las rutas relativas del `.tpproj` contra la
    /// carpeta del propio proyecto, igual que hace la GUI; de otro modo el
    /// mismo comando buscaría sprites distintos según el directorio desde el
    /// que se lance.
    #[test]
    fn las_rutas_relativas_del_tpproj_se_resuelven_contra_su_carpeta() {
        let tmp = std::env::temp_dir().join(format!("tp_cli_rel_{}", std::process::id()));
        let sprites = tmp.join("sprites");
        std::fs::create_dir_all(&sprites).expect("carpeta de sprites");
        let proyecto = tmp.join("mi.tpproj");

        let cfg = ProjectConfig {
            input_directory: sprites.clone(),
            output_directory: tmp.join("out"),
            ..ProjectConfig::default()
        };
        let texto = cfg.to_toml().expect("serializa el proyecto");
        let texto = texto
            .replace(&sprites.display().to_string(), "sprites")
            .replace(&tmp.join("out").display().to_string(), "out");
        assert!(
            texto.contains("input_directory = \"sprites\""),
            "el paso a relativo no cuadró: {texto}"
        );
        std::fs::write(&proyecto, texto).expect("escribe el .tpproj");

        let cargado = load_project_arg(&proyecto, true).expect("el proyecto debe cargar");

        assert_eq!(
            cargado.input_directory, sprites,
            "la ruta relativa debe resolverse contra la carpeta del .tpproj"
        );
        assert_eq!(cargado.output_directory, tmp.join("out"));
        std::fs::remove_dir_all(&tmp).ok();
    }
}
