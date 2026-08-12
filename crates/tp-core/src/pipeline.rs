//! Pipeline de Procesamiento (secuencia de ejecución PASO 1..11).
//!
//! 1.  Leer `ProjectConfig` y cargar la lista de imágenes.
//! 2.  [paralelo] Trimming + hash por imagen.
//! 3.  [secuencial] Resolver aliases (deduplicación).
//! 4.  [opcional] Contorno -> RDP -> Earcut (sprites no-alias).
//! 5.  Ordenar por área (descendente) — dentro del packer.
//! 6.  Insertar en MaxRects / Guillotine (nuevas páginas si hace falta).
//! 7.  Inicializar buffers de imagen por página.
//! 8.  [paralelo] Copiar píxeles (extrude, rotación, normal maps).
//! 9.  Cuantizar + dithering (si color_depth != RGBA8888).
//! 10. Cifrar si hay clave.
//! 11. Guardar imágenes + renderizar plantilla de metadatos.

use crate::config::ProjectConfig;
use crate::export;
use crate::ingest::{self, IngestedSprite};
use crate::pack::{self, PackItem, PackerOptions};
use crate::pixels;
use crate::polygon;
use crate::templates;
use crate::types::{AtlasPage, PackResult, PageInfo, Point2D, Rect, SpriteAsset};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

/// Result of a full pipeline run: serializable summary + in-memory pages
/// (for the GUI preview).
pub struct PipelineOutput {
    pub result: PackResult,
    pub pages: Vec<AtlasPage>,
}

/// Run the whole packing pipeline for a project configuration.
pub fn run(config: &ProjectConfig) -> Result<PipelineOutput, String> {
    config.validate()?;
    let mut stage_times: Vec<(String, u64)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // ------------------------------------------------------------------
    // PASO 1 + 2: discover, load (parallel), trim, hash
    // ------------------------------------------------------------------
    let t = Instant::now();
    let ingested = ingest::ingest(
        &config.input_directory,
        config.trim_threshold,
        config.enable_normal_maps,
        config.recursive,
    );
    warnings.extend(ingested.warnings);
    if ingested.sprites.is_empty() {
        return Err("No se encontraron sprites válidos en el directorio de entrada".into());
    }
    let sprites = ingested.sprites;
    stage_times.push(("ingest".into(), t.elapsed().as_millis() as u64));

    // ------------------------------------------------------------------
    // PASO 3: alias resolution
    // ------------------------------------------------------------------
    let t = Instant::now();
    let aliases = ingest::resolve_aliases(&sprites);
    let alias_count = aliases.iter().filter(|(a, _)| *a).count();
    stage_times.push(("aliasing".into(), t.elapsed().as_millis() as u64));

    let id_to_index: HashMap<&str, usize> = sprites
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.as_str(), i))
        .collect();

    // ------------------------------------------------------------------
    // PASO 4 (optional): polygon engine (contour -> RDP -> earcut)
    // ------------------------------------------------------------------
    let t = Instant::now();
    let mut meshes: Vec<Option<polygon::Polygons>> = vec![None; sprites.len()];
    if config.enable_polygon {
        meshes = sprites
            .par_iter()
            .enumerate()
            .map(|(i, s)| {
                if aliases[i].0 {
                    return None;
                }
                let tw = s.trimmed_bounds.width as usize;
                let th = s.trimmed_bounds.height as usize;
                let alpha: Vec<u8> = s.pixels.chunks_exact(4).map(|px| px[3]).collect();
                polygon::build_polygons(&alpha, tw as i32, th as i32, config.polygon_tolerance)
            })
            .collect();
    }
    stage_times.push(("polygons".into(), t.elapsed().as_millis() as u64));

    // ------------------------------------------------------------------
    // PASO 5 + 6: pack (sort by area inside the packer, multi-atlas)
    // ------------------------------------------------------------------
    let t = Instant::now();
    let mut item_ids: Vec<usize> = Vec::new();
    let mut items: Vec<PackItem> = Vec::new();
    for (i, s) in sprites.iter().enumerate() {
        if aliases[i].0 {
            continue;
        }
        item_ids.push(i);
        items.push(PackItem {
            id: s.id.clone(),
            width: s.trimmed_bounds.width,
            height: s.trimmed_bounds.height,
            mesh: meshes[i].as_ref().map(|m| m.mesh.clone()),
        });
    }
    let opts = PackerOptions::new(
        config.packing_strategy,
        config.allow_rotation,
        config.max_texture_size,
        config.padding,
        config.enable_polygon,
    );
    let pack_out = pack::pack(&items, &opts)?;
    stage_times.push(("packing".into(), t.elapsed().as_millis() as u64));

    // Frame bookkeeping: non-alias sprites get their placement.
    let mut frame: Vec<Option<Rect>> = vec![None; sprites.len()];
    let mut rotated: Vec<bool> = vec![false; sprites.len()];
    let mut page_idx: Vec<i32> = vec![-1; sprites.len()];
    for p in &pack_out.pages {
        for pl in &p.placements {
            if let Some(&i) = id_to_index.get(pl.id.as_str()) {
                frame[i] = Some(pl.frame);
                rotated[i] = pl.rotated;
                page_idx[i] = p.index as i32;
            }
        }
    }

    // Aliases reuse the frame of their target.
    for (i, _s) in sprites.iter().enumerate() {
        if aliases[i].0 {
            if let Some(target) = &aliases[i].1 {
                if let Some(&ti) = id_to_index.get(target.as_str()) {
                    frame[i] = frame[ti];
                    rotated[i] = rotated[ti];
                    page_idx[i] = page_idx[ti];
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // PASO 7: allocate page buffers (+ normal-map canvases)
    // ------------------------------------------------------------------
    let mut pages: Vec<AtlasPage> = pack_out
        .pages
        .iter()
        .map(|p| AtlasPage::new(p.index, p.width, p.height))
        .collect();
    let has_normals = config.enable_normal_maps
        && sprites.iter().any(|s| s.normal_path.is_some());
    if has_normals {
        for page in &mut pages {
            page.normal_pixels = Some(vec![0u8; (page.width * page.height * 4) as usize]);
            page.has_normals = true;
        }
    }

    // Preload normal-map pixels (parallel).
    let t = Instant::now();
    let normal_load: Vec<Result<Option<(i32, i32, Vec<u8>)>, String>> = sprites
        .par_iter()
        .map(|s| match &s.normal_path {
            Some(p) => ingest::load_image_rgba(p).map(Some),
            None => Ok(None),
        })
        .collect();
    let mut normal_images: Vec<Option<(i32, i32, Vec<u8>)>> = Vec::with_capacity(normal_load.len());
    for item in normal_load {
        match item {
            Ok(img) => normal_images.push(img),
            Err(e) => {
                warnings.push(e);
                normal_images.push(None);
            }
        }
    }
    stage_times.push(("normal-load".into(), t.elapsed().as_millis() as u64));

    // ------------------------------------------------------------------
    // PASO 8: blit sprites into page buffers (parallel)
    // ------------------------------------------------------------------
    let t = Instant::now();
    let page_pixels: Vec<Mutex<Vec<u8>>> = pages
        .iter()
        .map(|p| Mutex::new(p.pixels.clone()))
        .collect();
    let page_normals: Vec<Mutex<Vec<u8>>> = pages
        .iter()
        .map(|p| Mutex::new(p.normal_pixels.clone().unwrap_or_default()))
        .collect();

    let pad = config.padding.max(0);
    let extrude = config.extrude.max(0);

    (0..sprites.len()).into_par_iter().for_each(|i| {
        let s = &sprites[i];
        if aliases[i].0 {
            return;
        }
        let Some(fr) = frame[i] else { return };
        let pi = page_idx[i] as usize;
        if pi >= page_pixels.len() {
            return;
        }
        let page = &pages[pi];
        let mut px = page_pixels[pi].lock().unwrap();
        pixels::blit_sprite(
            &mut px,
            page.width,
            page.height,
            fr,
            pad,
            extrude,
            &s.pixels,
            s.trimmed_bounds.width,
            s.trimmed_bounds.height,
            rotated[i],
        );
        drop(px);

        if let Some((nw, nh, npix)) = &normal_images[i] {
            let mut np = page_normals[pi].lock().unwrap();
            blit_normal(
                &mut np,
                page.width,
                page.height,
                fr,
                pad,
                extrude,
                npix,
                *nw,
                *nh,
                s,
                rotated[i],
            );
        }
    });

    for (i, page) in pages.iter_mut().enumerate() {
        page.pixels = page_pixels[i].lock().unwrap().clone();
        if page.normal_pixels.is_some() {
            page.normal_pixels = Some(page_normals[i].lock().unwrap().clone());
        }
    }
    stage_times.push(("blit".into(), t.elapsed().as_millis() as u64));

    // ------------------------------------------------------------------
    // PASO 9: quantization + dithering (parallel over pages)
    // ------------------------------------------------------------------
    let t = Instant::now();
    let depth = config.color_depth;
    let dither = config.dithering_algorithm;
    pages.par_iter_mut().for_each(|p| {
        pixels::apply_quantization(
            &mut p.pixels,
            p.width as usize,
            p.height as usize,
            depth,
            dither,
        );
        if let Some(n) = &mut p.normal_pixels {
            pixels::apply_quantization(n, p.width as usize, p.height as usize, depth, dither);
        }
    });
    stage_times.push(("quantize".into(), t.elapsed().as_millis() as u64));

    // ------------------------------------------------------------------
    // Assemble SpriteAsset list
    // ------------------------------------------------------------------
    let pivot_overrides = ingest::load_pivot_overrides(&config.input_directory).unwrap_or_default();
    let mut sprite_assets: Vec<SpriteAsset> = Vec::with_capacity(sprites.len());
    let alias_targets: std::collections::HashSet<String> = aliases
        .iter()
        .filter_map(|(_, t)| t.clone())
        .collect();

    for (i, s) in sprites.iter().enumerate() {
        let (is_alias, alias_target) = aliases[i].clone();
        let pivot = pivot_overrides
            .get(&s.id)
            .copied()
            .unwrap_or(Point2D::new(config.default_pivot_x, config.default_pivot_y));

        let fr = frame[i].unwrap_or_default();
        let vis = Rect::new(
            fr.x + pad,
            fr.y + pad,
            (fr.width - 2 * pad).max(0),
            (fr.height - 2 * pad).max(0),
        );
        let pi = page_idx[i].max(0) as usize;

        let (mesh, contours) = match &meshes[i] {
            Some(poly) => {
                let page = &pages[pi];
                // UV frame: visible region origin + local trimmed dims.
                let uv_frame = Rect::new(
                    vis.x,
                    vis.y,
                    s.trimmed_bounds.width,
                    s.trimmed_bounds.height,
                );
                let uvs = polygon::compute_uvs(
                    &poly.mesh.vertices,
                    &uv_frame,
                    page.width,
                    page.height,
                    rotated[i],
                );
                let mut mesh = poly.mesh.clone();
                mesh.uvs = uvs;
                (Some(mesh), poly.contours.clone())
            }
            None => (None, vec![]),
        };

        sprite_assets.push(SpriteAsset {
            id: s.id.clone(),
            source_path: s.source_path.display().to_string(),
            raw_width: s.raw_width,
            raw_height: s.raw_height,
            trimmed_bounds: s.trimmed_bounds,
            offset_x: s.trimmed_bounds.x,
            offset_y: s.trimmed_bounds.y,
            pixel_hash: s.pixel_hash.clone(),
            is_alias,
            alias_target_id: alias_target,
            pivot,
            mesh,
            allocated_frame: fr,
            visible_frame: vis,
            is_rotated: rotated[i],
            atlas_page_index: page_idx[i],
            normal_source_path: s.normal_path.as_ref().map(|p| p.display().to_string()),
            is_alias_target: alias_targets.contains(&s.id),
            contours,
        });
    }

    // ------------------------------------------------------------------
    // PASO 10 + 11: export images (+ variants), encryption, metadata
    // ------------------------------------------------------------------
    let t = Instant::now();
    let output_dir = &config.output_directory;
    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("No se pudo crear {}: {e}", output_dir.display()))?;

    let mut output_files: Vec<String> = Vec::new();
    let mut base_page_infos: Vec<PageInfo> = Vec::new();

    for scale in &config.scale_variants {
        let is_base = (*scale - 1.0).abs() < 1e-6;
        let variant = if is_base {
            String::new()
        } else {
            format!("_{scale}x")
        };

        let mut variant_page_infos: Vec<PageInfo> = Vec::new();
        let mut variant_image_files: Vec<String> = Vec::new();

        for page in &pages {
            let (sw, sh) = if is_base {
                (page.width as usize, page.height as usize)
            } else {
                (
                    ((page.width as f32) * scale).round().max(1.0) as usize,
                    ((page.height as f32) * scale).round().max(1.0) as usize,
                )
            };

            let file_name = page_file_name(config, page.index, &variant);
            let bytes = {
                let (scaled, _w, _h) = if is_base {
                    (page.pixels.clone(), sw, sh)
                } else {
                    export::scale_rgba(&page.pixels, page.width as usize, page.height as usize, *scale)
                };
                export::encode_to_bytes(&scaled, _w, _h, config.gpu_format)?
            };

            let (final_name, final_bytes) = match &config.encryption_key {
                Some(key) => (
                    format!("{file_name}.tpenc"),
                    export::encrypt_bytes(&bytes, key)?,
                ),
                None => (file_name, bytes),
            };

            write_file(&output_dir.join(&final_name), &final_bytes)?;
            output_files.push(final_name.clone());

            // Normal-map page.
            let mut normal_name = None;
            if let Some(npix) = &page.normal_pixels {
                let nfile = normal_page_file_name(config, page.index, &variant);
                let (nscaled, nw2, nh2) = if is_base {
                    (npix.clone(), sw, sh)
                } else {
                    export::scale_rgba(npix, page.width as usize, page.height as usize, *scale)
                };
                let nbytes = export::encode_to_bytes(&nscaled, nw2, nh2, config.gpu_format)?;
                let (nfinal_name, nfinal_bytes) = match &config.encryption_key {
                    Some(key) => (
                        format!("{nfile}.tpenc"),
                        export::encrypt_bytes(&nbytes, key)?,
                    ),
                    None => (nfile, nbytes),
                };
                write_file(&output_dir.join(&nfinal_name), &nfinal_bytes)?;
                output_files.push(nfinal_name.clone());
                normal_name = Some(nfinal_name);
            }

            variant_image_files.push(final_name.clone());
            variant_page_infos.push(PageInfo {
                index: page.index,
                width: sw as i32,
                height: sh as i32,
                file_name: final_name,
                format: config.gpu_format.as_str().to_string(),
                has_normals: page.has_normals,
                normal_file_name: normal_name,
                encrypted: config.encryption_key.is_some(),
                fill_ratio: fill_ratio(&page.pixels, page.width, page.height),
            });
        }

        if is_base {
            base_page_infos = variant_page_infos.clone();
        }

        // Metadata via template engine.
        let meta_name = metadata_file_name(config, &variant);
        let content = templates::render(
            &pending_result(config, &sprite_assets, &warnings, &stage_times, output_files.clone(), alias_count, &base_page_infos),
            &variant_page_infos,
            &variant_image_files,
            *scale,
            config,
        )?;
        write_file(&output_dir.join(&meta_name), content.as_bytes())?;
        output_files.push(meta_name);
    }
    stage_times.push(("export".into(), t.elapsed().as_millis() as u64));

    let result = PackResult {
        config: config.clone(),
        sprites: sprite_assets,
        pages: base_page_infos,
        warnings,
        stage_times_ms: stage_times,
        total_sprites: sprites.len(),
        alias_count,
        output_files,
    };

    Ok(PipelineOutput { result, pages })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Sample the normal-map pixels that correspond to the diffuse's trimmed
/// bounds, then blit them into the frame with the same rotation/extrude.
fn blit_normal(
    page: &mut [u8],
    page_w: i32,
    page_h: i32,
    frame: Rect,
    padding: i32,
    extrude: i32,
    normal: &[u8],
    nw: i32,
    nh: i32,
    sprite: &IngestedSprite,
    rotated: bool,
) {
    let (tw, th) = (sprite.trimmed_bounds.width, sprite.trimmed_bounds.height);
    if tw <= 0 || th <= 0 {
        return;
    }
    // If the normal map has the original sprite's dimensions, copy the same
    // sub-rect; otherwise scale the whole map to the original size first.
    let trimmed: Vec<u8> = if nw == sprite.raw_width && nh == sprite.raw_height {
        let mut out = vec![0u8; (tw * th * 4) as usize];
        for y in 0..th {
            let src_row = (sprite.trimmed_bounds.y + y) * nw + sprite.trimmed_bounds.x;
            let dst_row = y * tw;
            out[(dst_row * 4) as usize..((dst_row + tw) * 4) as usize]
                .copy_from_slice(&normal[(src_row * 4) as usize..((src_row + tw) * 4) as usize]);
        }
        out
    } else {
        // Nearest-neighbor scale to the original size, then sub-rect copy.
        let mut scaled = vec![0u8; (sprite.raw_width * sprite.raw_height * 4) as usize];
        for y in 0..sprite.raw_height {
            let sy = ((y as f32 / sprite.raw_height as f32) * nh as f32).min(nh as f32 - 1.0) as i32;
            for x in 0..sprite.raw_width {
                let sx = ((x as f32 / sprite.raw_width as f32) * nw as f32).min(nw as f32 - 1.0) as i32;
                let src = ((sy * nw + sx) * 4) as usize;
                let dst = ((y * sprite.raw_width + x) * 4) as usize;
                scaled[dst..dst + 4].copy_from_slice(&normal[src..src + 4]);
            }
        }
        let mut out = vec![0u8; (tw * th * 4) as usize];
        for y in 0..th {
            let src_row = (sprite.trimmed_bounds.y + y) * sprite.raw_width + sprite.trimmed_bounds.x;
            let dst_row = y * tw;
            out[(dst_row * 4) as usize..((dst_row + tw) * 4) as usize]
                .copy_from_slice(&scaled[(src_row * 4) as usize..((src_row + tw) * 4) as usize]);
        }
        out
    };

    pixels::blit_sprite(
        page,
        page_w,
        page_h,
        frame,
        padding,
        extrude,
        &trimmed,
        tw,
        th,
        rotated,
    );
}

fn page_file_name(config: &ProjectConfig, index: usize, variant: &str) -> String {
    if index == 0 {
        format!("{}{}.{}", config.base_file_name, variant, config.gpu_format.file_extension())
    } else {
        format!(
            "{}_{}{}.{}",
            config.base_file_name,
            index,
            variant,
            config.gpu_format.file_extension()
        )
    }
}

fn normal_page_file_name(config: &ProjectConfig, index: usize, variant: &str) -> String {
    if index == 0 {
        format!("{}_normal{}.{}", config.base_file_name, variant, config.gpu_format.file_extension())
    } else {
        format!(
            "{}_normal_{}{}.{}",
            config.base_file_name,
            index,
            variant,
            config.gpu_format.file_extension()
        )
    }
}

fn metadata_file_name(config: &ProjectConfig, variant: &str) -> String {
    format!(
        "{}{}.{}",
        config.base_file_name,
        variant,
        templates::metadata_extension(config.template_format)
    )
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("No se pudo escribir {}: {e}", path.display()))
}

fn fill_ratio(pixels: &[u8], w: i32, h: i32) -> f32 {
    if w <= 0 || h <= 0 {
        return 0.0;
    }
    let mut solid = 0i64;
    for px in pixels.chunks_exact(4) {
        if px[3] > 0 {
            solid += 1;
        }
    }
    solid as f32 / (w * h) as f32
}

fn pending_result(
    config: &ProjectConfig,
    sprites: &[SpriteAsset],
    warnings: &[String],
    stage_times: &[(String, u64)],
    output_files: Vec<String>,
    alias_count: usize,
    base_pages: &[PageInfo],
) -> PackResult {
    PackResult {
        config: config.clone(),
        sprites: sprites.to_vec(),
        pages: base_pages.to_vec(),
        warnings: warnings.to_vec(),
        stage_times_ms: stage_times.to_vec(),
        total_sprites: sprites.len(),
        alias_count,
        output_files,
    }
}
