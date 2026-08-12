//! TexturePacker-RS command-line interface.
//!
//! ```
//! tp-cli pack project.tpproj
//! tp-cli pack --input sprites/ --output build/ --max-size 4096 --format astc
//! tp-cli decrypt atlas_0.png.tpenc --key secreto -o atlas.png
//! ```

use std::path::PathBuf;
use tp_core::config::{
    ColorDepth, DitheringAlgorithm, GpuFormat, PackingStrategy, ProjectConfig, TemplateFormat,
};

fn usage() -> ! {
    println!(
        "TexturePacker-RS {}\n\
         \n\
         USO:\n\
         \x20 tp-cli pack <proyecto.tpproj>\n\
         \x20 tp-cli pack --input DIR --output DIR [opciones]\n\
         \x20 tp-cli decrypt <archivo.tpenc> --key CLAVE -o salida.png\n\
         \n\
         OPCIONES de pack:\n\
         \x20 --max-size N          Tamaño máximo del atlas (512..8192, potencia de 2)\n\
         \x20 --padding N           Padding entre sprites (px)\n\
         \x20 --extrude N           Extrude de bordes (px)\n\
         \x20 --no-rotation         Desactivar rotación 90°\n\
         \x20 --no-trim             Desactivar recorte de bordes\n\
         \x20 --trim-threshold N    Umbral de alpha para recorte (0-255)\n\
         \x20 --polygon             Modo polígono (mallas + empaquetado por contorno)\n\
         \x20 --tolerance F         Tolerancia RDP para polígonos (px)\n\
         \x20 --no-aliasing         Desactivar deduplicación por hash\n\
         \x20 --color-depth T       RGBA8888 | RGBA4444 | RGB565\n\
         \x20 --dither T            none | floyd | atkinson\n\
         \x20 --format T            png | webp | astc | etc2 | pvrtc\n\
         \x20 --strategy T          bssf | baf | blsf | guillotine\n\
         \x20 --variants LIST       Escalas, p.ej. 1.0,0.5 (genera variantes @2x/@1x)\n\
         \x20 --template-format T   json | xml | plist | cpp | tsv | text\n\
         \x20 --template RUTA       Plantilla Mustache personalizada\n\
         \x20 --key CLAVE           Cifrar texturas con AES-256-GCM\n\
         \x20 --no-normals          No empaquetar mapas de normales\n\
         \x20 --no-recursive        No buscar en subdirectorios\n\
         \x20 --base-name NOMBRE    Nombre base de los archivos de salida\n\
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
        if let Some(v) = a.strip_prefix("--") {
            if let Some(val) = it.next() {
                values.push((v.to_string(), val.clone()));
            } else {
                flags.push(v.to_string());
            }
        } else if let Some(v) = a.strip_prefix('-') {
            // single-dash option like -o
            if let Some(val) = it.next() {
                values.push((v.to_string(), val.clone()));
            } else {
                flags.push(v.to_string());
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
        cfg.max_texture_size = v.parse().unwrap_or_else(|_| fail("--max-size inválido".into()));
    }
    if let Some(v) = val("padding") {
        cfg.padding = v.parse().unwrap_or_else(|_| fail("--padding inválido".into()));
    }
    if let Some(v) = val("extrude") {
        cfg.extrude = v.parse().unwrap_or_else(|_| fail("--extrude inválido".into()));
    }
    if let Some(v) = val("trim-threshold") {
        cfg.trim_threshold = v.parse().unwrap_or_else(|_| fail("--trim-threshold inválido".into()));
    }
    if let Some(v) = val("tolerance") {
        cfg.polygon_tolerance = v.parse().unwrap_or_else(|_| fail("--tolerance inválido".into()));
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
            "floyd" => DitheringAlgorithm::FloydSteinberg,
            "atkinson" => DitheringAlgorithm::Atkinson,
            _ => fail(format!("--dither inválido: {v}")),
        };
    }
    if let Some(v) = val("format") {
        cfg.gpu_format = match v.to_ascii_lowercase().as_str() {
            "png" => GpuFormat::Png,
            "webp" => GpuFormat::WebP,
            "astc" => GpuFormat::Astc4x4,
            "etc2" => GpuFormat::Etc2Rgba,
            "pvrtc" => GpuFormat::Pvrtc4Bpp,
            _ => fail(format!("--format inválido: {v}")),
        };
    }
    if let Some(v) = val("strategy") {
        cfg.packing_strategy = match v.to_ascii_lowercase().as_str() {
            "bssf" => PackingStrategy::Bssf,
            "baf" => PackingStrategy::Baf,
            "blsf" => PackingStrategy::Blsf,
            "guillotine" => PackingStrategy::Guillotine,
            _ => fail(format!("--strategy inválido: {v}")),
        };
    }
    if let Some(v) = val("variants") {
        let parsed: Vec<f32> = v
            .split([',', ';', ' '])
            .filter(|s| !s.trim().is_empty())
            .filter_map(|s| s.trim().parse().ok())
            .filter(|x| *x > 0.0 && *x <= 1.0)
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
    if flags.iter().any(|f| f == "no-aliasing") {
        cfg.enable_aliasing = false;
    }
    if flags.iter().any(|f| f == "no-normals") {
        cfg.enable_normal_maps = false;
    }
    if flags.iter().any(|f| f == "no-recursive") {
        cfg.recursive = false;
    }

    if cfg.input_directory.as_os_str().is_empty() {
        fail("Falta --input DIR (o pasa un archivo .tpproj)".into());
    }
    if cfg.output_directory.as_os_str().is_empty() {
        fail("Falta --output DIR (o pasa un archivo .tpproj)".into());
    }

    let started = std::time::Instant::now();
    let out = tp_core::pipeline::run(&cfg)
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
    std::fs::write(&out_path, &plain)
        .unwrap_or_else(|e| fail(format!("No se pudo escribir {}: {e}", out_path.display())));
    let _ = flags;
    println!("✔ Descifrado: {}", out_path.display());
}
