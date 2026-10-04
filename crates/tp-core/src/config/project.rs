use super::variants::lcm_capped;
use super::{
    scale_denominator, AlphaHandling, BasicSortBy, ColorDepth, DitheringAlgorithm, DxtMode,
    FolderGroup, GdxFilter, GpuFormat, ManualGrid, PackMode, PackingAlgorithm, PackingStrategy,
    PixelFormat, PngDither, ScaleMode, SizeConstraint, SortOrder, TemplateFormat, TrimMode,
    VariantOptions, VARIANT_PRESETS,
};
use crate::error::{Result, TpError};
use crate::types::Point2D;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// The project configuration. Mirrors the spec's `ProjectConfig` plus a few
/// sensible extensions (packing strategy, variants, template format, pivots).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub input_directory: PathBuf,
    pub output_directory: PathBuf,
    /// Maximum texture size per axis (power of two, e.g. 2048, 4096).
    pub max_texture_size: i32,
    /// Pixels of transparent padding around every frame (anti-bleeding).
    pub padding: i32,
    /// Pixels of border-pixel extrusion around every frame (anti-bleeding).
    pub extrude: i32,
    /// Allow 90° rotation while packing.
    pub allow_rotation: bool,
    /// Trim fully-transparent borders before packing.
    pub enable_trim: bool,
    /// Alpha threshold (0-255) below which a pixel is considered transparent.
    pub trim_threshold: i32,
    /// Which trim mode to apply when `enable_trim` is set.
    #[serde(default)]
    pub trim_mode: TrimMode,
    /// Transparent margin kept around the trimmed bounding box, in pixels.
    #[serde(default)]
    pub trim_margin: i32,
    /// Enable polygon (mesh) extraction and polygon-aware packing.
    pub enable_polygon: bool,
    /// Ramer-Douglas-Peucker simplification tolerance, in pixels.
    pub polygon_tolerance: f32,
    /// Deduplicate identical sprites via pixel hashing (alias detection).
    pub enable_aliasing: bool,
    pub color_depth: ColorDepth,
    pub dithering_algorithm: DitheringAlgorithm,
    pub gpu_format: GpuFormat,
    /// Esfuerzo de optimización PNG sin pérdida, 0-7. El valor 1 (por
    /// defecto) escribe PNG indexado de 8 bits
    /// cuando la imagen tiene 256 colores o menos.
    #[serde(default = "default_png_opt_level")]
    pub png_opt_level: u8,
    /// Dithering de la paleta al publicar PNG-8
    /// (PngQuant Low/Medium/High).
    #[serde(default)]
    pub png8_dither: PngDither,
    /// Calidad JPEG 0-100 (`--jpg-quality`; 80 por defecto).
    #[serde(default = "default_jpg_quality")]
    pub jpg_quality: u8,
    /// Calidad WebP: 0-100 = lossy, ≥101 = sin pérdidas
    /// (`--webp-quality`; por defecto sin pérdidas).
    #[serde(default = "default_webp_quality")]
    pub webp_quality: u16,
    /// Formato de píxel de salida (pixel format): software embebible o
    /// compresión de hardware (PVRTC/ETC/DXT/ASTC), según la tabla de
    /// formatos. Solo formatos compatibles con la textura elegida.
    #[serde(default)]
    pub pixel_format: PixelFormat,
    /// Calidad PVRTC (`--pvr-quality`), 0-7, 3 por defecto: pasadas de
    /// refinamiento de los extremos de cada bloque.
    #[serde(default = "default_pvr_quality")]
    pub pvr_quality: u8,
    /// Calidad ETC1 (`--etc1-quality`), 0-100, 70 por defecto.
    #[serde(default = "default_etc1_quality")]
    pub etc1_quality: u8,
    /// Calidad ETC2 (`--etc2-quality`), 0-100, 70 por defecto.
    #[serde(default = "default_etc2_quality")]
    pub etc2_quality: u8,
    /// Calidad ASTC (`--astc-quality`), 0-4, 2 por defecto (preset de
    /// astcenc: fastest/fast/medium/thorough/exhaustive).
    #[serde(default = "default_astc_quality")]
    pub astc_quality: u8,
    /// Calidad Basis ETC1S (`--basis-quality`), 0-100, 50 por defecto
    /// (≈128, la calidad por defecto de Basis).
    #[serde(default = "default_basis_quality")]
    pub basis_quality: u8,
    /// Cuantización DXT1/DXT3/DXT5 (`--dxt-mode`).
    #[serde(default)]
    pub dxt_mode: DxtMode,
    /// Cache busting del data format (`--cache-busting`): añade `?v=<hash>`
    /// a las referencias de la textura en los metadatos, como los data
    /// formats de Pixi/Phaser.
    #[serde(default)]
    pub cache_busting: bool,
    /// Filtro de muestreo declarado en el data format de LibGDX
    /// (`--gdx-filter`).
    #[serde(default)]
    pub gdx_filter: GdxFilter,
    /// Dibuja el contorno de cada sprite sobre la hoja publicada
    /// (`--shape-debug`), para depurar reparto y polígonos.
    #[serde(default)]
    pub shape_debug: bool,
    /// Voltear la textura verticalmente (`--flip-y`); solo formatos
    /// de hardware (ASTC/ETC2/ETC1/PVRTC).
    #[serde(default)]
    pub flip_vertical: bool,
    /// Optional AES-256-GCM key; texture files are encrypted when set.
    #[serde(default)]
    pub encryption_key: Option<String>,
    /// Nombre de una clave del almacén global (`--key-name`): se usa solo
    /// cuando `encryption_key` está vacía.
    #[serde(default)]
    pub encryption_key_name: Option<String>,
    /// Path to a custom Mustache template; when `None` a built-in is used
    /// according to `template_format`.
    pub export_template: Option<PathBuf>,
    /// Metadata output language.
    pub template_format: TemplateFormat,
    /// Id del preset de formato de datos
    /// (`crate::dataformats::DATA_FORMATS`); vacío = sólo se usa la
    /// familia de `template_format` (los proyectos antiguos). Lo fija
    /// [`ProjectConfig::apply_data_format`] y decide la extensión del
    /// fichero de datos.
    #[serde(default)]
    pub data_format: String,
    /// Extra *class* file (Swift, `--class-file`, spritekit-swift). Empty
    /// disables it.
    #[serde(default)]
    pub class_file: String,
    /// Extra C++/ObjC *header* file (`--header-file`). Empty disables it.
    #[serde(default)]
    pub header_file: String,
    /// Extra C++ *source* file (`--source-file`). Empty disables it.
    #[serde(default)]
    pub source_file: String,
    /// Extra sprite id list (`--spriteids-file`, amethyst/Rust).
    #[serde(default)]
    pub spriteids_file: String,
    /// Packing algorithm / heuristic.
    pub packing_strategy: PackingStrategy,
    /// Packing algorithm family. `enable_polygon` takes
    /// precedence; a legacy `packing_strategy = "Guillotine"` also selects it.
    #[serde(default)]
    pub algorithm: PackingAlgorithm,
    /// Effort spent searching the minimum atlas size.
    #[serde(default)]
    pub pack_mode: PackMode,
    /// Required atlas dimensions.
    #[serde(default)]
    pub size_constraints: SizeConstraint,
    /// Force the atlas to be square.
    #[serde(default)]
    pub force_squared: bool,
    /// Fixed atlas width; `0` lets the packer decide.
    #[serde(default)]
    pub fixed_width: i32,
    /// Fixed atlas height; `0` lets the packer decide.
    #[serde(default)]
    pub fixed_height: i32,
    /// Sort criterion for the `Basic` algorithm.
    #[serde(default)]
    pub basic_sort_by: BasicSortBy,
    /// Sort direction for the `Basic` algorithm.
    #[serde(default)]
    pub basic_order: SortOrder,
    /// Manual algorithm: atlas position (x, y) fixed by hand per sprite id,
    /// in trimmed sprite coordinates (as shown in the GUI preview). Sprites
    /// without an entry fall back to the Basic row layout.
    #[serde(default)]
    pub manual_positions: HashMap<String, (i32, i32)>,
    /// Optional snap grid for the Manual algorithm (None = free dragging).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_grid: Option<ManualGrid>,
    /// Pack-by-folder groups: one sheet per group inside its own output
    /// subfolder. The first entry is always the default/main group (empty
    /// name, output root); it also receives every unassigned sprite.
    #[serde(default = "default_folder_groups")]
    pub folder_groups: Vec<FolderGroup>,
    /// Automatic pack-by-folder: every
    /// input subfolder becomes an output subfolder — sprites in `<in>/ui/`
    /// land in `<out>/ui/atlas.png`, root-level sprites in `<out>/atlas.png`.
    /// Overrides manual groups when enabled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_folder_groups: bool,
    /// Scale variants to emit, e.g. `[1.0, 0.5]` produces `atlas.png` and
    /// `atlas-hd.png` (sufijos de variante tipo `-hd`, `@2x`...).
    pub scale_variants: Vec<f32>,
    /// Named variants (scaling variants): `(scale, name)` pairs whose
    /// name replaces the automatic `{v}` suffix (p. ej. `1.0 → -ipadhd`,
    /// `0.5 → -hd`). Empty = automatic suffixes.
    #[serde(default)]
    pub variant_names: Vec<(f32, String)>,
    /// Per-variant options (sprite filter, max texture size, identical
    /// layout) keyed by scale. An entry whose scale is not listed in
    /// `scale_variants` is ignored (and rejected by `validate`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variant_options: Vec<VariantOptions>,
    /// Auto co-pack `*_normal.png` companions in the same frames.
    pub enable_normal_maps: bool,
    /// Suffix that marks a file as the normal map of a sprite: `hero` looks
    /// for `hero<normalsuffix>`. Empty = never match by name.
    #[serde(default = "default_normal_map_suffix")]
    pub normal_map_suffix: String,
    /// Substring a file's relative path must contain to count as a normal
    /// map (e.g. `normals/`). Empty = no path filter.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub normal_map_filter: String,
    /// Classify images as normal maps from their color when neither the
    /// suffix nor the filter matches (blue-dominant heuristic).
    #[serde(default)]
    pub normal_map_auto_detect: bool,
    /// Base file name of the normal-map sheet; empty = `<base_file_name>_normal`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub normal_map_sheet: String,
    /// Default normalized pivot for all sprites.
    pub default_pivot_x: f32,
    pub default_pivot_y: f32,
    /// Per-sprite pivot edited in the GUI (`sprite id → normalized pivot`).
    /// It wins over `pivots.json` and over `default_pivot_*`, and travels in
    /// the project so the published data matches what the preview shows.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub pivot_overrides: HashMap<String, Point2D>,
    /// Per-sprite 9-patch borders edited in the GUI (`[left, top, right,
    /// bottom]` in source pixels, `[0; 4]`/absent = no border). It wins over
    /// `borders.json`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub border_overrides: HashMap<String, [i32; 4]>,
    /// Per-channel tolerance (0-255) for automatic 9-patch border detection:
    /// two pixels count as "same color" when no channel differs more than this.
    #[serde(default)]
    pub detect_border_tolerance: i32,
    /// Max rows/columns inspected per side when auto-detecting 9-patch
    /// borders (0 = unlimited).
    #[serde(default = "default_detect_border_max_search")]
    pub detect_border_max_search: i32,
    /// Naming scheme for atlas pages: `atlas`, `atlas_1`, `atlas_2`, ...
    /// Placeholders `{n}` (índice desde 0), `{n1}` (desde 1) y `{v}`
    /// (sufijo de variante) se expanden al nombrar cada hoja y su data file.
    pub base_file_name: String,
    /// Allow emitting more than one sprite sheet when the sprites do not fit
    /// in a single texture (multipack). When `false`, such a pack
    /// fails with an error instead.
    #[serde(default = "default_true")]
    pub multipack: bool,
    /// Recurse into subdirectories of `input_directory`.
    pub recursive: bool,
    /// Extra sprite files or folders added on top of `input_directory`
    /// (the GUI's "Add sprites" / "Add smart folder" actions).
    #[serde(default)]
    pub extra_inputs: Vec<PathBuf>,
    /// Sprite files removed from the input set (the GUI's "Remove sprites"
    /// action). Matched against every discovered file path.
    #[serde(default)]
    pub excluded_inputs: Vec<PathBuf>,
    /// Space kept between the sprites and the border of the sprite sheet
    /// (border padding). Independent from `padding`.
    #[serde(default)]
    pub border_padding: i32,
    /// Extend sprite sizes (with transparency) to be divisible by this value
    /// (common divisor). `1` keeps sizes untouched.
    #[serde(default = "default_divisor")]
    pub common_divisor_x: i32,
    /// Same as [`Self::common_divisor_x`] for the vertical axis.
    #[serde(default = "default_divisor")]
    pub common_divisor_y: i32,
    /// Place the top-left corners of sprites on atlas coordinates divisible
    /// by this value (align to grid). `0` disables the option.
    #[serde(default)]
    pub align_to_grid: i32,
    /// How transparent pixels are handled before packing (alpha handling).
    #[serde(default)]
    pub alpha_handling: AlphaHandling,
    /// Resampling used when generating scale variants (scale mode).
    #[serde(default)]
    pub scale_mode: ScaleMode,
    /// Path prepended to the texture file name inside the metadata
    /// (texture path), e.g. `/assets`.
    #[serde(default)]
    pub texture_path: Option<String>,
    /// Remove image file extensions from the sprite names (trim sprite
    /// names). When `false` the names keep e.g. `.png`.
    #[serde(default = "default_true")]
    pub trim_sprite_names: bool,
    /// Prepend the smart folder's name to the sprite names of its files
    /// (prepend folder name).
    #[serde(default)]
    pub prepend_folder_name: bool,
    /// Group sprites sharing a base name plus numeric suffix into animations
    /// exposed in the metadata (auto-detect animations). Sprites like
    /// `walk_001.png`, `walk_002.png`, `walk_003.png` define `walk`.
    #[serde(default = "default_true")]
    pub enable_auto_detect_animations: bool,
    /// Maximum sheet width in pixels (`--max-width`). `0` falls back to
    /// [`Self::max_texture_size`], which caps both axes.
    #[serde(default)]
    pub max_width: i32,
    /// Maximum sheet height in pixels (`--max-height`). `0` falls back to
    /// [`Self::max_texture_size`].
    #[serde(default)]
    pub max_height: i32,
    /// Opaque colour filling the whole sheet under the sprites
    /// (`--background-color`). `None` keeps the sheet transparent.
    #[serde(default)]
    pub background_color: Option<[u8; 4]>,
    /// Wildcard patterns (`*`/`?`) of paths left out of the atlas
    /// (`--ignore-files`, repeatable).
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
    /// Name substitutions applied to every sprite id after ingest
    /// (`--replace "<regexp>=<text>"`, repeatable).
    #[serde(default)]
    pub name_replacements: Vec<(String, String)>,
    /// Resolution written to the PNG sheet (`--dpi`). `None` writes no
    /// `pHYs` chunk, keeping the encoder's default.
    #[serde(default)]
    pub dpi: Option<u32>,
    /// Turn the flat colour of opaque sprites into transparency
    /// (`--heuristic-mask`).
    #[serde(default)]
    pub heuristic_mask: bool,
    /// Write output files even when their bytes are unchanged
    /// (`--force-publish`); by default identical files are left untouched.
    #[serde(default)]
    pub force_publish: bool,
    /// Media query wrapping the CSS of variants above 1×
    /// (`--css-media-query-2x`). `None` leaves the rules unwrapped.
    #[serde(default)]
    pub css_media_query_2x: Option<String>,
    /// Prefix for every CSS class name (`--css-sprite-prefix`, e.g. `icon-`).
    #[serde(default)]
    pub css_sprite_prefix: Option<String>,
    /// Demo exporter property, available to templates as
    /// `exporterProperties.string_property` (`--plain-string-property`).
    #[serde(default)]
    pub plain_string_property: Option<String>,
    /// Same, as a boolean, for `exporterProperties.bool_property`
    /// (`--plain-bool-property`).
    #[serde(default)]
    pub plain_bool_property: Option<bool>,
    /// Extra directory of `<id>.hbs` data formats
    /// (`--custom-exporters-directory`) accepted by `--format` and
    /// `--template-format`.
    #[serde(default)]
    pub custom_exporters_directory: Option<PathBuf>,
}

fn default_detect_border_max_search() -> i32 {
    64
}

pub(super) fn default_true() -> bool {
    true
}

fn default_png_opt_level() -> u8 {
    1
}

fn default_jpg_quality() -> u8 {
    80
}

fn default_pvr_quality() -> u8 {
    3
}

fn default_etc1_quality() -> u8 {
    70
}

fn default_etc2_quality() -> u8 {
    70
}

fn default_astc_quality() -> u8 {
    2
}

fn default_basis_quality() -> u8 {
    50
}

fn default_webp_quality() -> u16 {
    101
}

fn default_divisor() -> i32 {
    1
}

fn default_folder_groups() -> Vec<FolderGroup> {
    vec![FolderGroup::default()]
}

fn default_normal_map_suffix() -> String {
    "_normal".to_string()
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            input_directory: PathBuf::new(),
            output_directory: PathBuf::new(),
            max_texture_size: 2048,
            padding: 2,
            extrude: 1,
            allow_rotation: true,
            enable_trim: true,
            trim_threshold: 1,
            trim_mode: TrimMode::default(),
            trim_margin: 0,
            enable_polygon: false,
            polygon_tolerance: 1.5,
            enable_aliasing: true,
            color_depth: ColorDepth::Rgba8888,
            dithering_algorithm: DitheringAlgorithm::FloydSteinberg,
            gpu_format: GpuFormat::Png,
            png_opt_level: 1,
            png8_dither: PngDither::default(),
            jpg_quality: 80,
            webp_quality: 101,
            pixel_format: PixelFormat::default(),
            pvr_quality: default_pvr_quality(),
            etc1_quality: default_etc1_quality(),
            etc2_quality: default_etc2_quality(),
            astc_quality: default_astc_quality(),
            basis_quality: default_basis_quality(),
            dxt_mode: DxtMode::default(),
            cache_busting: false,
            gdx_filter: GdxFilter::default(),
            shape_debug: false,
            flip_vertical: false,
            encryption_key: None,
            encryption_key_name: None,
            export_template: None,
            template_format: TemplateFormat::Json,
            data_format: String::new(),
            class_file: String::new(),
            header_file: String::new(),
            source_file: String::new(),
            spriteids_file: String::new(),
            packing_strategy: PackingStrategy::Bssf,
            algorithm: PackingAlgorithm::default(),
            pack_mode: PackMode::default(),
            size_constraints: SizeConstraint::default(),
            force_squared: false,
            fixed_width: 0,
            fixed_height: 0,
            basic_sort_by: BasicSortBy::default(),
            basic_order: SortOrder::default(),
            manual_positions: HashMap::new(),
            folder_groups: default_folder_groups(),
            scale_variants: vec![1.0],
            variant_names: Vec::new(),
            variant_options: Vec::new(),
            enable_normal_maps: true,
            normal_map_suffix: default_normal_map_suffix(),
            normal_map_filter: String::new(),
            normal_map_auto_detect: false,
            normal_map_sheet: String::new(),
            default_pivot_x: 0.5,
            default_pivot_y: 0.5,
            pivot_overrides: HashMap::new(),
            border_overrides: HashMap::new(),
            detect_border_tolerance: 0,
            detect_border_max_search: 64,
            base_file_name: "atlas".to_string(),
            multipack: true,
            recursive: true,
            extra_inputs: Vec::new(),
            excluded_inputs: Vec::new(),
            border_padding: 0,
            common_divisor_x: 1,
            common_divisor_y: 1,
            align_to_grid: 0,
            alpha_handling: AlphaHandling::default(),
            scale_mode: ScaleMode::default(),
            texture_path: None,
            trim_sprite_names: true,
            prepend_folder_name: false,
            enable_auto_detect_animations: true,
            max_width: 0,
            max_height: 0,
            background_color: None,
            ignore_patterns: Vec::new(),
            name_replacements: Vec::new(),
            dpi: None,
            heuristic_mask: false,
            force_publish: false,
            css_media_query_2x: None,
            css_sprite_prefix: None,
            plain_string_property: None,
            plain_bool_property: None,
            custom_exporters_directory: None,
            manual_grid: None,
            auto_folder_groups: false,
        }
    }
}

impl ProjectConfig {
    /// Trim mode actually applied by the pipeline: `enable_trim` acts as the
    /// master switch (docs treat *Trim* and *Trim mode* as one setting).
    pub fn effective_trim_mode(&self) -> TrimMode {
        if self.enable_trim {
            self.trim_mode
        } else {
            TrimMode::None
        }
    }

    /// Width cap handed to the packer: `--max-width` when set, otherwise the
    /// square cap of [`Self::max_texture_size`].
    pub fn effective_max_width(&self) -> i32 {
        if self.max_width > 0 {
            self.max_width
        } else {
            self.max_texture_size
        }
    }

    /// Height cap handed to the packer: `--max-height` when set, otherwise
    /// the square cap of [`Self::max_texture_size`].
    pub fn effective_max_height(&self) -> i32 {
        if self.max_height > 0 {
            self.max_height
        } else {
            self.max_texture_size
        }
    }

    /// Effective per-axis divisor: only *Common divisor* extends sprite
    /// sizes. *Align to grid* moves the sprites instead of resizing them
    /// (the packer snaps every frame origin), so it plays no part here.
    ///
    /// To that this adds the **common factor the scaling variants need**:
    /// the base sheet must be divisible by the denominator of every
    /// identical-layout scale, so rescaling it keeps integer sizes and
    /// coordinates (the factor is derived from the variant list).
    pub fn effective_divisors(&self) -> (i32, i32) {
        let v = self.variant_common_divisor();
        (
            lcm_capped(self.common_divisor_x.max(1), v, 2048),
            lcm_capped(self.common_divisor_y.max(1), v, 2048),
        )
    }

    /// Common factor (0 = none) the identical-layout variants demand from
    /// the base sheet: the LCM of their scale denominators, capped at the
    /// 2048 the `Common divisor` validation allows. Variants that accept
    /// fractional values, pack on their own (sprite filter / own texture
    /// cap) or have no representable denominator are left out of the common
    /// divisor calculation.
    pub fn variant_common_divisor(&self) -> i32 {
        let mut div = 1;
        for &scale in &self.scale_variants {
            if !self.variant_shares_base_sheet(scale) {
                continue;
            }
            if self
                .variant_options_for(scale)
                .is_some_and(|o| o.accept_fractional)
            {
                continue;
            }
            if let Some(d) = scale_denominator(scale) {
                div = lcm_capped(div, d, 2048);
            }
        }
        if div > 1 {
            div
        } else {
            0
        }
    }

    /// True when this scale reuses the base sheet scaled (no filter, no
    /// texture cap of its own, identical layout requested) — i.e. when its
    /// geometry comes from rounding the base sheet instead of being packed
    /// at its own scale. Mirrors `plan_variants` before the sprites are
    /// ingested.
    pub fn variant_shares_base_sheet(&self, scale: f32) -> bool {
        let Some(opts) = self.variant_options_for(scale) else {
            return true;
        };
        opts.force_identical_layout
            && opts.sprite_filter.trim().is_empty()
            && opts
                .max_texture_size
                .is_none_or(|m| m == self.max_texture_size)
    }

    /// Scales whose identical sheet needs fractional values: their
    /// denominator is not covered by the common divisor (no representable
    /// denominator, a cap or another variant opted out with
    /// `accept_fractional`). Their frames get rounded on export.
    pub fn variant_fractional_scales(&self) -> Vec<f32> {
        let div = self.variant_common_divisor();
        self.scale_variants
            .iter()
            .copied()
            .filter(|&scale| self.variant_shares_base_sheet(scale))
            .filter(|&scale| !scale_denominator(scale).is_some_and(|d| d > 0 && div % d == 0))
            .collect()
    }

    /// El preset de formato de datos seleccionado (`None` = el proyecto solo
    /// declara la familia de `template_format`, como antes de existir la
    /// lista de exportadores).
    pub fn data_format_preset(&self) -> Option<&'static crate::dataformats::DataFormatPreset> {
        if self.data_format.is_empty() {
            None
        } else {
            crate::dataformats::find_data_format(&self.data_format)
        }
    }

    /// Convierte el proyecto a otro formato de datos, igual que el botón
    /// «Data Format»: cambia la familia de plantilla, la
    /// extensión del fichero de datos y aplica los *valores recomendados* del
    /// preset. Devuelve `false` cuando el id no existe (no se toca nada).
    pub fn apply_data_format(&mut self, id: &str) -> bool {
        let Some(preset) = crate::dataformats::find_data_format(id) else {
            return false;
        };
        self.data_format = preset.id.to_string();
        self.template_format = preset.family;
        self.apply_data_format_defaults()
    }

    /// `--custom-exporters-directory` + un id propio: si existe
    /// `dir/{id}.hbs`, ese exportador pasa a ser la plantilla de salida y
    /// devuelve `true`. La familia y la extensión las sigue decidiendo
    /// `data_format`, donde el exportador propio sólo aporta el texto.
    ///
    /// **No escribe `data_format`**: `validate()` sólo acepta ids de la lista
    /// oficial, y el id propio no es un formato de datos, es una plantilla.
    pub fn select_custom_exporter(&mut self, id: &str) -> bool {
        let Some(dir) = self.custom_exporters_directory.clone() else {
            return false;
        };
        if id.is_empty() || id.contains(['/', '\\']) {
            return false;
        }
        let path = dir.join(format!("{id}.hbs"));
        if !path.is_file() {
            return false;
        }
        self.export_template = Some(path);
        true
    }

    /// Quita la plantilla propia, pero sólo si la que hay apunta al
    /// directorio de exportadores: una plantilla elegida aparte (`--template`
    /// o el botón «…» de la GUI) no se toca. Devuelve `true` si borró algo.
    pub fn clear_custom_exporter(&mut self) -> bool {
        let inside = match (
            self.custom_exporters_directory.as_deref(),
            self.export_template.as_deref(),
        ) {
            (Some(dir), Some(path)) => path.parent() == Some(dir),
            _ => false,
        };
        if inside {
            self.export_template = None;
        }
        inside
    }

    /// Aplica solo los valores recomendados del preset actual (la opción
    /// «Update to recommended values» del diálogo de conversión): rotación,
    /// algoritmo y auto-detección de animaciones.
    pub fn apply_data_format_defaults(&mut self) -> bool {
        let Some(preset) = self.data_format_preset() else {
            return false;
        };
        if let Some(v) = preset.allow_rotation {
            self.allow_rotation = v;
        }
        if let Some(a) = preset.algorithm {
            self.algorithm = a;
        }
        if let Some(v) = preset.auto_detect_animations {
            self.enable_auto_detect_animations = v;
        }
        true
    }

    /// Applies a [`VARIANT_PRESETS`] entry by name, overwriting the current
    /// scale list, suffixes and per-variant options. Returns `false` when
    /// the name is unknown.
    pub fn apply_variant_preset(&mut self, name: &str) -> bool {
        let Some(preset) = VARIANT_PRESETS.iter().find(|p| p.name == name) else {
            return false;
        };
        self.scale_variants = preset.variants.iter().map(|(s, _)| *s).collect();
        self.variant_names = preset
            .variants
            .iter()
            .map(|(s, n)| (*s, (*n).to_string()))
            .collect();
        self.variant_options = preset
            .variants
            .iter()
            .map(|(s, _)| VariantOptions {
                scale: *s,
                accept_fractional: preset.fractional.iter().any(|f| (f - s).abs() < 1e-6),
                ..VariantOptions::default()
            })
            .collect();
        true
    }

    /// Algorithm actually used by the packer. Selecting the *Polygon* trim
    /// mode switches the algorithm to *Polygon*
    /// automatically; the legacy `enable_polygon` / `--polygon` switches map
    /// to the same behavior, and a legacy `packing_strategy = "Guillotine"`
    /// still selects the Guillotine algorithm.
    pub fn effective_algorithm(&self) -> PackingAlgorithm {
        // Las mallas explícitas ganan a todo.
        if self.enable_polygon {
            return PackingAlgorithm::Polygon;
        }
        // Manual es una elección explícita del usuario: gana al «auto» del
        // trim mode Polygon (legacy), que solo aplica a los otros algoritmos.
        if self.algorithm == PackingAlgorithm::Manual {
            return PackingAlgorithm::Manual;
        }
        if self.effective_trim_mode() == TrimMode::Polygon {
            return PackingAlgorithm::Polygon;
        }
        if self.packing_strategy == PackingStrategy::Guillotine {
            PackingAlgorithm::Guillotine
        } else {
            self.algorithm
        }
    }

    /// Options configured for a scale variant (`None` = defaults: every
    /// sprite, the project's `max_texture_size`, identical layout).
    pub fn variant_options_for(&self, scale: f32) -> Option<&VariantOptions> {
        self.variant_options
            .iter()
            .find(|o| (o.scale - scale).abs() < 1e-6)
    }

    /// Heuristic actually used by MaxRects (the legacy `Guillotine` strategy
    /// value is consumed by [`Self::effective_algorithm`]).
    pub fn effective_strategy(&self) -> PackingStrategy {
        match self.packing_strategy {
            PackingStrategy::Guillotine => PackingStrategy::Bssf,
            s => s,
        }
    }

    /// Width alignment (in pixels) for `WordAligned`: every row must fill
    /// complete memory words of `color_depth`.
    pub fn word_align_mod(&self) -> i32 {
        match self.color_depth.bytes_per_pixel() {
            1 => 4,
            2 => 2,
            3 => 4,
            _ => 1,
        }
    }

    /// Validate the configuration, returning a human-readable error on failure.
    pub fn validate(&self) -> Result<()> {
        if self.max_texture_size <= 0 || (self.max_texture_size & (self.max_texture_size - 1)) != 0
        {
            return Err(TpError::Config(format!(
                "max_texture_size debe ser una potencia de dos positiva (se obtuvo {})",
                self.max_texture_size
            )));
        }
        if self.padding < 0 || self.extrude < 0 || self.border_padding < 0 {
            return Err(TpError::Config(
                "padding, border_padding y extrude no pueden ser negativos".to_string(),
            ));
        }
        if self.border_padding * 2 >= self.max_texture_size {
            return Err(TpError::Config(format!(
                "border_padding ({}) deja el atlas interior vacío en un atlas de {}",
                self.border_padding, self.max_texture_size
            )));
        }
        for (name, value) in [
            ("max_width", self.max_width),
            ("max_height", self.max_height),
        ] {
            if !(0..=16384).contains(&value) {
                return Err(TpError::Config(format!(
                    "{name} debe estar entre 0 (sin tope propio) y 16384 (se obtuvo {value})"
                )));
            }
            if value > 0
                && self.size_constraints == SizeConstraint::Pot
                && (value & (value - 1)) != 0
            {
                return Err(TpError::Config(format!(
                    "{name} debe ser una potencia de dos con size_constraints = POT \
                     (se obtuvo {value})"
                )));
            }
            if value > 0 && self.border_padding * 2 >= value {
                return Err(TpError::Config(format!(
                    "border_padding ({}) deja el atlas interior vacío con {name} = {}",
                    self.border_padding, value
                )));
            }
        }
        if let Some(dpi) = self.dpi {
            if !(1..=1_000_000).contains(&dpi) {
                return Err(TpError::Config(format!(
                    "dpi debe estar entre 1 y 1000000 (se obtuvo {dpi})"
                )));
            }
        }
        for (pattern, _) in &self.name_replacements {
            if let Err(e) = regex::Regex::new(pattern) {
                return Err(TpError::Config(format!(
                    "name_replacements: la expresión regular «{pattern}» no es válida: {e}"
                )));
            }
        }
        for (name, value) in [
            ("common_divisor_x", self.common_divisor_x),
            ("common_divisor_y", self.common_divisor_y),
        ] {
            if !(1..=2048).contains(&value) {
                return Err(TpError::Config(format!(
                    "{name} debe estar entre 1 y 2048 (se obtuvo {value})"
                )));
            }
        }
        if !(0..=2048).contains(&self.align_to_grid) {
            return Err(TpError::Config(format!(
                "align_to_grid debe estar entre 0 y 2048 (se obtuvo {})",
                self.align_to_grid
            )));
        }
        if self.png_opt_level > 7 {
            return Err(TpError::Config(format!(
                "png_opt_level debe estar entre 0 y 7 (se obtuvo {})",
                self.png_opt_level
            )));
        }
        if self.jpg_quality > 100 {
            return Err(TpError::Config(format!(
                "jpg_quality debe estar entre 0 y 100 (se obtuvo {})",
                self.jpg_quality
            )));
        }
        if self.pvr_quality > 7 {
            return Err(TpError::Config(format!(
                "pvr_quality debe estar entre 0 y 7 (se obtuvo {})",
                self.pvr_quality
            )));
        }
        for (name, value) in [
            ("etc1_quality", self.etc1_quality),
            ("etc2_quality", self.etc2_quality),
            ("basis_quality", self.basis_quality),
        ] {
            if value > 100 {
                return Err(TpError::Config(format!(
                    "{name} debe estar entre 0 y 100 (se obtuvo {value})"
                )));
            }
        }
        if self.astc_quality > 4 {
            return Err(TpError::Config(format!(
                "astc_quality debe estar entre 0 y 4 (se obtuvo {})",
                self.astc_quality
            )));
        }
        if !self.pixel_format.is_compatible_with(self.gpu_format) {
            return Err(TpError::Config(format!(
                "pixel_format {} no es compatible con el formato de textura {}",
                self.pixel_format.as_str(),
                self.gpu_format.as_str()
            )));
        }
        // Id de formato de datos conocido (vacío = solo cuenta la familia).
        if !self.data_format.is_empty()
            && crate::dataformats::find_data_format(&self.data_format).is_none()
        {
            return Err(TpError::Config(format!(
                "data_format desconocido: {:?}",
                self.data_format
            )));
        }
        let (div_x, div_y) = self.effective_divisors();
        if div_x > 2048 || div_y > 2048 {
            return Err(TpError::Config(format!(
                "El common divisor supera 2048 ({div_x}x{div_y})"
            )));
        }
        if let Some(path) = &self.texture_path {
            if path.trim().is_empty() {
                return Err(TpError::Config(
                    "texture_path no puede ser una cadena vacía".to_string(),
                ));
            }
        }
        // Umbral de transparencia admitido: 1 a 255.
        if !(1..=255).contains(&self.trim_threshold) {
            return Err(TpError::Config(
                "trim_threshold debe estar entre 1 y 255".to_string(),
            ));
        }
        if !(0..=256).contains(&self.trim_margin) {
            return Err(TpError::Config(
                "trim_margin debe estar entre 0 y 256".to_string(),
            ));
        }
        if self.polygon_tolerance < 0.0 {
            return Err(TpError::Config(
                "polygon_tolerance no puede ser negativa".to_string(),
            ));
        }
        // La hoja de normales es un nombre base, no una ruta.
        let sheet = self.normal_map_sheet.trim();
        if sheet.contains('/') || sheet.contains('\\') || sheet.contains("..") {
            return Err(TpError::Config(
                "normal_map_sheet debe ser un nombre de fichero, no una ruta".to_string(),
            ));
        }
        if self.scale_variants.is_empty() {
            return Err(TpError::Config(
                "scale_variants no puede estar vacío".to_string(),
            ));
        }
        // La escala puede ser >1 (p. ej. @2x Retina) hasta 8.
        for s in &self.scale_variants {
            if *s <= 0.0 || *s > 8.0 {
                return Err(TpError::Config(format!(
                    "scale_variants debe estar en (0, 8] (se obtuvo {s})"
                )));
            }
        }
        for (s, name) in &self.variant_names {
            if *s <= 0.0 || *s > 8.0 {
                return Err(TpError::Config(format!(
                    "variant_names debe estar en (0, 8] (se obtuvo {s})"
                )));
            }
            if name.contains('/') || name.contains('\\') || name.contains("..") {
                return Err(TpError::Config(format!(
                    "variant_names no admite rutas (se obtuvo {name:?})"
                )));
            }
        }
        for (i, opt) in self.variant_options.iter().enumerate() {
            if opt.scale <= 0.0 || opt.scale > 8.0 {
                return Err(TpError::Config(format!(
                    "variant_options: la escala debe estar en (0, 8] (se obtuvo {})",
                    opt.scale
                )));
            }
            if !self
                .scale_variants
                .iter()
                .any(|s| (s - opt.scale).abs() < 1e-6)
            {
                return Err(TpError::Config(format!(
                    "variant_options: la escala {} no está en scale_variants",
                    opt.scale
                )));
            }
            if self.variant_options[i + 1..]
                .iter()
                .any(|b| (b.scale - opt.scale).abs() < 1e-6)
            {
                return Err(TpError::Config(format!(
                    "variant_options: la escala {} aparece más de una vez",
                    opt.scale
                )));
            }
            if let Some(m) = opt.max_texture_size {
                if !(0..=16384).contains(&m) || (m != 0 && (m & (m - 1)) != 0) {
                    return Err(TpError::Config(format!(
                        "variant_options: max_texture_size de la escala {} debe ser 0 \
                         (usar el del proyecto) o una potencia de dos hasta 16384 \
                         (se obtuvo {m})",
                        opt.scale
                    )));
                }
            }
        }
        if self.encryption_key.as_deref() == Some("") {
            return Err(TpError::Config(
                "encryption_key no puede ser una cadena vacía".to_string(),
            ));
        }
        for (name, value) in [
            ("fixed_width", self.fixed_width),
            ("fixed_height", self.fixed_height),
        ] {
            if !(0..=8192).contains(&value) {
                return Err(TpError::Config(format!(
                    "{name} debe estar entre 0 (auto) y 8192 (se obtuvo {value})"
                )));
            }
        }
        if self.fixed_width > 0
            && self.fixed_height > 0
            && self.border_padding * 2 >= self.fixed_width.min(self.fixed_height)
        {
            return Err(TpError::Config(format!(
                "border_padding ({}) deja el interior vacío en un atlas fijo de {}x{}",
                self.border_padding, self.fixed_width, self.fixed_height
            )));
        }
        Ok(())
    }

    /// Serialize the config to a TOML string (`.tpproj` project file).
    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Parse a `.tpproj` TOML project file.
    ///
    /// Como `serde` aborta en el **primer** campo ausente, un proyecto
    /// incompleto daba errores de uno en uno («missing field input_directory»,
    /// corregir, volver a cargar, «missing field padding»...). Aquí se
    /// pre-analiza el TOML y se validan **todos** los campos obligatorios de
    /// una vez: un solo mensaje lista los que faltan.
    pub fn from_toml(text: &str) -> Result<Self> {
        let value: toml::Value = toml::from_str(text)?;
        if let Some(table) = value.as_table() {
            let missing: Vec<&str> = REQUIRED_TOML_FIELDS
                .iter()
                .copied()
                .filter(|f| !table.contains_key(*f))
                .collect();
            if !missing.is_empty() {
                return Err(TpError::Config(if missing.len() == 1 {
                    format!(
                        "falta un campo obligatorio en el proyecto TOML: `{}`",
                        missing[0]
                    )
                } else {
                    format!(
                        "faltan {} campos obligatorios en el proyecto TOML: {}",
                        missing.len(),
                        missing
                            .iter()
                            .map(|f| format!("`{f}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }));
            }
        }
        // Delegar en serde con la lista ya verificada: los errores restantes
        // son de tipo/valor con línea y columna del TOML.
        Ok(toml::from_str(text)?)
    }
}

/// Campos obligatorios de un `.tpproj`: todo lo que `ProjectConfig` serializa
/// sin `#[serde(default)]` ni `skip_serializing_if` y que no sea `Option`
/// (un `Option` ausente ya se deserializa como `None`, y `toml` ni siquiera
/// lo escribe cuando es `None`). Los campos con default quedan fuera a
/// propósito para que los proyectos antiguos sigan cargando.
///
/// El test `required_fields_list_is_honest` mantiene esta lista sincronizada
/// con el struct: si añades/quitas un campo sin default, el test falla y te
/// pide actualizarla (orden = orden de aparición en el struct).
const REQUIRED_TOML_FIELDS: &[&str] = &[
    "input_directory",
    "output_directory",
    "max_texture_size",
    "padding",
    "extrude",
    "allow_rotation",
    "enable_trim",
    "trim_threshold",
    "enable_polygon",
    "polygon_tolerance",
    "enable_aliasing",
    "color_depth",
    "dithering_algorithm",
    "gpu_format",
    "template_format",
    "packing_strategy",
    "scale_variants",
    "enable_normal_maps",
    "default_pivot_x",
    "default_pivot_y",
    "base_file_name",
    "recursive",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_format_conversion_swaps_family_extension_and_recommended_values() {
        let mut cfg = ProjectConfig::default();
        assert!(cfg.data_format_preset().is_none());
        assert!(cfg.data_format.is_empty());

        // Convertir a libGDX: familia, extensión y recomendados del preset.
        assert!(cfg.apply_data_format("libgdx"));
        assert_eq!(cfg.data_format, "libgdx");
        assert_eq!(cfg.template_format, TemplateFormat::LibgdxAtlas);
        assert_eq!(cfg.data_format_preset().map(|p| p.extension), Some("atlas"));
        assert!(cfg.enable_auto_detect_animations);

        // CSS recomienda no rotar y la familia cambia a la suya.
        cfg.allow_rotation = true;
        assert!(cfg.apply_data_format("css"));
        assert_eq!(cfg.template_format, TemplateFormat::Css);
        assert_eq!(cfg.data_format_preset().map(|p| p.extension), Some("css"));
        assert!(!cfg.allow_rotation);

        // Sólo recomendados: la familia no se mueve.
        cfg.template_format = TemplateFormat::Json;
        assert!(cfg.apply_data_format_defaults());
        assert_eq!(cfg.template_format, TemplateFormat::Json);
        assert_eq!(cfg.data_format, "css");

        // Desconocido: no toca nada.
        assert!(!cfg.apply_data_format("no-existe"));
        assert_eq!(cfg.data_format, "css");
    }

    #[test]
    fn missing_fields_are_reported_all_at_once() {
        // Proyecto mínimo con SOLO 3 campos: el error debe listar TODOS
        // los obligatorios ausentes de una vez (no el primero que serde
        // encuentre).
        let err = ProjectConfig::from_toml("input_directory = \"in\"\n").unwrap_err();
        let msg = err.to_string();
        for field in [
            "output_directory",
            "max_texture_size",
            "padding",
            "recursive",
        ] {
            assert!(
                msg.contains(field),
                "el error debería mencionar `{field}`: {msg}"
            );
        }
        // Un solo campo ausente → mensaje singular.
        let mut text = ProjectConfig::default().to_toml().unwrap();
        let line = text
            .lines()
            .find(|l| l.starts_with("recursive"))
            .map(str::to_string)
            .unwrap();
        text = text.replace(&line, "");
        let err = ProjectConfig::from_toml(&text).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("falta un campo obligatorio"),
            "mensaje singular esperado: {msg}"
        );
        // Un TOML que no es una tabla (p. ej. vacío) tampoco rompe: sin
        // campos no hay lista que verificar y serde da su error normal.
        assert!(ProjectConfig::from_toml("").is_err());
    }

    #[test]
    fn pivot_and_border_overrides_survive_toml_roundtrip() {
        let mut cfg = ProjectConfig::default();
        cfg.pivot_overrides
            .insert("sub/hero".into(), Point2D::new(0.25, 0.75));
        cfg.border_overrides.insert("sub/hero".into(), [1, 2, 3, 4]);

        let text = cfg.to_toml().unwrap();
        let back = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(
            back.pivot_overrides.get("sub/hero"),
            Some(&Point2D::new(0.25, 0.75))
        );
        assert_eq!(back.border_overrides.get("sub/hero"), Some(&[1, 2, 3, 4]));

        // Sin ediciones no ensucian el TOML ni son campos obligatorios.
        let clean = ProjectConfig::default().to_toml().unwrap();
        assert!(!clean.contains("pivot_overrides"));
        assert!(!clean.contains("border_overrides"));
        assert!(ProjectConfig::from_toml(&clean)
            .unwrap()
            .pivot_overrides
            .is_empty());
    }

    #[test]
    fn fase_c_exporter_fields_survive_toml_roundtrip() {
        let cfg = ProjectConfig {
            css_sprite_prefix: Some("icon-".into()),
            css_media_query_2x: Some("(-webkit-min-device-pixel-ratio: 2)".into()),
            plain_string_property: Some("hola".into()),
            plain_bool_property: Some(false),
            custom_exporters_directory: Some(PathBuf::from("mis-exportadores")),
            ..ProjectConfig::default()
        };

        let back = ProjectConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert_eq!(back.css_sprite_prefix.as_deref(), Some("icon-"));
        assert_eq!(
            back.css_media_query_2x.as_deref(),
            Some("(-webkit-min-device-pixel-ratio: 2)")
        );
        assert_eq!(back.plain_string_property.as_deref(), Some("hola"));
        assert_eq!(back.plain_bool_property, Some(false));
        assert_eq!(
            back.custom_exporters_directory,
            Some(PathBuf::from("mis-exportadores"))
        );
        back.validate().unwrap();

        // Sin ellas el TOML no ensucia y los proyectos antiguos siguen
        // cargando (todas llevan #[serde(default)]).
        let clean = ProjectConfig::default().to_toml().unwrap();
        assert!(!clean.contains("css_sprite_prefix"));
        assert!(!clean.contains("plain_string_property"));
        assert!(!clean.contains("custom_exporters_directory"));
        let plain = ProjectConfig::from_toml(&clean).unwrap();
        assert!(plain.css_media_query_2x.is_none());
        assert!(plain.plain_bool_property.is_none());
        assert!(plain.custom_exporters_directory.is_none());
    }

    #[test]
    fn custom_exporter_is_selected_from_the_directory_and_cleared_only_there() {
        let dir = std::env::temp_dir().join(format!("tp_config_exporters_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mi.hbs"), "{{this.filename}}").unwrap();

        let mut cfg = ProjectConfig::default();
        // Sin directorio no hay exportador propio que seleccionar.
        assert!(!cfg.select_custom_exporter("mi"));
        assert!(cfg.export_template.is_none());

        cfg.custom_exporters_directory = Some(dir.clone());
        assert!(cfg.select_custom_exporter("mi"));
        assert_eq!(
            cfg.export_template.as_deref(),
            Some(dir.join("mi.hbs").as_path())
        );
        // data_format no se toca: el id propio es una plantilla, no un
        // formato de datos, y validate() sólo admite los oficiales.
        assert_eq!(cfg.data_format, ProjectConfig::default().data_format);
        cfg.validate().unwrap();

        assert!(!cfg.select_custom_exporter("no-existe"));
        assert!(!cfg.select_custom_exporter(""));
        assert!(!cfg.select_custom_exporter("../fuera"));
        assert_eq!(
            cfg.export_template.as_deref(),
            Some(dir.join("mi.hbs").as_path()),
            "los ids inválidos no cambian la plantilla activa"
        );

        assert!(cfg.clear_custom_exporter());
        assert!(cfg.export_template.is_none());
        assert!(!cfg.clear_custom_exporter(), "ya no hay nada que quitar");

        cfg.export_template = Some(PathBuf::from("plantilla-ajena.hbs"));
        assert!(!cfg.clear_custom_exporter(), "la de fuera no se borra");
        assert!(cfg.export_template.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn required_fields_list_is_honest() {
        // La lista REQUIRED_TOML_FIELDS debe describir la realidad de
        // serde: cada campo listado DEBE ser exigido por serde (quitarlo
        // del TOML rompe la carga), y nada fuera de la lista puede ser
        // obligatorio (proyectos antiguos deben seguir cargando).
        let cfg = ProjectConfig::default();
        let full = cfg.to_toml().unwrap();
        for field in REQUIRED_TOML_FIELDS {
            let line = full
                .lines()
                .find(|l| l.starts_with(*field))
                .unwrap_or_else(|| panic!("el campo {field} no aparece en el TOML generado"));
            let stripped = full.replace(line, "");
            let err = match ProjectConfig::from_toml(&stripped) {
                Ok(_) => panic!(
                    "{field} está en REQUIRED_TOML_FIELDS pero serde no lo exige (tiene default o es Option): sácalo de la lista"
                ),
                Err(e) => e.to_string(),
            };
            assert!(
                err.contains(field),
                "al quitar `{field}` el error no lo menciona: {err}"
            );
        }
    }

    #[test]
    fn roundtrip_toml() {
        let cfg = ProjectConfig::default();
        let text = cfg.to_toml().unwrap();
        let back = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(back.max_texture_size, cfg.max_texture_size);
        assert_eq!(back.packing_strategy, cfg.packing_strategy);
        assert_eq!(back.encryption_key, cfg.encryption_key);
    }

    #[test]
    fn validation() {
        let too_small = ProjectConfig {
            max_texture_size: 1000,
            ..ProjectConfig::default()
        };
        assert!(too_small.validate().is_err());

        let empty_variants = ProjectConfig {
            scale_variants: vec![],
            ..ProjectConfig::default()
        };
        assert!(empty_variants.validate().is_err());

        let bad_png_opt = ProjectConfig {
            png_opt_level: 8,
            ..ProjectConfig::default()
        };
        assert!(bad_png_opt.validate().is_err());

        let bad_jpg = ProjectConfig {
            jpg_quality: 101,
            ..ProjectConfig::default()
        };
        assert!(bad_jpg.validate().is_err());

        let bad_basis = ProjectConfig {
            basis_quality: 101,
            ..ProjectConfig::default()
        };
        assert!(bad_basis.validate().is_err());

        let ok = ProjectConfig {
            png_opt_level: 7,
            jpg_quality: 100,
            basis_quality: 100,
            ..ProjectConfig::default()
        };
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn roundtrip_keeps_new_settings() {
        let cfg = ProjectConfig {
            border_padding: 8,
            common_divisor_x: 4,
            common_divisor_y: 2,
            align_to_grid: 8,
            alpha_handling: AlphaHandling::PremultiplyAlpha,
            scale_mode: ScaleMode::Fast,
            texture_path: Some("/assets".into()),
            trim_sprite_names: false,
            prepend_folder_name: true,
            multipack: false,
            png_opt_level: 4,
            png8_dither: PngDither::Low,
            jpg_quality: 90,
            webp_quality: 75,
            pixel_format: PixelFormat::Rgb888,
            flip_vertical: true,
            ..ProjectConfig::default()
        };

        let back = ProjectConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert_eq!(back.border_padding, 8);
        assert_eq!(back.common_divisor_x, 4);
        assert_eq!(back.common_divisor_y, 2);
        assert_eq!(back.align_to_grid, 8);
        assert_eq!(back.alpha_handling, AlphaHandling::PremultiplyAlpha);
        assert_eq!(back.scale_mode, ScaleMode::Fast);
        assert_eq!(back.texture_path.as_deref(), Some("/assets"));
        assert!(!back.trim_sprite_names);
        assert!(back.prepend_folder_name);
        assert!(!back.multipack);
        assert_eq!(back.png_opt_level, 4);
        assert_eq!(back.png8_dither, PngDither::Low);
        assert_eq!(back.jpg_quality, 90);
        assert_eq!(back.webp_quality, 75);
        assert_eq!(back.pixel_format, PixelFormat::Rgb888);
        assert!(back.flip_vertical);
    }

    #[test]
    fn legacy_project_defaults_for_new_settings() {
        // A `.tpproj` written before the Lote 5 fields must keep working.
        let cfg = ProjectConfig::default();
        let mut text = cfg.to_toml().unwrap();
        for field in [
            "border_padding",
            "common_divisor_x",
            "common_divisor_y",
            "align_to_grid",
            "alpha_handling",
            "scale_mode",
            "texture_path",
            "trim_sprite_names",
            "prepend_folder_name",
            "algorithm",
            "pack_mode",
            "size_constraints",
            "force_squared",
            "fixed_width",
            "fixed_height",
            "basic_sort_by",
            "basic_order",
            "multipack",
            "png_opt_level",
            "png8_dither",
            "jpg_quality",
            "webp_quality",
            "pixel_format",
            "flip_vertical",
            "basis_quality",
        ] {
            if let Some(line) = text
                .lines()
                .find(|l| l.starts_with(field))
                .map(str::to_string)
            {
                text = text.replace(&line, "");
            }
        }
        assert!(!text.contains("border_padding"));
        let back = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(back.border_padding, 0);
        assert_eq!(back.common_divisor_x, 1);
        assert_eq!(back.common_divisor_y, 1);
        assert_eq!(back.align_to_grid, 0);
        assert_eq!(back.alpha_handling, AlphaHandling::KeepTransparentPixels);
        assert_eq!(back.scale_mode, ScaleMode::Smooth);
        assert_eq!(back.texture_path, None);
        assert!(back.trim_sprite_names);
        assert!(!back.prepend_folder_name);
        assert_eq!(back.algorithm, PackingAlgorithm::MaxRects);
        assert_eq!(back.pack_mode, PackMode::Good);
        assert_eq!(back.size_constraints, SizeConstraint::AnySize);
        assert!(!back.force_squared);
        assert_eq!(back.fixed_width, 0);
        assert_eq!(back.fixed_height, 0);
        assert_eq!(back.basic_sort_by, BasicSortBy::Best);
        assert_eq!(back.basic_order, SortOrder::Ascending);
        assert!(back.multipack);
        assert_eq!(back.png_opt_level, 1);
        assert_eq!(back.png8_dither, PngDither::High);
        assert_eq!(back.jpg_quality, 80);
        assert_eq!(back.webp_quality, 101);
        assert_eq!(back.pixel_format, PixelFormat::Rgba8888);
        assert!(!back.flip_vertical);
        assert_eq!(back.basis_quality, 50, "calidad ETC1S por defecto");
        assert!(back.validate().is_ok());
    }

    #[test]
    fn normal_map_settings_roundtrip_and_validate() {
        let cfg = ProjectConfig {
            enable_normal_maps: true,
            normal_map_suffix: "_n".into(),
            normal_map_filter: "normals/".into(),
            normal_map_auto_detect: true,
            normal_map_sheet: "mynorms".into(),
            ..ProjectConfig::default()
        };
        let back = ProjectConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert_eq!(back.normal_map_suffix, "_n");
        assert_eq!(back.normal_map_filter, "normals/");
        assert!(back.normal_map_auto_detect);
        assert_eq!(back.normal_map_sheet, "mynorms");
        assert!(back.validate().is_ok());

        // Los campos nuevos no son obligatorios en un `.tpproj` antiguo.
        let mut text = ProjectConfig::default().to_toml().unwrap();
        for field in [
            "normal_map_suffix",
            "normal_map_filter",
            "normal_map_auto_detect",
            "normal_map_sheet",
        ] {
            if let Some(line) = text
                .lines()
                .find(|l| l.starts_with(field))
                .map(str::to_string)
            {
                text = text.replace(&line, "");
            }
        }
        assert!(!text.contains("normal_map_"));
        let legacy = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(legacy.normal_map_suffix, "_normal");
        assert_eq!(legacy.normal_map_filter, "");
        assert!(!legacy.normal_map_auto_detect);
        assert_eq!(legacy.normal_map_sheet, "");

        // La hoja de normales es un nombre de fichero, no una ruta.
        for sheet in ["out/norms", "out\\norms", "../norms"] {
            let bad = ProjectConfig {
                normal_map_sheet: sheet.into(),
                ..ProjectConfig::default()
            };
            let err = bad
                .validate()
                .expect_err("normal_map_sheet debería rechazar rutas");
            assert!(
                err.to_string().contains("normal_map_sheet"),
                "mensaje raro: {err}"
            );
        }
    }

    #[test]
    fn data_format_extras_roundtrip_and_default_off() {
        let cfg = ProjectConfig {
            cache_busting: true,
            gdx_filter: GdxFilter::Nearest,
            shape_debug: true,
            ..ProjectConfig::default()
        };
        let back = ProjectConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert!(back.cache_busting);
        assert_eq!(back.gdx_filter, GdxFilter::Nearest);
        assert!(back.shape_debug);
        assert!(back.validate().is_ok());

        // Un `.tpproj` anterior a estos campos sigue cargando: apagados.
        let mut text = ProjectConfig::default().to_toml().unwrap();
        for field in ["cache_busting", "gdx_filter", "shape_debug"] {
            if let Some(line) = text
                .lines()
                .find(|l| l.starts_with(field))
                .map(str::to_string)
            {
                text = text.replace(&line, "");
            }
        }
        let legacy = ProjectConfig::from_toml(&text).unwrap();
        assert!(!legacy.cache_busting);
        assert!(!legacy.shape_debug);
        assert_eq!(legacy.gdx_filter, GdxFilter::Linear);
        assert!(legacy.validate().is_ok());
    }

    #[test]
    fn global_key_name_roundtrip_and_legacy_default() {
        let cfg = ProjectConfig {
            encryption_key_name: Some("proyecto".into()),
            ..ProjectConfig::default()
        };
        let back = ProjectConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert_eq!(back.encryption_key_name.as_deref(), Some("proyecto"));
        assert!(back.validate().is_ok());

        // Un `.tpproj` anterior a este campo sigue cargando: sin clave global.
        let mut text = ProjectConfig::default().to_toml().unwrap();
        if let Some(line) = text
            .lines()
            .find(|l| l.starts_with("encryption_key_name"))
            .map(str::to_string)
        {
            text = text.replace(&line, "");
        }
        assert!(!text.contains("encryption_key_name"));
        let legacy = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(legacy.encryption_key_name, None);
        assert!(legacy.validate().is_ok());
    }

    #[test]
    fn variants_extend_the_common_divisor_unless_they_opt_out() {
        let mut cfg = ProjectConfig {
            scale_variants: vec![1.0, 0.5, 0.25],
            ..ProjectConfig::default()
        };
        // 0.5 → 2 y 0.25 → 4: el común divisor del proyecto pasa a 4 en los
        // dos ejes, que es lo que hace entera la hoja idéntica.
        assert_eq!(cfg.variant_common_divisor(), 4);
        assert_eq!(cfg.effective_divisors(), (4, 4));

        // «Accept fractional values» la saca del cómputo: vuelve a 2 y esa
        // es la única escala que queda con valores fraccionarios.
        cfg.variant_options = vec![VariantOptions {
            scale: 0.25,
            accept_fractional: true,
            ..VariantOptions::default()
        }];
        assert_eq!(cfg.variant_common_divisor(), 2);
        assert_eq!(cfg.effective_divisors(), (2, 2));
        assert_eq!(cfg.variant_fractional_scales(), vec![0.25]);

        // Un filtro la empaqueta sola: tampoco cuenta, y no redondea nada.
        cfg.variant_options = vec![VariantOptions {
            scale: 0.25,
            sprite_filter: "hero*".into(),
            ..VariantOptions::default()
        }];
        assert_eq!(cfg.variant_common_divisor(), 2);
        assert!(cfg.variant_fractional_scales().is_empty());

        // El common divisor explícito del proyecto sigue mandando si es mayor.
        cfg.common_divisor_x = 8;
        assert_eq!(cfg.effective_divisors(), (8, 2));

        // Escala sin denominador representable: no estira nada por su culpa.
        cfg.scale_variants = vec![1.0, 0.999];
        cfg.variant_options.clear();
        cfg.common_divisor_x = 1;
        assert_eq!(cfg.variant_common_divisor(), 0);
        assert_eq!(cfg.variant_fractional_scales(), vec![0.999]);
    }

    #[test]
    fn variant_presets_overwrite_the_variant_list() {
        let mut cfg = ProjectConfig::default();
        assert!(cfg.apply_variant_preset("iPad + iPhone (documentación)"));
        assert_eq!(cfg.scale_variants, vec![1.0, 0.5, 0.25]);
        assert_eq!(cfg.variant_names[0], (1.0, "-ipadhd".to_string()));
        assert_eq!(cfg.variant_names[2], (0.25, String::new()));
        assert!(cfg.validate().is_ok());

        // El preset fraccionario marca la variante 1/3 como «accept
        // fractional values» y las demás no.
        assert!(cfg.apply_variant_preset("Descuentos 1/2, 1/3 y 1/4"));
        let third = 1.0 / 3.0;
        assert!(cfg.variant_options_for(third).unwrap().accept_fractional);
        assert!(!cfg.variant_options_for(0.5).unwrap().accept_fractional);
        assert!(cfg.validate().is_ok());

        assert!(!cfg.apply_variant_preset("no existe"));

        // Preset de una sola variante y todos los presets validan.
        assert!(cfg.apply_variant_preset("Ninguna"));
        assert_eq!(cfg.scale_variants, vec![1.0]);
        assert!(cfg.validate().is_ok());
        for preset in VARIANT_PRESETS {
            let mut c = ProjectConfig::default();
            assert!(c.apply_variant_preset(preset.name), "{}", preset.name);
            assert!(c.validate().is_ok(), "{}", preset.name);
        }
    }

    #[test]
    fn accept_fractional_roundtrips_and_defaults_off() {
        let cfg = ProjectConfig {
            scale_variants: vec![1.0, 0.5],
            variant_options: vec![VariantOptions {
                scale: 0.5,
                accept_fractional: true,
                ..VariantOptions::default()
            }],
            ..ProjectConfig::default()
        };
        let back = ProjectConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert!(back.variant_options_for(0.5).unwrap().accept_fractional);
        assert!(back.validate().is_ok());

        // Un `.tpproj` anterior al campo sigue cargando: apagado.
        let mut text = ProjectConfig::default().to_toml().unwrap();
        for line in text
            .clone()
            .lines()
            .filter(|l| l.starts_with("accept_fractional"))
        {
            text = text.replace(line, "");
        }
        let legacy = ProjectConfig::from_toml(&text).unwrap();
        assert!(legacy.variant_options.iter().all(|o| !o.accept_fractional));
    }

    #[test]
    fn extra_data_file_settings_roundtrip_and_default_empty() {
        let cfg = ProjectConfig {
            class_file: "Sprites.swift".into(),
            header_file: "Sprites.h".into(),
            source_file: "Sprites.cpp".into(),
            spriteids_file: "spriteids.txt".into(),
            ..ProjectConfig::default()
        };
        let back = ProjectConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert_eq!(back.class_file, "Sprites.swift");
        assert_eq!(back.header_file, "Sprites.h");
        assert_eq!(back.source_file, "Sprites.cpp");
        assert_eq!(back.spriteids_file, "spriteids.txt");
        assert!(back.validate().is_ok());

        // Un `.tpproj` anterior a estos campos sigue cargando: vacío = no escribir.
        let mut text = ProjectConfig::default().to_toml().unwrap();
        for field in ["class_file", "header_file", "source_file", "spriteids_file"] {
            if let Some(line) = text
                .lines()
                .find(|l| l.starts_with(field))
                .map(str::to_string)
            {
                text = text.replace(&line, "");
            }
        }
        assert!(!text.contains("spriteids_file"));
        let legacy = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(legacy.class_file, "");
        assert_eq!(legacy.header_file, "");
        assert_eq!(legacy.source_file, "");
        assert_eq!(legacy.spriteids_file, "");
        assert!(legacy.validate().is_ok());
    }

    #[test]
    fn fixed_size_validation() {
        let mut cfg = ProjectConfig {
            fixed_width: 9999,
            ..ProjectConfig::default()
        };
        assert!(cfg.validate().is_err());
        cfg.fixed_width = 0;
        cfg.fixed_width = 256;
        cfg.fixed_height = 64;
        cfg.border_padding = 128;
        assert!(cfg.validate().is_err());
        cfg.border_padding = 8;
        assert!(cfg.validate().is_ok());
        assert_eq!(cfg.word_align_mod(), 1);
        cfg.color_depth = ColorDepth::Rgb565;
        assert_eq!(cfg.word_align_mod(), 2);
    }

    #[test]
    fn effective_divisors_ignore_align_to_grid() {
        let mut cfg = ProjectConfig::default();
        assert_eq!(cfg.effective_divisors(), (1, 1));
        cfg.common_divisor_x = 4;
        assert_eq!(cfg.effective_divisors(), (4, 1));
        // Alinear a rejilla no estira sprites: mueve el packer.
        cfg.align_to_grid = 6;
        assert_eq!(cfg.effective_divisors(), (4, 1));
        assert!(cfg.validate().is_ok());
        cfg.common_divisor_x = 0;
        assert!(cfg.validate().is_err());
        cfg.common_divisor_x = 4;
        cfg.border_padding = -1;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn texture_path_and_alpha_parsing() {
        let blank_texture_path = ProjectConfig {
            texture_path: Some("  ".into()),
            ..ProjectConfig::default()
        };
        assert!(blank_texture_path.validate().is_err());
        assert_eq!(
            AlphaHandling::parse("premultiply-alpha"),
            Some(AlphaHandling::PremultiplyAlpha)
        );
        assert_eq!(
            AlphaHandling::parse("reduce_border_artifacts"),
            Some(AlphaHandling::ReduceBorderArtifacts)
        );
        assert_eq!(AlphaHandling::parse("nope"), None);
        assert_eq!(ScaleMode::parse("fast"), Some(ScaleMode::Fast));
        assert_eq!(ScaleMode::parse("scale2x"), Some(ScaleMode::Scale2x));
        assert_eq!(ScaleMode::parse("scale3x"), Some(ScaleMode::Scale3x));
        assert_eq!(ScaleMode::parse("scale4x"), Some(ScaleMode::Scale4x));
        assert_eq!(ScaleMode::parse("eagle"), Some(ScaleMode::Eagle));
        assert_eq!(ScaleMode::parse("nope"), None);
        // Los modos de pixel art exigen su factor entero; los genéricos, ninguno.
        assert_eq!(ScaleMode::Scale2x.required_factor(), Some(2));
        assert_eq!(ScaleMode::Scale3x.required_factor(), Some(3));
        assert_eq!(ScaleMode::Scale4x.required_factor(), Some(4));
        assert_eq!(ScaleMode::Eagle.required_factor(), Some(2));
        assert_eq!(ScaleMode::Smooth.required_factor(), None);
        assert_eq!(ScaleMode::Fast.required_factor(), None);
    }

    #[test]
    fn variant_options_validate_scale_duplicates_and_max_size() {
        let base = || ProjectConfig {
            scale_variants: vec![1.0, 0.5],
            ..ProjectConfig::default()
        };

        let mut cfg = base();
        cfg.variant_options = vec![VariantOptions {
            scale: 0.75,
            ..VariantOptions::default()
        }];
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("no está en scale_variants"), "{err}");

        let mut cfg = base();
        cfg.variant_options = vec![
            VariantOptions {
                scale: 0.5,
                ..VariantOptions::default()
            },
            VariantOptions {
                scale: 0.5,
                sprite_filter: "x".into(),
                ..VariantOptions::default()
            },
        ];
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("más de una vez"), "{err}");

        let mut cfg = base();
        cfg.variant_options = vec![VariantOptions {
            scale: 0.5,
            max_texture_size: Some(1000),
            ..VariantOptions::default()
        }];
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("potencia de dos"), "{err}");

        let mut cfg = base();
        cfg.variant_options = vec![VariantOptions {
            scale: 0.5,
            max_texture_size: Some(1024),
            ..VariantOptions::default()
        }];
        cfg.validate().expect("opciones válidas");

        // Serialización: solo aparecen si hay opciones y sobreviven al TOML.
        let text = cfg.to_toml().unwrap();
        assert!(text.contains("variant_options"));
        let back = ProjectConfig::from_toml(&text).unwrap();
        assert_eq!(back.variant_options, cfg.variant_options);
        assert!(!base().to_toml().unwrap().contains("variant_options"));
    }
}
