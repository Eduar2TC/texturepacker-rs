//! TexturePacker-RS command-line interface.
//!
//! ```
//! tp-cli pack project.tpproj
//! tp-cli pack --input sprites/ --output build/ --max-size 4096 --format astc
//! tp-cli decrypt atlas_0.png.tpenc --key secreto -o atlas.png
//! ```

use std::path::PathBuf;
use tp_core::config::{
    AlphaHandling, BasicSortBy, ColorDepth, DitheringAlgorithm, DxtMode, GpuFormat, PackMode,
    PackingAlgorithm, PackingStrategy, PixelFormat, PngDither, ProjectConfig, ScaleMode,
    SizeConstraint, SortOrder, TemplateFormat, TrimMode,
};

fn usage() -> ! {
    println!(
        "TexturePacker-RS {}\n\
         \n\
         USO:\n\
         \x20 tp-cli pack <proyecto.tpproj>\n\
         \x20 tp-cli pack --input DIR --output DIR [opciones]\n\
         \x20 tp-cli decrypt <archivo.tpenc> --key CLAVE -o salida.png [--pixel-format F]\n\
         \n\
         OPCIONES de pack:\n\
         \x20 --max-size N          Tamaño máximo del atlas (512..8192, potencia de 2)\n\
         \x20 --shape-padding N    Espacio entre sprites (px); alias: --padding\n\
         \x20 --padding N           Combinación de --shape-padding y --border-padding\n\
         \x20 --border-padding N    Margen entre los sprites y el borde (px)\n\
         \x20 --align-to-grid N     Alinea las esquinas de los sprites a N px (0 = off)\n\
         \x20 --extrude N           Extrude de bordes (px)\n\
         \x20 --no-rotation         Desactivar rotación 90°\n\
         \x20 --trim-threshold N    Umbral de alpha para recorte (1-255, por defecto 1)\n\
         \x20 --trim-margin N       Margen transparente tras el recorte (px)\n\
         \x20 --keep-extension       Los nombres de sprite conservan la extensión\n\
         \x20 --prepend-folder-name Antepone el nombre de la carpeta inteligente\n\
         \x20 --polygon             Modo polígono (mallas + empaquetado por contorno)\n\
         \x20 --no-aliasing         Desactivar deduplicación por hash (alias: --disable-auto-alias)\n\
         \x20 --no-auto-animations  No agrupar sprites walk_001..N como animaciones\n\
         \x20 --color-depth T       RGBA8888 | RGBA4444 | RGB565\n\
         \x20 --dither T            none | nn | linear | floyd | floyd-alpha | atkinson | atkinson-alpha\n\
         \x20 --alpha-handling T    keep | clear | bleed | premultiply\n\
         \x20 --scale-mode T        smooth | fast | scale2x | scale3x | scale4x | eagle\n\
         \x20 --texture-path RUTA   Prefijo de la textura en los metadatos (p. ej. /assets)\n\
         \x20 --format T            png | png8 | jpg | webp | bmp | tga | tiff | dds\n\
         \x20                        zktx | pvr3gz | pvr3ccz | pkm | ktx | astc | etc2 | pvrtc\n\
         \x20 --png-opt-level N     Optimización PNG sin pérdida, 0-7 (1 = indexa si ≤256 colores)\n\
         \x20 --png8-dither T       Dithering PNG-8: low | medium | high\n\
         \x20 --jpg-quality N       Calidad JPG (0-100)\n\
         \x20 --pixel-format T      rgba8888 | rgb888 | alpha8 | intensity8 | alpha-intensity8 | rgba5551 | rgba5555 | bgra8888\n\
         \x20                        rgba4444 | rgb565 | pvrtc2bpp-rgba | pvrtc4bpp-rgba | pvrtc2bpp-rgb | pvrtc4bpp-rgb\n\
         \x20                        etc1 | etc2 | etc2-rgb | dxt1 | dxt5 | astc-4x4 | astc-8x8 | astc-12x12\n\
         \x20 --pvr-quality N       Calidad PVRTC 0-7 (defecto 3)\n\
         \x20 --etc1-quality N      Calidad ETC1 0-100 (defecto 70)\n\
         \x20 --etc2-quality N      Calidad ETC2 0-100 (defecto 70)\n\
         \x20 --astc-quality N      Calidad ASTC 0-4: 0=fastest .. 4=exhaustive (defecto 2)\n\
         \x20 --dxt-mode T          DXT_LINEAR (error uniforme) | DXT_PERCEPTUAL (ponderado)\n\
         \x20 --strategy T          bssf (ShortSideFit) | baf (AreaFit) | blsf (LongSideFit) | best | bottom-left | contact-point | guillotine (alias: --maxrects-heuristics)\n\
         \x20 --algorithm T         maxrects | polygon | guillotine | grid | basic | manual\n\
         \x20 --auto-folders        pack por carpetas automático: cada subcarpeta de entrada produce su hoja en la subcarpeta de salida\n\
         \x20 --basic-sort-by T     best | name | width | height | area | circumference\n\
         \x20 --basic-order T       ascending | descending\n\
         \x20 --pack-mode T         fast | good | best\n\
         \x20 --size-constraints T  any | pot | multiple-of-4 | word-aligned\n\
         \x20 --force-squared       Atlas cuadrado\n\
         \x20 --width N             Ancho fijo del atlas (0 = automático)\n\
         \x20 --variant E:NOMBRE    Variante con nombre, p.ej. 1.0:-ipadhd,0.5:-hd\n\
         \x20 --variants LIST       Escalas, p.ej. 2,0.5 (sufijos @2x, -hd)\n\
         \x20 --template-format T   json | xml | plist | cpp | tsv | text\n\
         \x20 --class-file F       Fichero de clase Swift extra (spritekit-swift)\n\
         \x20 --header-file F      Cabecera C++/ObjC extra (cocos2d-x)\n\
         \x20 --source-file F      Código fuente C++ extra (cocos2d-x)\n\
         \x20 --spriteids-file F   Lista de ids de sprites extra (amethyst)\n\
         \x20 --key CLAVE           Cifrar texturas con AES-256-GCM\n\
         \x20 --no-normals          No empaquetar mapas de normales\n\
         \x20 --normalmap-suffix T  Sufijo del mapa de normales (defecto _normal)\n\
         \x20 --normalmap-filter T  Ficheros con esta subcadena en la ruta son normales\n\
         \x20 --normalmap-sheet N   Nombre base de la hoja de normales (defecto <imagen>_normal)\n\
         \x20 --normalmap-detect    Detectar mapas de normales por su color\n\
         \x20 --no-recursive        No buscar en subdirectorios\n\
         \x20 --base-name NOMBRE    Nombre base de la salida (admite {{n}} {{n1}} {{v}})\n\
         \x20 --no-multipack        No emitir varias hojas (falla si no caben en una)\n\
         \x20 --multipack           Permitir varias hojas (anula --no-multipack)\n\
         \x20 --print-json          Imprimir los metadatos JSON en stdout",
        env!("CARGO_PKG_VERSION")
    );
    std::process::exit(0);
}

fn fail(msg: String) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        usage();
    }
    match args[0].as_str() {
        "pack" => cmd_pack(&args[1..]),
        "decrypt" => cmd_decrypt(&args[1..]),
        other => fail(format!("Comando desconocido: {other}")),
    }
}

fn parse_args(args: &[String]) -> (Option<PathBuf>, Vec<(String, String)>, Vec<String>) {
    let mut flags = Vec::new();
    let mut values = Vec::new();
    let mut positional = Vec::new();
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        // `--flag valor` solo cuando el siguiente argumento no es otra opción
        // (así los booleanos como --no-rotation pueden encadenarse).
        if let Some(v) = a.strip_prefix("--") {
            match it.peek() {
                Some(next) if !next.starts_with('-') => {
                    let val = it.next().unwrap().clone();
                    values.push((v.to_string(), val));
                }
                _ => flags.push(v.to_string()),
            }
        } else if let Some(v) = a.strip_prefix('-') {
            // single-dash option like -o
            match it.peek() {
                Some(next) if !next.starts_with('-') => {
                    let val = it.next().unwrap().clone();
                    values.push((v.to_string(), val));
                }
                _ => flags.push(v.to_string()),
            }
        } else {
            positional.push(a.clone());
        }
    }
    let project = positional.first().map(PathBuf::from);
    (project, values, flags)
}

fn cmd_pack(args: &[String]) {
    let (project, values, flags) = parse_args(args);
    let has = |k: &str| values.iter().any(|(v, _)| v == k);
    let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());

    let mut cfg = ProjectConfig::default();

    if let Some(p) = &project {
        let text = std::fs::read_to_string(p)
            .unwrap_or_else(|e| fail(format!("No se pudo leer {}: {e}", p.display())));
        cfg = ProjectConfig::from_toml(&text)
            .unwrap_or_else(|e| fail(format!("Proyecto inválido {}: {e}", p.display())));
    }
    if let Some(v) = val("input") {
        cfg.input_directory = PathBuf::from(v);
    }
    if let Some(v) = val("output") {
        cfg.output_directory = PathBuf::from(v);
    }
    if let Some(v) = val("max-size") {
        cfg.max_texture_size = v
            .parse()
            .unwrap_or_else(|_| fail("--max-size inválido".into()));
    }
    // `--shape-padding` es el nombre canónico; `--padding` queda
    // como alias corto.
    if let Some(v) = val("shape-padding").or_else(|| val("padding")) {
        cfg.padding = v
            .parse()
            .unwrap_or_else(|_| fail("--shape-padding inválido".into()));
    }
    if let Some(v) = val("extrude") {
        cfg.extrude = v
            .parse()
            .unwrap_or_else(|_| fail("--extrude inválido".into()));
    }
    if let Some(v) = val("border-padding") {
        cfg.border_padding = v
            .parse()
            .unwrap_or_else(|_| fail("--border-padding inválido".into()));
    }
    if let Some(v) = val("common-divisor") {
        let d: i32 = v
            .parse()
            .unwrap_or_else(|_| fail("--common-divisor inválido".into()));
        cfg.common_divisor_x = d;
        cfg.common_divisor_y = d;
    }
    if let Some(v) = val("align-to-grid").or_else(|| val("align")) {
        cfg.align_to_grid = v
            .parse()
            .unwrap_or_else(|_| fail("--align-to-grid inválido".into()));
    }
    if let Some(v) = val("alpha-handling") {
        cfg.alpha_handling = AlphaHandling::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--alpha-handling inválido: {v}")));
    }
    if let Some(v) = val("scale-mode") {
        cfg.scale_mode = ScaleMode::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--scale-mode inválido: {v}")));
    }
    if let Some(v) = val("texture-path") {
        cfg.texture_path = Some(v);
    }
    if let Some(v) = val("trim-threshold") {
        cfg.trim_threshold = v
            .parse()
            .unwrap_or_else(|_| fail("--trim-threshold inválido".into()));
    }
    if let Some(v) = val("trim-mode") {
        cfg.trim_mode = TrimMode::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--trim-mode inválido: {v}")));
        cfg.enable_trim = cfg.trim_mode != TrimMode::None;
    }
    if let Some(v) = val("trim-margin") {
        cfg.trim_margin = v
            .parse()
            .unwrap_or_else(|_| fail("--trim-margin inválido".into()));
    }
    if let Some(v) = val("tolerance") {
        cfg.polygon_tolerance = v
            .parse()
            .unwrap_or_else(|_| fail("--tolerance inválido".into()));
    }
    if let Some(v) = val("color-depth") {
        cfg.color_depth = match v.to_ascii_lowercase().as_str() {
            "rgba8888" => ColorDepth::Rgba8888,
            "rgba4444" => ColorDepth::Rgba4444,
            "rgb565" => ColorDepth::Rgb565,
            _ => fail(format!("--color-depth inválido: {v}")),
        };
    }
    if let Some(v) = val("dither") {
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
            _ => fail(format!(
                "--dither inválido: {v} (usa none | NearestNeighbour | Linear | FloydSteinberg | FloydSteinbergAlpha | Atkinson | AtkinsonAlpha; alias nn, floyd, atkinson…)"
            )),
        };
    }
    if let Some(v) = val("format") {
        cfg.gpu_format = GpuFormat::parse(&v).unwrap_or_else(|| {
            fail(format!(
                "--format inválido: {v} (usa png | png8 | jpg | webp | bmp | tga | tiff | \
                 dds | zktx | pvr3gz | pvr3ccz | pkm | ktx | astc | etc2 | pvrtc)"
            ))
        });
    }
    if let Some(v) = val("png-opt-level") {
        cfg.png_opt_level = v
            .parse()
            .unwrap_or_else(|_| fail("--png-opt-level inválido".into()));
    }
    if let Some(v) = val("png8-dither") {
        cfg.png8_dither = match v.to_ascii_lowercase().as_str() {
            "low" => PngDither::Low,
            "medium" => PngDither::Medium,
            "high" => PngDither::High,
            _ => fail(format!("--png8-dither inválido: {v}")),
        };
    }
    if let Some(v) = val("jpg-quality") {
        cfg.jpg_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--jpg-quality inválido".into()));
    }
    if let Some(v) = val("webp-quality") {
        cfg.webp_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--webp-quality inválido".into()));
    }
    if let Some(v) = val("pixel-format") {
        cfg.pixel_format = parse_pixel_format(&v);
    }
    if let Some(v) = val("pvr-quality") {
        cfg.pvr_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--pvr-quality inválido".into()));
    }
    if let Some(v) = val("etc1-quality") {
        cfg.etc1_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--etc1-quality inválido".into()));
    }
    if let Some(v) = val("etc2-quality") {
        cfg.etc2_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--etc2-quality inválido".into()));
    }
    if let Some(v) = val("astc-quality") {
        cfg.astc_quality = v
            .parse()
            .unwrap_or_else(|_| fail("--astc-quality inválido".into()));
    }
    if let Some(v) = val("dxt-mode") {
        cfg.dxt_mode = DxtMode::parse(&v).unwrap_or_else(|| {
            fail(format!(
                "--dxt-mode inválido: {v} (DXT_LINEAR | DXT_PERCEPTUAL)"
            ))
        });
    }
    if let Err(e) = check_export_flags(&cfg) {
        fail(e);
    }
    if let Some(v) = val("strategy").or_else(|| val("maxrects-heuristics")) {
        cfg.packing_strategy = PackingStrategy::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--strategy inválido: {v}")));
    }
    if let Some(v) = val("algorithm") {
        cfg.algorithm = PackingAlgorithm::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--algorithm inválido: {v}")));
    }
    if let Some(v) = val("basic-sort-by") {
        cfg.basic_sort_by = BasicSortBy::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--basic-sort-by inválido: {v}")));
    }
    if let Some(v) = val("basic-order") {
        cfg.basic_order = SortOrder::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--basic-order inválido: {v}")));
    }
    if let Some(v) = val("pack-mode") {
        cfg.pack_mode = PackMode::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--pack-mode inválido: {v}")));
    }
    if let Some(v) = val("size-constraints") {
        cfg.size_constraints = SizeConstraint::parse(v.as_str())
            .unwrap_or_else(|| fail(format!("--size-constraints inválido: {v}")));
    }
    if let Some(v) = val("width") {
        cfg.fixed_width = v
            .parse()
            .unwrap_or_else(|_| fail("--width inválido".into()));
    }
    if let Some(v) = val("height") {
        cfg.fixed_height = v
            .parse()
            .unwrap_or_else(|_| fail("--height inválido".into()));
    }
    // --variant <escala>[:<nombre>]; acepta varios separados por
    // coma. El nombre se usa tal cual como sufijo {v}.
    if let Some(v) = val("variant") {
        let mut names: Vec<(f32, String)> = Vec::new();
        for part in v.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let mut fields = part.split(':');
            let scale: f32 = fields
                .next()
                .unwrap_or_default()
                .trim()
                .parse()
                .unwrap_or_else(|_| fail(format!("--variant inválido: {part}")));
            if !(0.0..=8.0).contains(&scale) {
                fail(format!(
                    "--variant inválido (escala fuera de rango): {part}"
                ));
            }
            let name = fields.next().unwrap_or("").trim().to_string();
            names.push((scale, name));
        }
        if !names.is_empty() {
            cfg.variant_names = names;
        }
    }
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
    if let Some(v) = val("template-format") {
        cfg.template_format = match v.to_ascii_lowercase().as_str() {
            "json" => TemplateFormat::Json,
            "xml" => TemplateFormat::Xml,
            "plist" => TemplateFormat::Plist,
            "cpp" => TemplateFormat::CppHeader,
            "tsv" => TemplateFormat::Tsv,
            "text" => TemplateFormat::PlainText,
            _ => fail(format!("--template-format inválido: {v}")),
        };
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
    if let Some(v) = val("key") {
        cfg.encryption_key = if v.is_empty() { None } else { Some(v) };
    }
    if let Some(v) = val("base-name") {
        cfg.base_file_name = v;
    }
    if flags.iter().any(|f| f == "no-rotation") {
        cfg.allow_rotation = false;
    }
    if flags.iter().any(|f| f == "no-trim") {
        cfg.enable_trim = false;
    }
    if flags.iter().any(|f| f == "polygon") {
        cfg.enable_polygon = true;
    }
    if flags
        .iter()
        .any(|f| f == "no-aliasing" || f == "disable-auto-alias")
    {
        cfg.enable_aliasing = false;
    }
    if flags.iter().any(|f| f == "no-normals") {
        cfg.enable_normal_maps = false;
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
    if flags.iter().any(|f| f == "normalmap-detect") {
        cfg.normal_map_auto_detect = true;
    }
    if flags.iter().any(|f| f == "keep-extension") {
        cfg.trim_sprite_names = false;
    }
    if flags.iter().any(|f| f == "prepend-folder-name") {
        cfg.prepend_folder_name = true;
    }
    if flags.iter().any(|f| f == "no-recursive") {
        cfg.recursive = false;
    }
    if flags.iter().any(|f| f == "force-squared") {
        cfg.force_squared = true;
    }
    if flags.iter().any(|f| f == "no-multipack") {
        cfg.multipack = false;
    }
    if flags.iter().any(|f| f == "auto-folders") {
        cfg.auto_folder_groups = true;
    }
    if flags.iter().any(|f| f == "multipack") {
        cfg.multipack = true;
    }
    if flags.iter().any(|f| f == "flip-y" || f == "flip-vertical") {
        cfg.flip_vertical = true;
    }

    if cfg.input_directory.as_os_str().is_empty() {
        fail("Falta --input DIR (o pasa un archivo .tpproj)".into());
    }
    if cfg.output_directory.as_os_str().is_empty() {
        fail("Falta --output DIR (o pasa un archivo .tpproj)".into());
    }

    let started = std::time::Instant::now();
    // Pack por carpetas: si hay grupos con nombre y sprites, cada grupo
    // escribe su hoja en `<output>/<grupo>/`.
    let grouped = cfg
        .folder_groups
        .iter()
        .any(|g| !g.name.is_empty() && !g.sprites.is_empty());
    let out = if grouped {
        tp_core::pipeline::run_grouped(&cfg)
    } else {
        tp_core::pipeline::run(&cfg)
    }
    .unwrap_or_else(|e| fail(format!("Empaquetado fallido: {e}")));
    let result = &out.result;

    println!(
        "✔ Empaquetado en {} ms: {} sprites ({} aliases), {} página(s)",
        started.elapsed().as_millis(),
        result.total_sprites,
        result.alias_count,
        result.pages.len()
    );
    for w in &result.warnings {
        println!("⚠ {w}");
    }
    for f in &result.output_files {
        println!("  → {f}");
    }
    for (stage, ms) in &result.stage_times_ms {
        println!("  [{stage}] {ms} ms");
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
}

fn cmd_decrypt(args: &[String]) {
    let (file, values, flags) = parse_args(args);
    let Some(file) = file else {
        fail("decrypt necesita un archivo .tpenc".into());
    };
    let Some(key) = values.iter().find(|(v, _)| v == "key").map(|(_, v)| v) else {
        fail("decrypt necesita --key CLAVE".into());
    };
    let out_path = values
        .iter()
        .find(|(v, _)| v == "o" || v == "out")
        .map(|(_, v)| PathBuf::from(v))
        .unwrap_or_else(|| {
            let mut p = file.clone();
            p.set_extension("dec.png");
            p
        });
    let data = std::fs::read(&file)
        .unwrap_or_else(|e| fail(format!("No se pudo leer {}: {e}", file.display())));
    let plain = tp_core::export::decrypt_bytes(&data, key)
        .unwrap_or_else(|e| fail(format!("Descifrado fallido: {e}")));
    let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.as_str());

    // Vista previa: con `--pixel-format` se re-aplica la conversión del atlas
    // (BGRA8888 y demás) para que la imagen se vea con los colores correctos.
    let preview = match val("pixel-format") {
        Some(pf) => tp_core::export::decode_texture_preview_png(&plain, parse_pixel_format(pf))
            .unwrap_or_else(|e| fail(format!("No se pudo generar la vista previa: {e}"))),
        None => plain,
    };

    std::fs::write(&out_path, &preview)
        .unwrap_or_else(|e| fail(format!("No se pudo escribir {}: {e}", out_path.display())));
    let _ = flags;
    match val("pixel-format") {
        Some(pf) => println!(
            "✔ Descifrado + vista previa ({}): {}",
            pf.to_ascii_uppercase(),
            out_path.display()
        ),
        None => println!("✔ Descifrado: {}", out_path.display()),
    }
}

/// Reglas de exportación: rangos de calidades y que el pixel format elegido
/// sea soportado por el formato de textura. Devuelve el mensaje de error.
fn check_export_flags(cfg: &ProjectConfig) -> Result<(), String> {
    if cfg.pvr_quality > 7 {
        return Err(format!(
            "--pvr-quality fuera de rango (0-7): {}",
            cfg.pvr_quality
        ));
    }
    if cfg.etc1_quality > 100 {
        return Err(format!(
            "--etc1-quality fuera de rango (0-100): {}",
            cfg.etc1_quality
        ));
    }
    if cfg.etc2_quality > 100 {
        return Err(format!(
            "--etc2-quality fuera de rango (0-100): {}",
            cfg.etc2_quality
        ));
    }
    if cfg.astc_quality > 4 {
        return Err(format!(
            "--astc-quality fuera de rango (0-4): {}",
            cfg.astc_quality
        ));
    }
    if !cfg.pixel_format.is_compatible_with(cfg.gpu_format) {
        return Err(format!(
            "pixel format {} no es soportado por --format {}",
            cfg.pixel_format.as_str(),
            cfg.gpu_format.as_str()
        ));
    }
    Ok(())
}

/// Parseo compartido del flag `--pixel-format` (pack y decrypt).
/// Mayúsculas/minúsculas y `-`/`_` indiferentes: se normaliza antes de casar.
fn parse_pixel_format(v: &str) -> PixelFormat {
    let norm: String = v
        .to_ascii_lowercase()
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .collect();
    match norm.as_str() {
        "rgba8888" => PixelFormat::Rgba8888,
        "rgb888" => PixelFormat::Rgb888, // compone sobre negro al decodificar
        "alpha8" => PixelFormat::Alpha8, // nivel de alfa → gris
        "intensity8" => PixelFormat::Intensity8,
        "alphaintensity8" => PixelFormat::AlphaIntensity8,
        "rgba5551" | "5551" => PixelFormat::Rgba5551,
        "rgba5555" | "5555" => PixelFormat::Rgba5555,
        "bgra8888" => PixelFormat::Bgra8888,
        "rgba4444" | "4444" => PixelFormat::Rgba4444,
        "rgb565" | "565" => PixelFormat::Rgb565,
        "pvrtc2bpprgba" | "pvrtci2bpprgba" => PixelFormat::Pvrtc2BppRgba,
        "pvrtc4bpprgba" | "pvrtci4bpprgba" => PixelFormat::Pvrtc4BppRgba,
        "pvrtc2bpprgb" | "pvrtci2bpprgb" => PixelFormat::Pvrtc2BppRgb,
        "pvrtc4bpprgb" | "pvrtci4bpprgb" => PixelFormat::Pvrtc4BppRgb,
        "etc1" | "etc1rgb" => PixelFormat::Etc1Rgb,
        "etc2" | "etc2rgba" => PixelFormat::Etc2Rgba,
        "etc2rgb" => PixelFormat::Etc2Rgb,
        "dxt1" => PixelFormat::Dxt1,
        "dxt5" => PixelFormat::Dxt5,
        "astc4x4" => PixelFormat::Astc4x4,
        "astc5x4" => PixelFormat::Astc5x4,
        "astc5x5" => PixelFormat::Astc5x5,
        "astc6x5" => PixelFormat::Astc6x5,
        "astc6x6" => PixelFormat::Astc6x6,
        "astc8x5" => PixelFormat::Astc8x5,
        "astc8x6" => PixelFormat::Astc8x6,
        "astc8x8" => PixelFormat::Astc8x8,
        "astc10x5" => PixelFormat::Astc10x5,
        "astc10x6" => PixelFormat::Astc10x6,
        "astc10x8" => PixelFormat::Astc10x8,
        "astc10x10" => PixelFormat::Astc10x10,
        "astc12x10" => PixelFormat::Astc12x10,
        "astc12x12" => PixelFormat::Astc12x12,
        _ => fail(format!("--pixel-format inválido: {v}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn pixel_format_flag_is_shared_between_commands() {
        assert!(matches!(
            parse_pixel_format("BGRA8888"),
            PixelFormat::Bgra8888
        ));
        assert!(matches!(
            parse_pixel_format("rgba5555"),
            PixelFormat::Rgba5555
        ));
        // Variantes con alias.
        assert!(matches!(parse_pixel_format("5551"), PixelFormat::Rgba5551));
        assert!(matches!(
            parse_pixel_format("Alpha-Intensity8"),
            PixelFormat::AlphaIntensity8
        ));
    }

    #[test]
    fn pixel_format_gpu_tokens_and_astc_blocks_parse() {
        assert!(matches!(
            parse_pixel_format("PVRTCI_2BPP_RGBA"),
            PixelFormat::Pvrtc2BppRgba
        ));
        assert!(matches!(
            parse_pixel_format("pvrtc-4bpp-rgb"),
            PixelFormat::Pvrtc4BppRgb
        ));
        assert!(matches!(parse_pixel_format("etc1"), PixelFormat::Etc1Rgb));
        assert!(matches!(
            parse_pixel_format("ETC2_RGB"),
            PixelFormat::Etc2Rgb
        ));
        assert!(matches!(parse_pixel_format("dxt5"), PixelFormat::Dxt5));
        assert!(matches!(
            parse_pixel_format("astc-12x12"),
            PixelFormat::Astc12x12
        ));
        assert!(matches!(
            parse_pixel_format("rgba4444"),
            PixelFormat::Rgba4444
        ));
        assert!(matches!(parse_pixel_format("rgb565"), PixelFormat::Rgb565));
    }

    #[test]
    fn pixel_format_must_match_the_texture_format() {
        let mut cfg = ProjectConfig {
            pixel_format: PixelFormat::Dxt1,
            gpu_format: GpuFormat::Dds,
            ..Default::default()
        };
        assert!(check_export_flags(&cfg).is_ok());

        cfg.gpu_format = GpuFormat::Png;
        let err = check_export_flags(&cfg).unwrap_err();
        assert!(err.contains("DXT1") && err.contains("PNG"), "{err}");

        // Un pixel format de software vale con cualquier formato de textura.
        cfg.pixel_format = PixelFormat::Rgba8888;
        assert!(check_export_flags(&cfg).is_ok());
    }

    #[test]
    fn quality_flags_must_stay_inside_the_original_ranges() {
        assert!(check_export_flags(&ProjectConfig {
            pvr_quality: 8,
            ..Default::default()
        })
        .unwrap_err()
        .contains("--pvr-quality"));
        assert!(check_export_flags(&ProjectConfig {
            astc_quality: 5,
            ..Default::default()
        })
        .unwrap_err()
        .contains("--astc-quality"));
        assert!(check_export_flags(&ProjectConfig {
            etc1_quality: 101,
            ..Default::default()
        })
        .unwrap_err()
        .contains("--etc1-quality"));
        assert!(check_export_flags(&ProjectConfig {
            etc2_quality: 101,
            ..Default::default()
        })
        .unwrap_err()
        .contains("--etc2-quality"));
        assert!(check_export_flags(&ProjectConfig {
            etc2_quality: 0,
            ..Default::default()
        })
        .is_ok());
    }

    #[test]
    fn quality_and_dxt_mode_flags_parse() {
        let (_, values, _) = parse_args(&args(&[
            "--pvr-quality",
            "7",
            "--etc1-quality",
            "25",
            "--etc2-quality",
            "100",
            "--astc-quality",
            "4",
            "--dxt-mode",
            "DXT_PERCEPTUAL",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("pvr-quality").as_deref(), Some("7"));
        assert_eq!(val("etc1-quality").as_deref(), Some("25"));
        assert_eq!(val("etc2-quality").as_deref(), Some("100"));
        assert_eq!(val("astc-quality").as_deref(), Some("4"));
        assert!(matches!(
            DxtMode::parse(val("dxt-mode").as_deref().unwrap()),
            Some(DxtMode::Perceptual)
        ));
    }

    #[test]
    fn decrypt_writes_preview_png_with_bgra_restored() {
        use image::ImageEncoder;

        // Atlas "publicado" con BGRA8888 (R y B intercambiados) y cifrado.
        let bgra = vec![255u8, 0, 0, 255, 0, 0, 255, 255]; // azul, rojo
        let mut atlas = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(
            &mut atlas,
            image::codecs::png::CompressionType::Default,
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(&bgra, 2, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
        let enc = tp_core::export::encrypt_bytes(&atlas, "clave").unwrap();
        let f = std::env::temp_dir().join("tpcli_decrypt_preview.tpenc");
        let o = std::env::temp_dir().join("tpcli_decrypt_preview_out.png");
        std::fs::write(&f, &enc).unwrap();

        cmd_decrypt(&args(&[
            f.to_str().unwrap(),
            "--key",
            "clave",
            "-o",
            o.to_str().unwrap(),
            "--pixel-format",
            "bgra8888",
        ]));
        let out = std::fs::read(&o).unwrap();
        let img = image::load_from_memory(&out).unwrap().to_rgba8();
        // Con la conversión, R/B vuelven al orden natural RGBA.
        assert_eq!(img.as_raw(), &[0, 0, 255, 255, 255, 0, 0, 255]);
        let _ = std::fs::remove_file(&f);
        let _ = std::fs::remove_file(f.with_extension("dec.png"));
        let _ = std::fs::remove_file(&o);
    }

    #[test]
    fn boolean_flags_can_be_chained() {
        let (_, values, flags) = parse_args(&args(&[
            "--input",
            "in",
            "--output",
            "out",
            "--no-rotation",
            "--keep-extension",
            "--prepend-folder-name",
        ]));
        assert_eq!(
            values,
            vec![
                ("input".to_string(), "in".to_string()),
                ("output".to_string(), "out".to_string())
            ]
        );
        assert!(flags.iter().any(|f| f == "no-rotation"));
        assert!(flags.iter().any(|f| f == "keep-extension"));
        assert!(flags.iter().any(|f| f == "prepend-folder-name"));
    }

    #[test]
    fn lote7_multipack_flags_parse() {
        let (_, _, flags) = parse_args(&args(&[
            "--input",
            "in",
            "--output",
            "out",
            "--no-multipack",
        ]));
        assert!(flags.iter().any(|f| f == "no-multipack"));

        let (_, _, flags) = parse_args(&args(&["--input", "in", "--output", "out", "--multipack"]));
        assert!(flags.iter().any(|f| f == "multipack"));
        assert!(!flags.iter().any(|f| f == "no-multipack"));
    }

    #[test]
    fn lote8_export_flags_parse() {
        let (_, values, flags) = parse_args(&args(&[
            "--input",
            "in",
            "--output",
            "out",
            "--format",
            "jpg",
            "--jpg-quality",
            "90",
            "--webp-quality",
            "80",
            "--png-opt-level",
            "4",
            "--png8-dither",
            "low",
            "--pixel-format",
            "rgb888",
            "--flip-y",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("format").as_deref(), Some("jpg"));
        assert_eq!(val("jpg-quality").as_deref(), Some("90"));
        assert_eq!(val("webp-quality").as_deref(), Some("80"));
        assert_eq!(val("png-opt-level").as_deref(), Some("4"));
        assert_eq!(val("png8-dither").as_deref(), Some("low"));
        assert_eq!(val("pixel-format").as_deref(), Some("rgb888"));
        assert!(flags.iter().any(|f| f == "flip-y"));
    }

    #[test]
    fn normalmap_flags_parse() {
        let (_, values, flags) = parse_args(&args(&[
            "--normalmap-suffix",
            "_n",
            "--normalmap-filter",
            "normals/",
            "--normalmap-sheet",
            "norms",
            "--normalmap-detect",
            "--no-normals",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("normalmap-suffix").as_deref(), Some("_n"));
        assert_eq!(val("normalmap-filter").as_deref(), Some("normals/"));
        assert_eq!(val("normalmap-sheet").as_deref(), Some("norms"));
        assert!(flags.iter().any(|f| f == "normalmap-detect"));
        assert!(flags.iter().any(|f| f == "no-normals"));
    }

    #[test]
    fn extra_data_file_flags_parse() {
        let (_, values, _) = parse_args(&args(&[
            "--class-file",
            "Sprites.swift",
            "--header-file",
            "Sprites.h",
            "--source-file",
            "Sprites.cpp",
            "--spriteids-file",
            "spriteids.txt",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("class-file").as_deref(), Some("Sprites.swift"));
        assert_eq!(val("header-file").as_deref(), Some("Sprites.h"));
        assert_eq!(val("source-file").as_deref(), Some("Sprites.cpp"));
        assert_eq!(val("spriteids-file").as_deref(), Some("spriteids.txt"));
    }

    #[test]
    fn lote5_value_flags_parse() {
        let (_, values, _) = parse_args(&args(&[
            "--border-padding",
            "8",
            "--common-divisor",
            "4",
            "--align",
            "4",
            "--alpha-handling",
            "premultiply",
            "--scale-mode",
            "fast",
            "--texture-path",
            "/assets",
            "--dither",
            "floyd-alpha",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("border-padding").as_deref(), Some("8"));
        assert_eq!(val("common-divisor").as_deref(), Some("4"));
        assert_eq!(val("align").as_deref(), Some("4"));
        assert_eq!(val("alpha-handling").as_deref(), Some("premultiply"));
        assert_eq!(val("scale-mode").as_deref(), Some("fast"));
        assert_eq!(val("texture-path").as_deref(), Some("/assets"));
        assert_eq!(val("dither").as_deref(), Some("floyd-alpha"));
    }

    #[test]
    fn lote5_enums_parse_from_cli_tokens() {
        assert_eq!(
            AlphaHandling::parse("bleed"),
            Some(AlphaHandling::ReduceBorderArtifacts)
        );
        assert_eq!(
            AlphaHandling::parse("premultiply"),
            Some(AlphaHandling::PremultiplyAlpha)
        );
        assert_eq!(ScaleMode::parse("fast"), Some(ScaleMode::Fast));
        assert_eq!(
            ScaleMode::parse("smooth"),
            Some(tp_core::config::ScaleMode::Smooth)
        );
    }

    #[test]
    fn canonical_flags_parse() {
        // Aliases con los nombres canónicos de las opciones.
        let (_, values, flags) = parse_args(&args(&[
            "--input",
            "in",
            "--output",
            "out",
            "--shape-padding",
            "3",
            "--align-to-grid",
            "8",
            "--maxrects-heuristics",
            "contact-point",
            "--disable-auto-alias",
            "--variant",
            "1.0:-ipadhd,0.5:-hd",
        ]));
        assert_eq!(val2(&values, "shape-padding"), Some("3"));
        assert_eq!(val2(&values, "align-to-grid"), Some("8"));
        assert_eq!(val2(&values, "maxrects-heuristics"), Some("contact-point"));
        assert!(flags.contains(&"disable-auto-alias".to_string()));
        assert_eq!(val2(&values, "variant"), Some("1.0:-ipadhd,0.5:-hd"));
    }

    fn val2<'a>(values: &'a [(String, String)], key: &str) -> Option<&'a str> {
        values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn lote6_pack_flags_parse() {
        let (_, values, flags) = parse_args(&args(&[
            "--algorithm",
            "grid",
            "--pack-mode",
            "best",
            "--size-constraints",
            "pot",
            "--width",
            "1024",
            "--height",
            "512",
            "--basic-sort-by",
            "name",
            "--basic-order",
            "desc",
            "--strategy",
            "contact-point",
            "--force-squared",
        ]));
        let val = |k: &str| values.iter().find(|(v, _)| v == k).map(|(_, v)| v.clone());
        assert_eq!(val("algorithm").as_deref(), Some("grid"));
        assert_eq!(val("pack-mode").as_deref(), Some("best"));
        assert_eq!(val("size-constraints").as_deref(), Some("pot"));
        assert_eq!(val("width").as_deref(), Some("1024"));
        assert_eq!(val("height").as_deref(), Some("512"));
        assert_eq!(val("basic-sort-by").as_deref(), Some("name"));
        assert_eq!(val("basic-order").as_deref(), Some("desc"));
        assert_eq!(val("strategy").as_deref(), Some("contact-point"));
        assert!(flags.iter().any(|f| f == "force-squared"));
    }

    #[test]
    fn lote6_enums_parse_from_cli_tokens() {
        assert_eq!(
            PackingAlgorithm::parse("grid"),
            Some(PackingAlgorithm::Grid)
        );
        assert_eq!(PackMode::parse("best"), Some(PackMode::Best));
        assert_eq!(SizeConstraint::parse("pot"), Some(SizeConstraint::Pot));
        assert_eq!(
            SizeConstraint::parse("multiple-of-4"),
            Some(SizeConstraint::MultipleOf4)
        );
        assert_eq!(
            PackingStrategy::parse("contact-point"),
            Some(PackingStrategy::ContactPoint)
        );
        assert_eq!(
            PackingStrategy::parse("bottom-left"),
            Some(PackingStrategy::BottomLeft)
        );
        assert_eq!(
            BasicSortBy::parse("circumference"),
            Some(BasicSortBy::Circumference)
        );
        assert_eq!(SortOrder::parse("desc"), Some(SortOrder::Descending));
    }
}
