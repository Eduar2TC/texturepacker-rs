use serde::{Deserialize, Serialize};

/// Filtro de muestreo que el data format de LibGDX declara en la hoja
/// (`filter: Linear, Linear` / `filter: Nearest, Nearest`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GdxFilter {
    #[default]
    #[serde(rename = "Linear")]
    Linear,
    #[serde(rename = "Nearest")]
    Nearest,
}

impl GdxFilter {
    pub fn as_str(&self) -> &'static str {
        match self {
            GdxFilter::Linear => "Linear",
            GdxFilter::Nearest => "Nearest",
        }
    }

    /// Acepta el token en cualquier mayúscula.
    pub fn parse(v: &str) -> Option<Self> {
        Some(match v.to_ascii_lowercase().as_str() {
            "linear" => GdxFilter::Linear,
            "nearest" => GdxFilter::Nearest,
            _ => return None,
        })
    }
}

/// Output GPU texture format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GpuFormat {
    #[default]
    #[serde(rename = "PNG")]
    Png,
    /// PNG indexado de 8 bits (hasta 256 colores).
    #[serde(rename = "PNG8")]
    Png8,
    /// JPEG lossy sin canal alfa.
    #[serde(rename = "JPG")]
    Jpg,
    #[serde(rename = "WEBP", alias = "WebP")]
    WebP,
    /// Bitmap sin comprimir (24/32 bits, sin alfa si el formato de píxel lo
    /// descarta).
    #[serde(rename = "BMP")]
    Bmp,
    /// TGA/TARGA sin comprimir.
    #[serde(rename = "TGA")]
    Tga,
    /// TIFF sin comprimir (LZW lo decide el codificador de `image`).
    #[serde(rename = "TIFF")]
    Tiff,
    /// DDS sin comprimir (RGBA8 con máscaras de canal).
    #[serde(rename = "DDS")]
    Dds,
    /// KTX v1 con el contenido zlib-comprimido (`.zktx`, estilo libGDX).
    #[serde(rename = "ZKTX")]
    Zktx,
    /// PVR v3 (PVRTC1 4bpp) con el fichero entero en gzip.
    #[serde(rename = "PVR3GZ")]
    Pvr3Gz,
    /// PVR v3 (PVRTC1 4bpp) en el contenedor CCZ de Cocos2D (zlib).
    #[serde(rename = "PVR3CCZ")]
    Pvr3Ccz,
    /// ETC1 RGB en un contenedor PKM (`.pkm`).
    #[serde(rename = "ETC1")]
    Etc1,
    /// ETC1 RGB en un contenedor KTX (`.ktx`).
    #[serde(rename = "ETC1_KTX")]
    Etc1Ktx,
    #[serde(rename = "ASTC_4x4")]
    Astc4x4,
    #[serde(rename = "ETC2_RGBA")]
    Etc2Rgba,
    #[serde(rename = "PVRTC_4BPP")]
    Pvrtc4Bpp,
    /// KTX v2 con el contenido crudo (vkFormat 37/43 + DFD), sin
    /// supercompresión.
    #[serde(rename = "KTX2")]
    Ktx2,
    /// Basis Universal ETC1S (`.basis`), transcodificable en tiempo de carga.
    #[serde(rename = "BASIS")]
    Basis,
}

impl GpuFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            GpuFormat::Png => "PNG",
            GpuFormat::Png8 => "PNG8",
            GpuFormat::Jpg => "JPG",
            GpuFormat::WebP => "WebP",
            GpuFormat::Bmp => "BMP",
            GpuFormat::Tga => "TGA",
            GpuFormat::Tiff => "TIFF",
            GpuFormat::Dds => "DDS",
            GpuFormat::Zktx => "ZKTX",
            GpuFormat::Pvr3Gz => "PVR3GZ",
            GpuFormat::Pvr3Ccz => "PVR3CCZ",
            GpuFormat::Etc1 => "ETC1",
            GpuFormat::Etc1Ktx => "ETC1_KTX",
            GpuFormat::Astc4x4 => "ASTC_4x4",
            GpuFormat::Etc2Rgba => "ETC2_RGBA",
            GpuFormat::Pvrtc4Bpp => "PVRTC_4BPP",
            GpuFormat::Ktx2 => "KTX2",
            GpuFormat::Basis => "BASIS",
        }
    }

    /// Whether this build can encode the format (`ASTC_4x4` and `BASIS` need
    /// the `gpu-formats` feature; every other format is always available).
    pub fn is_supported(&self) -> bool {
        !matches!(self, GpuFormat::Astc4x4 | GpuFormat::Basis) || cfg!(feature = "gpu-formats")
    }

    /// Hardware-compressed formats (flip-y solo aplica a estos).
    pub fn is_hardware(&self) -> bool {
        matches!(
            self,
            GpuFormat::Astc4x4
                | GpuFormat::Etc2Rgba
                | GpuFormat::Pvrtc4Bpp
                | GpuFormat::Pvr3Gz
                | GpuFormat::Pvr3Ccz
                | GpuFormat::Etc1
                | GpuFormat::Etc1Ktx
                | GpuFormat::Basis
        )
    }

    pub fn file_extension(&self) -> &'static str {
        match self {
            GpuFormat::Png | GpuFormat::Png8 => "png",
            GpuFormat::Jpg => "jpg",
            GpuFormat::WebP => "webp",
            GpuFormat::Bmp => "bmp",
            GpuFormat::Tga => "tga",
            GpuFormat::Tiff => "tiff",
            GpuFormat::Dds => "dds",
            GpuFormat::Zktx => "zktx",
            GpuFormat::Pvr3Gz => "pvr.gz",
            GpuFormat::Pvr3Ccz => "pvr.ccz",
            GpuFormat::Etc1 => "pkm",
            GpuFormat::Etc1Ktx => "ktx",
            GpuFormat::Astc4x4 => "astc",
            GpuFormat::Etc2Rgba => "ktx",
            GpuFormat::Pvrtc4Bpp => "pvr",
            GpuFormat::Ktx2 => "ktx2",
            GpuFormat::Basis => "basis",
        }
    }

    /// CLI token for the format (`--format`), plus the aliases the CLI used
    /// to keep for itself (`png-8`, `jpeg`, …).
    pub fn parse(v: &str) -> Option<GpuFormat> {
        Some(match v.to_ascii_lowercase().as_str() {
            "png" => GpuFormat::Png,
            "png8" | "png-8" => GpuFormat::Png8,
            "jpg" | "jpeg" => GpuFormat::Jpg,
            "webp" => GpuFormat::WebP,
            "bmp" => GpuFormat::Bmp,
            "tga" => GpuFormat::Tga,
            "tif" | "tiff" => GpuFormat::Tiff,
            "dds" => GpuFormat::Dds,
            "zktx" => GpuFormat::Zktx,
            "pvr3gz" | "pvr-gz" => GpuFormat::Pvr3Gz,
            "pvr3ccz" | "pvr-ccz" => GpuFormat::Pvr3Ccz,
            "pkm" | "etc1" => GpuFormat::Etc1,
            "ktx" | "etc1-ktx" | "etc1_ktx" => GpuFormat::Etc1Ktx,
            "astc" => GpuFormat::Astc4x4,
            "etc2" => GpuFormat::Etc2Rgba,
            "pvrtc" | "pvr3" => GpuFormat::Pvrtc4Bpp,
            "ktx2" => GpuFormat::Ktx2,
            "basis" => GpuFormat::Basis,
            _ => return None,
        })
    }
}

/// Formato de píxel de salida para formatos de software (pixel format).
/// Los formatos de hardware comprimen RGBA y lo ignoran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PixelFormat {
    #[default]
    #[serde(rename = "RGBA8888")]
    Rgba8888,
    /// RGB sin alfa; la transparencia se compone sobre negro.
    #[serde(rename = "RGB888")]
    Rgb888,
    /// Solo el canal alfa (escala de grises).
    #[serde(rename = "ALPHA8")]
    Alpha8,
    /// Luminancia (escala de grises).
    #[serde(rename = "INTENSITY8")]
    Intensity8,
    /// Luminancia + alfa (grises + alfa).
    #[serde(rename = "ALPHA_INTENSITY8")]
    AlphaIntensity8,
    /// 16 bits: R5 G5 B5 + 1 bit de transparencia (RGBA5551).
    /// Los archivos siguen siendo PNG estándar con los colores reducidos a la
    /// rejilla 5-5-5-1 (expansión por replicación de bits).
    #[serde(rename = "RGBA5551")]
    Rgba5551,
    /// 20 bits: R5 G5 B5 + 5 bits de transparencia (RGBA5555). Como
    /// RGBA5551 pero el alfa se cuantiza a la rejilla de 5 bits en lugar de
    /// colapsar a 0/255.
    #[serde(rename = "RGBA5555")]
    Rgba5555,
    /// 32 bits con canales reordenados a B,G,R,A (BGRA8888); el PNG
    /// resultante lleva los canales R y B invertidos, para motores que cargan
    /// texturas en orden BGRA (p. ej. cocos2d).
    #[serde(rename = "BGRA8888")]
    Bgra8888,
    /// 16 bits: R4 G4 B4 A4 (RGBA4444), con replicación de bits al expandir.
    #[serde(rename = "RGBA4444")]
    Rgba4444,
    /// 16 bits: R5 G6 B5 sin alfa (RGB565); la transparencia se compone
    /// sobre negro.
    #[serde(rename = "RGB565")]
    Rgb565,
    // ---------------------------------------------------------------------
    // Formatos de píxel de hardware (GPU). Solo se pueden elegir con un
    // formato de textura que admita la compresión correspondiente; con un
    // formato de software se ignoran (y la GUI/CLI avisa).
    // ---------------------------------------------------------------------
    /// PVRTC1 2 bits/píxel con alfa.
    #[serde(rename = "PVRTCI_2BPP_RGBA")]
    Pvrtc2BppRgba,
    /// PVRTC1 4 bits/píxel con alfa.
    #[serde(rename = "PVRTCI_4BPP_RGBA")]
    Pvrtc4BppRgba,
    /// PVRTC1 2 bits/píxel opaco (sin alfa).
    #[serde(rename = "PVRTCI_2BPP_RGB")]
    Pvrtc2BppRgb,
    /// PVRTC1 4 bits/píxel opaco (sin alfa).
    #[serde(rename = "PVRTCI_4BPP_RGB")]
    Pvrtc4BppRgb,
    /// ETC1 RGB (sin alfa).
    #[serde(rename = "ETC1_RGB")]
    Etc1Rgb,
    /// ETC2 RGB (sin alfa).
    #[serde(rename = "ETC2_RGB")]
    Etc2Rgb,
    /// ETC2 RGBA (color + alfa con EAC).
    #[serde(rename = "ETC2_RGBA")]
    Etc2Rgba,
    /// S3TC/BC1: 4 bits/píxel con 1 bit de alfa.
    #[serde(rename = "DXT1")]
    Dxt1,
    /// S3TC/BC2: 8 bits/píxel con alfa explícito de 4 bits (sin
    /// interpolar): el alfa duro de pixel-art sale exacto y no se
    /// deforma con la interpolación de BC3.
    #[serde(rename = "DXT3")]
    Dxt3,
    /// S3TC/BC3: 8 bits/píxel con alfa interpolado.
    #[serde(rename = "DXT5")]
    Dxt5,
    /// ASTC con bloque 4x4 (8 bpp).
    #[serde(rename = "ASTC_4x4")]
    Astc4x4,
    /// ASTC con bloque 5x4 (6.40 bpp).
    #[serde(rename = "ASTC_5x4")]
    Astc5x4,
    /// ASTC con bloque 5x5 (5.12 bpp).
    #[serde(rename = "ASTC_5x5")]
    Astc5x5,
    /// ASTC con bloque 6x5 (4.27 bpp).
    #[serde(rename = "ASTC_6x5")]
    Astc6x5,
    /// ASTC con bloque 6x6 (3.56 bpp).
    #[serde(rename = "ASTC_6x6")]
    Astc6x6,
    /// ASTC con bloque 8x5 (3.20 bpp).
    #[serde(rename = "ASTC_8x5")]
    Astc8x5,
    /// ASTC con bloque 8x6 (2.67 bpp).
    #[serde(rename = "ASTC_8x6")]
    Astc8x6,
    /// ASTC con bloque 8x8 (2.00 bpp).
    #[serde(rename = "ASTC_8x8")]
    Astc8x8,
    /// ASTC con bloque 10x5 (2.56 bpp).
    #[serde(rename = "ASTC_10x5")]
    Astc10x5,
    /// ASTC con bloque 10x6 (2.13 bpp).
    #[serde(rename = "ASTC_10x6")]
    Astc10x6,
    /// ASTC con bloque 10x8 (1.60 bpp).
    #[serde(rename = "ASTC_10x8")]
    Astc10x8,
    /// ASTC con bloque 10x10 (1.28 bpp).
    #[serde(rename = "ASTC_10x10")]
    Astc10x10,
    /// ASTC con bloque 12x10 (1.07 bpp).
    #[serde(rename = "ASTC_12x10")]
    Astc12x10,
    /// ASTC con bloque 12x12 (0.89 bpp).
    #[serde(rename = "ASTC_12x12")]
    Astc12x12,
}

impl PixelFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            PixelFormat::Rgba8888 => "RGBA8888",
            PixelFormat::Rgb888 => "RGB888",
            PixelFormat::Alpha8 => "ALPHA8",
            PixelFormat::Intensity8 => "INTENSITY8",
            PixelFormat::AlphaIntensity8 => "ALPHA_INTENSITY8",
            PixelFormat::Rgba5551 => "RGBA5551",
            PixelFormat::Rgba5555 => "RGBA5555",
            PixelFormat::Bgra8888 => "BGRA8888",
            PixelFormat::Rgba4444 => "RGBA4444",
            PixelFormat::Rgb565 => "RGB565",
            PixelFormat::Pvrtc2BppRgba => "PVRTCI_2BPP_RGBA",
            PixelFormat::Pvrtc4BppRgba => "PVRTCI_4BPP_RGBA",
            PixelFormat::Pvrtc2BppRgb => "PVRTCI_2BPP_RGB",
            PixelFormat::Pvrtc4BppRgb => "PVRTCI_4BPP_RGB",
            PixelFormat::Etc1Rgb => "ETC1_RGB",
            PixelFormat::Etc2Rgb => "ETC2_RGB",
            PixelFormat::Etc2Rgba => "ETC2_RGBA",
            PixelFormat::Dxt1 => "DXT1",
            PixelFormat::Dxt3 => "DXT3",
            PixelFormat::Dxt5 => "DXT5",
            PixelFormat::Astc4x4 => "ASTC_4x4",
            PixelFormat::Astc5x4 => "ASTC_5x4",
            PixelFormat::Astc5x5 => "ASTC_5x5",
            PixelFormat::Astc6x5 => "ASTC_6x5",
            PixelFormat::Astc6x6 => "ASTC_6x6",
            PixelFormat::Astc8x5 => "ASTC_8x5",
            PixelFormat::Astc8x6 => "ASTC_8x6",
            PixelFormat::Astc8x8 => "ASTC_8x8",
            PixelFormat::Astc10x5 => "ASTC_10x5",
            PixelFormat::Astc10x6 => "ASTC_10x6",
            PixelFormat::Astc10x8 => "ASTC_10x8",
            PixelFormat::Astc10x10 => "ASTC_10x10",
            PixelFormat::Astc12x10 => "ASTC_12x10",
            PixelFormat::Astc12x12 => "ASTC_12x12",
        }
    }

    /// Un token de hardware (PVRTC/ETC/DXT/ASTC) en vez de un formato de
    /// software embebible en PNG/JPG/….
    pub fn is_gpu(&self) -> bool {
        !matches!(
            self,
            PixelFormat::Rgba8888
                | PixelFormat::Rgb888
                | PixelFormat::Alpha8
                | PixelFormat::Intensity8
                | PixelFormat::AlphaIntensity8
                | PixelFormat::Rgba5551
                | PixelFormat::Rgba5555
                | PixelFormat::Bgra8888
                | PixelFormat::Rgba4444
                | PixelFormat::Rgb565
        )
    }

    /// Tamaño de bloque ASTC `(x, y)` cuando el token es `ASTC_*`.
    pub fn astc_block(&self) -> Option<(u8, u8)> {
        Some(match self {
            PixelFormat::Astc4x4 => (4, 4),
            PixelFormat::Astc5x4 => (5, 4),
            PixelFormat::Astc5x5 => (5, 5),
            PixelFormat::Astc6x5 => (6, 5),
            PixelFormat::Astc6x6 => (6, 6),
            PixelFormat::Astc8x5 => (8, 5),
            PixelFormat::Astc8x6 => (8, 6),
            PixelFormat::Astc8x8 => (8, 8),
            PixelFormat::Astc10x5 => (10, 5),
            PixelFormat::Astc10x6 => (10, 6),
            PixelFormat::Astc10x8 => (10, 8),
            PixelFormat::Astc10x10 => (10, 10),
            PixelFormat::Astc12x10 => (12, 10),
            PixelFormat::Astc12x12 => (12, 12),
            _ => return None,
        })
    }

    /// `true` si el formato de textura pedido puede alojar este pixel format
    /// (tabla de referencia: solo se pueden elegir los compatibles). Los
    /// formatos de software siempre valen: los de hardware los ignoran.
    pub fn is_compatible_with(&self, format: GpuFormat) -> bool {
        if !self.is_gpu() {
            return true;
        }
        match self {
            PixelFormat::Pvrtc2BppRgba
            | PixelFormat::Pvrtc4BppRgba
            | PixelFormat::Pvrtc2BppRgb
            | PixelFormat::Pvrtc4BppRgb => matches!(
                format,
                GpuFormat::Pvrtc4Bpp | GpuFormat::Pvr3Gz | GpuFormat::Pvr3Ccz
            ),
            PixelFormat::Etc1Rgb => matches!(format, GpuFormat::Etc1 | GpuFormat::Etc1Ktx),
            PixelFormat::Etc2Rgb | PixelFormat::Etc2Rgba => format == GpuFormat::Etc2Rgba,
            PixelFormat::Dxt1 | PixelFormat::Dxt3 | PixelFormat::Dxt5 => format == GpuFormat::Dds,
            f if f.astc_block().is_some() => format == GpuFormat::Astc4x4,
            _ => false,
        }
    }

    /// Código del pixel format en la cabecera PVR v3 (0=PVRTC1 2bpp RGB,
    /// 1=2bpp RGBA, 2=4bpp RGB, 3=4bpp RGBA). Un formato de píxel que no es
    /// PVRTC cae al valor por defecto: 4bpp RGBA.
    pub fn pvr_header_code(&self) -> u64 {
        match self {
            PixelFormat::Pvrtc2BppRgb => 0,
            PixelFormat::Pvrtc2BppRgba => 1,
            PixelFormat::Pvrtc4BppRgb => 2,
            _ => 3,
        }
    }

    /// `true` cuando el pixel format PVRTC elegido es de 2 bits/píxel.
    pub fn is_pvrtc_2bpp(&self) -> bool {
        matches!(self, PixelFormat::Pvrtc2BppRgb | PixelFormat::Pvrtc2BppRgba)
    }

    /// `true` cuando el pixel format descarta el canal alfa (PVRTC RGB).
    pub fn drops_alpha(&self) -> bool {
        matches!(self, PixelFormat::Pvrtc2BppRgb | PixelFormat::Pvrtc4BppRgb)
    }
}

/// Built-in metadata template languages (the spec mentions JSON/XML/Plist/C++).
///
/// Cada variante es una *familia* de salida: la lista completa de
/// exportadores (`crate::dataformats::DATA_FORMATS`) se mapea
/// sobre estas familias, así que un preset cambia la familia y la extensión
/// del fichero de datos a la vez.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TemplateFormat {
    /// JSON con `frames` como lista (familia `json-array`).
    #[default]
    #[serde(rename = "JSON")]
    Json,
    /// JSON con `frames` como mapa por nombre (familia `json`).
    #[serde(rename = "JsonHash")]
    JsonHash,
    /// JSON de Phaser 3: una entrada de textura por página bajo `textures`.
    #[serde(rename = "Phaser")]
    Phaser,
    /// JSON hash de PixiJS: como `JsonHash` con `image` en cada frame.
    #[serde(rename = "PixiJson")]
    PixiJson,
    /// XML genérico `<TextureAtlas><sprite …/></TextureAtlas>`.
    #[serde(rename = "XML")]
    Xml,
    /// XML Sparrow/Starling (`<SubTexture …/>`).
    #[serde(rename = "Starling")]
    Starling,
    /// Plist v3 de Cocos2D (la misma estructura que usan `spritekit` y
    /// `spritekit-swift`).
    #[serde(rename = "Plist")]
    Plist,
    /// Plist de UIKit con claves escalares por campo.
    #[serde(rename = "UIKit")]
    UIKitPlist,
    /// Atlas de texto de libGDX (`xy`/`size`/`orig`/`offset`/`index`).
    #[serde(rename = "LibgdxAtlas")]
    LibgdxAtlas,
    /// Atlas de texto de Spine (cabezera de página + regiones sangradas).
    #[serde(rename = "SpineAtlas")]
    SpineAtlas,
    /// Reglas CSS de sprites (también para `less` y `sass-mixins`).
    #[serde(rename = "Css")]
    Css,
    /// Cabecera C++/ObjC con la tabla de sprites (fichero extra).
    #[serde(rename = "CppHeader")]
    CppHeader,
    /// TSV con una fila por sprite (fichero propio, sin id).
    #[serde(rename = "TSV")]
    Tsv,
    /// Texto plano (el exportador de ejemplo `plain`).
    #[serde(rename = "PlainText")]
    PlainText,
    /// Solo la hoja de textura: no se escribe fichero de datos.
    #[serde(rename = "SpriteSheetOnly")]
    SpriteSheetOnly,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectConfig;

    #[test]
    fn output_format_tokens_and_extensions() {
        // Token CLI -> variante -> extensión del fichero publicado.
        let cases = [
            ("png", GpuFormat::Png, "png"),
            ("png8", GpuFormat::Png8, "png"),
            ("jpeg", GpuFormat::Jpg, "jpg"),
            ("webp", GpuFormat::WebP, "webp"),
            ("bmp", GpuFormat::Bmp, "bmp"),
            ("tga", GpuFormat::Tga, "tga"),
            ("tiff", GpuFormat::Tiff, "tiff"),
            ("dds", GpuFormat::Dds, "dds"),
            ("zktx", GpuFormat::Zktx, "zktx"),
            ("pvr3gz", GpuFormat::Pvr3Gz, "pvr.gz"),
            ("pvr3ccz", GpuFormat::Pvr3Ccz, "pvr.ccz"),
            ("pkm", GpuFormat::Etc1, "pkm"),
            ("ktx", GpuFormat::Etc1Ktx, "ktx"),
            ("astc", GpuFormat::Astc4x4, "astc"),
            ("etc2", GpuFormat::Etc2Rgba, "ktx"),
            ("pvrtc", GpuFormat::Pvrtc4Bpp, "pvr"),
            ("pvr3", GpuFormat::Pvrtc4Bpp, "pvr"),
            ("ktx2", GpuFormat::Ktx2, "ktx2"),
            ("basis", GpuFormat::Basis, "basis"),
        ];
        for (token, want, ext) in cases {
            let got = GpuFormat::parse(token).unwrap_or_else(|| panic!("token {token}"));
            assert_eq!(got, want, "{token}");
            assert_eq!(got.file_extension(), ext, "{token}");
        }
        assert_eq!(GpuFormat::parse("svg"), None);

        // Los formatos de hardware siguen distinguiéndose de los de software.
        assert!(GpuFormat::Etc1.is_hardware());
        assert!(GpuFormat::Pvr3Ccz.is_hardware());
        assert!(GpuFormat::Basis.is_hardware(), "Basis comprime en GPU");
        assert!(!GpuFormat::Bmp.is_hardware());
        assert!(!GpuFormat::Dds.is_hardware());
        assert!(!GpuFormat::Zktx.is_hardware());
        assert!(!GpuFormat::Ktx2.is_hardware(), "KTX2 guarda crudo");
        assert!(GpuFormat::Etc1.is_supported() && GpuFormat::Bmp.is_supported());
        assert!(GpuFormat::Ktx2.is_supported(), "KTX2 no necesita C++");
        assert_eq!(
            GpuFormat::Basis.is_supported(),
            cfg!(feature = "gpu-formats"),
            "Basis depende de la feature gpu-formats"
        );

        // Serde: los nombres del TOML vuelven a la misma variante.
        for format in [
            GpuFormat::Bmp,
            GpuFormat::Pvr3Gz,
            GpuFormat::Etc1Ktx,
            GpuFormat::Ktx2,
            GpuFormat::Basis,
        ] {
            let cfg = ProjectConfig {
                gpu_format: format,
                ..ProjectConfig::default()
            };
            let back = ProjectConfig::from_toml(&cfg.to_toml().unwrap()).unwrap();
            assert_eq!(back.gpu_format, format, "{format:?}");
        }
    }
}
