//! Presets de formato de datos («data formats») del original.
//!
//! TexturePacker llama *data format* al exportador que decide el layout del
//! fichero de metadatos y una parte de los ajustes por defecto (`--format
//! <name>`; `TexturePacker --exporter-list` los imprime uno por línea). Este
//! módulo replica esa lista completa: cada entrada fija la **familia** de
//! plantilla que lo dibuja ([`TemplateFormat`]), la extensión del fichero de
//! datos y los *valores recomendados* que la conversión de proyecto aplica
//! (rotación, algoritmo y auto-detección de animaciones).
//!
//! Las dieciséis familias tienen plantilla propia en
//! [`crate::templates::builtin_template`]. Los exportadores de motores
//! minoritarios —muchos ya retirados del original— se emiten con la
//! estructura de la familia genérica `JsonHash` y la extensión por defecto;
//! esa desviación está anotada en el informe comparativo.

use crate::config::{PackingAlgorithm, TemplateFormat};

/// Agrupaciones usadas por el combo de la GUI (y por los mensajes de error),
/// en el orden en que se muestran.
pub const CATEGORIES: &[&str] = &[
    "Genéricos",
    "Atlas de texto",
    "JSON de motores",
    "plist y XML",
    "Motores minoritarios",
    "Extras del propio clon",
];

/// Un exportador de datos: id original, etiqueta, familia de plantilla,
/// extensión del fichero de datos y ajustes recomendados.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataFormatPreset {
    /// Id tal y como lo acepta `--format`/`--template-format` del original.
    pub id: &'static str,
    /// Nombre mostrado en la GUI (español).
    pub label: &'static str,
    /// Familia de plantilla que genera la salida.
    pub family: TemplateFormat,
    /// Extensión del data file; vacía = no se escribe fichero de datos.
    pub extension: &'static str,
    /// Agrupación en el combo (una de [`CATEGORIES`]).
    pub category: &'static str,
    /// Valor recomendado de rotación (`None` = no tocar).
    pub allow_rotation: Option<bool>,
    /// Algoritmo recomendado (`None` = no tocar).
    pub algorithm: Option<PackingAlgorithm>,
    /// Auto-detección de animaciones recomendada (`None` = no tocar).
    pub auto_detect_animations: Option<bool>,
}

impl DataFormatPreset {
    /// Entrada sin valores recomendados.
    const fn plain(
        id: &'static str,
        label: &'static str,
        family: TemplateFormat,
        extension: &'static str,
        category: &'static str,
    ) -> Self {
        Self {
            id,
            label,
            family,
            extension,
            category,
            allow_rotation: None,
            algorithm: None,
            auto_detect_animations: None,
        }
    }

    const fn rot(mut self, allow_rotation: bool) -> Self {
        self.allow_rotation = Some(allow_rotation);
        self
    }

    const fn alg(mut self, algorithm: PackingAlgorithm) -> Self {
        self.algorithm = Some(algorithm);
        self
    }

    const fn anim(mut self, auto_detect_animations: bool) -> Self {
        self.auto_detect_animations = Some(auto_detect_animations);
        self
    }
}

/// Los exportadores que el `--help`/`--exporter-list` del original enumera,
/// agrupados por familia de salida.
pub const DATA_FORMATS: &[DataFormatPreset] = &[
    // --- Genéricos ---------------------------------------------------------
    DataFormatPreset::plain(
        "json",
        "JSON (hash)",
        TemplateFormat::JsonHash,
        "json",
        "Genéricos",
    ),
    DataFormatPreset::plain(
        "json-array",
        "JSON (array)",
        TemplateFormat::Json,
        "json",
        "Genéricos",
    ),
    DataFormatPreset::plain(
        "xml",
        "XML genérico",
        TemplateFormat::Xml,
        "xml",
        "Genéricos",
    ),
    DataFormatPreset::plain(
        "plain",
        "Texto plano (exportador de ejemplo)",
        TemplateFormat::PlainText,
        "txt",
        "Genéricos",
    ),
    DataFormatPreset::plain(
        "spritesheet-only",
        "Solo la hoja, sin fichero de datos",
        TemplateFormat::SpriteSheetOnly,
        "",
        "Genéricos",
    ),
    // --- Atlas de texto ----------------------------------------------------
    DataFormatPreset::plain(
        "libgdx",
        "libGDX TextureAtlas",
        TemplateFormat::LibgdxAtlas,
        "atlas",
        "Atlas de texto",
    )
    .anim(true),
    DataFormatPreset::plain(
        "spine",
        "Spine atlas",
        TemplateFormat::SpineAtlas,
        "atlas",
        "Atlas de texto",
    ),
    DataFormatPreset::plain("css", "CSS", TemplateFormat::Css, "css", "Atlas de texto").rot(false),
    DataFormatPreset::plain(
        "css-simple",
        "CSS simple",
        TemplateFormat::Css,
        "css",
        "Atlas de texto",
    )
    .rot(false),
    DataFormatPreset::plain(
        "less",
        "LESS",
        TemplateFormat::Css,
        "less",
        "Atlas de texto",
    )
    .rot(false),
    DataFormatPreset::plain(
        "sass-mixins",
        "SASS mixins",
        TemplateFormat::Css,
        "sass",
        "Atlas de texto",
    )
    .rot(false),
    // --- JSON de motores ---------------------------------------------------
    DataFormatPreset::plain(
        "phaser",
        "Phaser 3",
        TemplateFormat::Phaser,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "phaser-json-hash",
        "Phaser (JSON hash)",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "phaser-json-array",
        "Phaser (JSON array)",
        TemplateFormat::Json,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "pixijs4",
        "PixiJS 4",
        TemplateFormat::PixiJson,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "egret",
        "Egret Engine",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "spriter",
        "Spriter",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "melonjs",
        "MelonJS",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "vplay",
        "V-Play",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "mapbox",
        "Mapbox",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "panda",
        "Panda 2",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "x2d",
        "x2d",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "unity",
        "Unity (JSON en .txt)",
        TemplateFormat::JsonHash,
        "txt",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "unity-texture2d",
        "Unity TexturePacker Importer",
        TemplateFormat::JsonHash,
        "tpsheet",
        "JSON de motores",
    )
    .alg(PackingAlgorithm::Polygon),
    DataFormatPreset::plain(
        "unreal-paper2d",
        "Unreal Engine / Paper2D",
        TemplateFormat::JsonHash,
        "paper2dsprites",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "monogame",
        "MonoGame TexturePacker Importer",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "corona-imagesheet",
        "Corona image sheet",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "easeljs",
        "EaselJS",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "zim",
        "ZIM",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "spark",
        "Spark AR Studio",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "spritestudio",
        "OPTPiX SpriteStudio 5",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "gamemaker-texturegroup",
        "GameMaker texture group",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "amethyst",
        "Amethyst (ids en Rust con --spriteids-file)",
        TemplateFormat::JsonHash,
        "json",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "godot-spritesheet",
        "Godot spritesheet",
        TemplateFormat::JsonHash,
        "tpsheet",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "godot-tileset",
        "Godot tileset",
        TemplateFormat::JsonHash,
        "tpset",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "godot3-spritesheet",
        "Godot 3 spritesheet",
        TemplateFormat::JsonHash,
        "tpsheet",
        "JSON de motores",
    ),
    DataFormatPreset::plain(
        "godot3-tileset",
        "Godot 3 tileset",
        TemplateFormat::JsonHash,
        "tpset",
        "JSON de motores",
    ),
    // --- plist y XML -------------------------------------------------------
    DataFormatPreset::plain(
        "cocos2d",
        "Cocos2D (plist v3)",
        TemplateFormat::Plist,
        "plist",
        "plist y XML",
    )
    .anim(true),
    DataFormatPreset::plain(
        "cocos2d-v2",
        "Cocos2D v2 (plist, obsoleto)",
        TemplateFormat::Plist,
        "plist",
        "plist y XML",
    )
    .anim(true),
    DataFormatPreset::plain(
        "cocos2d-x",
        "Cocos2D-x (plist v3)",
        TemplateFormat::Plist,
        "plist",
        "plist y XML",
    )
    .rot(true)
    .anim(true),
    DataFormatPreset::plain(
        "spritekit",
        "SpriteKit (plist + cabecera ObjC)",
        TemplateFormat::Plist,
        "plist",
        "plist y XML",
    )
    .anim(true),
    DataFormatPreset::plain(
        "spritekit-swift",
        "SpriteKit (plist + clase Swift)",
        TemplateFormat::Plist,
        "plist",
        "plist y XML",
    )
    .anim(true),
    DataFormatPreset::plain(
        "sparrow",
        "Sparrow / Starling (XML)",
        TemplateFormat::Starling,
        "xml",
        "plist y XML",
    ),
    DataFormatPreset::plain(
        "uikit",
        "UIKit (plist)",
        TemplateFormat::UIKitPlist,
        "plist",
        "plist y XML",
    ),
    // --- Motores minoritarios ---------------------------------------------
    // Muchos ya no existen en el original: se aceptan con la estructura
    // genérica JsonHash y su extensión por defecto (desviación documentada).
    DataFormatPreset::plain(
        "2dtoolkit",
        "2D Toolkit",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "agk",
        "AppGameKit",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "batterytech",
        "BatteryTech",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "bhive",
        "BHive",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "caat",
        "CAAT (Canvas Advanced Animation Toolkit)",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "cegui",
        "CEGUI / OGRE",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "gideros",
        "Gideros",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "kwik",
        "Kwik",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "libRocket",
        "libRocket",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "molecule",
        "Molecule",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "moai",
        "Moai",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "orx",
        "Orx",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "shiva3d",
        "Shiva3D",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "shiva3d-jpsprite",
        "Shiva3D + JPSprite",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "slick2d",
        "Slick2D",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "tresensa",
        "TreSensa TGE",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    DataFormatPreset::plain(
        "wave-engine-1",
        "WaveEngine",
        TemplateFormat::JsonHash,
        "json",
        "Motores minoritarios",
    ),
    // --- Extras del propio clon -------------------------------------------
    // No son ids del original: son las dos plantillas nativas que el clon
    // añadía antes de tener la lista completa.
    DataFormatPreset::plain(
        "cpp-header",
        "Cabecera C++/ObjC",
        TemplateFormat::CppHeader,
        "h",
        "Extras del propio clon",
    ),
    DataFormatPreset::plain(
        "tsv",
        "TSV",
        TemplateFormat::Tsv,
        "tsv",
        "Extras del propio clon",
    ),
];

/// Busca un preset por id, sin distinguir mayúsculas.
pub fn find_data_format(id: &str) -> Option<&'static DataFormatPreset> {
    DATA_FORMATS.iter().find(|p| p.id.eq_ignore_ascii_case(id))
}

/// Ids de [`DATA_FORMATS`] para mensajes de error.
pub fn data_format_ids() -> impl Iterator<Item = &'static str> {
    DATA_FORMATS.iter().map(|p| p.id)
}

/// Busca un preset cuya extensión de fichero de datos coincida, para poder
/// sugerir `--format <id>` cuando `--data` llega con una extensión que el
/// formato elegido no escribe.
pub fn find_data_format_by_extension(ext: &str) -> Option<&'static DataFormatPreset> {
    DATA_FORMATS
        .iter()
        .find(|p| !p.extension.is_empty() && p.extension.eq_ignore_ascii_case(ext))
}

/// Presets de una categoría, en el orden de [`DATA_FORMATS`] (combo de la
/// GUI agrupado por familia de motor).
pub fn data_formats_in_category(
    category: &'static str,
) -> impl Iterator<Item = &'static DataFormatPreset> + 'static {
    DATA_FORMATS.iter().filter(move |p| p.category == category)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_is_known_and_ids_are_unique() {
        let mut ids: Vec<&str> = Vec::new();
        for preset in DATA_FORMATS {
            assert!(
                CATEGORIES.contains(&preset.category),
                "{} agrupa en {}",
                preset.id,
                preset.category
            );
            assert!(!ids.contains(&preset.id), "id duplicado: {}", preset.id);
            assert!(!preset.label.is_empty());
            ids.push(preset.id);
        }
        assert!(
            DATA_FORMATS.len() >= 60,
            "el original enumera 60+ formatos, hay {}",
            DATA_FORMATS.len()
        );
    }

    #[test]
    fn the_original_exporter_names_are_all_present() {
        for id in [
            "json",
            "json-array",
            "xml",
            "plain",
            "libgdx",
            "spine",
            "phaser",
            "pixijs4",
            "cocos2d",
            "cocos2d-x",
            "spritekit",
            "spritekit-swift",
            "sparrow",
            "uikit",
            "unity",
            "unity-texture2d",
            "unreal-paper2d",
            "monogame",
            "orx",
            "css",
            "less",
            "sass-mixins",
            "spritesheet-only",
            "amethyst",
            "zim",
            "gamemaker-texturegroup",
            "godot-spritesheet",
        ] {
            assert!(find_data_format(id).is_some(), "falta el formato {id}");
        }
        assert!(
            find_data_format("JSON").is_some(),
            "la búsqueda ignora mayúsculas"
        );
        assert!(find_data_format("no-existe").is_none());
    }

    #[test]
    fn every_family_has_a_template() {
        use crate::templates::builtin_template;
        for preset in DATA_FORMATS {
            // Dos casos especiales: «spritesheet-only» no dibuja nada y el
            // JSON con `frames` en lista lo serializa `render` sin plantilla.
            if preset.family == TemplateFormat::SpriteSheetOnly {
                assert!(preset.extension.is_empty());
                continue;
            }
            if preset.family == TemplateFormat::Json {
                assert_eq!(preset.extension, "json");
                continue;
            }
            assert!(
                !builtin_template(preset.family).is_empty(),
                "{} no tiene plantilla",
                preset.id
            );
        }
    }

    #[test]
    fn recommended_values_are_only_set_where_they_matter() {
        // Los formatos CSS no admiten rotación (background-position).
        for id in ["css", "css-simple", "less", "sass-mixins"] {
            assert_eq!(
                find_data_format(id).and_then(|p| p.allow_rotation),
                Some(false),
                "{id} no puede rotar"
            );
        }
        // Solo los formatos con animaciones en el original las auto-detectan.
        for id in [
            "libgdx",
            "cocos2d",
            "cocos2d-x",
            "spritekit",
            "spritekit-swift",
        ] {
            assert_eq!(
                find_data_format(id).and_then(|p| p.auto_detect_animations),
                Some(true),
                "{id} debe recomendar animaciones"
            );
        }
        assert_eq!(
            find_data_format("unity-texture2d").and_then(|p| p.algorithm),
            Some(PackingAlgorithm::Polygon)
        );
    }
}
