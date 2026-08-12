//! Motor de Plantillas de Metadatos.
//!
//! Collects the final state of every processed asset and renders it with a
//! Mustache-compatible engine (handlebars). Built-in formats: JSON, XML
//! (libgdx TextureAtlas), Plist (cocos2d), C++ header, TSV and plain text.
//! Users can supply their own `.hbs` template via `export_template`.

use crate::config::{ProjectConfig, TemplateFormat};
use crate::types::{PackResult, PageInfo};
use serde_json::{json, Value};

/// Extension for the metadata file of a template format.
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
pub fn build_context(
    result: &PackResult,
    page_infos: &[PageInfo],
    image_files: &[String],
    scale: f32,
) -> Value {
    let mut frames = Vec::new();
    for sprite in &result.sprites {
        let frame = scaled_rect(&sprite.allocated_frame, scale);
        let visible = scaled_rect(&sprite.visible_frame, scale);

        let trimmed = sprite.trimmed_bounds.width > 0 && sprite.trimmed_bounds.height > 0;

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
            "spriteSourceSize": {"x": visible.x, "y": visible.y, "w": visible.width, "h": visible.height},
            "sourceSize": {"w": (sprite.raw_width as f32 * scale).round() as i64, "h": (sprite.raw_height as f32 * scale).round() as i64},
            "pivot": {"x": sprite.pivot.x, "y": sprite.pivot.y},
            "page": sprite.atlas_page_index,
            "aliased": sprite.is_alias,
            "aliasTarget": sprite.alias_target_id,
            "polygon": polygon,
            "mesh": mesh,
            "hasNormalMap": sprite.normal_source_path.is_some(),
        }));
    }

    let first_image = image_files.first().cloned().unwrap_or_default();
    let meta_pages: Vec<Value> = page_infos
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "index": p.index,
                "width": p.width,
                "height": p.height,
                "file": image_files.get(i).cloned().unwrap_or_default(),
                "fillRatio": round4(p.fill_ratio),
            })
        })
        .collect();

    json!({
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
) -> Result<String, String> {
    let ctx = build_context(result, page_infos, image_files, scale);

    match config.template_format {
        TemplateFormat::Json => {
            serde_json::to_string_pretty(&ctx).map_err(|e| format!("JSON: {e}"))
        }
        other => {
            let template = match &config.export_template {
                Some(path) => std::fs::read_to_string(path)
                    .map_err(|e| format!("No se pudo leer la plantilla {}: {e}", path.display()))?,
                None => builtin_template(other).to_string(),
            };
            render_mustache(&template, &ctx)
        }
    }
}

/// Render a Mustache template against a JSON context.
pub fn render_mustache(template: &str, ctx: &Value) -> Result<String, String> {
    let mut reg = handlebars::Handlebars::new();
    reg.set_strict_mode(false);
    reg.register_escape_fn(handlebars::no_escape);
    reg.render_template(template, ctx)
        .map_err(|e| format!("Error de plantilla: {e}"))
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
		<key>realTextureFileName</key>
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

    fn sample_result() -> PackResult {
        let mut cfg = ProjectConfig::default();
        cfg.color_depth = crate::config::ColorDepth::Rgba8888;
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
    fn custom_template() {
        let mut r = sample_result();
        r.config.template_format = TemplateFormat::PlainText;
        let out = render(&r, &pages(), &["atlas.png".into()], 1.0, &r.config).unwrap();
        assert!(out.contains("hero: frame=(10,20,94,74)"));
    }
}
