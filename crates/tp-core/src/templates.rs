//! Motor de Plantillas de Metadatos.
//!
//! Collects the final state of every processed asset and renders it with a
//! Mustache-compatible engine (handlebars). Built-in formats: JSON, XML
//! (libgdx TextureAtlas), Plist (cocos2d), C++ header, TSV and plain text.
//! Users can supply their own `.hbs` template via `export_template`.

use crate::config::{GdxFilter, ProjectConfig, TemplateFormat, TrimMode};
use crate::error::Result;
use crate::types::{PackResult, PageInfo, SpriteAsset};
use serde_json::{json, Value};
use std::collections::HashSet;

/// Extension for the metadata file of a template format.
/// File extension of the data file for each built-in format: LibGDX uses an
/// *XML* atlas (`.atlas` is libgdx's own pack file), cocos2d uses *plist*,
/// C++/ObjC exporters write a *header*.
pub fn metadata_extension(format: TemplateFormat) -> &'static str {
    match format {
        TemplateFormat::Json | TemplateFormat::Phaser | TemplateFormat::PixiJson => "json",
        TemplateFormat::JsonHash => "json",
        TemplateFormat::Xml | TemplateFormat::Starling => "xml",
        TemplateFormat::Plist | TemplateFormat::UIKitPlist => "plist",
        TemplateFormat::LibgdxAtlas | TemplateFormat::SpineAtlas => "atlas",
        TemplateFormat::Css => "css",
        TemplateFormat::CppHeader => "h",
        TemplateFormat::Tsv => "tsv",
        TemplateFormat::PlainText => "txt",
        // «spritesheet-only»: no se escribe fichero de datos.
        TemplateFormat::SpriteSheetOnly => "",
    }
}

/// Extensión del fichero de datos de un proyecto: la del preset de formato
/// de datos (`crate::dataformats`) cuando el
/// proyecto declara uno, y la de la familia en caso contrario.
pub fn data_file_extension(config: &ProjectConfig) -> &'static str {
    match config.data_format_preset() {
        Some(preset) if !preset.extension.is_empty() => preset.extension,
        _ => metadata_extension(config.template_format),
    }
}

/// Build the template context (a JSON value) for a packing result.
///
/// `scale` scales frame coordinates, source sizes and UVs (for @2x/@1x
/// variants). All numeric values are rounded to integers where appropriate.
/// One auto-detected animation.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedAnimation {
    /// Animation name: the sprite base name without its numeric suffix.
    pub name: String,
    /// Ordered sprite ids (`walk_001`, `walk_002`, ...).
    pub frames: Vec<String>,
}

/// Group sprites whose names share a base plus numeric suffix into
/// animations: `walk_001.png`, `walk_002.png`,
/// `walk_003.png` → animation `walk` with 3 frames. Sprites are grouped by
/// their longest common prefix ending in a separator (`_`, `-`, `.` or space)
/// followed by digits only. A group needs ≥ 2 members to count as an
/// animation, and ids keep the source order.
pub fn detect_animations(sprite_ids: &[String]) -> Vec<DetectedAnimation> {
    use std::collections::BTreeMap;

    fn split_suffix(id: &str) -> Option<(&str, &str, u64)> {
        let digits_end = id.len() - id.chars().rev().take_while(|c| c.is_ascii_digit()).count();
        if digits_end == 0 || digits_end == id.len() {
            return None;
        }
        let (base, num) = id.split_at(digits_end);
        let sep = base.chars().last()?;
        if !matches!(sep, '_' | '-' | '.' | ' ') {
            return None;
        }
        let num_value: u64 = num.parse().ok()?;
        // `base` conserva el separador final: walk_001 → base "walk_"
        Some((base, num, num_value))
    }

    let mut groups: BTreeMap<&str, Vec<(&str, u64)>> = BTreeMap::new();
    for id in sprite_ids {
        if let Some((base, _num, value)) = split_suffix(id) {
            groups.entry(base).or_default().push((id, value));
        }
    }

    let mut animations = Vec::new();
    for (base, mut members) in groups {
        if members.len() < 2 {
            continue;
        }
        members.sort_by_key(|(_, v)| *v);
        // La animación se llama como el base sin el separador final.
        let name = base.trim_end_matches(['_', '-', '.', ' ']).to_string();
        animations.push(DetectedAnimation {
            name,
            frames: members.into_iter().map(|(id, _)| id.to_string()).collect(),
        });
    }
    animations
}

pub fn build_context(
    result: &PackResult,
    page_infos: &[PageInfo],
    image_files: &[String],
    scale: f32,
    trim_mode: TrimMode,
) -> Value {
    let mut frames = Vec::new();
    for sprite in &result.sprites {
        let frame = scaled_rect(&sprite.allocated_frame, scale);
        let visible = scaled_rect(&sprite.visible_frame, scale);

        // Source-space geometry: `trimmed_bounds` lives in the *original*
        // image, while `visible_frame` is the position inside the atlas.
        let tw = sprite.trimmed_bounds.width;
        let th = sprite.trimmed_bounds.height;
        let trimmed = trim_mode.trims() && (tw < sprite.raw_width || th < sprite.raw_height);
        let scale_i = |v: i32| (v as f32 * scale).round() as i32;
        let off_x = scale_i(sprite.trimmed_bounds.x);
        let off_y = scale_i(sprite.trimmed_bounds.y);
        let tw_s = scale_i(tw);
        let th_s = scale_i(th);
        let raw_w = scale_i(sprite.raw_width);
        let raw_h = scale_i(sprite.raw_height);

        let (sss_x, sss_y, sss_w, sss_h, src_w, src_h) = match trim_mode {
            // Crop, flush position: the sprite looks as if it never had
            // transparency (offset and original size are dropped).
            TrimMode::Crop if trimmed => (0, 0, tw_s, th_s, tw_s, th_s),
            // No trimming at all: everything stays at 0/0 with the raw size.
            _ if !trimmed => (0, 0, raw_w, raw_h, raw_w, raw_h),
            // Trim / CropKeepPos / Polygon: keep the offset so the engine can
            // restore the original placement.
            _ => (off_x, off_y, tw_s, th_s, raw_w, raw_h),
        };

        // `Crop` moves the anchor into the trimmed space; every other mode
        // reports the pivot relative to the original sprite.
        let pivot = match trim_mode {
            TrimMode::Crop if trimmed => {
                let px = sprite.pivot.x * sprite.raw_width as f32 - sprite.trimmed_bounds.x as f32;
                let py = sprite.pivot.y * sprite.raw_height as f32 - sprite.trimmed_bounds.y as f32;
                json!({
                    "x": round2((px / tw as f32).clamp(0.0, 1.0)),
                    "y": round2((py / th as f32).clamp(0.0, 1.0)),
                })
            }
            _ => json!({"x": sprite.pivot.x, "y": sprite.pivot.y}),
        };

        // Polygon data (local mesh vertices + recomputed UVs for this scale).
        let (polygon, mesh) = match &sprite.mesh {
            Some(m) if !m.vertices.is_empty() => {
                let page = page_infos
                    .iter()
                    .find(|p| p.index == sprite.atlas_page_index as usize);
                let (pw, ph) = page
                    .map(|p| (p.width as f32, p.height as f32))
                    .unwrap_or((1.0, 1.0));
                // UV frame: visible region origin + *local* trimmed dims.
                let uv_frame = crate::types::Rect {
                    x: visible.x,
                    y: visible.y,
                    width: (sprite.trimmed_bounds.width as f32 * scale).round() as i32,
                    height: (sprite.trimmed_bounds.height as f32 * scale).round() as i32,
                };
                let uvs = crate::polygon::compute_uvs(
                    &m.vertices,
                    &uv_frame,
                    pw as i32,
                    ph as i32,
                    sprite.is_rotated,
                );
                let polygon: Vec<Value> = m
                    .vertices
                    .iter()
                    .map(|p| json!([round2(p.x), round2(p.y)]))
                    .collect();
                let mesh = json!({
                    "vertices": m.vertices.iter().map(|p| json!({"x": round2(p.x), "y": round2(p.y)})).collect::<Vec<_>>(),
                    "indices": m.indices,
                    "uvs": uvs.iter().map(|p| json!({"x": round4(p.x), "y": round4(p.y)})).collect::<Vec<_>>(),
                });
                (Some(polygon), Some(mesh))
            }
            _ => (None, None),
        };

        // Starling/Sparrow escribe el marco recortado como `frameX`/`frameY`
        // negativos (desplazamiento desde el borde del sprite original) más
        // el tamaño original completo.
        let frame_x = -sss_x;
        let frame_y = -sss_y;
        // Offset del centro del recorte respecto al centro del original
        // (semántica de `offset` de los plist de Cocos2D/SpriteKit): se
        // publica como cadena «x,y», como hace el formato plist.
        let center_x = sss_x as f32 + sss_w as f32 / 2.0 - src_w as f32 / 2.0;
        let center_y = sss_y as f32 + sss_h as f32 / 2.0 - src_h as f32 / 2.0;
        // Offset desde el borde izquierdo/inferior del original (semántica de
        // `offset` de los atlas de libGDX y Spine: «whitespace stripped from
        // the left and bottom edges»).
        let bottom_left_x = sss_x;
        let bottom_left_y = src_h - sss_y - sss_h;

        frames.push(json!({
            "filename": sprite.id,
            "frame": {"x": frame.x, "y": frame.y, "w": frame.width, "h": frame.height},
            "rotated": sprite.is_rotated,
            "trimmed": trimmed,
            "spriteSourceSize": {"x": sss_x, "y": sss_y, "w": sss_w, "h": sss_h},
            "sourceSize": {"w": src_w, "h": src_h},
            "pivot": pivot,
            "border": sprite.border.map(|b| json!({"left": b[0], "top": b[1], "right": b[2], "bottom": b[3]})),
            "page": sprite.atlas_page_index,
            "aliased": sprite.is_alias,
            "aliasTarget": sprite.alias_target_id,
            "polygon": polygon,
            "mesh": mesh,
            "hasNormalMap": sprite.normal_source_path.is_some(),
            "frameX": frame_x,
            "frameY": frame_y,
            "frameWidth": src_w,
            "frameHeight": src_h,
            "offset": format!("{},{}", fmt_offset(center_x), fmt_offset(center_y)),
            "offsetBottomLeft": {"x": bottom_left_x, "y": bottom_left_y},
            // Nombre usable como clase/selector CSS (`.hero.png` no es un
            // identificador válido): reutiliza el saneador de los ficheros
            // extra (`hero/idle.png` → `hero_idle_png`).
            "cssClass": ident(&sprite.id),
        }));
    }

    // Texture path — prepend the configured path to the texture file
    // name referenced by the metadata (e.g. `/assets` + `atlas.png`).
    let texture_path = result
        .config
        .texture_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| p.trim_end_matches('/'));
    let with_texture_path = |file: String| match texture_path {
        Some(prefix) => format!("{prefix}/{file}"),
        None => file,
    };
    // Cache busting (`--cache-busting`): `atlas.png?v=<hash>` en las
    // referencias de la textura, como los data formats de Pixi/Phaser.
    let bust = |file: String, version: &str| {
        if result.config.cache_busting && !version.is_empty() {
            format!("{file}?v={version}")
        } else {
            file
        }
    };

    // Auto-detect animations — group `walk_001..00N` into `walk`.
    let animations: Vec<Value> = if result.config.enable_auto_detect_animations {
        let ids: Vec<String> = result.sprites.iter().map(|s| s.id.clone()).collect();
        detect_animations(&ids)
            .into_iter()
            .map(|a| {
                json!({
                    "name": a.name,
                    "frames": a.frames,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    let first_image = bust(
        with_texture_path(image_files.first().cloned().unwrap_or_default()),
        &page_infos
            .first()
            .map(|p| p.cache_version.clone())
            .unwrap_or_default(),
    );
    let meta_pages: Vec<Value> = page_infos
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "index": p.index,
                "width": p.width,
                "height": p.height,
                "file": bust(
                    with_texture_path(image_files.get(i).cloned().unwrap_or_default()),
                    &p.cache_version,
                ),
                "fillRatio": round4(p.fill_ratio),
            })
        })
        .collect();

    // Filtro que el data format de LibGDX declara en la hoja.
    let filter = match result.config.gdx_filter {
        GdxFilter::Linear => "Linear, Linear",
        GdxFilter::Nearest => "Nearest, Nearest",
    };

    // Las páginas con sus frames agrupados: lo consumen los exportadores
    // multi-hoja (Phaser escribe una textura por página en el mismo fichero).
    let sheets: Vec<Value> = page_infos
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "index": p.index,
                "file": bust(
                    with_texture_path(image_files.get(i).cloned().unwrap_or_default()),
                    &p.cache_version,
                ),
                "width": p.width,
                "height": p.height,
                "frames": frames
                    .iter()
                    .filter(|f| f["page"].as_i64() == Some(p.index as i64))
                    .cloned()
                    .collect::<Vec<_>>(),
            })
        })
        .collect();

    json!({
        "animations": animations,
        "filter": filter,
        "meta": {
            "app": "TexturePacker-RS",
            "version": env!("CARGO_PKG_VERSION"),
            "image": first_image,
            "format": result.config.color_depth.as_str(),
            "size": {"w": page_infos.first().map(|p| p.width).unwrap_or(0), "h": page_infos.first().map(|p| p.height).unwrap_or(0)},
            "scale": scale_str(scale),
            "pages": meta_pages,
        },
        "frames": frames,
        "sheets": sheets,
    })
}

fn scaled_rect(r: &crate::types::Rect, scale: f32) -> crate::types::Rect {
    crate::types::Rect {
        x: (r.x as f32 * scale).round() as i32,
        y: (r.y as f32 * scale).round() as i32,
        width: (r.width as f32 * scale).round() as i32,
        height: (r.height as f32 * scale).round() as i32,
    }
}

fn scale_str(s: f32) -> String {
    if (s - 1.0).abs() < 1e-6 {
        "1".to_string()
    } else {
        format!("{s}")
    }
}

fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn round4(v: f32) -> f32 {
    (v * 10000.0).round() / 10000.0
}

/// Offset de plist sin colas de cero: `4` → `"4"`, `-1.5` → `"-1.5"`.
fn fmt_offset(v: f32) -> String {
    let r = round2(v);
    if r.fract().abs() < 1e-6 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// Render the metadata for a packing result.
///
/// - `TemplateFormat::Json`: the context serialized as pretty JSON.
/// - otherwise: a built-in Mustache template (or the user's custom template
///   from `config.export_template`) rendered with handlebars.
pub fn render(
    result: &PackResult,
    page_infos: &[PageInfo],
    image_files: &[String],
    scale: f32,
    config: &ProjectConfig,
) -> Result<String> {
    let mut ctx = build_context(
        result,
        page_infos,
        image_files,
        scale,
        config.effective_trim_mode(),
    );
    apply_exporter_properties(&mut ctx, config);
    if config.template_format == TemplateFormat::Css {
        apply_css_sprite_prefix(&mut ctx, config);
    }

    let rendered = match config.template_format {
        // «spritesheet-only»: el pipeline no llama a `render`, pero por si
        // acaso no se dibuja nada.
        TemplateFormat::SpriteSheetOnly => String::new(),
        // El JSON sólo se serializa cuando no hay plantilla propia: un
        // exportador de `--custom-exporters-directory` (o un `--template`)
        // manda incluso con la familia json por defecto.
        TemplateFormat::Json if config.export_template.is_none() => {
            // El JSON con `frames` en lista expone solo su contrato de
            // siempre: `sheets` y los offsets nuevos son internos de los
            // otros exportadores, y por frame se descartan (los recortes de
            // Starling, el `offset` de los atlas y las clases CSS).
            const EXTRA_FRAME_KEYS: [&str; 7] = [
                "frameX",
                "frameY",
                "frameWidth",
                "frameHeight",
                "offset",
                "offsetBottomLeft",
                "cssClass",
            ];
            let frames: Vec<Value> = ctx
                .get("frames")
                .and_then(Value::as_array)
                .map(|frames| {
                    frames
                        .iter()
                        .map(|frame| {
                            let mut frame = frame.as_object().cloned().unwrap_or_default();
                            for key in EXTRA_FRAME_KEYS {
                                frame.remove(key);
                            }
                            Value::Object(frame)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let out = json!({
                "animations": ctx.get("animations").cloned().unwrap_or(Value::Null),
                "filter": ctx.get("filter").cloned().unwrap_or(Value::Null),
                "meta": ctx.get("meta").cloned().unwrap_or(Value::Null),
                "frames": Value::Array(frames),
            });
            serde_json::to_string_pretty(&out).map_err(crate::error::TpError::Json)?
        }
        other => {
            let template = match &config.export_template {
                Some(path) => std::fs::read_to_string(path).map_err(|e| {
                    crate::error::TpError::Other(format!(
                        "No se pudo leer la plantilla {}: {e}",
                        path.display()
                    ))
                })?,
                None => builtin_template(other).to_string(),
            };
            render_mustache(&template, &ctx)?
        }
    };
    Ok(wrap_css_media_query(rendered, config, scale))
}

/// `--plain-string-property` / `--plain-bool-property`: las propiedades de
/// demo del exportador quedan en la raíz del contexto como
/// `exporterProperties.{string_property,bool_property}`. Los marcadores
/// `has_string`/`has_bool` dicen a la plantilla cuáles existen, porque un
/// `false` también hay que escribirlo.
fn apply_exporter_properties(ctx: &mut Value, config: &ProjectConfig) {
    if config.plain_string_property.is_none() && config.plain_bool_property.is_none() {
        return;
    }
    let mut props = serde_json::Map::new();
    if let Some(text) = &config.plain_string_property {
        props.insert("string_property".to_string(), Value::String(text.clone()));
        props.insert("has_string".to_string(), Value::Bool(true));
    }
    if let Some(flag) = config.plain_bool_property {
        props.insert("bool_property".to_string(), Value::Bool(flag));
        props.insert("has_bool".to_string(), Value::Bool(true));
    }
    if let Some(root) = ctx.as_object_mut() {
        root.insert("exporterProperties".to_string(), Value::Object(props));
    }
}

/// `--css-sprite-prefix`: prefijo de cada clase CSS (`icon-hero`), en la
/// raíz y en las páginas. Sólo se llama con la familia CSS.
fn apply_css_sprite_prefix(ctx: &mut Value, config: &ProjectConfig) {
    let Some(prefix) = &config.css_sprite_prefix else {
        return;
    };
    fn prefix_frames(frames: &mut [Value], prefix: &str) {
        for frame in frames.iter_mut() {
            let Some(class) = frame.get("cssClass").and_then(Value::as_str) else {
                continue;
            };
            let value = format!("{prefix}{class}");
            if let Some(obj) = frame.as_object_mut() {
                obj.insert("cssClass".to_string(), Value::String(value));
            }
        }
    }
    if let Some(frames) = ctx.get_mut("frames").and_then(Value::as_array_mut) {
        prefix_frames(frames, prefix);
    }
    if let Some(sheets) = ctx.get_mut("sheets").and_then(Value::as_array_mut) {
        for sheet in sheets.iter_mut() {
            if let Some(frames) = sheet.get_mut("frames").and_then(Value::as_array_mut) {
                prefix_frames(frames, prefix);
            }
        }
    }
}

/// `--css-media-query-2x`: envuelve en la media query pedida la hoja CSS de
/// las variantes por encima de 1× (la «variante -2x»); la base queda como
/// está. Los demás formatos salen intactos.
fn wrap_css_media_query(rendered: String, config: &ProjectConfig, scale: f32) -> String {
    if config.template_format != TemplateFormat::Css || scale <= 1.0 {
        return rendered;
    }
    match &config.css_media_query_2x {
        Some(query) if !query.trim().is_empty() => {
            format!("@media {} {{\n{rendered}}}\n", query.trim())
        }
        _ => rendered,
    }
}

/// Render a Mustache template against a JSON context.
pub fn render_mustache(template: &str, ctx: &Value) -> Result<String> {
    let mut reg = handlebars::Handlebars::new();
    reg.set_strict_mode(false);
    reg.register_escape_fn(handlebars::no_escape);
    reg.render_template(template, ctx).map_err(Into::into)
}

/// Extra data files for frameworks, like `--class-file`,
/// `--header-file`, `--source-file` and `--spriteids-file`. Each entry is
/// `(file name, contents)`; options left empty are skipped.
///
/// The identifier sanitiser maps every non-ASCII-alphanumeric character to
/// `_` (`hero/idle.png` → `hero_idle_png`) and prefixes digits, and names are
/// de-duplicated so the generated C++/Swift always compiles.
pub fn extra_files(config: &ProjectConfig, sprites: &[SpriteAsset]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let ns = ident(&config.base_file_name);
    let mut used: std::collections::HashSet<String> = HashSet::new();
    let ids: Vec<(&str, String)> = sprites
        .iter()
        .map(|s| {
            let mut name = ident(&s.id);
            while !used.insert(name.clone()) {
                name.push('_');
            }
            (s.id.as_str(), name)
        })
        .collect();

    if !config.spriteids_file.trim().is_empty() {
        let mut list = String::new();
        for (id, _) in &ids {
            list.push_str(id);
            list.push('\n');
        }
        out.push((config.spriteids_file.clone(), list));
    }
    if !config.header_file.trim().is_empty() {
        let guard = format!("{}_SPRITES_H", ns.to_uppercase());
        let mut h = String::new();
        h.push_str("// Generado por TexturePacker-RS — no editar.\n");
        h.push_str(&format!("#ifndef {guard}\n#define {guard}\n\n"));
        h.push_str(&format!("namespace {ns} {{\n"));
        for (id, name) in &ids {
            h.push_str(&format!("extern const char* const {name}; // {id}\n"));
        }
        h.push_str(&format!("}} // namespace {ns}\n\n#endif // {guard}\n"));
        out.push((config.header_file.clone(), h));
    }
    if !config.source_file.trim().is_empty() {
        let include = if !config.header_file.trim().is_empty() {
            config
                .header_file
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&config.header_file)
                .to_string()
        } else {
            format!("{ns}.h")
        };
        let mut src = String::new();
        src.push_str("// Generado por TexturePacker-RS — no editar.\n");
        src.push_str(&format!("#include \"{include}\"\n\n"));
        src.push_str(&format!("namespace {ns} {{\n"));
        for (id, name) in &ids {
            src.push_str(&format!("const char* const {name} = \"{id}\";\n"));
        }
        src.push_str(&format!("}} // namespace {ns}\n"));
        out.push((config.source_file.clone(), src));
    }
    if !config.class_file.trim().is_empty() {
        let mut cls = String::new();
        cls.push_str("// Generado por TexturePacker-RS — no editar.\n");
        cls.push_str(&format!("public enum {ns} {{\n"));
        for (id, name) in &ids {
            cls.push_str(&format!("  public static let {name} = \"{id}\"\n"));
        }
        cls.push_str("}\n");
        out.push((config.class_file.clone(), cls));
    }
    out
}

/// C/C++/Swift identifier for a sprite id or base file name.
fn ident(raw: &str) -> String {
    let mut out: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.is_empty() {
        return "_".to_string();
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

pub(crate) fn builtin_template(format: TemplateFormat) -> &'static str {
    match format {
        TemplateFormat::Xml => r#"<TextureAtlas imagePath="{{meta.image}}" filter="{{filter}}">
{{#each frames}}	<sprite n="{{this.filename}}" x="{{this.frame.x}}" y="{{this.frame.y}}" w="{{this.frame.w}}" h="{{this.frame.h}}" oX="{{this.spriteSourceSize.x}}" oY="{{this.spriteSourceSize.y}}" oW="{{this.sourceSize.w}}" oH="{{this.sourceSize.h}}"{{#if this.rotated}} r="y"{{/if}}{{#if this.trimmed}} t="y"{{/if}}/>
{{/each}}</TextureAtlas>
"#,
        TemplateFormat::Plist => r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>frames</key>
	<dict>
{{#each frames}}		<key>{{this.filename}}</key>
		<dict>
			<key>frame</key>
			<string>{{this.frame.x}},{{this.frame.y}},{{this.frame.w}},{{this.frame.h}}</string>
			<key>offset</key>
			<string>{{this.offset}}</string>
			<key>rotated</key>
			{{#if this.rotated}}<true/>{{else}}<false/>{{/if}}
			<key>sourceSize</key>
			<string>{{this.sourceSize.w}},{{this.sourceSize.h}}</string>
			{{#if this.trimmed}}<key>trimmed</key>
			<true/>{{else}}<key>trimmed</key>
			<false/>{{/if}}
			{{#if this.aliased}}<key>alias</key>
			<string>{{this.aliasTarget}}</string>
			{{/if}}
		</dict>
{{/each}}	</dict>
	<key>metadata</key>
	<dict>
		<key>format</key>
		<integer>3</integer>
		{{#if animations}}<key>animations</key>
		<dict>
{{#each animations}}			<key>{{this.name}}</key>
			<array>
{{#each this.frames}}				<string>{{this}}</string>
{{/each}}			</array>
{{/each}}		</dict>
		{{/if}}<key>realTextureFileName</key>
		<string>{{meta.image}}</string>
		<key>size</key>
		<string>{{meta.size.w}},{{meta.size.h}}</string>
		<key>textureFileName</key>
		<string>{{meta.image}}</string>
	</dict>
</dict>
</plist>
"#,
        // JSON con `frames` como mapa por nombre (familia `json`).
        TemplateFormat::JsonHash => r#"{
  "frames": {
{{#each frames}}    "{{this.filename}}": {
      "frame": {"x": {{this.frame.x}}, "y": {{this.frame.y}}, "w": {{this.frame.w}}, "h": {{this.frame.h}}},
      "rotated": {{#if this.rotated}}true{{else}}false{{/if}},
      "trimmed": {{#if this.trimmed}}true{{else}}false{{/if}},
      "spriteSourceSize": {"x": {{this.spriteSourceSize.x}}, "y": {{this.spriteSourceSize.y}}, "w": {{this.spriteSourceSize.w}}, "h": {{this.spriteSourceSize.h}}},
      "sourceSize": {"w": {{this.sourceSize.w}}, "h": {{this.sourceSize.h}}},
      "pivot": {"x": {{this.pivot.x}}, "y": {{this.pivot.y}}}
    }{{#unless @last}},
{{/unless}}{{/each}}
  },
  "meta": {
    "app": "{{@root.meta.app}}",
    "version": "{{@root.meta.version}}",
    "image": "{{@root.meta.image}}",
    "format": "{{@root.meta.format}}",
    "size": {"w": {{@root.meta.size.w}}, "h": {{@root.meta.size.h}}},
    "scale": "{{@root.meta.scale}}"
  }
}
"#,
        // JSON hash de PixiJS: lo mismo que el hash genérico con la imagen
        // declarada en cada frame.
        TemplateFormat::PixiJson => r#"{
  "frames": {
{{#each frames}}    "{{this.filename}}": {
      "frame": {"x": {{this.frame.x}}, "y": {{this.frame.y}}, "w": {{this.frame.w}}, "h": {{this.frame.h}}},
      "rotated": {{#if this.rotated}}true{{else}}false{{/if}},
      "trimmed": {{#if this.trimmed}}true{{else}}false{{/if}},
      "spriteSourceSize": {"x": {{this.spriteSourceSize.x}}, "y": {{this.spriteSourceSize.y}}, "w": {{this.spriteSourceSize.w}}, "h": {{this.spriteSourceSize.h}}},
      "sourceSize": {"w": {{this.sourceSize.w}}, "h": {{this.sourceSize.h}}},
      "pivot": {"x": {{this.pivot.x}}, "y": {{this.pivot.y}}},
      "image": "{{@root.meta.image}}"
    }{{#unless @last}},
{{/unless}}{{/each}}
  },
  "meta": {
    "app": "{{@root.meta.app}}",
    "version": "{{@root.meta.version}}",
    "image": "{{@root.meta.image}}",
    "format": "{{@root.meta.format}}",
    "size": {"w": {{@root.meta.size.w}}, "h": {{@root.meta.size.h}}},
    "scale": "{{@root.meta.scale}}"
  }
}
"#,
        // Phaser 3: una entrada de textura por hoja con sus frames dentro.
        TemplateFormat::Phaser => r#"{
  "textures": [
{{#each sheets}}    {
      "image": "{{this.file}}",
      "format": "{{@root.meta.format}}",
      "size": {"w": {{this.width}}, "h": {{this.height}}},
      "scale": {{@root.meta.scale}},
      "frames": [
{{#each this.frames}}        {
          "filename": "{{this.filename}}",
          "frame": {"x": {{this.frame.x}}, "y": {{this.frame.y}}, "w": {{this.frame.w}}, "h": {{this.frame.h}}},
          "rotated": {{#if this.rotated}}true{{else}}false{{/if}},
          "trimmed": {{#if this.trimmed}}true{{else}}false{{/if}},
          "spriteSourceSize": {"x": {{this.spriteSourceSize.x}}, "y": {{this.spriteSourceSize.y}}, "w": {{this.spriteSourceSize.w}}, "h": {{this.spriteSourceSize.h}}},
          "sourceSize": {"w": {{this.sourceSize.w}}, "h": {{this.sourceSize.h}}},
          "pivot": {"x": {{this.pivot.x}}, "y": {{this.pivot.y}}}
        }{{#unless @last}},
{{/unless}}{{/each}}
      ]
    }{{#unless @last}},{{/unless}}{{/each}}
  ],
  "meta": {
    "app": "{{@root.meta.app}}",
    "version": "{{@root.meta.version}}",
    "image": "{{@root.meta.image}}",
    "format": "{{@root.meta.format}}",
    "size": {"w": {{@root.meta.size.w}}, "h": {{@root.meta.size.h}}},
    "scale": "{{@root.meta.scale}}",
    "type": "original",
    "multiPack": true,
    "prioritySort": "normal"
  }
}
"#,
        // Sparrow/Starling: `frameX`/`frameY` negativos y tamaño original.
        TemplateFormat::Starling => r#"<TextureAtlas imagePath="{{meta.image}}">
{{#each frames}}	<SubTexture name="{{this.filename}}" x="{{this.frame.x}}" y="{{this.frame.y}}" width="{{this.frame.w}}" height="{{this.frame.h}}"{{#if this.trimmed}} frameX="{{this.frameX}}" frameY="{{this.frameY}}" frameWidth="{{this.frameWidth}}" frameHeight="{{this.frameHeight}}"{{/if}}{{#if this.rotated}} rotated="true"{{/if}}/>
{{/each}}</TextureAtlas>
"#,
        // UIKit: plist con una clave escalar por campo.
        TemplateFormat::UIKitPlist => r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>frames</key>
	<dict>
{{#each frames}}		<key>{{this.filename}}</key>
		<dict>
			<key>x</key>
			<integer>{{this.frame.x}}</integer>
			<key>y</key>
			<integer>{{this.frame.y}}</integer>
			<key>width</key>
			<integer>{{this.frame.w}}</integer>
			<key>height</key>
			<integer>{{this.frame.h}}</integer>
			<key>rotated</key>
			{{#if this.rotated}}<true/>{{else}}<false/>{{/if}}
			<key>trimmed</key>
			{{#if this.trimmed}}<true/>{{else}}<false/>{{/if}}
			<key>offsetX</key>
			<integer>{{this.spriteSourceSize.x}}</integer>
			<key>offsetY</key>
			<integer>{{this.spriteSourceSize.y}}</integer>
			<key>originalWidth</key>
			<integer>{{this.sourceSize.w}}</integer>
			<key>originalHeight</key>
			<integer>{{this.sourceSize.h}}</integer>
		</dict>
{{/each}}	</dict>
	<key>metadata</key>
	<dict>
		<key>textureFileName</key>
		<string>{{meta.image}}</string>
		<key>size</key>
		<string>{{meta.size.w}},{{meta.size.h}}</string>
	</dict>
</dict>
</plist>
"#,
        // Atlas de texto de libGDX: una sección por hoja, `offset` contado
        // desde el borde izquierdo/inferior del original.
        TemplateFormat::LibgdxAtlas => r#"{{#each sheets}}{{this.file}}
size: {{this.width}}, {{this.height}}
format: {{@root.meta.format}}
filter: {{@root.filter}}
repeat: none
{{#each this.frames}}
{{this.filename}}
rotate: {{#if this.rotated}}true{{else}}false{{/if}}
xy: {{this.frame.x}}, {{this.frame.y}}
size: {{this.frame.w}}, {{this.frame.h}}
orig: {{this.sourceSize.w}}, {{this.sourceSize.h}}
offset: {{this.offsetBottomLeft.x}}, {{this.offsetBottomLeft.y}}
index: -1
{{/each}}
{{/each}}"#,
        // Atlas de texto de Spine: mismas claves, sangradas dos espacios y
        // región y región separadas por línea en blanco.
        TemplateFormat::SpineAtlas => r#"{{#each sheets}}{{this.file}}
size: {{this.width}}, {{this.height}}
format: {{@root.meta.format}}
filter: {{@root.filter}}
repeat: none

{{#each this.frames}}{{this.filename}}
  rotate: {{#if this.rotated}}true{{else}}false{{/if}}
  xy: {{this.frame.x}}, {{this.frame.y}}
  size: {{this.frame.w}}, {{this.frame.h}}
  orig: {{this.sourceSize.w}}, {{this.sourceSize.h}}
  offset: {{this.offsetBottomLeft.x}}, {{this.offsetBottomLeft.y}}
  index: -1

{{/each}}{{/each}}"#,
        // CSS (también para `less` y `sass-mixins`): una regla por sprite.
        TemplateFormat::Css => r#"/* Generado por TexturePacker-RS — hojas como sprites */
{{#each sheets}}/* Página {{this.index}}: {{this.file}} */
{{#each this.frames}}.{{this.cssClass}} {
  width: {{this.frame.w}}px;
  height: {{this.frame.h}}px;
  background-image: url({{../file}});
  background-position: -{{this.frame.x}}px -{{this.frame.y}}px;
}
{{/each}}
{{/each}}"#,
        // Solo la hoja: no se dibuja nada (el pipeline omite el fichero).
        TemplateFormat::SpriteSheetOnly => "",
        TemplateFormat::CppHeader => r#"// Generated by TexturePacker-RS v{{meta.version}} — do not edit.
#pragma once

namespace atlas {
struct Sprite {
    const char* name;
    int x, y, w, h;
    float offsetX, offsetY;   // offset in the original image
    float originalW, originalH;
    bool rotated;
    float pivotX, pivotY;     // normalized 0..1
    int page;
};

static constexpr Sprite kSprites[] = {
{{#each frames}}    { "{{this.filename}}", {{this.frame.x}}, {{this.frame.y}}, {{this.frame.w}}, {{this.frame.h}}, {{this.spriteSourceSize.x}}f, {{this.spriteSourceSize.y}}f, {{this.sourceSize.w}}f, {{this.sourceSize.h}}f, {{#if this.rotated}}true{{else}}false{{/if}}, {{this.pivot.x}}f, {{this.pivot.y}}f, {{this.page}} },
{{/each}}};

static constexpr int kSpriteCount = {{frames.length}};
} // namespace atlas
"#,
        TemplateFormat::Tsv => {
            "name\tx\ty\tw\th\toffsetX\toffsetY\toriginalWidth\toriginalHeight\trotated\tpage\tpivotX\tpivotY\n{{#each frames}}{{this.filename}}\t{{this.frame.x}}\t{{this.frame.y}}\t{{this.frame.w}}\t{{this.frame.h}}\t{{this.spriteSourceSize.x}}\t{{this.spriteSourceSize.y}}\t{{this.sourceSize.w}}\t{{this.sourceSize.h}}\t{{#if this.rotated}}1{{else}}0{{/if}}\t{{this.page}}\t{{this.pivot.x}}\t{{this.pivot.y}}\n{{/each}}"
        }
        TemplateFormat::PlainText => {
            "{{#each frames}}{{this.filename}}: frame=({{this.frame.x}},{{this.frame.y}},{{this.frame.w}},{{this.frame.h}}) rotated={{#if this.rotated}}true{{else}}false{{/if}} page={{this.page}} pivot=({{this.pivot.x}},{{this.pivot.y}}) offset=({{this.spriteSourceSize.x}},{{this.spriteSourceSize.y}}) size=({{this.sourceSize.w}},{{this.sourceSize.h}}){{#if this.aliased}} alias-of={{this.aliasTarget}}{{/if}}\n{{/each}}{{#if exporterProperties.has_string}}string_property: {{exporterProperties.string_property}}\n{{/if}}{{#if exporterProperties.has_bool}}bool_property: {{exporterProperties.bool_property}}\n{{/if}}"
        }
        TemplateFormat::Json => unreachable!("JSON is serialized directly"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectConfig;
    use crate::types::{PageInfo, Rect, SpriteAsset};

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn detect_animations_groups_numeric_suffixes() {
        let got = detect_animations(&ids(&[
            "walk_001", "walk_002", "walk_003", "hero", "run-1", "run-2",
        ]));
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "run");
        assert_eq!(
            got[0].frames,
            vec!["run-1".to_string(), "run-2".to_string()]
        );
        assert_eq!(got[1].name, "walk");
        assert_eq!(
            got[1].frames,
            vec![
                "walk_001".to_string(),
                "walk_002".to_string(),
                "walk_003".to_string()
            ]
        );
    }

    #[test]
    fn detect_animations_requires_two_members_and_separator() {
        // Sin separador antes del número no hay grupo; un miembro solo tampoco.
        let got = detect_animations(&ids(&["walk1", "walk_2", "jump_1"]));
        assert!(got.is_empty());
    }

    #[test]
    fn detect_animations_sorts_by_number_not_by_name() {
        // `10` debe ir tras `9`, no antes por orden lexicográfico.
        let got = detect_animations(&ids(&["f_9", "f_10", "f_11"]));
        assert_eq!(
            got[0].frames,
            vec!["f_9".to_string(), "f_10".to_string(), "f_11".to_string()]
        );
    }

    #[test]
    fn extra_files_generate_cpp_swift_and_id_list() {
        let mut cfg = ProjectConfig {
            base_file_name: "atlas".into(),
            class_file: "Sprites.swift".into(),
            header_file: "Sprites.h".into(),
            source_file: "Sprites.cpp".into(),
            spriteids_file: "spriteids.txt".into(),
            ..ProjectConfig::default()
        };
        let mut sprites = sample_result().sprites;
        sprites[0].id = "1up/idle.png".into();
        let mut twin = sprites[0].clone();
        twin.id = "1up-idle.png".into(); // mismo identificador: se deduplica
        sprites.push(twin);

        let files = extra_files(&cfg, &sprites);
        assert_eq!(files.len(), 4);
        let get = |name: &str| -> String {
            files
                .iter()
                .find(|(n, _)| n == name)
                .unwrap_or_else(|| panic!("falta {name}"))
                .1
                .clone()
        };

        assert_eq!(get("spriteids.txt"), "1up/idle.png\n1up-idle.png\n");

        let header = get("Sprites.h");
        assert!(header.contains("#ifndef ATLAS_SPRITES_H"), "{header}");
        assert!(header.contains("namespace atlas {"), "{header}");
        assert!(
            header.contains("extern const char* const _1up_idle_png; // 1up/idle.png"),
            "{header}"
        );
        assert!(header.contains("_1up_idle_png_; "), "sin dedupe: {header}");
        assert!(header.contains("#endif // ATLAS_SPRITES_H"), "{header}");

        let source = get("Sprites.cpp");
        assert!(source.contains("#include \"Sprites.h\""), "{source}");
        assert!(
            source.contains("const char* const _1up_idle_png = \"1up/idle.png\";"),
            "{source}"
        );

        let swift = get("Sprites.swift");
        assert!(swift.contains("public enum atlas {"), "{swift}");
        assert!(
            swift.contains("public static let _1up_idle_png = \"1up/idle.png\""),
            "{swift}"
        );

        // Vacío = no se escribe ese fichero.
        cfg.class_file.clear();
        cfg.header_file.clear();
        let files = extra_files(&cfg, &sprites);
        assert_eq!(files.len(), 2);
        let source = files
            .iter()
            .find(|(n, _)| n == "Sprites.cpp")
            .map(|(_, c)| c.clone())
            .unwrap();
        // Sin cabecera configurada se incluye la derivada del nombre base.
        assert!(source.contains("#include \"atlas.h\""), "{source}");
    }

    fn sample_result() -> PackResult {
        let cfg = ProjectConfig {
            color_depth: crate::config::ColorDepth::Rgba8888,
            ..ProjectConfig::default()
        };
        PackResult {
            config: cfg,
            sprites: vec![SpriteAsset {
                id: "hero".into(),
                source_path: "hero.png".into(),
                raw_width: 100,
                raw_height: 80,
                trimmed_bounds: Rect::new(4, 5, 90, 70),
                offset_x: 4,
                offset_y: 5,
                pixel_hash: "abc".into(),
                is_alias: false,
                alias_target_id: None,
                pivot: crate::types::Point2D::new(0.5, 0.5),
                border: Some([4, 4, 4, 4]),
                mesh: None,
                allocated_frame: Rect::new(10, 20, 94, 74),
                visible_frame: Rect::new(12, 22, 90, 70),
                is_rotated: false,
                atlas_page_index: 0,
                normal_source_path: None,
                is_alias_target: true,
                contours: vec![],
            }],
            pages: vec![],
            warnings: vec![],
            stage_times_ms: vec![],
            total_sprites: 1,
            alias_count: 0,
            output_files: vec!["atlas.png".into()],
        }
    }

    fn pages() -> Vec<PageInfo> {
        vec![PageInfo {
            index: 0,
            width: 256,
            height: 256,
            file_name: "atlas.png".into(),
            format: "RGBA8888".into(),
            has_normals: false,
            normal_file_name: None,
            encrypted: false,
            fill_ratio: 0.5,
            cache_version: String::new(),
        }]
    }

    #[test]
    fn json_contains_frames() {
        let r = sample_result();
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["frames"][0]["filename"], "hero");
        assert_eq!(v["frames"][0]["frame"]["x"], 10);
        assert_eq!(v["meta"]["image"], "atlas.png");
    }

    #[test]
    fn json_array_keeps_its_original_contract() {
        let r = sample_result();
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();

        // Las claves nuevas de los otros exportadores no se cuelan: ni la
        // agrupación por hojas ni los offsets/instancias por frame.
        assert!(v.get("sheets").is_none(), "{out}");
        let frame = &v["frames"][0];
        for key in [
            "frameX",
            "frameY",
            "frameWidth",
            "frameHeight",
            "offset",
            "offsetBottomLeft",
            "cssClass",
        ] {
            assert!(frame.get(key).is_none(), "sobra {key}:\n{out}");
        }

        // Las de siempre siguen intactas.
        for key in [
            "filename",
            "frame",
            "rotated",
            "trimmed",
            "spriteSourceSize",
            "sourceSize",
            "pivot",
            "page",
            "aliased",
        ] {
            assert!(frame.get(key).is_some(), "falta {key}:\n{out}");
        }
    }

    #[test]
    fn cache_busting_appends_the_file_version_to_the_texture_reference() {
        let mut r = sample_result();
        r.config.cache_busting = true;
        r.config.texture_path = Some("/assets".into());
        let mut ps = pages();
        ps[0].cache_version = "ab12cd34".into();

        let out = render(&r, &ps, &["atlas.png".into()], 1.0, &r.config).unwrap();
        assert!(
            out.contains("/assets/atlas.png?v=ab12cd34"),
            "falta el sufijo de cache busting:\n{out}"
        );

        // Apagado: la referencia vuelve a ser el nombre limpio.
        r.config.cache_busting = false;
        let out = render(&r, &ps, &["atlas.png".into()], 1.0, &r.config).unwrap();
        assert!(out.contains("/assets/atlas.png\""), "sobra ?v:\n{out}");
    }

    #[test]
    fn gdx_filter_is_declared_on_the_xml_atlas() {
        let mut r = sample_result();
        r.config.template_format = TemplateFormat::Xml;

        r.config.gdx_filter = GdxFilter::Nearest;
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        assert!(
            out.contains(r#"filter="Nearest, Nearest""#),
            "filtro ausente:\n{out}"
        );

        r.config.gdx_filter = GdxFilter::Linear;
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        assert!(out.contains(r#"filter="Linear, Linear""#), "{out}");
    }

    #[test]
    fn texture_path_prefixes_image_references() {
        let mut r = sample_result();
        r.config.texture_path = Some("/assets/".into());
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["meta"]["image"], "/assets/atlas.png");
        assert_eq!(v["meta"]["pages"][0]["file"], "/assets/atlas.png");

        // Sin texture_path el nombre no cambia.
        r.config.texture_path = None;
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["meta"]["image"], "atlas.png");

        // El prefijo también llega a las plantillas Mustache (XML).
        r.config.texture_path = Some("/assets".into());
        r.config.template_format = TemplateFormat::Xml;
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        assert!(
            out.contains("imagePath=\"/assets/atlas.png\""),
            "out: {out}"
        );
    }

    #[test]
    fn xml_renders() {
        let mut r = sample_result();
        r.config.template_format = TemplateFormat::Xml;
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        assert!(out.contains("<TextureAtlas"));
        assert!(out.contains("n=\"hero\""));
        assert!(out.contains("x=\"10\""));
    }

    #[test]
    fn plist_renders() {
        let mut r = sample_result();
        r.config.template_format = TemplateFormat::Plist;
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        assert!(out.contains("<plist"));
        assert!(out.contains("<key>hero</key>"));
        assert!(out.contains("10,20,94,74"));
    }

    #[test]
    fn scale_half() {
        let r = sample_result();
        let out = render(&r, &pages(), &["atlas.png".into()], 0.5, &r.config).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["frames"][0]["frame"]["x"], 5);
        assert_eq!(v["frames"][0]["frame"]["w"], 47);
        assert_eq!(v["meta"]["scale"], "0.5");
    }

    #[test]
    fn sprite_source_size_reports_source_offset_not_atlas() {
        let r = sample_result();
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let sss = &v["frames"][0]["spriteSourceSize"];
        assert_eq!(
            sss["x"], 4,
            "offset X debe ser el del original, no el atlas"
        );
        assert_eq!(
            sss["y"], 5,
            "offset Y debe ser el del original, no el atlas"
        );
        assert_eq!(sss["w"], 90);
        assert_eq!(v["frames"][0]["trimmed"], true);
        assert_eq!(v["frames"][0]["sourceSize"]["w"], 100);
    }

    #[test]
    fn trim_mode_none_keeps_original_geometry() {
        let mut r = sample_result();
        r.config.enable_trim = false;
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let f = &v["frames"][0];
        assert_eq!(f["trimmed"], false);
        assert_eq!(f["spriteSourceSize"]["x"], 0);
        assert_eq!(f["spriteSourceSize"]["w"], 100);
        assert_eq!(f["sourceSize"]["h"], 80);
    }

    #[test]
    fn crop_flushes_position_and_moves_pivot() {
        let mut r = sample_result();
        r.config.trim_mode = crate::config::TrimMode::Crop;
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let f = &v["frames"][0];
        assert_eq!(f["spriteSourceSize"]["x"], 0);
        assert_eq!(f["spriteSourceSize"]["w"], 90);
        assert_eq!(f["sourceSize"]["w"], 90);
        // (0.5*100 - 4) / 90 = 0.5111... -> 0.51
        let px = f["pivot"]["x"].as_f64().unwrap();
        assert!((px - 0.51).abs() < 1e-6, "pivot.x = {px}");
    }

    #[test]
    fn custom_template() {
        let mut r = sample_result();
        r.config.template_format = TemplateFormat::PlainText;
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        assert!(out.contains("hero: frame=(10,20,94,74)"));
    }
}
