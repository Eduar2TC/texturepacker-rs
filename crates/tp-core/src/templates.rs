//! Motor de Plantillas de Metadatos.
//!
//! Collects the final state of every processed asset and renders it with a
//! Mustache-compatible engine (handlebars). Built-in formats: JSON, XML
//! (libgdx TextureAtlas), Plist (cocos2d), C++ header, TSV and plain text.
//! Users can supply their own `.hbs` template via `export_template`.

use crate::config::{ProjectConfig, TemplateFormat, TrimMode};
use crate::error::Result;
use crate::types::{PackResult, PageInfo};
use serde_json::{json, Value};

/// Extension for the metadata file of a template format.
/// File extension of the data file for each built-in format, following the
/// conventions of the official TexturePacker exporters: LibGDX is an *XML*
/// atlas (`.atlas` would be libgdx's own pack file), cocos2d uses *plist*,
/// C++/ObjC exporters write a *header*.
pub fn metadata_extension(format: TemplateFormat) -> &'static str {
    match format {
        TemplateFormat::Json => "json",
        TemplateFormat::Xml => "xml",
        TemplateFormat::Plist => "plist",
        TemplateFormat::CppHeader => "h",
        TemplateFormat::Tsv => "tsv",
        TemplateFormat::PlainText => "txt",
    }
}

/// Build the template context (a JSON value) for a packing result.
///
/// `scale` scales frame coordinates, source sizes and UVs (for @2x/@1x
/// variants). All numeric values are rounded to integers where appropriate.
/// One auto-detected animation (docs: *Auto-detect animations*).
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedAnimation {
    /// Animation name: the sprite base name without its numeric suffix.
    pub name: String,
    /// Ordered sprite ids (`walk_001`, `walk_002`, ...).
    pub frames: Vec<String>,
}

/// Group sprites whose names share a base plus numeric suffix into
/// animations, TexturePacker-style: `walk_001.png`, `walk_002.png`,
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
        }));
    }

    // Docs: *Texture path* — prepend the configured path to the texture file
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

    // Docs: *Auto-detect animations* — group `walk_001..00N` into `walk`.
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

    let first_image = with_texture_path(image_files.first().cloned().unwrap_or_default());
    let meta_pages: Vec<Value> = page_infos
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "index": p.index,
                "width": p.width,
                "height": p.height,
                "file": with_texture_path(image_files.get(i).cloned().unwrap_or_default()),
                "fillRatio": round4(p.fill_ratio),
            })
        })
        .collect();

    json!({
        "animations": animations,
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
    let ctx = build_context(
        result,
        page_infos,
        image_files,
        scale,
        config.effective_trim_mode(),
    );

    match config.template_format {
        TemplateFormat::Json => {
            serde_json::to_string_pretty(&ctx).map_err(crate::error::TpError::Json)
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
            render_mustache(&template, &ctx)
        }
    }
}

/// Render a Mustache template against a JSON context.
pub fn render_mustache(template: &str, ctx: &Value) -> Result<String> {
    let mut reg = handlebars::Handlebars::new();
    reg.set_strict_mode(false);
    reg.register_escape_fn(handlebars::no_escape);
    reg.render_template(template, ctx).map_err(Into::into)
}

fn builtin_template(format: TemplateFormat) -> &'static str {
    match format {
        TemplateFormat::Xml => r#"<TextureAtlas imagePath="{{meta.image}}">
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
			<string>{{this.spriteSourceSize.x}},{{this.spriteSourceSize.y}}</string>
			<key>rotated</key>
			{{#if this.rotated}}<true/>{{else}}<false/>{{/if}}
			<key>sourceSize</key>
			<string>{{this.sourceSize.w}},{{this.sourceSize.h}}</string>
			{{#if this.trimmed}}<key>trimmed</key>
			<true/>{{else}}<key>trimmed</key>
			<false/>{{/if}}
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
            "{{#each frames}}{{this.filename}}: frame=({{this.frame.x}},{{this.frame.y}},{{this.frame.w}},{{this.frame.h}}) rotated={{#if this.rotated}}true{{else}}false{{/if}} page={{this.page}} pivot=({{this.pivot.x}},{{this.pivot.y}}) offset=({{this.spriteSourceSize.x}},{{this.spriteSourceSize.y}}) size=({{this.sourceSize.w}},{{this.sourceSize.h}}){{#if this.aliased}} alias-of={{this.aliasTarget}}{{/if}}\n{{/each}}"
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
