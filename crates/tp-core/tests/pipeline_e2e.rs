//! End-to-end pipeline tests: generate real PNG sprites on disk and run the
//! full pipeline (trim -> hash/alias -> pack -> blit -> quantize -> export ->
//! templates), verifying the on-disk outputs.

use std::path::{Path, PathBuf};
use tp_core::config::{
    ColorDepth, DitheringAlgorithm, GpuFormat, PackMode, PackingAlgorithm, PackingStrategy,
    ProjectConfig, SizeConstraint, TemplateFormat,
};
use tp_core::types::Rect;
use tp_core::{export, pipeline};

/// Write a solid-color PNG to `path`.
fn write_png(path: &Path, w: u32, h: u32, color: [u8; 4]) {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba(color));
    img.save(path).unwrap();
}

/// Write a PNG with a distinctive red first pixel (used to verify rotation).
fn write_wide_png(path: &Path, w: u32, h: u32) {
    write_corner_png(path, w, h, [0, 255, 0, 255]);
}

/// Solid `color` with a red pixel at (0,0).
fn write_corner_png(path: &Path, w: u32, h: u32, color: [u8; 4]) {
    let mut img = image::RgbaImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        *px = if x == 0 && y == 0 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba(color)
        };
    }
    img.save(path).unwrap();
}

fn make_input_dir(base: &Path, name: &str) -> PathBuf {
    let dir = base.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Fixture {
    dir: std::path::PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("tp_e2e_{name}_{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn full_pipeline_with_aliases_rotation_and_normals() {
    let fx = Fixture::new("full");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    // Sprites (PNG files with transparent padding to exercise trimming).
    let make_trimmed = |name: &str, w: u32, h: u32, color: [u8; 4]| {
        let mut img = image::RgbaImage::new(w + 4, h + 4); // 2px transparent border
        for (x, y, px) in img.enumerate_pixels_mut() {
            if x >= 2 && y >= 2 && x < w + 2 && y < h + 2 {
                *px = image::Rgba(color);
            }
        }
        img.save(input.join(format!("{name}.png"))).unwrap();
    };

    make_trimmed("red", 16, 16, [255, 0, 0, 255]);
    make_trimmed("red_copy", 16, 16, [255, 0, 0, 255]); // identical -> alias
    make_trimmed("green", 8, 8, [0, 255, 0, 255]);
    // tall (16x40) only fits rotated below the big wide sprite (58x21):
    // BSSF places wide first (larger area), then tall must rotate 90°.
    write_corner_png(&input.join("tall.png"), 16, 40, [0, 0, 255, 255]);
    write_wide_png(&input.join("wide.png"), 58, 21);
    make_trimmed("coin", 12, 12, [255, 215, 0, 255]);
    // Normal map for `coin` (same dimensions as the raw file: 16x16).
    let mut normal = image::RgbaImage::new(16, 16);
    for px in normal.pixels_mut() {
        *px = image::Rgba([128, 128, 255, 255]);
    }
    normal.save(input.join("coin_normal.png")).unwrap();

    // Enough sprites to force multiple pages in a 64x64 atlas.
    for i in 0..40 {
        let c = [(i * 3) as u8, (i * 7) as u8, (i * 11) as u8, 255];
        make_trimmed(&format!("tile_{i}"), 6, 6, c);
    }

    let cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output.clone(),
        max_texture_size: 64,
        padding: 1,
        extrude: 1,
        allow_rotation: true,
        enable_trim: true,
        enable_aliasing: true,
        enable_normal_maps: true,
        template_format: TemplateFormat::Json,
        packing_strategy: PackingStrategy::Bssf,
        ..ProjectConfig::default()
    };

    let out = pipeline::run(&cfg).expect("pipeline should succeed");
    let result = &out.result;

    // ---- Output files exist -------------------------------------------------
    assert!(!result.output_files.is_empty());
    for f in &result.output_files {
        assert!(output.join(f).exists(), "missing output file: {f}");
    }
    let meta_path = output.join("atlas.json");
    assert!(meta_path.exists());

    // ---- Metadata parses and contains all sprites ---------------------------
    let meta_text = std::fs::read_to_string(&meta_path).unwrap();
    let meta: serde_json::Value = serde_json::from_str(&meta_text).unwrap();
    let frames = meta["frames"].as_array().unwrap();
    assert_eq!(frames.len(), result.total_sprites);

    // ---- Aliasing -----------------------------------------------------------
    let red = result.sprites.iter().find(|s| s.id == "red").unwrap();
    let copy = result.sprites.iter().find(|s| s.id == "red_copy").unwrap();
    assert!(copy.is_alias);
    assert_eq!(copy.alias_target_id.as_deref(), Some("red"));
    assert_eq!(copy.allocated_frame, red.allocated_frame);
    assert_eq!(copy.atlas_page_index, red.atlas_page_index);
    assert!(result.alias_count >= 1);

    // ---- Trimming -----------------------------------------------------------
    // The raw file was 20x20 with a 16x16 sprite offset (2,2).
    assert_eq!(red.trimmed_bounds, Rect::new(2, 2, 16, 16));
    assert_eq!(red.offset_x, 2);
    assert_eq!(red.offset_y, 2);

    // ---- Rotation -----------------------------------------------------------
    let wide = result.sprites.iter().find(|s| s.id == "wide").unwrap();
    let tall = result.sprites.iter().find(|s| s.id == "tall").unwrap();
    let rotated_sprite = [&wide, &tall]
        .into_iter()
        .find(|s| s.is_rotated)
        .expect("expected a rotated sprite");
    assert!(tall.is_rotated, "tall should have been rotated");

    // Verify rotated pixel placement: the sprite's red corner (local 0,0) must
    // sit at atlas (vis.x + trim_h - 1, vis.y) — rotation maps (x,y)->(H-1-y,x).
    let page = &out.pages[rotated_sprite.atlas_page_index as usize];
    let vis = &rotated_sprite.visible_frame;
    let ax = vis.x + rotated_sprite.trimmed_bounds.height - 1;
    let ay = vis.y;
    let i = ((ay * page.width + ax) * 4) as usize;
    assert_eq!(
        &page.pixels[i..i + 4],
        &[255, 0, 0, 255],
        "rotated red corner"
    );

    // ---- Normal maps --------------------------------------------------------
    let coin = result.sprites.iter().find(|s| s.id == "coin").unwrap();
    assert!(coin.normal_source_path.is_some());
    let normal_page = &out.pages[coin.atlas_page_index as usize];
    assert!(normal_page.has_normals);
    let n = normal_page.normal_pixels.as_ref().unwrap();
    // Sample the center of the coin's visible frame in the normal canvas.
    let cx = coin.visible_frame.x + coin.visible_frame.width / 2;
    let cy = coin.visible_frame.y + coin.visible_frame.height / 2;
    let i = ((cy * normal_page.width + cx) * 4) as usize;
    assert_eq!(&n[i..i + 4], &[128, 128, 255, 255]);
    assert!(output.join("atlas_normal.png").exists());

    // ---- On-disk PNG matches the in-memory page ----------------------------
    let disk = image::open(output.join("atlas.png")).expect("atlas.png decodes");
    let disk_rgba = disk.to_rgba8();
    let (dw, dh) = disk_rgba.dimensions();
    let hero = result.sprites.iter().find(|s| s.id == "red").unwrap();
    let hx = hero.visible_frame.x as u32 + 4;
    let hy = hero.visible_frame.y as u32 + 4;
    assert!(hx < dw && hy < dh);
    let p = disk_rgba.get_pixel(hx, hy).0;
    assert_eq!(p, [255, 0, 0, 255], "atlas.png pixel at hero frame");

    // ---- Frame non-overlap in the atlas -------------------------------------
    for page in &out.pages {
        let rects: Vec<Rect> = result
            .sprites
            .iter()
            .filter(|s| s.atlas_page_index as usize == page.index && !s.is_alias)
            .map(|s| s.allocated_frame)
            .collect();
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                assert!(!rects[i].intersects(&rects[j]));
            }
        }
    }
}

#[test]
fn encryption_and_quantization_and_variants() {
    let fx = Fixture::new("enc");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");
    write_png(&input.join("a.png"), 8, 8, [100, 150, 200, 255]);
    write_png(&input.join("b.png"), 16, 16, [10, 20, 30, 255]);

    let mut cfg = ProjectConfig {
        input_directory: input,
        output_directory: output.clone(),
        max_texture_size: 64,
        padding: 1,
        extrude: 0,
        encryption_key: Some("clave-secreta".into()),
        color_depth: ColorDepth::Rgba4444,
        dithering_algorithm: DitheringAlgorithm::FloydSteinberg,
        ..ProjectConfig::default()
    };
    cfg.gpu_format = GpuFormat::Png;
    cfg.scale_variants = vec![1.0, 0.5];
    cfg.template_format = TemplateFormat::Json;

    let out = pipeline::run(&cfg).unwrap();
    let result = &out.result;

    // Encrypted files end in .tpenc and decrypt back to valid PNGs.
    let enc_files: Vec<&String> = result
        .output_files
        .iter()
        .filter(|f| f.ends_with(".tpenc"))
        .collect();
    assert!(!enc_files.is_empty());
    for f in &enc_files {
        let bytes = std::fs::read(output.join(f)).unwrap();
        let plain = export::decrypt_bytes(&bytes, "clave-secreta").unwrap();
        let img = image::load_from_memory(&plain).expect("decrypted data is a valid image");
        assert!(img.width() >= 4);
        assert!(img.height() >= 4);
    }
    // Wrong key fails.
    let bytes = std::fs::read(output.join(enc_files[0])).unwrap();
    assert!(export::decrypt_bytes(&bytes, "otra-clave").is_err());

    // Both scale variants produced metadata files.
    assert!(output.join("atlas.json").exists());
    assert!(output.join("atlas-hd.json").exists());

    // The @1x variant has half-sized frame coordinates.
    let half: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("atlas-hd.json")).unwrap())
            .unwrap();
    let full: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("atlas.json")).unwrap()).unwrap();
    let full_frame = &full["frames"][0]["frame"];
    let half_frame = &half["frames"][0]["frame"];
    assert_eq!(
        half_frame["x"].as_i64().unwrap(),
        full_frame["x"].as_i64().unwrap() / 2
    );

    // Quantization visible in the page pixels (values are multiples of 17).
    let page = &out.pages[0];
    let px = &page.pixels[0..4];
    for v in px {
        assert!(*v % 17 == 0 || *v == 0, "quantized channel {v}");
    }
}

#[test]
fn custom_template_and_polygon_mode() {
    let fx = Fixture::new("poly");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");
    write_png(&input.join("circle.png"), 16, 16, [255, 0, 0, 255]);
    write_png(&input.join("square.png"), 12, 12, [0, 255, 0, 255]);

    // Custom template.
    let tpl = fx.dir.join("custom.hbs");
    std::fs::write(
        &tpl,
        "{{#each frames}}sprite[{{@index}}]={{this.filename}} at ({{this.frame.x}},{{this.frame.y}})\n{{/each}}",
    )
    .unwrap();

    let cfg = ProjectConfig {
        input_directory: input,
        output_directory: output.clone(),
        max_texture_size: 64,
        enable_polygon: true,
        polygon_tolerance: 1.0,
        packing_strategy: PackingStrategy::Guillotine,
        template_format: TemplateFormat::PlainText,
        export_template: Some(tpl),
        ..ProjectConfig::default()
    };

    let out = pipeline::run(&cfg).unwrap();
    let result = &out.result;

    // Polygons built for non-alias sprites.
    let circle = result.sprites.iter().find(|s| s.id == "circle").unwrap();
    assert!(circle.mesh.is_some(), "polygon mode should build meshes");
    let mesh = circle.mesh.as_ref().unwrap();
    assert!(!mesh.vertices.is_empty());
    assert_eq!(mesh.uvs.len(), mesh.vertices.len());
    assert!(!mesh.indices.is_empty());
    assert!(mesh
        .indices
        .iter()
        .all(|&i| (i as usize) < mesh.vertices.len()));

    // Custom template output.
    let rendered = std::fs::read_to_string(output.join("atlas.txt")).unwrap();
    assert!(rendered.contains("sprite[0]=circle"));
    assert!(rendered.contains("sprite[1]=square"));
}

#[test]
fn etc2_pvrtc_and_astc_export_paths() {
    let fx = Fixture::new("etc");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");
    write_png(&input.join("a.png"), 8, 8, [200, 100, 50, 255]);

    // ETC2 (built-in encoder) — must work without any feature flags.
    let mut cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output.clone(),
        max_texture_size: 64,
        gpu_format: GpuFormat::Etc2Rgba,
        ..ProjectConfig::default()
    };
    let _out = pipeline::run(&cfg).unwrap();
    let ktx = output.join("atlas.ktx");
    assert!(ktx.exists());
    let bytes = std::fs::read(&ktx).unwrap();
    assert_eq!(&bytes[..12], b"\xABKTX 11\xBB\r\n\x1A\n");

    // PVRTC (built-in encoder) — works without any feature flags. PVR needs a
    // power-of-two sheet, so constrain the (now auto-cropped) atlas size.
    cfg.gpu_format = GpuFormat::Pvrtc4Bpp;
    cfg.size_constraints = SizeConstraint::Pot;
    let out = pipeline::run(&cfg).unwrap();
    let pvr = output.join("atlas.pvr");
    assert!(pvr.exists());
    let bytes = std::fs::read(&pvr).unwrap();
    assert_eq!(&bytes[..4], b"PVR\x03");
    // Payload after the 52-byte PVR v3 header must be w*h at 4bpp, and it
    // must decode through an independent decoder.
    let (w, h) = (out.pages[0].width, out.pages[0].height);
    assert!((w as u32).is_power_of_two() && (h as u32).is_power_of_two());
    let payload = &bytes[52..];
    assert_eq!(payload.len(), (w * h / 2) as usize);
    let mut buf = vec![0u32; (w * h) as usize];
    texture2ddecoder::decode_pvrtc_4bpp(payload, w as usize, h as usize, &mut buf).unwrap();

    #[cfg(feature = "gpu-formats")]
    {
        cfg.gpu_format = GpuFormat::Astc4x4;
        let _out = pipeline::run(&cfg).unwrap();
        let astc = output.join("atlas.astc");
        assert!(astc.exists());
        let bytes = std::fs::read(&astc).unwrap();
        assert_eq!(&bytes[..4], &[0x13, 0xAB, 0xA1, 0x5C]);
    }
}

#[test]
fn oversized_sprites_error_cleanly() {
    let fx = Fixture::new("big");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");
    write_png(&input.join("huge.png"), 300, 300, [1, 2, 3, 255]);

    let cfg = ProjectConfig {
        input_directory: input,
        output_directory: output,
        max_texture_size: 128,
        ..ProjectConfig::default()
    };
    match pipeline::run(&cfg) {
        Err(err) => assert!(
            err.to_string().contains("huge"),
            "error should name the sprite: {err}"
        ),
        Ok(_) => panic!("expected an error for an oversized sprite"),
    }
}

#[test]
fn lote5_border_divisor_names_and_transparency() {
    use tp_core::config::AlphaHandling;

    let fx = Fixture::new("lote5");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    // 10x10 opaco con un píxel transparente que arrastra color residual.
    let mut img = image::RgbaImage::from_pixel(10, 10, image::Rgba([200, 100, 50, 255]));
    img.put_pixel(0, 9, image::Rgba([123, 45, 67, 0]));
    img.save(input.join("blob.png")).unwrap();

    let cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output.clone(),
        max_texture_size: 64,
        padding: 1,
        border_padding: 8,
        common_divisor_x: 4,
        common_divisor_y: 6,
        alpha_handling: AlphaHandling::PremultiplyAlpha,
        texture_path: Some("/assets".into()),
        trim_sprite_names: false,
        enable_normal_maps: false,
        enable_aliasing: false,
        ..ProjectConfig::default()
    };

    let out = pipeline::run(&cfg).expect("pipeline should succeed");
    let result = &out.result;

    // *Trim sprite names* desactivado: el id conserva la extensión.
    assert_eq!(result.sprites[0].id, "blob.png");
    // *Common divisor*: 10x10 estirado a 12x12 (múltiplo de 4 y de 6).
    let bounds = result.sprites[0].trimmed_bounds;
    assert_eq!(
        (bounds.width, bounds.height),
        (12, 12),
        "divisor común 4x6 sobre un sprite de 10x10"
    );

    // *Texture path*: el JSON referencia la textura bajo /assets.
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("atlas.json")).unwrap()).unwrap();
    assert_eq!(json["meta"]["image"], "/assets/atlas.png");
    assert_eq!(json["meta"]["pages"][0]["file"], "/assets/atlas.png");
    assert_eq!(json["frames"][0]["filename"], "blob.png");

    // *Border padding*: ningún píxel opaco a menos de 8 px del borde.
    let page = &out.pages[0];
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for y in 0..page.height {
        for x in 0..page.width {
            if page.pixels[((y * page.width + x) * 4 + 3) as usize] > 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    assert!(min_x >= 8, "píxel opaco a {min_x} px del borde izquierdo");
    assert!(min_y >= 8, "píxel opaco a {min_y} px del borde superior");
    assert!(
        max_x < page.width - 8,
        "píxel opaco a {} px del borde derecho",
        page.width - 1 - max_x
    );
    assert!(
        max_y < page.height - 8,
        "píxel opaco a {} px del borde inferior",
        page.height - 1 - max_y
    );

    // *PremultiplyAlpha*: el color residual del píxel transparente se anula
    // (a=0 => rgb=0) y los opacos quedan multiplicados por su alfa.
    let bytes = std::fs::read(output.join("atlas.png")).unwrap();
    let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
    assert!(
        !decoded.pixels().any(|p| p.0 == [123, 45, 67, 0]),
        "el color residual debe haberse premultiplicado a negro"
    );
}

#[test]
fn lote5_align_to_grid_rounds_padding_and_positions() {
    let fx = Fixture::new("lote5grid");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    for (i, (w, h)) in [(7, 5), (11, 9), (6, 14), (9, 6), (13, 8)]
        .iter()
        .enumerate()
    {
        write_png(
            &input.join(format!("cell_{i}.png")),
            *w,
            *h,
            [(i * 40) as u8, 120, 200, 255],
        );
    }

    let cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output.clone(),
        max_texture_size: 64,
        padding: 1,        // se redondeará a 4
        border_padding: 1, // se redondeará a 4
        align_to_grid: 4,
        enable_normal_maps: false,
        ..ProjectConfig::default()
    };

    let out = pipeline::run(&cfg).expect("pipeline should succeed");
    assert!(
        out.result
            .warnings
            .iter()
            .any(|w| w.contains("rejilla") && w.contains("4")),
        "warnings: {:?}",
        out.result.warnings
    );

    // Todos los frames caen en coordenadas múltiplos de 4.
    for s in &out.result.sprites {
        assert_eq!(s.allocated_frame.x % 4, 0, "{}", s.allocated_frame.x);
        assert_eq!(s.allocated_frame.y % 4, 0, "{}", s.allocated_frame.y);
    }

    // Y el borde efectivo es el múltiplo redondeado (4), no el original (1).
    let page = &out.pages[0];
    let mut min_x = i32::MAX;
    for y in 0..page.height {
        for x in 0..page.width {
            if page.pixels[((y * page.width + x) * 4 + 3) as usize] > 0 {
                min_x = min_x.min(x);
            }
        }
    }
    assert!(
        min_x >= 4,
        "el padding de borde redondeado es 4: min_x={min_x}"
    );
}

#[test]
fn lote6_grid_constraints_and_fixed_size() {
    let fx = Fixture::new("lote6");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    write_png(&input.join("big.png"), 40, 40, [220, 60, 60, 255]);
    write_png(&input.join("wide.png"), 32, 12, [60, 220, 60, 255]);
    write_png(&input.join("small1.png"), 8, 8, [60, 60, 220, 255]);
    write_png(&input.join("small2.png"), 8, 8, [220, 220, 60, 255]);

    // Grid + POT: el atlas recortado debe ser potencia de dos y las celdas
    // del tamaño del mayor sprite inflado (40 + 2*padding).
    let mut cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output.clone(),
        max_texture_size: 512,
        algorithm: PackingAlgorithm::Grid,
        size_constraints: SizeConstraint::Pot,
        pack_mode: PackMode::Good,
        ..ProjectConfig::default()
    };
    let out = pipeline::run(&cfg).unwrap();
    let page = &out.pages[0];
    assert!(
        (page.width as u32).is_power_of_two() && (page.height as u32).is_power_of_two(),
        "{}x{} no es POT",
        page.width,
        page.height
    );
    let cell = 40 + 2 * cfg.padding;
    for s in out.result.sprites.iter().filter(|s| !s.is_alias) {
        assert_eq!(
            s.allocated_frame.x % cell,
            0,
            "x fuera de celda: {}",
            s.allocated_frame.x
        );
        assert_eq!(
            s.allocated_frame.y % cell,
            0,
            "y fuera de celda: {}",
            s.allocated_frame.y
        );
    }
    // Sin solapes entre los sprites.
    let frames: Vec<tp_core::types::Rect> = out
        .result
        .sprites
        .iter()
        .filter(|s| !s.is_alias)
        .map(|s| s.allocated_frame)
        .collect();
    for i in 0..frames.len() {
        for j in (i + 1)..frames.len() {
            assert!(!frames[i].intersects(&frames[j]));
        }
    }

    // Basic + tamaño fijo: las dimensiones finales son exactamente las pedidas.
    cfg.algorithm = PackingAlgorithm::Basic;
    cfg.size_constraints = SizeConstraint::AnySize;
    cfg.pack_mode = PackMode::Fast;
    cfg.fixed_width = 256;
    cfg.fixed_height = 128;
    let out = pipeline::run(&cfg).unwrap();
    assert_eq!(out.pages[0].width, 256);
    assert_eq!(out.pages[0].height, 128);
    assert_eq!(out.result.sprites.iter().filter(|s| !s.is_alias).count(), 4);
}

#[test]
fn lote7_multipack_placeholders_per_page_data() {
    let fx = Fixture::new("lote7_placeholders");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    // 100 opaque 10x10 tiles cannot fit in one 64x64 sheet (padding 1 →
    // 12x12 each, 25 per sheet) so the packer must emit several pages.
    for i in 0..100 {
        let c = [
            (i % 255) as u8,
            (i * 3 % 255) as u8,
            (i * 7 % 255) as u8,
            255,
        ];
        write_png(&input.join(format!("tile_{i}.png")), 10, 10, c);
    }

    let cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output.clone(),
        max_texture_size: 64,
        padding: 1,
        extrude: 0,
        base_file_name: "hoja{n1}".into(),
        template_format: TemplateFormat::Json,
        ..ProjectConfig::default()
    };

    let out = pipeline::run(&cfg).expect("pipeline should succeed");
    let result = &out.result;
    assert!(
        out.pages.len() >= 2,
        "expected multipack, got {} pages",
        out.pages.len()
    );

    // One texture + one data file per page, all named via {n1}.
    let mut collected: Vec<String> = Vec::new();
    let mut page_count = 0;
    for (i, page) in out.pages.iter().enumerate() {
        let tex = format!("hoja{}.png", i + 1);
        let data = format!("hoja{}.json", i + 1);
        assert!(output.join(&tex).exists(), "missing {tex}");
        assert!(output.join(&data).exists(), "missing {data}");
        // The on-disk texture must decode with the page's dimensions.
        let disk = image::open(output.join(&tex)).unwrap().to_rgba8();
        assert_eq!(
            (disk.width() as i32, disk.height() as i32),
            (page.width, page.height)
        );
        let _ = page;

        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(output.join(&data)).unwrap()).unwrap();
        assert_eq!(meta["meta"]["image"].as_str(), Some(tex.as_str()));
        assert_eq!(meta["meta"]["pages"].as_array().unwrap().len(), 1);
        assert_eq!(meta["meta"]["pages"][0]["index"].as_u64(), Some(i as u64));
        let frames = meta["frames"].as_array().unwrap();
        assert!(!frames.is_empty(), "{data} has no frames");
        for f in frames {
            // Every frame belongs to this sheet only.
            assert_eq!(f["page"].as_i64(), Some(i as i64), "wrong page in {data}");
            collected.push(f["filename"].as_str().unwrap().to_string());
        }
        page_count += 1;
    }
    assert_eq!(page_count, out.pages.len());

    // No combined/legacy data file, and every sprite lands exactly once.
    assert!(!output.join("hoja.json").exists());
    collected.sort();
    let mut ids: Vec<String> = result.sprites.iter().map(|s| s.id.clone()).collect();
    ids.sort();
    assert_eq!(collected, ids);

    // All recorded output files exist (nested-free names in this test).
    for f in &result.output_files {
        assert!(output.join(f).exists(), "missing output file: {f}");
    }
}

#[test]
fn lote7_multipack_legacy_names_and_warning() {
    let fx = Fixture::new("lote7_legacy");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    for i in 0..100 {
        write_png(
            &input.join(format!("s{i}.png")),
            10,
            10,
            [i as u8, 0, 0, 255],
        );
    }

    let cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output.clone(),
        max_texture_size: 64,
        padding: 1,
        extrude: 0,
        base_file_name: "atlas".into(),
        ..ProjectConfig::default()
    };

    let out = pipeline::run(&cfg).expect("pipeline should succeed");
    assert!(out.pages.len() >= 2);

    // Implicit naming: atlas.png, atlas_1.png, ... but a single data file.
    assert!(output.join("atlas.png").exists());
    assert!(output.join("atlas_1.png").exists());
    assert!(output.join("atlas.json").exists());
    assert!(!output.join("atlas_1.json").exists());

    // The user is told to add a {n1} placeholder.
    assert!(
        out.result.warnings.iter().any(|w| w.contains("{n1}")),
        "warnings: {:?}",
        out.result.warnings
    );

    // The combined file spans every page.
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("atlas.json")).unwrap()).unwrap();
    let frames = meta["frames"].as_array().unwrap();
    assert_eq!(frames.len(), out.result.total_sprites);
    let pages: std::collections::HashSet<i64> =
        frames.iter().map(|f| f["page"].as_i64().unwrap()).collect();
    assert!(pages.len() >= 2, "frames should span several pages");
    assert_eq!(
        meta["meta"]["pages"].as_array().unwrap().len(),
        out.pages.len()
    );
}

#[test]
fn lote7_variant_placeholder_and_scaled_pages() {
    let fx = Fixture::new("lote7_variants");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    for i in 0..100 {
        write_png(
            &input.join(format!("v{i}.png")),
            10,
            10,
            [0, i as u8, 0, 255],
        );
    }

    let cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output.clone(),
        max_texture_size: 64,
        padding: 1,
        extrude: 0,
        base_file_name: "paq{n1}{v}".into(),
        scale_variants: vec![1.0, 0.5],
        template_format: TemplateFormat::Json,
        ..ProjectConfig::default()
    };

    let out = pipeline::run(&cfg).expect("pipeline should succeed");
    assert!(out.pages.len() >= 2);

    // Base variant: paq1.png / paq1.json ...
    // Scaled variant: paq1-hd.png / paq1-hd.json ... (sufijo {v} de variante)
    for i in 1..=out.pages.len() {
        assert!(output.join(format!("paq{i}.png")).exists());
        assert!(output.join(format!("paq{i}.json")).exists());
        assert!(output.join(format!("paq{i}-hd.png")).exists());
        assert!(output.join(format!("paq{i}-hd.json")).exists());
    }
    // The scaled data file reports the scaled size and scale factor.
    let scaled: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("paq1-hd.json")).unwrap())
            .unwrap();
    assert_eq!(scaled["meta"]["scale"].as_str(), Some("0.5"));
    let w = scaled["meta"]["size"]["w"].as_i64().unwrap();
    assert_eq!(w, (out.pages[0].width as f32 * 0.5).round() as i64);
}

#[test]
fn lote7_multipack_disabled_errors_when_it_overflows() {
    let fx = Fixture::new("lote7_nomultipack");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    for i in 0..100 {
        write_png(
            &input.join(format!("n{i}.png")),
            10,
            10,
            [0, 0, i as u8, 255],
        );
    }

    let cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output.clone(),
        max_texture_size: 64,
        padding: 1,
        extrude: 0,
        multipack: false,
        ..ProjectConfig::default()
    };

    let err = match pipeline::run(&cfg) {
        Ok(_) => panic!("must fail without multipack"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("Multipack"), "error: {err}");
    assert!(
        !output.join("atlas.json").exists(),
        "no metadata on failure"
    );

    // Small input still packs into a single sheet with multipack disabled.
    let small_in = make_input_dir(&fx.dir, "small");
    write_png(&small_in.join("one.png"), 8, 8, [1, 2, 3, 255]);
    let ok = ProjectConfig {
        input_directory: small_in,
        output_directory: fx.dir.join("out_small"),
        max_texture_size: 64,
        multipack: false,
        ..ProjectConfig::default()
    };
    let out = pipeline::run(&ok).expect("single sheet must succeed");
    assert_eq!(out.pages.len(), 1);
    assert!(ok.output_directory.join("atlas.png").exists());
}

#[test]
fn lote8_jpg_png8_webp_and_pixel_formats_export() {
    use tp_core::config::PixelFormat;

    let fx = Fixture::new("lote8_formats");
    let input = make_input_dir(&fx.dir, "in");
    // Varios colores (gradiente) para forzar cuantización en PNG-8.
    for i in 0..16 {
        write_png(
            &input.join(format!("s{i}.png")),
            8,
            8,
            [(i * 16) as u8, 255 - (i * 16) as u8, (i * 8) as u8, 255],
        );
    }
    let run = |output: std::path::PathBuf, extra: fn(&mut ProjectConfig)| {
        let mut cfg = ProjectConfig {
            input_directory: input.clone(),
            output_directory: output,
            max_texture_size: 64,
            ..ProjectConfig::default()
        };
        extra(&mut cfg);
        pipeline::run(&cfg).expect("pipeline should succeed")
    };

    // ---- JPG: atlas.jpg existe, decodifica y queda registrado ---------------
    let out_jpg = fx.dir.join("out_jpg");
    let out = run(out_jpg.clone(), |c| c.gpu_format = GpuFormat::Jpg);
    assert!(out_jpg.join("atlas.jpg").exists());
    assert_eq!(out.result.pages[0].format, "JPG");
    let img = image::open(out_jpg.join("atlas.jpg")).expect("jpg decodes");
    assert_eq!(img.width(), out.pages[0].width as u32);
    assert_eq!(img.height(), out.pages[0].height as u32);

    // ---- PNG-8: paleta indexada de 8 bits ----------------------------------
    let out_p8 = fx.dir.join("out_png8");
    run(out_p8.clone(), |c| c.gpu_format = GpuFormat::Png8);
    let png_bytes = std::fs::read(out_p8.join("atlas.png")).unwrap();
    assert_eq!(png_bytes[25], 3, "PNG-8 debe usar color type 3 (indexado)");
    let decoded = image::load_from_memory(&png_bytes).unwrap().to_rgba8();
    let unique: std::collections::HashSet<[u8; 4]> = decoded.pixels().map(|p| p.0).collect();
    assert!(
        unique.len() <= 256,
        "PNG-8 produjo {} colores",
        unique.len()
    );

    // ---- WebP lossy (q=60): decodificable y menor que el lossless ----------
    let out_wl = fx.dir.join("out_webp_lossless");
    run(out_wl.clone(), |c| c.gpu_format = GpuFormat::WebP);
    let out_wy = fx.dir.join("out_webp_lossy");
    run(out_wy.clone(), |c| {
        c.gpu_format = GpuFormat::WebP;
        c.webp_quality = 60;
    });
    let lossless = std::fs::read(out_wl.join("atlas.webp")).unwrap();
    let lossy = std::fs::read(out_wy.join("atlas.webp")).unwrap();
    assert!(
        lossy.len() < lossless.len(),
        "{} >= {}",
        lossy.len(),
        lossless.len()
    );
    image::load_from_memory(&lossy).expect("lossy webp decodes");

    // ---- ALPHA8: escala de grises (relleno 255, fondo 0) --------------------
    let out_a8 = fx.dir.join("out_alpha8");
    run(out_a8.clone(), |c| c.pixel_format = PixelFormat::Alpha8);
    let img = image::open(out_a8.join("atlas.png")).unwrap();
    assert_eq!(img.color(), image::ColorType::L8);
    let luma = img.to_luma8();
    assert!(luma.pixels().any(|p| p.0[0] == 255), "relleno opaco");
    assert!(luma.pixels().any(|p| p.0[0] == 0), "fondo transparente");
}

#[test]
fn lote8_flip_vertical_only_for_hardware_formats() {
    let fx = Fixture::new("lote8_flip");
    let input = make_input_dir(&fx.dir, "in");

    // Sprite 16x16: mitad superior roja, mitad inferior azul.
    let mut half = image::RgbaImage::new(16, 16);
    for (_x, y, px) in half.enumerate_pixels_mut() {
        *px = if y < 8 {
            image::Rgba([220, 30, 30, 255])
        } else {
            image::Rgba([30, 30, 220, 255])
        };
    }
    half.save(input.join("half.png")).unwrap();

    // (a) Formato de software + flip: se avisa y el PNG no se voltea.
    let out_png = fx.dir.join("out_png");
    let cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: out_png.clone(),
        max_texture_size: 64,
        gpu_format: GpuFormat::Png,
        flip_vertical: true,
        ..ProjectConfig::default()
    };
    let out = pipeline::run(&cfg).unwrap();
    assert!(
        out.result.warnings.iter().any(|w| w.contains("hardware")),
        "warnings: {:?}",
        out.result.warnings
    );
    let decoded = image::open(out_png.join("atlas.png")).unwrap().to_rgba8();
    let top = decoded.get_pixel(2, 2).0;
    let bottom = decoded.get_pixel(2, decoded.height() - 3).0;
    assert!(
        top[0] > 150 && top[2] < 100,
        "fila superior sin flip: {top:?}"
    );
    assert!(
        bottom[2] > 150 && bottom[0] < 100,
        "fila inferior sin flip: {bottom:?}"
    );

    // (b) ETC2 + flip: las filas del atlas quedan volteadas.
    let out_off = fx.dir.join("out_off");
    let cfg_off = ProjectConfig {
        input_directory: input.clone(),
        output_directory: out_off.clone(),
        max_texture_size: 64,
        gpu_format: GpuFormat::Etc2Rgba,
        flip_vertical: false,
        ..ProjectConfig::default()
    };
    let packed_off = pipeline::run(&cfg_off).unwrap();
    let out_on = fx.dir.join("out_on");
    let cfg_on = ProjectConfig {
        input_directory: input.clone(),
        output_directory: out_on.clone(),
        max_texture_size: 64,
        gpu_format: GpuFormat::Etc2Rgba,
        flip_vertical: true,
        ..ProjectConfig::default()
    };
    pipeline::run(&cfg_on).unwrap();

    let w = packed_off.pages[0].width as usize;
    let h = packed_off.pages[0].height as usize;
    let decode_ktx = |path: &Path| -> Vec<u32> {
        let bytes = std::fs::read(path).unwrap();
        let payload = &bytes[68..]; // cabecera KTX (64) + imageSize (4)
        let mut buf = vec![0u32; w * h];
        texture2ddecoder::decode_etc2_rgba8(payload, w, h, &mut buf).unwrap();
        buf
    };
    let off = decode_ktx(&out_off.join("atlas.ktx"));
    let on = decode_ktx(&out_on.join("atlas.ktx"));
    // Cada píxel es [b,g,r,a] en little-endian.
    let red = |buf: &[u32], x: usize, y: usize| ((buf[y * w + x] >> 16) & 0xff) as u8;
    let blue = |buf: &[u32], x: usize, y: usize| (buf[y * w + x] & 0xff) as u8;
    assert!(red(&off, 4, 4) > 150, "sin flip la fila 0 es roja");
    assert!(
        blue(&off, 4, h - 5) > 150,
        "sin flip la última fila es azul"
    );
    assert!(blue(&on, 4, 4) > 150, "con flip la fila 0 es azul");
    assert!(red(&on, 4, h - 5) > 150, "con flip la última fila es roja");
}

#[test]
fn lote8_extra_input_formats_roundtrip() {
    let fx = Fixture::new("extra_formats");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");
    write_png(&input.join("base.png"), 8, 8, [10, 20, 30, 255]);

    // Solid TGA and QOI must be ingested exactly like a PNG.
    let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
    img.save(input.join("solid.tga")).unwrap();
    img.save(input.join("solid.qoi")).unwrap();

    // Sprite with visible content (2x2 red block at top-left) in BMP.
    write_corner_png(&input.join("corner.bmp"), 8, 8, [200, 200, 200, 255]);

    let cfg = ProjectConfig {
        input_directory: input,
        output_directory: output,
        ..ProjectConfig::default()
    };
    let out = pipeline::run(&cfg).unwrap();
    let mut names: Vec<&str> = out.result.sprites.iter().map(|s| s.id.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["base", "corner", "solid", "solid"],
        "los 4 sprites deben ingestarse con id por nombre (2 aliases de 'solid')"
    );

    // Los 2 'solid' son alias del primero (mismo hash de píxeles).
    assert_eq!(out.result.alias_count, 2, "solid.tga/solid.qoi son aliases");

    // Unpack del atlas: el sprite 'corner' debe conservar su contenido.
    let corner = out
        .result
        .sprites
        .iter()
        .find(|s| s.id == "corner")
        .unwrap();
    assert!(!corner.is_rotated, "8x8 cabe sin rotar en 256x256");
    let page = &out.pages[corner.atlas_page_index as usize];
    let px = |x: i32, y: i32| -> [u8; 4] {
        let f = &corner.visible_frame;
        let i = ((f.y + y) * page.width + (f.x + x)) as usize * 4;
        [
            page.pixels[i],
            page.pixels[i + 1],
            page.pixels[i + 2],
            page.pixels[i + 3],
        ]
    };
    assert_eq!(px(0, 0), [255, 0, 0, 255], "píxel (0,0) es el rojo del BMP");
    assert_eq!(px(2, 2), [200, 200, 200, 255], "relleno gris intacto");

    // La salida en disco también es PNG estándar decodificable.
    assert!(fx.dir.join("out").join("atlas.png").exists());
}

#[test]
fn lote9_borders_from_file_and_metadata() {
    let fx = Fixture::new("borders");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    // Sprite con contenido visible y bordes 9-patch declarados en borders.json
    // (medidos sobre la imagen SIN recortar: 2px transparentes por lado).
    write_corner_png(&input.join("panel.png"), 16, 16, [90, 90, 90, 255]);
    std::fs::write(input.join("borders.json"), r#"{ "panel": [4, 6, 5, 3] }"#).unwrap();

    let cfg = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output,
        template_format: TemplateFormat::Json,
        ..ProjectConfig::default()
    };
    let out = pipeline::run(&cfg).unwrap();

    // El sprite cargó su borde desde borders.json.
    let panel = out
        .result
        .sprites
        .iter()
        .find(|s| s.id == "panel")
        .expect("sprite panel");
    assert_eq!(panel.border, Some([4, 6, 5, 3]));

    // El metadato JSON expone el borde por cara.
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fx.dir.join("out").join("atlas.json")).unwrap(),
    )
    .unwrap();
    let frame = &meta["frames"][0];
    assert_eq!(frame["filename"], "panel");
    assert_eq!(frame["border"]["left"], 4);
    assert_eq!(frame["border"]["top"], 6);
    assert_eq!(frame["border"]["right"], 5);
    assert_eq!(frame["border"]["bottom"], 3);

    // Sin borders.json no hay borde en el resultado.
    std::fs::remove_file(input.join("borders.json")).unwrap();
    let out2 = pipeline::run(&cfg).unwrap();
    assert!(out2.result.sprites[0].border.is_none());
}

#[test]
fn lote10_auto_detect_animations_metadata() {
    let fx = Fixture::new("animations");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    // walk_001..003 definen la animación `walk`; hero queda fuera.
    for (name, color) in [
        ("walk_001", [10, 0, 0, 255]),
        ("walk_002", [20, 0, 0, 255]),
        ("walk_003", [30, 0, 0, 255]),
        ("hero", [40, 0, 0, 255]),
    ] {
        write_png(&input.join(format!("{name}.png")), 8, 8, color);
    }

    let base = ProjectConfig {
        input_directory: input.clone(),
        output_directory: output.clone(),
        template_format: TemplateFormat::Json,
        ..ProjectConfig::default()
    };

    // Por defecto (activado): el JSON expone la animación con sus frames.
    let _out = pipeline::run(&base).unwrap();
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("atlas.json")).unwrap()).unwrap();
    let anims = meta["animations"].as_array().expect("animations array");
    assert_eq!(anims.len(), 1, "solo `walk` es una animación: {anims:?}");
    assert_eq!(anims[0]["name"], "walk");
    assert_eq!(
        anims[0]["frames"],
        serde_json::json!(["walk_001", "walk_002", "walk_003"])
    );

    // Plist (cocos2d): la sección `animations` aparece en el metadata dict.
    let mut plist_cfg = base.clone();
    plist_cfg.template_format = TemplateFormat::Plist;
    let _ = pipeline::run(&plist_cfg).unwrap();
    let plist = std::fs::read_to_string(output.join("atlas.plist")).unwrap();
    assert!(plist.contains("<key>animations</key>"));
    assert!(plist.contains("<key>walk</key>"));
    assert!(plist.contains("<string>walk_002</string>"));

    // Desactivado: no hay sección de animaciones.
    let mut off = base.clone();
    off.enable_auto_detect_animations = false;
    let _ = pipeline::run(&off).unwrap();
    let meta_off: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("atlas.json")).unwrap()).unwrap();
    assert!(meta_off["animations"].as_array().unwrap().is_empty());
}

#[test]
fn lote10_pixel_formats_rgba5551_and_bgra8888() {
    let fx = Fixture::new("pxfmt");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");
    write_png(&input.join("px.png"), 8, 8, [120, 60, 240, 255]);

    let base = ProjectConfig {
        input_directory: input,
        output_directory: output.clone(),
        ..ProjectConfig::default()
    };

    // BGRA8888: el atlas en disco lleva R y B intercambiados.
    let mut bgra = base.clone();
    bgra.pixel_format = tp_core::config::PixelFormat::Bgra8888;
    let out = pipeline::run(&bgra).unwrap();
    let sheet = image::open(output.join("atlas.png")).unwrap().to_rgba8();
    let (x, y) = {
        let s = &out.result.sprites[0];
        (s.visible_frame.x, s.visible_frame.y)
    };
    assert_eq!(sheet.get_pixel(x as u32, y as u32).0, [240, 60, 120, 255]);

    // RGBA5551: cada canal queda en la rejilla de 5 bits expandida por
    // replicación (expand5(q(c))) y el alfa es 0/255. Entrada [120, 60, 240].
    let mut p5551 = base.clone();
    p5551.pixel_format = tp_core::config::PixelFormat::Rgba5551;
    pipeline::run(&p5551).unwrap();
    let sheet = image::open(output.join("atlas.png")).unwrap().to_rgba8();
    let px = sheet.get_pixel(x as u32, y as u32).0;
    let expand5 = |v: u8| (v << 3) | (v >> 2);
    let q = |c: u8| ((c as u16 * 31 + 127) / 255) as u8;
    assert_eq!(px, [expand5(q(120)), expand5(q(60)), expand5(q(240)), 255]);
    // Solo hay 32 niveles posibles por canal (los valores de expand5(0..=31)).
    let valid: Vec<u8> = (0..=31u8).map(expand5).collect();
    for c in &px[..3] {
        assert!(valid.contains(c), "canal {c} fuera de la rejilla 5 bits");
    }

    // RGBA5555: como 5551 pero el alfa también queda en la rejilla de 5 bits
    // (100 → expand5(q(100)) = 99, no 0/255).
    let mut p5555 = base.clone();
    p5555.pixel_format = tp_core::config::PixelFormat::Rgba5555;
    pipeline::run(&p5555).unwrap();
    let sheet = image::open(output.join("atlas.png")).unwrap().to_rgba8();
    let px = sheet.get_pixel(x as u32, y as u32).0;
    assert_eq!(
        px,
        [
            expand5(q(120)),
            expand5(q(60)),
            expand5(q(240)),
            expand5(q(255))
        ]
    );
    assert!(
        valid.contains(&px[3]),
        "alfa { } fuera de la rejilla 5 bits",
        px[3]
    );
}

#[test]
fn run_preview_does_not_write_any_file() {
    let fx = Fixture::new("preview");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    write_png(&input.join("a.png"), 24, 24, [200, 30, 30, 255]);
    write_png(&input.join("b.png"), 30, 18, [30, 200, 30, 255]);

    let cfg = ProjectConfig {
        input_directory: input,
        output_directory: output.clone(),
        ..ProjectConfig::default()
    };

    // Vista previa: resultado válido en memoria y salida intacta.
    let preview = tp_core::pipeline::run_preview(&cfg).unwrap();
    assert_eq!(preview.result.total_sprites, 2);
    assert!(!preview.pages.is_empty());
    assert!(
        !output.exists(),
        "run_preview no debe crear el directorio de salida"
    );

    // El mismo config, publicado, sí escribe los ficheros.
    let published = tp_core::pipeline::run(&cfg).unwrap();
    assert_eq!(published.result.total_sprites, 2);
    assert!(output.join("atlas.png").is_file());
    assert!(output.join("atlas.json").is_file());
}

#[test]
fn manual_algorithm_keeps_gui_positions() {
    let fx = Fixture::new("manual");
    let input = make_input_dir(&fx.dir, "in");
    let output = fx.dir.join("out");

    write_png(&input.join("a.png"), 20, 12, [255, 0, 0, 255]);
    write_png(&input.join("b.png"), 10, 10, [0, 255, 0, 255]);

    let mut positions = std::collections::HashMap::new();
    // Posición manual del sprite "a": (15, 3).
    positions.insert("a".to_string(), (15, 3));
    positions.insert("b".to_string(), (40, 30));
    let cfg = ProjectConfig {
        input_directory: input,
        output_directory: output.clone(),
        algorithm: tp_core::config::PackingAlgorithm::Manual,
        pack_mode: PackMode::Fast,
        trim_mode: tp_core::config::TrimMode::Trim,
        manual_positions: positions,
        ..ProjectConfig::default()
    };

    let out = tp_core::pipeline::run(&cfg).unwrap();
    let sheet = output.join("atlas.png");
    assert!(sheet.is_file());

    // Cada sprite termina exactamente donde se pidió (+ borde/padding).
    let find = |id: &str| {
        out.result
            .sprites
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .visible_frame
    };
    let a = find("a");
    assert_eq!((a.x, a.y), (15 + 2, 3 + 2), "posición manual no respetada");
    let b = find("b");
    assert_eq!((b.x, b.y), (40 + 2, 30 + 2));
    assert!(!a.intersects(&b));
}
