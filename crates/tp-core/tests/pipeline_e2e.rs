//! End-to-end pipeline tests: generate real PNG sprites on disk and run the
//! full pipeline (trim -> hash/alias -> pack -> blit -> quantize -> export ->
//! templates), verifying the on-disk outputs.

use std::path::{Path, PathBuf};
use tp_core::config::{
    ColorDepth, DitheringAlgorithm, GpuFormat, PackingStrategy, ProjectConfig, TemplateFormat,
};
use tp_core::types::Rect;
use tp_core::{pipeline, export};

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
        let dir = std::env::temp_dir().join(format!(
            "tp_e2e_{name}_{}",
            std::process::id()
        ));
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

    let mut cfg = ProjectConfig::default();
    cfg.input_directory = input.clone();
    cfg.output_directory = output.clone();
    cfg.max_texture_size = 64;
    cfg.padding = 1;
    cfg.extrude = 1;
    cfg.allow_rotation = true;
    cfg.enable_trim = true;
    cfg.enable_aliasing = true;
    cfg.enable_normal_maps = true;
    cfg.template_format = TemplateFormat::Json;
    cfg.packing_strategy = PackingStrategy::Bssf;

    let out = pipeline::run(&cfg).expect("pipeline should succeed");
    let result = &out.result;

    // ---- Output files exist -------------------------------------------------
    assert!(!result.output_files.is_empty());
    for f in &result.output_files {
        assert!(
            output.join(f).exists(),
            "missing output file: {f}"
        );
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
    assert_eq!(&page.pixels[i..i + 4], &[255, 0, 0, 255], "rotated red corner");

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

    let mut cfg = ProjectConfig::default();
    cfg.input_directory = input;
    cfg.output_directory = output.clone();
    cfg.max_texture_size = 64;
    cfg.padding = 1;
    cfg.extrude = 0;
    cfg.encryption_key = Some("clave-secreta".into());
    cfg.color_depth = ColorDepth::Rgba4444;
    cfg.dithering_algorithm = DitheringAlgorithm::FloydSteinberg;
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
    assert!(output.join("atlas_0.5x.json").exists());

    // The @1x variant has half-sized frame coordinates.
    let half: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("atlas_0.5x.json")).unwrap())
            .unwrap();
    let full: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(output.join("atlas.json")).unwrap())
            .unwrap();
    let full_frame = &full["frames"][0]["frame"];
    let half_frame = &half["frames"][0]["frame"];
    assert_eq!(half_frame["x"].as_i64().unwrap(), full_frame["x"].as_i64().unwrap() / 2);

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

    let mut cfg = ProjectConfig::default();
    cfg.input_directory = input;
    cfg.output_directory = output.clone();
    cfg.max_texture_size = 64;
    cfg.enable_polygon = true;
    cfg.polygon_tolerance = 1.0;
    cfg.packing_strategy = PackingStrategy::Guillotine;
    cfg.template_format = TemplateFormat::PlainText;
    cfg.export_template = Some(tpl);

    let out = pipeline::run(&cfg).unwrap();
    let result = &out.result;

    // Polygons built for non-alias sprites.
    let circle = result.sprites.iter().find(|s| s.id == "circle").unwrap();
    assert!(circle.mesh.is_some(), "polygon mode should build meshes");
    let mesh = circle.mesh.as_ref().unwrap();
    assert!(!mesh.vertices.is_empty());
    assert_eq!(mesh.uvs.len(), mesh.vertices.len());
    assert!(!mesh.indices.is_empty());
    assert!(mesh.indices.iter().all(|&i| (i as usize) < mesh.vertices.len()));

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
    let mut cfg = ProjectConfig::default();
    cfg.input_directory = input.clone();
    cfg.output_directory = output.clone();
    cfg.max_texture_size = 64;
    cfg.gpu_format = GpuFormat::Etc2Rgba;
    let _out = pipeline::run(&cfg).unwrap();
    let ktx = output.join("atlas.ktx");
    assert!(ktx.exists());
    let bytes = std::fs::read(&ktx).unwrap();
    assert_eq!(&bytes[..12], b"\xABKTX 11\xBB\r\n\x1A\n");

    // PVRTC (built-in encoder) — works without any feature flags.
    cfg.gpu_format = GpuFormat::Pvrtc4Bpp;
    let _out = pipeline::run(&cfg).unwrap();
    let pvr = output.join("atlas.pvr");
    assert!(pvr.exists());
    let bytes = std::fs::read(&pvr).unwrap();
    assert_eq!(&bytes[..4], b"PVR\x03");
    // Payload after the 52-byte PVR v3 header: 64x64 at 4bpp = 2048 bytes,
    // and it must decode through an independent decoder.
    let payload = &bytes[52..];
    assert_eq!(payload.len(), 64 * 64 / 2);
    let mut buf = vec![0u32; 64 * 64];
    texture2ddecoder::decode_pvrtc_4bpp(payload, 64, 64, &mut buf).unwrap();

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

    let mut cfg = ProjectConfig::default();
    cfg.input_directory = input;
    cfg.output_directory = output;
    cfg.max_texture_size = 128;
    match pipeline::run(&cfg) {
        Err(err) => assert!(err.contains("huge"), "error should name the sprite: {err}"),
        Ok(_) => panic!("expected an error for an oversized sprite"),
    }
}
