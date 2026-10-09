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

use crate::config::{
    scale_denominator, AlphaHandling, FolderGroup, ProjectConfig, ScaleMode, VariantOptions,
};
use crate::error::{Result, TpError};
use crate::export;
use crate::ingest::{self, IngestedSprite};
use crate::pack::{self, PackItem, PackerOptions};
use crate::pixels;
use crate::polygon;
use crate::progress::{Progress, Segment};
use crate::templates;
use crate::types::{AtlasPage, PackResult, PageInfo, Point2D, Rect, SpriteAsset};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

/// Loaded normal-map companion: `(width, height, RGBA8 pixels)`.
type NormalLoad = Result<Option<(i32, i32, Vec<u8>)>>;

/// Result of a full pipeline run: serializable summary + in-memory pages
/// (for the GUI preview).
pub struct PipelineOutput {
    pub result: PackResult,
    pub pages: Vec<AtlasPage>,
}

/// Run the whole packing pipeline for a project configuration and write
/// the exported image/metadata files to the output directory.
pub fn run(config: &ProjectConfig) -> Result<PipelineOutput> {
    // Los grupos viajan siempre: el gancho de `execute` decide si están
    // activos (modo manual con asignaciones o modo automático por carpetas).
    execute(config, true, Some(&config.folder_groups), None, None)
}

/// Like [`run`], feeding `progreso` with every phase of the run: the status
/// bar of the GUI reads it from another thread while packing.
pub fn run_with_progress(config: &ProjectConfig, progreso: &Progress) -> Result<PipelineOutput> {
    execute(
        config,
        true,
        Some(&config.folder_groups),
        None,
        Some(progreso),
    )
}

/// Run the pipeline without touching the disk: pack in memory only, so the
/// GUI can show a live preview of the workspace. No directory is created and
/// no image or metadata file is rendered or written.
pub fn run_preview(config: &ProjectConfig) -> Result<PipelineOutput> {
    execute(config, false, Some(&config.folder_groups), None, None)
}

/// Pack by manual folder groups (`folder_groups`): one pipeline run per
/// group, each writing its sheet(s) into `<output_directory>/<grupo>/` (the
/// default group writes into the output root). Sprites not assigned to any
/// group land in the default group. Activates when any explicit group has
/// members; the merged result keeps every group's sprites and pages for the
/// GUI preview.
pub fn run_grouped(config: &ProjectConfig) -> Result<PipelineOutput> {
    execute(config, true, Some(&config.folder_groups), None, None)
}

/// Like [`run_grouped`], feeding `progreso` (same call as [`run_with_progress`]:
/// `execute` is what decides whether the groups are active).
pub fn run_grouped_with_progress(
    config: &ProjectConfig,
    progreso: &Progress,
) -> Result<PipelineOutput> {
    execute(
        config,
        true,
        Some(&config.folder_groups),
        None,
        Some(progreso),
    )
}

/// In-memory version of [`run_grouped`] for the live preview.
pub fn run_grouped_preview(config: &ProjectConfig) -> Result<PipelineOutput> {
    execute(config, false, Some(&config.folder_groups), None, None)
}

fn execute(
    config: &ProjectConfig,
    write_to_disk: bool,
    groups: Option<&[FolderGroup]>,
    variant: Option<&VariantRun>,
    progreso: Option<&Progress>,
) -> Result<PipelineOutput> {
    config.validate()?;
    // Clave global: si el proyecto solo nombra una clave guardada, se
    // resuelve aquí — en la publicación, si no existe, se falla antes de
    // escribir nada.
    let resolved_key = crate::keys::resolve_config(config, write_to_disk)?;
    let config: &ProjectConfig = &resolved_key;
    let mut stage_times: Vec<(String, u64)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // ------------------------------------------------------------------
    // Barra de la tira: la corrida se parte en cinco tramos contiguos —
    // ingerir, variantes, componer, pintar, escribir— y cada fase avanza
    // dentro del suyo con sus propios trabajos. Como los tramos son
    // contiguos y el último cierra la barra, arrancar una fase sólo puede
    // empujar el porcentaje hacia delante y terminar la última lo deja en
    // 1. La corrida agrupada no llega a las tres últimas: se para en
    // `run_groups`, que se lleva lo que sobra de la primera.
    // ------------------------------------------------------------------
    let tramos = Segment::split(Segment::FULL, &[350, 100, 150, 200, 200]);
    let tramo_grupos = Segment {
        start: tramos[0].start + tramos[0].len,
        len: Segment::FULL.len - tramos[0].start - tramos[0].len,
    };
    if let Some(prog) = progreso {
        // El total de la ingesta lo fija ésta al descubrir los ficheros.
        prog.begin_phase(tramos[0], 0);
    }

    // ------------------------------------------------------------------
    // Ajustes de rejilla (align to grid / common divisor / border padding).
    // Alinear obliga a que el padding sea múltiplo del valor; los
    // tamaños de los sprites se estiran hasta el múltiplo común en la ingesta.
    // ------------------------------------------------------------------
    // Además de *Align to grid*, el común divisor de las variantes idénticas
    // alinea los orígenes: sin eso, un frame en posición impar no sería
    // entero al escalarlo a 0.5 y la hoja idéntica perdería su proporción.
    let align = config
        .align_to_grid
        .max(config.variant_common_divisor())
        .max(0);
    let align_up = |v: i32| {
        let v = v.max(0);
        if align > 0 {
            let rest = v % align;
            if rest == 0 {
                v
            } else {
                v + (align - rest)
            }
        } else {
            v
        }
    };
    let pad = align_up(config.padding);
    let border = align_up(config.border_padding);
    if pad != config.padding || border != config.border_padding {
        warnings.push(format!(
            "Padding ajustado a {pad} px y borde a {border} px para respetar la rejilla de {align} px"
        ));
    }
    let (div_x, div_y) = config.effective_divisors();

    // Avisos de exportación (flip-y, pixel format).
    if config.flip_vertical && !config.gpu_format.is_hardware() {
        warnings.push(
            "Voltear verticalmente (flip Y) solo aplica a formatos de hardware \
             (ASTC/ETC2/ETC1/PVRTC); se ignora con el formato actual"
                .to_string(),
        );
    }
    if config.pixel_format != crate::config::PixelFormat::Rgba8888
        && config.gpu_format.is_hardware()
    {
        warnings.push(
            "El formato de píxel solo aplica a los formatos de software \
             (PNG/PNG8/JPG/WebP/BMP/TGA/TIFF/DDS/ZKTX); los formatos de \
             hardware comprimen RGBA y lo ignoran"
                .to_string(),
        );
    }

    // ------------------------------------------------------------------
    // PASO 1 + 2: discover, load (parallel), trim, hash
    // ------------------------------------------------------------------
    let t = Instant::now();
    let ingested = ingest::ingest_con(
        &ingest::IngestOptions {
            input_directory: &config.input_directory,
            trim_threshold: config.trim_threshold,
            trim_mode: config.effective_trim_mode(),
            trim_margin: config.trim_margin,
            enable_normal_maps: config.enable_normal_maps,
            normal_map_suffix: config.normal_map_suffix.clone(),
            normal_map_filter: config.normal_map_filter.clone(),
            normal_map_auto_detect: config.normal_map_auto_detect,
            recursive: config.recursive,
            extra_inputs: &config.extra_inputs,
            excluded_inputs: &config.excluded_inputs,
            trim_sprite_names: config.trim_sprite_names,
            prepend_folder_name: config.prepend_folder_name,
            common_divisor_x: div_x,
            common_divisor_y: div_y,
            ignore_patterns: &config.ignore_patterns,
            name_replacements: &config.name_replacements,
            heuristic_mask: config.heuristic_mask,
        },
        progreso,
    );
    warnings.extend(ingested.warnings);
    if ingested.sprites.is_empty() {
        return Err("No se encontraron sprites válidos en el directorio de entrada".into());
    }
    let mut sprites = ingested.sprites;
    stage_times.push(("ingest".into(), t.elapsed().as_millis() as u64));

    // Empaquetado por carpetas (manual por grupos o automático por
    // subcarpetas): una ejecución por grupo con los sprites de los demás
    // grupos excluidos (se re-ingestan filtrados).
    if let Some(groups) = groups {
        if config.auto_folder_groups {
            // Modo automático: cada subcarpeta
            // de entrada se convierte en un grupo con su mismo nombre; los
            // sprites de la raíz van a la hoja principal (siempre la página 0
            // y las subcarpetas en orden alfabético, para salida determinista).
            let mut derived: Vec<FolderGroup> = vec![FolderGroup::default()];
            for s in &sprites {
                let sub = s
                    .source_path
                    .strip_prefix(&config.input_directory)
                    .ok()
                    .and_then(|rel| rel.parent())
                    .map(|p| {
                        // Nombre de grupo = ruta relativa con separador
                        // '/' SIEMPRE: el nombre cruza plataformas (pasa a
                        // output_directory.join y a los metadatos) y en
                        // Windows to_string_lossy daría '\'.
                        p.components()
                            .map(|c| c.as_os_str().to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("/")
                    })
                    .unwrap_or_default();
                if sub.is_empty() {
                    continue;
                }
                match derived.iter_mut().find(|g| g.name == sub) {
                    Some(g) => g.sprites.push(s.id.clone()),
                    None => derived.push(FolderGroup {
                        name: sub,
                        sprites: vec![s.id.clone()],
                    }),
                }
            }
            if derived.len() > 1 {
                derived[1..].sort_by(|a, b| a.name.cmp(&b.name));
            }
            return run_groups(
                config,
                &sprites,
                &derived,
                write_to_disk,
                progreso.map(|p| (p, tramo_grupos)),
            );
        }
        // Modo manual: activo en cuanto hay un grupo con nombre y sprites.
        if groups
            .iter()
            .any(|g| !g.name.is_empty() && !g.sprites.is_empty())
        {
            return run_groups(
                config,
                &sprites,
                groups,
                write_to_disk,
                progreso.map(|p| (p, tramo_grupos)),
            );
        }
    }

    // ------------------------------------------------------------------
    // PASO 2b: variantes de escala con empaquetado propio
    // ------------------------------------------------------------------
    // Solo al escribir: la vista previa no genera ficheros y se lleva como
    // siempre (una corrida y todas las variantes reescalando la hoja). Cada
    // variante que no puede reutilizar la hoja base corre por su cuenta,
    // con su filtro, su tamaño máximo y su geometría ya escalada.
    let mut variant_files: Vec<String> = Vec::new();
    let mut export_scales: Vec<f32> = config.scale_variants.clone();
    // Fase de variantes: su total sólo se sabe al planificarse (y sólo hay
    // variantes que planificar al escribir), así que hasta entonces la fase
    // está anunciada sin trabajos y se da por hecha si no llegan.
    if let Some(prog) = progreso {
        prog.begin_phase(tramos[1], 0);
    }
    // Los modos de escalado de pixel art solo entienden su factor entero; el
    // resto de escalas caen a Smooth. Solo se avisa en la corrida de arriba
    // (las corridas internas de variante repiten la misma lista).
    if variant.is_none() {
        if let Some(want) = config.scale_mode.required_factor() {
            let want = want as f32;
            let mut mismatched: Vec<String> = Vec::new();
            for s in config.scale_variants.iter().copied() {
                if (s - 1.0).abs() < 1e-6 || (s - want).abs() < 1e-6 {
                    continue;
                }
                let label = format!("{s}x");
                if !mismatched.contains(&label) {
                    mismatched.push(label);
                }
            }
            if !mismatched.is_empty() {
                warnings.push(format!(
                    "{} solo se aplica a la escala exacta {want}x; las variantes {} se \
                     reescalarán con Smooth",
                    config.scale_mode.as_str(),
                    mismatched.join(", ")
                ));
            }
        }
    }
    if let Some(v) = variant {
        scale_ingested_sprites(&mut sprites, v.scale, config.scale_mode);
    } else if write_to_disk {
        let (identical, passes, plan_warnings) = plan_variants(config, &sprites);
        warnings.extend(plan_warnings);
        export_scales = identical;
        if let Some(prog) = progreso {
            prog.set_total(passes.len());
        }
        for p in passes {
            let mut pcfg = config.clone();
            let mut excluded = pcfg.excluded_inputs.clone();
            excluded.extend(p.excluded.clone());
            pcfg.excluded_inputs = excluded;
            pcfg.scale_variants = vec![p.run.scale];
            pcfg.max_texture_size = p.max_texture_size;
            scale_variant_geometry(&mut pcfg, p.run.scale);
            let t = Instant::now();
            let out = execute(&pcfg, true, None, Some(&p.run), None)?;
            warnings.extend(out.result.warnings);
            variant_files.extend(out.result.output_files);
            let inner: u64 = out.result.stage_times_ms.iter().map(|(_, ms)| *ms).sum();
            stage_times.push((
                format!("variante {}", variant_suffix_for(config, p.run.scale)),
                inner + t.elapsed().as_millis() as u64,
            ));
            if let Some(prog) = progreso {
                prog.add(1);
            }
        }
    }

    // ------------------------------------------------------------------
    // PASO 3: alias resolution
    // ------------------------------------------------------------------
    // Fase de composición: cinco sub-etapas sin trabajo por sprite que
    // avisar (dentro del empaquetador no hay gancho), así que cada etapa
    // cerrada es un trabajo de la barra.
    if let Some(prog) = progreso {
        prog.begin_phase(tramos[2], 5);
    }
    let t = Instant::now();
    let aliases = if config.enable_aliasing {
        ingest::resolve_aliases(&sprites)
    } else {
        vec![(false, None); sprites.len()]
    };
    let alias_count = aliases.iter().filter(|(a, _)| *a).count();
    stage_times.push(("aliasing".into(), t.elapsed().as_millis() as u64));
    if let Some(prog) = progreso {
        prog.add(1);
    }

    // Un id duplicado por dos ficheros CON CONTENIDO DISTINTO corrompería la
    // salida en silencio: hay dos colocaciones con el mismo id, el `HashMap`
    // se quedaría con una, el otro sprite recibiría `frame = None` y
    // publicaría un `Rect(0,0,0,0)` en los datos.
    //
    // Los ids repetidos SÍ son legítimos cuando los repetidos son alias
    // (`solid.tga` y `solid.qoi` con los mismos píxeles): solo se empaqueta
    // uno y los demás reutilizan su marco, así que no hay dos colocaciones.
    let mut packed_ids: HashMap<&str, usize> = HashMap::new();
    for (i, s) in sprites.iter().enumerate() {
        if aliases[i].0 {
            continue;
        }
        *packed_ids.entry(s.id.as_str()).or_default() += 1;
    }
    let mut duplicated: Vec<&str> = packed_ids
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(id, _)| *id)
        .collect();
    if !duplicated.is_empty() {
        duplicated.sort_unstable();
        const SHOWN: usize = 8;
        let head: Vec<&str> = duplicated.iter().take(SHOWN).copied().collect();
        let rest = duplicated.len() - head.len();
        let more = if rest > 0 {
            format!(" y {rest} más")
        } else {
            String::new()
        };
        return Err(crate::error::TpError::Pack(format!(
            "{} id(s) de sprite duplicado(s): {}{more}. Dos ficheros distintos con \
             contenido diferente generan el mismo nombre en el atlas; renómbralos, \
             separa las carpetas o activa «prepend folder name» para desambiguarlos.",
            duplicated.len(),
            head.join(", "),
        )));
    }

    // Índice por id de los que llevan colocación: los alias se excluyen para
    // que un alias que comparta id con su propio objetivo no se quede con el
    // marco del sprite real (los objetivos de alias son siempre no-alias).
    let mut id_to_index: HashMap<&str, usize> = HashMap::with_capacity(sprites.len());
    for (i, s) in sprites.iter().enumerate() {
        if !aliases[i].0 {
            id_to_index.entry(s.id.as_str()).or_insert(i);
        }
    }

    // ------------------------------------------------------------------
    // PASO 4 (optional): polygon engine (contour -> RDP -> earcut)
    // ------------------------------------------------------------------
    let t = Instant::now();
    // Algorithm → Polygon — trim mode Polygon (or the legacy
    // `enable_polygon` switch) enables mesh extraction and polygon packing.
    let use_polygon = config.effective_algorithm() == crate::config::PackingAlgorithm::Polygon;
    let mut meshes: Vec<Option<polygon::Polygons>> = vec![None; sprites.len()];
    if use_polygon {
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
    if let Some(prog) = progreso {
        prog.add(1);
    }

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
    let opts = PackerOptions {
        strategy: config.effective_strategy(),
        algorithm: config.effective_algorithm(),
        pack_mode: config.pack_mode,
        size_constraints: config.size_constraints,
        force_squared: config.force_squared,
        fixed_width: config.fixed_width,
        fixed_height: config.fixed_height,
        basic_sort_by: config.basic_sort_by,
        basic_order: config.basic_order,
        manual_positions: config.manual_positions.clone(),
        manual_grid: config.manual_grid,
        word_align_mod: config.word_align_mod(),
        align_grid: align,
        max_width: config.effective_max_width(),
        max_height: config.effective_max_height(),
        ..PackerOptions::new(
            config.packing_strategy,
            config.allow_rotation,
            config.max_texture_size,
            pad,
            border,
            use_polygon,
        )
    };
    let pack_out = pack::pack(&items, &opts)?;
    stage_times.push(("packing".into(), t.elapsed().as_millis() as u64));
    if let Some(prog) = progreso {
        prog.add(1);
    }

    // Multipack — con la opción desactivada todas las imágenes deben
    // caber en una sola hoja.
    if !config.multipack && pack_out.pages.len() > 1 {
        let (pw, ph) = (pack_out.pages[0].width, pack_out.pages[0].height);
        return Err(crate::error::TpError::Pack(format!(
            "Los sprites no caben en un solo atlas de {}x{} (harían {} hojas); \
             activa «Multipack» o aumenta el tamaño máximo",
            pw,
            ph,
            pack_out.pages.len()
        )));
    }
    // Variantes con layout idéntico: avisar si, al escalarlas, la hoja
    // supera el tamaño máximo que pidieron para esa variante.
    if variant.is_none() {
        for &scale in &export_scales {
            let Some(max) = config
                .variant_options_for(scale)
                .and_then(|o| o.max_texture_size)
                .filter(|&m| m > 0)
            else {
                continue;
            };
            if pack_out.pages.iter().any(|p| {
                let w = ((p.width as f32) * scale).round() as i32;
                let h = ((p.height as f32) * scale).round() as i32;
                w > max || h > max
            }) {
                warnings.push(format!(
                    "Variante {}: la hoja escalada supera su tamaño máximo de {max} px; \
                     con layout idéntico no se puede reducir (desactiva «layout idéntico» \
                     o sube el máximo).",
                    variant_suffix_for(config, scale)
                ));
            }
        }
    }

    // Multipack placeholders — avisar cuando varias hojas se nombran
    // con el sufijo implícito `_N` en vez de un placeholder `{n}`/`{n1}`.
    if pack_out.pages.len() > 1 && !has_page_placeholder(&config.base_file_name) {
        warnings.push(format!(
            "Multipack: {} hojas generadas y el nombre base \"{}\" no contiene {{n}} o {{n1}}; \
             se nombran con el sufijo _N (p. ej. {}_1). Añade {{n1}} al nombre base para \
             nombrar cada hoja (p. ej. {}{{n1}}).",
            pack_out.pages.len(),
            config.base_file_name,
            config.base_file_name,
            config.base_file_name
        ));
    }

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
    let has_normals = config.enable_normal_maps && sprites.iter().any(|s| s.normal_path.is_some());
    if has_normals {
        for page in &mut pages {
            page.normal_pixels = Some(vec![0u8; (page.width * page.height * 4) as usize]);
            page.has_normals = true;
        }
    }
    if let Some(prog) = progreso {
        prog.add(1);
    }

    // Preload normal-map pixels (parallel).
    let t = Instant::now();
    let normal_load: Vec<NormalLoad> = sprites
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
                warnings.push(e.to_string());
                normal_images.push(None);
            }
        }
    }
    stage_times.push(("normal-load".into(), t.elapsed().as_millis() as u64));
    if let Some(prog) = progreso {
        prog.add(1);
    }

    // ------------------------------------------------------------------
    // PASO 8: blit sprites into page buffers (parallel)
    // ------------------------------------------------------------------
    // Fase de pintado: el blit con un trabajo por sprite (los alias y los
    // que no tienen marco también cuentan: su turno ya está resuelto) y
    // cuatro pasos por página —ocupación, fondo, alfa y cuantización—. Dos
    // de esos pasos son condicionales y se quedan sin avisar: la barra no
    // acaba el tramo y la fase siguiente la empuja al suyo, que es lo único
    // que la puede mover hacia delante.
    if let Some(prog) = progreso {
        prog.begin_phase(tramos[3], sprites.len() + pages.len() * 4);
    }
    let t = Instant::now();
    // Los buffers viajan de `pages` a los candados y vuelven al final: así
    // el candado nunca lleva un clon del lienzo (de 64 MB en un atlas
    // 4096²), que antes se pagaba dos veces, al entrar y al salir.
    let page_pixels: Vec<Mutex<Vec<u8>>> = pages
        .iter_mut()
        .map(|p| Mutex::new(std::mem::take(&mut p.pixels)))
        .collect();
    let page_normals: Vec<Mutex<Vec<u8>>> = pages
        .iter_mut()
        .map(|p| Mutex::new(p.normal_pixels.take().unwrap_or_default()))
        .collect();

    let extrude = config.extrude.max(0);

    (0..sprites.len()).into_par_iter().for_each(|i| {
        // Un trabajo por sprite que entra al blit: los que saltan (alias o
        // sin marco) también cuentan, que su turno ya está resuelto.
        if let Some(prog) = progreso {
            prog.add(1);
        }
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
        let layout = pixels::BlitLayout {
            frame: fr,
            padding: pad,
            extrude,
            rotated: rotated[i],
        };
        // Extruir y rotar (mitad del trabajo del blit, y con la asignación
        // del buffer de por medio) corre sin candado: es trabajo sobre los
        // datos del propio sprite. El candado solo cubre la copia al lienzo,
        // que es lo único que comparte estado — los marcos son disjuntos.
        let prepped = pixels::prepare_blit(
            layout,
            pixels::TrimmedSprite {
                pixels: &s.pixels,
                width: s.trimmed_bounds.width,
                height: s.trimmed_bounds.height,
            },
        );
        {
            let mut px = page_pixels[pi].lock().unwrap();
            pixels::blit_prepared(&mut px, page.width, page.height, &prepped);
        }

        if let Some((nw, nh, npix)) = &normal_images[i] {
            let prep = prepare_blit_normal(
                layout,
                pixels::TrimmedSprite {
                    pixels: npix,
                    width: *nw,
                    height: *nh,
                },
                s,
            );
            if let Some(prep) = prep {
                let mut np = page_normals[pi].lock().unwrap();
                pixels::blit_prepared(&mut np, page.width, page.height, &prep);
            }
        }
    });

    // Devolver los buffers a sus páginas consumiendo los candados: sin el
    // clon de antes al salir.
    for (page, (pix, norm)) in pages
        .iter_mut()
        .zip(page_pixels.into_iter().zip(page_normals))
    {
        page.pixels = pix.into_inner().unwrap();
        if has_normals {
            page.normal_pixels = Some(norm.into_inner().unwrap());
        }
    }
    stage_times.push(("blit".into(), t.elapsed().as_millis() as u64));

    // Ocupación del atlas: se mide aquí, antes de rellenar el fondo, para
    // que siga contando píxeles de sprites y no el color de fondo.
    for page in &mut pages {
        page.fill_ratio = fill_ratio(&page.pixels, page.width, page.height);
    }
    if let Some(prog) = progreso {
        prog.add(pages.len());
    }

    // ------------------------------------------------------------------
    // PASO 8a: background colour (`--background-color`)
    // ------------------------------------------------------------------
    if let Some(bg) = config.background_color {
        let t = Instant::now();
        pages
            .par_iter_mut()
            .for_each(|p| pixels::fill_background(&mut p.pixels, bg));
        stage_times.push(("background".into(), t.elapsed().as_millis() as u64));
        if let Some(prog) = progreso {
            prog.add(pages.len());
        }
    }

    // ------------------------------------------------------------------
    // PASO 8b: transparency handling (alpha handling)
    // ------------------------------------------------------------------
    if config.alpha_handling != AlphaHandling::KeepTransparentPixels {
        let t = Instant::now();
        let mode = config.alpha_handling;
        pages.par_iter_mut().for_each(|p| {
            pixels::apply_alpha_handling(&mut p.pixels, p.width as usize, p.height as usize, mode);
        });
        stage_times.push(("alpha-handling".into(), t.elapsed().as_millis() as u64));
        if let Some(prog) = progreso {
            prog.add(pages.len());
        }
    }

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
    if let Some(prog) = progreso {
        prog.add(pages.len());
    }

    // ------------------------------------------------------------------
    // Assemble SpriteAsset list
    // ------------------------------------------------------------------
    // Precedencia: ediciones de la GUI (`config.*_overrides`) > sidecar
    // `pivots.json`/`borders.json` > pivot por defecto.
    let sidecar_pivots = ingest::load_pivot_overrides(&config.input_directory).unwrap_or_default();
    let sidecar_borders =
        ingest::load_border_overrides(&config.input_directory).unwrap_or_default();
    let mut sprite_assets: Vec<SpriteAsset> = Vec::with_capacity(sprites.len());
    let alias_targets: std::collections::HashSet<String> =
        aliases.iter().filter_map(|(_, t)| t.clone()).collect();

    for (i, s) in sprites.iter().enumerate() {
        let (is_alias, alias_target) = aliases[i].clone();
        let pivot = config
            .pivot_overrides
            .get(&s.id)
            .copied()
            .or_else(|| sidecar_pivots.get(&s.id).copied())
            .unwrap_or_else(|| Point2D::new(config.default_pivot_x, config.default_pivot_y));

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
            border: config
                .border_overrides
                .get(&s.id)
                .copied()
                .or_else(|| sidecar_borders.get(&s.id).copied()),
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

    // Shape debug: dibuja el contorno de cada sprite (rectángulo visible y
    // polígonos) sobre la hoja, así la vista previa y el fichero publicado
    // enseñan el mismo reparto.
    if config.shape_debug {
        for (pi, page) in pages.iter_mut().enumerate() {
            let shapes = debug_shapes(&sprite_assets, pi);
            export::draw_shape_debug(
                &mut page.pixels,
                page.width as usize,
                page.height as usize,
                &shapes,
            );
        }
    }

    // ------------------------------------------------------------------
    // PASO 10 + 11: export images (+ variants), encryption, metadata
    // ------------------------------------------------------------------
    let t = Instant::now();
    let output_dir = &config.output_directory;
    if write_to_disk {
        std::fs::create_dir_all(output_dir).map_err(|e| {
            TpError::Other(format!("No se pudo crear {}: {e}", output_dir.display()))
        })?;
    }

    let mut output_files: Vec<String> = Vec::new();

    // Opciones de codificación comunes a todas las hojas y variantes.
    let enc_opts = export::EncodeOptions::from_config(config);
    let flip_active = config.flip_vertical && config.gpu_format.is_hardware();

    // Páginas de esta corrida a su escala base (1.0, o la de la variante
    // cuando la corrida ES una): `result.pages` las usa la GUI tras
    // publicar y los data files las cotejan, haya o no una variante de esa
    // escala entre las que se exportan.
    let base_scale = variant.map(|v| v.scale).unwrap_or(1.0);
    let base_page_infos: Vec<PageInfo> = page_infos_at(config, &pages, base_scale);

    // Fase de escritura: una página y escala por trabajo —lo caro es
    // codificar cada hoja—. Los datos de la plantilla van después, sin
    // coste de codificación que avisar.
    if let Some(prog) = progreso {
        prog.begin_phase(tramos[4], pages.len() * export_scales.len());
    }

    for scale in &export_scales {
        // Una corrida de variante empaqueta ya a su escala: sus páginas se
        // publican tal cual (es = 1.0) y solo cambia el sufijo {v}.
        let es = if variant.is_some() { 1.0 } else { *scale };
        let is_base = (es - 1.0).abs() < 1e-6;
        let variant_name = variant_suffix_for(config, *scale);

        let mut variant_page_infos: Vec<PageInfo> = Vec::new();
        let mut variant_image_files: Vec<String> = Vec::new();

        // Codificar y escribir las hojas en paralelo: el encode (PNG, DXT o
        // ETC2) es lo caro de la publicación y cada página tiene su fichero,
        // así que no comparten nada. Lo único que se serializa es la
        // recolección de nombres, que se empuja en orden de página para que
        // `output_files` salga idéntico a como salía con el bucle secuencial.
        let exported: Vec<PageExport> = pages
            .par_iter()
            .map(|page| -> Result<PageExport> {
                let (sw, sh) = if is_base {
                    (page.width as usize, page.height as usize)
                } else {
                    (
                        ((page.width as f32) * es).round().max(1.0) as usize,
                        ((page.height as f32) * es).round().max(1.0) as usize,
                    )
                };

                let file_name = page_file_name(config, page.index, &variant_name);
                let mut normal_name = None;
                let mut written: Vec<String> = Vec::new();
                if write_to_disk {
                    // Sin escala ni giro el codificador lee el lienzo tal
                    // cual: ni se copia el búfer de la página (64 MB en un
                    // atlas 4096²) ni se pone mutable para nada.
                    let (rgba, w, h): (std::borrow::Cow<'_, [u8]>, usize, usize) = if !is_base {
                        let (mut scaled, w, h) = export::scale_rgba(
                            &page.pixels,
                            page.width as usize,
                            page.height as usize,
                            es,
                            config.scale_mode,
                        );
                        if flip_active {
                            export::flip_vertical_rgba(&mut scaled, w, h);
                        }
                        (std::borrow::Cow::Owned(scaled), w, h)
                    } else if flip_active {
                        let mut scaled = page.pixels.clone();
                        export::flip_vertical_rgba(&mut scaled, sw, sh);
                        (std::borrow::Cow::Owned(scaled), sw, sh)
                    } else {
                        (std::borrow::Cow::Borrowed(page.pixels.as_slice()), sw, sh)
                    };
                    let bytes = export::apply_dpi(
                        export::encode_to_bytes(&rgba, w, h, &enc_opts)?,
                        config.gpu_format,
                        config.dpi,
                    );

                    // Cache busting: el data format lleva `?v=<hash>` de este
                    // mismo fichero, así que cambia cuando cambia la hoja.
                    let cache_version = if config.cache_busting {
                        crate::hash::hash_bytes_short(&bytes)
                    } else {
                        String::new()
                    };
                    let (final_name, final_bytes) = match &config.encryption_key {
                        Some(key) => (
                            format!("{file_name}.tpenc"),
                            export::encrypt_bytes(&bytes, key)?,
                        ),
                        None => (file_name, bytes),
                    };

                    write_file(
                        &output_dir.join(&final_name),
                        &final_bytes,
                        config.force_publish,
                    )?;
                    written.push(final_name.clone());

                    // Normal-map page.
                    if let Some(npix) = &page.normal_pixels {
                        let nfile = normal_page_file_name(config, page.index, &variant_name);
                        let (nrgba, nw2, nh2): (std::borrow::Cow<'_, [u8]>, usize, usize) =
                            if !is_base {
                                let (mut scaled, nw2, nh2) = export::scale_rgba(
                                    npix,
                                    page.width as usize,
                                    page.height as usize,
                                    es,
                                    config.scale_mode,
                                );
                                if flip_active {
                                    export::flip_vertical_rgba(&mut scaled, nw2, nh2);
                                }
                                (std::borrow::Cow::Owned(scaled), nw2, nh2)
                            } else if flip_active {
                                let mut scaled = npix.clone();
                                export::flip_vertical_rgba(&mut scaled, sw, sh);
                                (std::borrow::Cow::Owned(scaled), sw, sh)
                            } else {
                                (std::borrow::Cow::Borrowed(npix.as_slice()), sw, sh)
                            };
                        let nbytes = export::apply_dpi(
                            export::encode_to_bytes(&nrgba, nw2, nh2, &enc_opts)?,
                            config.gpu_format,
                            config.dpi,
                        );
                        let (nfinal_name, nfinal_bytes) = match &config.encryption_key {
                            Some(key) => (
                                format!("{nfile}.tpenc"),
                                export::encrypt_bytes(&nbytes, key)?,
                            ),
                            None => (nfile, nbytes),
                        };
                        write_file(
                            &output_dir.join(&nfinal_name),
                            &nfinal_bytes,
                            config.force_publish,
                        )?;
                        written.push(nfinal_name.clone());
                        normal_name = Some(nfinal_name);
                    }

                    Ok(PageExport {
                        image_file: final_name.clone(),
                        info: PageInfo {
                            index: page.index,
                            width: sw as i32,
                            height: sh as i32,
                            file_name: final_name,
                            format: config.gpu_format.as_str().to_string(),
                            has_normals: page.has_normals,
                            normal_file_name: normal_name,
                            encrypted: config.encryption_key.is_some(),
                            fill_ratio: page.fill_ratio,
                            cache_version,
                        },
                        written,
                    })
                } else {
                    // Vista previa: sin codificar ni escribir, pero con los
                    // mismos nombres que escribiría la publicación (incluido el
                    // `.tpenc` y la hoja de normales), para que la pestaña
                    // «Archivos» liste lo que de verdad se va a generar.
                    let encrypt = |n: String| match &config.encryption_key {
                        Some(_) => format!("{n}.tpenc"),
                        None => n,
                    };
                    let final_name = encrypt(file_name.clone());
                    let normal_file = page
                        .normal_pixels
                        .as_ref()
                        .map(|_| normal_page_file_name(config, page.index, &variant_name))
                        .map(encrypt);
                    written.push(final_name.clone());
                    if let Some(n) = &normal_file {
                        written.push(n.clone());
                    }
                    Ok(PageExport {
                        image_file: final_name,
                        info: PageInfo {
                            index: page.index,
                            width: sw as i32,
                            height: sh as i32,
                            file_name,
                            format: config.gpu_format.as_str().to_string(),
                            has_normals: page.has_normals,
                            normal_file_name: normal_file,
                            encrypted: config.encryption_key.is_some(),
                            fill_ratio: page.fill_ratio,
                            // Sin codificar no hay hash posible; la vista previa
                            // solo lista nombres, que no cambian con el hash.
                            cache_version: String::new(),
                        },
                        written,
                    })
                }
            })
            .inspect(|_| {
                // Una hoja codificada (y escrita, si toca) es un trabajo
                // terminado; si el `?` de dentro corta, esa no se cuenta y
                // la fase siguiente empuja la barra a su tramo.
                if let Some(prog) = progreso {
                    prog.add(1);
                }
            })
            .collect::<Result<Vec<_>>>()?;
        for out in exported {
            output_files.extend(out.written);
            variant_image_files.push(out.image_file);
            variant_page_infos.push(out.info);
        }

        // Metadata via template engine. Con un placeholder de página en el
        // nombre base cada hoja escribe su propio data file con solo sus
        // frames (multipack); sin él se emite un único fichero con
        // todas las páginas.
        let per_page = has_page_placeholder(&config.base_file_name);
        if templates::data_file_extension(config).is_empty() {
            // «spritesheet-only»: el formato no escribe fichero de datos.
        } else if per_page {
            for (pinfo, image) in variant_page_infos.iter().zip(&variant_image_files) {
                let page_sprites: Vec<SpriteAsset> = sprite_assets
                    .iter()
                    .filter(|s| s.atlas_page_index == pinfo.index as i32)
                    .cloned()
                    .collect();
                let page_infos = [pinfo.clone()];
                let meta_name = metadata_file_name(config, &variant_name, pinfo.index, per_page);
                if write_to_disk {
                    let content = templates::render(
                        &pending_result(
                            config,
                            &page_sprites,
                            &warnings,
                            &stage_times,
                            output_files.clone(),
                            alias_count,
                            &page_infos,
                        ),
                        &page_infos,
                        std::slice::from_ref(image),
                        es,
                        config,
                    )?;
                    write_file(
                        &output_dir.join(&meta_name),
                        content.as_bytes(),
                        config.force_publish,
                    )?;
                }
                output_files.push(meta_name);
            }
        } else {
            let meta_name = metadata_file_name(config, &variant_name, 0, per_page);
            if write_to_disk {
                let content = templates::render(
                    &pending_result(
                        config,
                        &sprite_assets,
                        &warnings,
                        &stage_times,
                        output_files.clone(),
                        alias_count,
                        &base_page_infos,
                    ),
                    &variant_page_infos,
                    &variant_image_files,
                    es,
                    config,
                )?;
                write_file(
                    &output_dir.join(&meta_name),
                    content.as_bytes(),
                    config.force_publish,
                )?;
            }
            output_files.push(meta_name);
        }
    }

    // Ficheros de datos extra por framework (class/header/source/spriteids).
    // Solo en la corrida base: las variantes comparten los mismos nombres.
    if variant.is_none() {
        for (name, content) in templates::extra_files(config, &sprite_assets) {
            if write_to_disk {
                write_file(
                    &output_dir.join(&name),
                    content.as_bytes(),
                    config.force_publish,
                )?;
            }
            output_files.push(name);
        }
    }

    // Ficheros escritos por las variantes con empaquetado propio: los
    // reporta la misma corrida para que la GUI los liste.
    output_files.extend(variant_files);
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

// ---------------------------------------------------------------------------
// Scaling variants
// ---------------------------------------------------------------------------

/// A scaling variant that packs on its own: its geometry and canvas are
/// already at `scale`, so the run exports its sheet without rescaling it
/// again (only the `{v}` suffix marks it as a variant).
#[derive(Debug, Clone)]
struct VariantRun {
    scale: f32,
}

/// Scaling variant that needs its own packing run.
#[derive(Debug)]
struct VariantPass {
    run: VariantRun,
    /// Canvas cap of the pass (the project's own when the variant sets none).
    max_texture_size: i32,
    /// Source paths the variant's sprite filter leaves out.
    excluded: Vec<PathBuf>,
}

/// `{v}` suffix of a scale: an explicit `variant_names` entry (p. ej.
/// `1.0 → -ipadhd`) or the automatic `@2x` / `-hd` / `-sd` convention.
fn variant_suffix_for(config: &ProjectConfig, scale: f32) -> String {
    config
        .variant_names
        .iter()
        .find(|(s, _)| (*s - scale).abs() < 1e-6)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| variant_suffix(scale))
}

/// Split `config.scale_variants` into the scales this run can emit by
/// rescaling its sheet (identical layout) and the ones that need their own
/// packing, with the sprite paths each of them must exclude. Warnings
/// explain every option that cannot be honoured as requested.
fn plan_variants(
    config: &ProjectConfig,
    sprites: &[IngestedSprite],
) -> (Vec<f32>, Vec<VariantPass>, Vec<String>) {
    let mut identical = Vec::new();
    let mut passes = Vec::new();
    let mut warnings = Vec::new();
    let has_manual = !config.manual_positions.is_empty();

    for &scale in &config.scale_variants {
        let opts = config
            .variant_options_for(scale)
            .cloned()
            .unwrap_or(VariantOptions {
                scale,
                ..Default::default()
            });
        let suffix = variant_suffix_for(config, scale);

        let excluded: Vec<PathBuf> = sprites
            .iter()
            .filter(|s| {
                let file = s
                    .source_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                !opts.includes(&[s.id.as_str(), file.as_str()])
            })
            .map(|s| s.source_path.clone())
            .collect();
        if excluded.len() == sprites.len() {
            warnings.push(format!(
                "Variante {suffix}: el filtro de sprites excluye los {} sprites; se omite.",
                sprites.len()
            ));
            continue;
        }

        let max = opts.max_texture_size.filter(|&m| m > 0);
        let canvas_differs = max.is_some_and(|m| m != config.max_texture_size);
        let own_pack = !opts.force_identical_layout || !excluded.is_empty() || canvas_differs;
        if own_pack && has_manual {
            warnings.push(format!(
                "Variante {suffix}: hay posiciones manuales, así que se reutiliza la hoja \
                 base y ni el filtro ni el tamaño máximo se aplican."
            ));
        }
        if own_pack && !has_manual {
            passes.push(VariantPass {
                run: VariantRun { scale },
                max_texture_size: max.unwrap_or(config.max_texture_size),
                excluded,
            });
        } else {
            identical.push(scale);
        }
    }

    // Hoja idéntica redondeada: la escala no queda entera con el común
    // divisor (fraccionario pedido, denominador no representable o tope de
    // 2048). Aquí esos frames se guardan redondeados, así que se avisa.
    let div = config.variant_common_divisor();
    for &scale in &identical {
        if scale_denominator(scale).is_some_and(|d| d > 0 && div % d == 0) {
            continue;
        }
        let suffix = variant_suffix_for(config, scale);
        let msg = if config
            .variant_options_for(scale)
            .is_some_and(|o| o.accept_fractional)
        {
            format!(
                "Variante {suffix}: «aceptar valores fraccionarios» la deja fuera del común \
                 divisor, así que su hoja idéntica se redondea al píxel."
            )
        } else {
            format!(
                "Variante {suffix}: su escala {scale} no encaja en el común divisor {div} y su \
                 hoja idéntica se redondea al píxel (activa «aceptar valores fraccionarios» si \
                 quieres que se asuma)."
            )
        };
        warnings.push(msg);
    }
    (identical, passes, warnings)
}

/// Resample a variant's sprites (pixels and geometry) so packing, blitting
/// and metadata already work at the variant's scale.
fn scale_ingested_sprites(sprites: &mut [IngestedSprite], scale: f32, mode: ScaleMode) {
    if (scale - 1.0).abs() < 1e-6 {
        return;
    }
    let dim = |v: i32| ((v as f32) * scale).round().max(0.0) as i32;
    for s in sprites.iter_mut() {
        s.raw_width = dim(s.raw_width).max(1);
        s.raw_height = dim(s.raw_height).max(1);
        s.trimmed_bounds.x = dim(s.trimmed_bounds.x);
        s.trimmed_bounds.y = dim(s.trimmed_bounds.y);
        let (w, h) = (
            s.trimmed_bounds.width.max(0) as usize,
            s.trimmed_bounds.height.max(0) as usize,
        );
        if w > 0 && h > 0 && s.pixels.len() >= w * h * 4 {
            let (px, nw, nh) = export::scale_rgba(&s.pixels, w, h, scale, mode);
            s.pixels = px;
            s.trimmed_bounds.width = nw as i32;
            s.trimmed_bounds.height = nh as i32;
        } else {
            s.trimmed_bounds.width = dim(s.trimmed_bounds.width).max(1);
            s.trimmed_bounds.height = dim(s.trimmed_bounds.height).max(1);
        }
    }
}

/// Scale the project's pixel-valued options for a variant's own packing, so
/// its sheet keeps the same proportions as the base one.
fn scale_variant_geometry(config: &mut ProjectConfig, scale: f32) {
    if (scale - 1.0).abs() < 1e-6 {
        return;
    }
    let dim = |v: i32| ((v as f32) * scale).round().max(0.0) as i32;
    config.padding = dim(config.padding);
    config.border_padding = dim(config.border_padding);
    config.extrude = dim(config.extrude);
    config.align_to_grid = dim(config.align_to_grid);
    config.fixed_width = dim(config.fixed_width);
    config.fixed_height = dim(config.fixed_height);
    if let Some(grid) = config.manual_grid.as_mut() {
        grid.step = dim(grid.step).max(1);
    }
}

/// Contornos que `--shape-debug` dibuja en la página `page_index`: el
/// rectángulo visible de cada sprite y sus polígonos proyectados al lienzo
/// con la misma rotación 90° CW que `polygon::compute_uvs`.
fn debug_shapes(sprites: &[SpriteAsset], page_index: usize) -> Vec<export::DebugShape> {
    sprites
        .iter()
        .filter(|s| s.atlas_page_index == page_index as i32)
        .map(|s| {
            let frame = s.visible_frame;
            let contours = s
                .contours
                .iter()
                .map(|c| {
                    c.points
                        .iter()
                        .map(|p| {
                            let local = if s.is_rotated {
                                polygon::rotate_90_cw(*p, frame.height as f32)
                            } else {
                                *p
                            };
                            [frame.x as f32 + local.x, frame.y as f32 + local.y]
                        })
                        .collect()
                })
                .collect();
            export::DebugShape {
                rect: [frame.x, frame.y, frame.width, frame.height],
                contours,
            }
        })
        .collect()
}

/// `PageInfo` of every page at `scale`, with the file names this run really
/// writes (`.tpenc` included when encryption is on; names only while
/// previewing).
/// What a single page contributes to a variant's export: the names it added to
/// `output_files`, its data-file entry and its image name. Emitted in page
/// order so the parallel export keeps the sequential ordering.
struct PageExport {
    written: Vec<String>,
    image_file: String,
    info: PageInfo,
}

fn page_infos_at(config: &ProjectConfig, pages: &[AtlasPage], scale: f32) -> Vec<PageInfo> {
    let variant = variant_suffix_for(config, scale);
    pages
        .iter()
        .map(|page| {
            let name = page_file_name(config, page.index, &variant);
            let encrypt = |n: String| match &config.encryption_key {
                Some(_) => format!("{n}.tpenc"),
                None => n,
            };
            let (file_name, normal_file_name) = (
                encrypt(name),
                page.normal_pixels
                    .as_ref()
                    .map(|_| normal_page_file_name(config, page.index, &variant))
                    .map(encrypt),
            );
            PageInfo {
                index: page.index,
                width: page.width,
                height: page.height,
                file_name,
                format: config.gpu_format.as_str().to_string(),
                has_normals: page.has_normals,
                normal_file_name,
                encrypted: config.encryption_key.is_some(),
                fill_ratio: page.fill_ratio,
                cache_version: String::new(),
            }
        })
        .collect()
}

/// Pack-by-folder: one pipeline run per group, with every other group's
/// sprites excluded so each group packs independently. The default group
/// (empty name) takes the sprites listed in it plus every unassigned one and
/// writes into the output root; named groups write into `<output>/<name>/`.
/// Page indices from each sub-run are offset in order, so the merged result
/// keeps sprites, pages, page infos and the output file list coherent for the
/// GUI preview.
fn run_groups(
    config: &ProjectConfig,
    all_sprites: &[IngestedSprite],
    groups: &[FolderGroup],
    write_to_disk: bool,
    progreso: Option<(&Progress, Segment)>,
) -> Result<PipelineOutput> {
    use std::collections::{HashMap, HashSet};

    let mut groups: Vec<FolderGroup> = groups.to_vec();
    let listed: HashSet<&str> = groups
        .iter()
        .flat_map(|g| g.sprites.iter().map(String::as_str))
        .collect();
    let has_unassigned = all_sprites.iter().any(|s| !listed.contains(s.id.as_str()));
    // Si el proyecto no trae grupo por defecto y quedan sprites sueltos, se
    // añade al final.
    if !groups.iter().any(|g| g.name.is_empty()) && has_unassigned {
        groups.push(FolderGroup::default());
    }
    // Dueño de cada id: el primer grupo que lo lista; los no listados caen
    // en el grupo por defecto (sin nombre), que además recoge sus propios
    // ids — sin esa rama, listar un sprite en la hoja principal lo excluiría
    // de todas las hojas y desaparecería del atlas.
    let mut owner: HashMap<&str, usize> = HashMap::new();
    for (i, g) in groups.iter().enumerate() {
        for id in &g.sprites {
            owner.entry(id.as_str()).or_insert(i);
        }
    }
    let default_idx = groups.iter().position(|g| g.name.is_empty());
    let owns = |s: &IngestedSprite, i: usize| match owner.get(s.id.as_str()) {
        Some(&o) => o == i,
        None => default_idx == Some(i),
    };

    // Barra de la corrida agrupada: un trabajo por grupo con sprites (los
    // vacíos ni se ejecutan ni cuentan), y dentro de cada uno la corrida
    // entera —ingerir, componer, pintar, escribir— corre sin contar, porque
    // cada grupo es ya el trabajo que la barra está siguiendo.
    if let Some((prog, tramo)) = progreso {
        let con_sprites = (0..groups.len())
            .filter(|&i| all_sprites.iter().any(|s| owns(s, i)))
            .count();
        prog.begin_phase(tramo, con_sprites);
    }

    let t0 = Instant::now();
    let mut merged_sprites: Vec<SpriteAsset> = Vec::new();
    let mut merged_pages: Vec<AtlasPage> = Vec::new();
    let mut merged_page_infos: Vec<PageInfo> = Vec::new();
    let mut output_files: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut stage_times: Vec<(String, u64)> = Vec::new();
    let mut alias_count = 0usize;
    let mut page_offset = 0usize;

    for (i, g) in groups.iter().enumerate() {
        let members: Vec<&IngestedSprite> = all_sprites.iter().filter(|s| owns(s, i)).collect();
        if members.is_empty() {
            continue;
        }

        let mut gcfg = config.clone();
        if !g.name.is_empty() {
            gcfg.output_directory = config.output_directory.join(&g.name);
        }
        // Aislar el sub-run: los sprites de los demás grupos se excluyen por
        // ruta (la ingesta normaliza las rutas antes de comparar).
        let others: Vec<PathBuf> = all_sprites
            .iter()
            .filter(|s| !owns(s, i))
            .map(|s| s.source_path.clone())
            .collect();
        let mut excluded = gcfg.excluded_inputs.clone();
        excluded.extend(others);
        gcfg.excluded_inputs = excluded;
        gcfg.folder_groups = Vec::new(); // no recursión

        let mut out = execute(&gcfg, write_to_disk, None, None, None)?;
        if let Some((prog, _)) = progreso {
            prog.add(1);
        }
        warnings.append(&mut out.result.warnings);
        alias_count += out.result.alias_count;
        let group_prefix = if g.name.is_empty() {
            None
        } else {
            Some(g.name.as_str())
        };
        for f in out.result.output_files.iter_mut() {
            if let Some(prefix) = group_prefix {
                *f = format!("{prefix}/{f}");
            }
        }
        output_files.append(&mut out.result.output_files);
        for s in out.result.sprites.iter_mut() {
            if s.atlas_page_index >= 0 {
                s.atlas_page_index += page_offset as i32;
            }
        }
        merged_sprites.append(&mut out.result.sprites);
        let added = out.pages.len();
        for page in out.pages.iter_mut() {
            page.index += page_offset;
        }
        merged_pages.append(&mut out.pages);
        for info in out.result.pages.iter_mut() {
            info.index += page_offset;
        }
        merged_page_infos.append(&mut out.result.pages);
        page_offset += added;
    }
    stage_times.push(("grouped export".into(), t0.elapsed().as_millis() as u64));

    let result = PackResult {
        config: config.clone(),
        sprites: merged_sprites,
        pages: merged_page_infos,
        warnings,
        stage_times_ms: stage_times,
        total_sprites: all_sprites.len(),
        alias_count,
        output_files,
    };
    Ok(PipelineOutput {
        result,
        pages: merged_pages,
    })
}

/// Sample the normal-map pixels that correspond to the diffuse's trimmed
/// bounds, then blit them into the frame with the same rotation/extrude.
/// Phase 1 of the normal-map blit: fit the map to the sprite's trimmed box and
/// stage it for [`pixels::blit_prepared`].
///
/// Returns `None` for sprites with no trimmed area. Like the colour blit, all
/// the fitting work happens here, outside the page lock, so the lock is held
/// only for the copy.
fn prepare_blit_normal(
    layout: pixels::BlitLayout,
    normal: pixels::TrimmedSprite<'_>,
    sprite: &IngestedSprite,
) -> Option<pixels::PreparedBlit> {
    let (nw, nh) = (normal.width, normal.height);
    let normal = normal.pixels;
    let (tw, th) = (sprite.trimmed_bounds.width, sprite.trimmed_bounds.height);
    if tw <= 0 || th <= 0 {
        return None;
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
            let sy =
                ((y as f32 / sprite.raw_height as f32) * nh as f32).min(nh as f32 - 1.0) as i32;
            for x in 0..sprite.raw_width {
                let sx =
                    ((x as f32 / sprite.raw_width as f32) * nw as f32).min(nw as f32 - 1.0) as i32;
                let src = ((sy * nw + sx) * 4) as usize;
                let dst = ((y * sprite.raw_width + x) * 4) as usize;
                scaled[dst..dst + 4].copy_from_slice(&normal[src..src + 4]);
            }
        }
        let mut out = vec![0u8; (tw * th * 4) as usize];
        for y in 0..th {
            let src_row =
                (sprite.trimmed_bounds.y + y) * sprite.raw_width + sprite.trimmed_bounds.x;
            let dst_row = y * tw;
            out[(dst_row * 4) as usize..((dst_row + tw) * 4) as usize]
                .copy_from_slice(&scaled[(src_row * 4) as usize..((src_row + tw) * 4) as usize]);
        }
        out
    };

    Some(pixels::prepare_blit(
        layout,
        pixels::TrimmedSprite {
            pixels: &trimmed,
            width: tw,
            height: th,
        },
    ))
}

/// True when the base file name contains a multipack placeholder:
/// `{n}` (índice desde 0), `{n0}` (desde 0) or `{n1}` (desde 1).
fn has_page_placeholder(base: &str) -> bool {
    base.contains("{n}") || base.contains("{n0}") || base.contains("{n1}")
}

/// Expand a file-name stem for one sheet + scale variant.
///
/// Placeholders (multipack placeholders): `{n}`/`{n0}` → índice de
/// hoja desde 0, `{n1}` → desde 1, `{v}` → sufijo de variante (incluye el
/// guion bajo: `""` en la base, `_0.5x` en el resto). Cuando el nombre no
/// contiene el placeholder correspondiente se conserva la nomenclatura
/// implícita: `atlas`, `atlas_1`, `atlas_0.5x`, `atlas_1_0.5x`.
fn expand_name(base: &str, index: usize, variant: &str) -> String {
    let has_n = has_page_placeholder(base);
    let has_v = base.contains("{v}");
    let mut s = base.to_string();
    // Sufijo de página implícito: se coloca antes de `{v}` si lo hay.
    if !has_n && index > 0 {
        if let Some(pos) = s.find("{v}") {
            s.insert_str(pos, &format!("_{index}"));
        } else {
            s.push_str(&format!("_{index}"));
        }
    }
    s = s
        .replace("{n1}", &(index + 1).to_string())
        .replace("{n0}", &index.to_string())
        .replace("{n}", &index.to_string());
    if has_v {
        s = s.replace("{v}", variant);
    } else {
        s.push_str(variant);
    }
    s
}

/// Append `ext` unless the stem already ends with it (`sheet{n1}.png` no
/// debe producir `sheet1.png.png`).
fn with_ext(stem: &str, ext: &str) -> String {
    if stem.to_lowercase().ends_with(&format!(".{ext}")) {
        stem.to_string()
    } else {
        format!("{stem}.{ext}")
    }
}

/// Variant suffix `{v}` conventions: `@2x` for
/// integer scales (Retina/iOS), `-hd`/`-sd` for 0.5/1.0 cocos2d pairs, and the
/// plain scale value with dot for anything else (`_0.75x`-style is replaced by
/// `@0.75x`). Base scale 1.0 without other variants gets an empty suffix.
/// Automatic `{v}` suffix of a scale when the project does not name it
/// (`1.0` → nothing, `2.0` → `@2x`, `0.5` → `-hd`, `1/3` → `-sd`).
pub fn variant_suffix(scale: f32) -> String {
    match scale {
        s if (s - 2.0).abs() < 1e-6 => "@2x".to_string(),
        s if (s - 4.0).abs() < 1e-6 => "@4x".to_string(),
        s if (s - 0.5).abs() < 1e-6 => "-hd".to_string(),
        s if (s - 0.25).abs() < 1e-6 => "-lhd".to_string(),
        s if (s - 1.0 / 3.0).abs() < 1e-3 => "-sd".to_string(),
        s => {
            let text = format!("{s}");
            let text = text.trim_end_matches('0').trim_end_matches('.');
            if text == "1" {
                String::new()
            } else {
                format!("@{text}x")
            }
        }
    }
}

fn page_file_name(config: &ProjectConfig, index: usize, variant: &str) -> String {
    let stem = expand_name(&config.base_file_name, index, variant);
    with_ext(&stem, config.gpu_format.file_extension())
}

fn normal_page_file_name(config: &ProjectConfig, index: usize, variant: &str) -> String {
    let base = if config.normal_map_sheet.trim().is_empty() {
        format!("{}_normal", config.base_file_name)
    } else {
        config.normal_map_sheet.clone()
    };
    let stem = expand_name(&base, index, variant);
    with_ext(&stem, config.gpu_format.file_extension())
}

fn metadata_file_name(
    config: &ProjectConfig,
    variant: &str,
    index: usize,
    per_page: bool,
) -> String {
    let stem = if per_page {
        expand_name(&config.base_file_name, index, variant)
    } else {
        // Un único data file: la hoja 0 evita el índice implícito.
        expand_name(&config.base_file_name, 0, variant)
    };
    with_ext(&stem, templates::data_file_extension(config))
}

/// Write one output file, creating its folder first.
///
/// Publishing is incremental by default: a file whose bytes are already on
/// disk is left untouched (its mtime stays put, which incremental game builds
/// notice). `force` (`--force-publish`) rewrites it anyway.
fn write_file(path: &Path, bytes: &[u8], force: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                TpError::Other(format!("No se pudo crear {}: {e}", parent.display()))
            })?;
        }
    }
    if !force && std::fs::read(path).is_ok_and(|existing| existing == bytes) {
        return Ok(());
    }
    std::fs::write(path, bytes).map_err(|e| {
        crate::error::TpError::Other(format!("No se pudo escribir {}: {e}", path.display()))
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_name_keeps_implicit_naming() {
        assert_eq!(variant_suffix(1.0), "");
        assert_eq!(variant_suffix(0.5), "-hd");
        assert_eq!(variant_suffix(2.0), "@2x");
        assert_eq!(variant_suffix(4.0), "@4x");
        assert_eq!(variant_suffix(0.25), "-lhd");
        assert_eq!(variant_suffix(0.75), "@0.75x");
        assert_eq!(expand_name("atlas", 0, ""), "atlas");
        assert_eq!(expand_name("atlas", 1, ""), "atlas_1");
        assert_eq!(expand_name("atlas", 0, "_0.5x"), "atlas_0.5x");
        assert_eq!(expand_name("atlas", 1, "_0.5x"), "atlas_1_0.5x");
    }

    #[test]
    fn normal_sheet_name_honors_the_custom_base() {
        // Por defecto la hoja de normales sigue a la imagen.
        let base = ProjectConfig {
            base_file_name: "atlas".to_string(),
            ..ProjectConfig::default()
        };
        assert_eq!(normal_page_file_name(&base, 0, ""), "atlas_normal.png");
        // Con `normal_map_sheet` manda ese nombre base (con sus placeholders).
        let cfg = ProjectConfig {
            normal_map_sheet: "mynorms".to_string(),
            ..base
        };
        assert_eq!(normal_page_file_name(&cfg, 0, ""), "mynorms.png");
        assert_eq!(normal_page_file_name(&cfg, 1, ""), "mynorms_1.png");
        let cfg = ProjectConfig {
            normal_map_sheet: "n{n1}{v}".to_string(),
            ..cfg
        };
        assert_eq!(normal_page_file_name(&cfg, 2, "-hd"), "n3-hd.png");
    }

    #[test]
    fn expand_name_page_placeholders() {
        assert_eq!(expand_name("sheet{n}", 0, ""), "sheet0");
        assert_eq!(expand_name("sheet{n}", 3, "_2x"), "sheet3_2x");
        assert_eq!(expand_name("sheet{n0}", 0, ""), "sheet0");
        assert_eq!(expand_name("sheet{n1}", 0, ""), "sheet1");
        assert_eq!(expand_name("sheet{n1}", 1, ""), "sheet2");
        assert_eq!(expand_name("out/{n1}/atlas", 0, ""), "out/1/atlas");
    }

    #[test]
    fn expand_name_variant_placeholder() {
        assert_eq!(expand_name("atlas{v}", 0, ""), "atlas");
        assert_eq!(expand_name("atlas{v}", 0, "_0.5x"), "atlas_0.5x");
        assert_eq!(expand_name("atlas{v}", 1, "_0.5x"), "atlas_1_0.5x");
        assert_eq!(expand_name("atlas{n1}{v}", 0, ""), "atlas1");
        assert_eq!(expand_name("atlas{n1}{v}", 1, "_0.5x"), "atlas2_0.5x");
        assert_eq!(expand_name("atlas_{v}", 0, "_0.5x"), "atlas__0.5x");
    }

    #[test]
    fn with_ext_avoids_double_extension() {
        assert_eq!(with_ext("sheet1", "png"), "sheet1.png");
        assert_eq!(with_ext("sheet1.png", "png"), "sheet1.png");
        assert_eq!(with_ext("atlas", "json"), "atlas.json");
        assert_eq!(with_ext("sheet1.PNG", "png"), "sheet1.PNG");
    }

    #[test]
    fn page_placeholder_detection() {
        assert!(has_page_placeholder("sheet{n}"));
        assert!(has_page_placeholder("sheet{n0}"));
        assert!(has_page_placeholder("sheet{n1}"));
        assert!(!has_page_placeholder("atlas"));
        assert!(!has_page_placeholder("atlas{v}"));
        assert!(!has_page_placeholder("banana"));
    }
}
