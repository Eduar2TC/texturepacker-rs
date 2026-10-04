/// Línea de `--version`.
pub(crate) fn version_line() -> String {
    format!("TexturePacker-RS {}", env!("CARGO_PKG_VERSION"))
}

/// Ids de data formats que aceptan `--format`/`--template-format`, uno por
/// línea.
pub(crate) fn exporter_list_text() -> String {
    let mut ids: Vec<&str> = tp_core::dataformats::data_format_ids().collect();
    ids.sort_unstable();
    ids.dedup();
    let mut text = ids.join("\n");
    text.push('\n');
    text
}

/// Texto de la ayuda. Está separado de [`usage`] para poder testear que toda
/// opción documentada exista realmente en el parser.
fn help_text() -> String {
    format!(
        "TexturePacker-RS {}\n\
         \n\
         USOS:\n\
         \x20 tp-cli pack <proyecto.tpproj|proyecto.tps>\n\
         \x20 tp-cli pack --input DIR --output DIR [opciones]\n\
         \x20 tp-cli pack [opciones] <carpeta|imagen>…   (como el original: los sprites\n\
         \x20                       pueden ir en posiciónles, mezclados con las opciones)\n\
         \x20 tp-cli decrypt <archivo.tpenc> --key CLAVE -o salida.png [--pixel-format F]\n\
         \n\
         ENTRADA Y SALIDA:\n\
         \x20 --input DIR           Carpeta o fichero con los sprites (también en posiciónles)\n\
         \x20 --output DIR          Carpeta de salida de la hoja y de los metadatos\n\
         \x20 --sheet FICHERO       Hoja de salida: fija --output, el nombre base y el\n\
         \x20                        formato por su extensión (sobrescribe --output)\n\
         \x20 --data FICHERO        Metadatos de salida: fija --output y el nombre base;\n\
         \x20                        la extensión debe coincidir con el formato de datos\n\
         \x20 --format T            Doble acepción, como en el original:\n\
         \x20                          textura: png | png8 | jpg | webp | bmp | tga | tiff |\n\
         \x20                            dds | zktx | pvr3gz | pvr3ccz | pkm | ktx | astc |\n\
         \x20                            etc2 | pvrtc | ktx2 | basis\n\
         \x20                          datos: json | xml | plist | cpp | tsv | text o un\n\
         \x20                            exportador (json-hash, libgdx, cocos2d, phaser,\n\
         \x20                            pixijs4, sparrow, spine…)\n\
         \x20 --texture-format T    Formato de la hoja si no se deduce de la extensión de\n\
         \x20                        --sheet (tiene prioridad sobre --format de textura)\n\
         \x20 --base-name NOMBRE    Nombre base de la salida (admite {{n}} {{n1}} {{v}})\n\
         \x20 --template-format T   Sólo la acepción de datos de --format (familias y\n\
         \x20                        exportadores; json-hash es el hash del original)\n\
         \x20 --prepend-folder-name Antepone el nombre de la carpeta inteligente\n\
         \x20 --trim-sprite-names   Nombres de sprite sin extensión (defecto)\n\
         \x20 --keep-extension      Los nombres de sprite conservan la extensión\n\
         \x20 --no-recursive        No buscar en subdirectorios\n\
         \x20 --ignore-files PATRÓN Excluye los ficheros que coincidan (repetible; el *\n\
         \x20                        de glob_match también incluye /)\n\
         \x20 --replace PATRÓN=TEXTO Renombra sprites con una expresión regular (repetible,\n\
         \x20                        en orden; p.ej. ^old=new)\n\
         \x20 --heuristic-mask      Borra el color de fondo de un sprite totalmente opaco\n\
         \x20                        (color más frecuente del borde; antes de recortar)\n\
         \x20 --auto-folders        pack por carpetas automático: cada subcarpeta de entrada\n\
         \x20                        produce su hoja en la subcarpeta de salida\n\
         \x20 --multipack           Permitir varias hojas (anula --no-multipack)\n\
         \x20 --no-multipack        No emitir varias hojas (falla si no caben en una)\n\
         \x20 --cache-busting       Añade ?v=<hash> a la textura citada en los metadatos\n\
         \x20 --enable-cache-busting Alias del original de --cache-busting\n\
         \x20 --texture-path RUTA   Prefijo de la textura en los metadatos (alias: --texturepath)\n\
         \x20 --print-json          Imprimir los metadatos JSON en stdout\n\
         \x20 --force-publish       Reescribir la salida aunque los bytes no hayan cambiado\n\
         \x20 --save FICHERO        Guardar la configuración como .tpproj (o .tps, el XML\n\
         \x20                        del original) y terminar sin empaquetar\n\
         \n\
         ATLAS:\n\
         \x20 --max-size N          Tamaño máximo del atlas (512..8192, potencia de 2)\n\
         \x20 --max-width N         Tope propio del ancho del atlas (0 = sin tope, 1..16384)\n\
         \x20 --max-height N        Tope propio del alto del atlas (0 = sin tope, 1..16384)\n\
         \x20 --background-color HEX  Color con el que se rellena la hoja (RGB/RRGGBB/RRGGBBAA)\n\
         \x20 --width N             Ancho fijo del atlas (0 = automático)\n\
         \x20 --height N            Alto fijo del atlas (0 = automático)\n\
         \x20 --shape-padding N     Espacio entre sprites (px)\n\
         \x20 --padding N           Alias corto de --shape-padding\n\
         \x20 --border-padding N    Margen entre los sprites y el borde (px)\n\
         \x20 --align-to-grid N     Alinea las esquinas de los sprites a N px (0 = off);\n\
         \x20                        alias: --align\n\
         \x20 --extrude N           Extrude de bordes (px)\n\
         \x20 --common-divisor N    Ancho y alto de los sprites múltiplos de N (los dos ejes)\n\
         \x20 --common-divisor-x N  Igual, sólo el eje horizontal\n\
         \x20 --common-divisor-y N  Igual, sólo el eje vertical\n\
         \x20 --default-pivot-point X,Y  Pivot por defecto de todos los sprites, normalizado\n\
         \x20                        (0,0 = esquina superior izquierda; defecto 0.5,0.5)\n\
         \x20 --force-identical-layout  Mismo layout en todas las variantes (requiere\n\
         \x20                        allowfraction en las variantes afectadas)\n\
         \x20 --algorithm T         maxrects | polygon | guillotine | grid | basic | manual\n\
         \x20 --strategy T          bssf (ShortSideFit) | baf (AreaFit) | blsf (LongSideFit) |\n\
         \x20                        best | bottom-left | contact-point | guillotine\n\
         \x20                        (alias: --maxrects-heuristics)\n\
         \x20 --pack-mode T         fast | good | best\n\
         \x20 --basic-sort-by T     best | name | width | height | area | circumference\n\
         \x20 --basic-order T       ascending | descending\n\
         \x20 --size-constraints T  any | pot | multiple-of-4 | word-aligned\n\
         \x20 --force-squared       Atlas cuadrado\n\
         \x20 --no-rotation         Desactivar rotación 90°\n\
         \x20 --disable-rotation    Alias del original de --no-rotation\n\
         \x20 --enable-rotation     Activar la rotación 90° (defecto)\n\
         \x20 --polygon             Modo polígono (mallas + empaquetado por contorno)\n\
         \x20 --tolerance N         Tolerancia de la aproximación poligonal en px (defecto\n\
         \x20                        1,5); el original la pide con tracer-tolerance en otras\n\
         \x20                        unidades, así que no se acepta ese nombre\n\
         \x20 --trim-mode T         none | trim | crop | cropkeeppos | polygon (defecto trim)\n\
         \x20 --trim-threshold N    Umbral de alpha para recorte (1-255, por defecto 1)\n\
         \x20 --trim-margin N       Margen transparente tras el recorte (px)\n\
         \x20 --no-trim             No recortar los sprites (equivale a --trim-mode none)\n\
         \x20 --no-aliasing         Desactivar deduplicación por hash (alias: --disable-auto-alias)\n\
         \x20 --shape-debug         Dibuja los contornos de los sprites sobre la hoja\n\
         \x20 --scale-mode T        smooth | fast | scale2x | scale3x | scale4x | eagle\n\
         \x20 --variant E[:N[:F[:allowfraction[:W:H]]]]  Variante (repetible o por comas),\n\
         \x20                        p.ej. 0.5:-hd, 1.0:-ipadhd::*, 0.25:::allowfraction:1024:1024\n\
         \x20 --variants LIST       Escalas, p.ej. 2,0.5 (sufijos @2x, -hd)\n\
         \x20 --scale F             Escala todas las variantes por F (0 < F <= 8); con una\n\
         \x20                        única variante el sufijo de fichero queda vacío\n\
         \n\
         CALIDAD:\n\
         \x20 --color-depth T       RGBA8888 | RGBA4444 | RGB565\n\
         \x20 --dither T            none | nn | linear | floyd | floyd-alpha | atkinson |\n\
         \x20                        atkinson-alpha (alias: --dither-type)\n\
         \x20 --alpha-handling T    keep | clear | bleed | premultiply\n\
         \x20 --png-opt-level N     Optimización PNG sin pérdida, 0-7 (1 = indexa si ≤256 colores)\n\
         \x20 --png8-dither T       Dithering PNG-8: low | medium | high\n\
         \x20 --jpg-quality N       Calidad JPG (0-100)\n\
         \x20 --webp-quality N      Calidad WebP (0-100; por defecto sin pérdida)\n\
         \x20 --pixel-format T      rgba8888 | rgb888 | alpha8 | intensity8 | alpha-intensity8 | rgba5551 | rgba5555 | bgra8888\n\
         \x20                        rgba4444 | rgb565 | pvrtc2bpp-rgba | pvrtc4bpp-rgba | pvrtc2bpp-rgb | pvrtc4bpp-rgb\n\
         \x20                        etc1 | etc2 | etc2-rgb | dxt1 | dxt3 | dxt5 | astc-4x4 | astc-8x8 | astc-12x12\n\
         \x20                        (alias: --opt)\n\
         \x20 --pvr-quality N       Calidad PVRTC 0-7 (defecto 3)\n\
         \x20 --etc1-quality N      Calidad ETC1 0-100 (defecto 70)\n\
         \x20 --etc2-quality N      Calidad ETC2 0-100 (defecto 70)\n\
         \x20 --astc-quality N      Calidad ASTC 0-4: 0=fastest .. 4=exhaustive (defecto 2)\n\
         \x20 --basis-quality N     Calidad Basis ETC1S 0-100 (defecto 50; alias: --basisu-quality)\n\
         \x20 --dxt-mode T          DXT_LINEAR (error uniforme) | DXT_PERCEPTUAL (ponderado)\n\
         \x20 --flip-y              Voltea la textura (alias: --flip-pvr)\n\
         \x20 --dpi N               Resolución de la hoja en ppp (1..1000000); sólo PNG\n\
         \n\
         METADATOS Y EXPORTADORES:\n\
         \x20 --class-file F        Fichero de clase Swift extra (spritekit-swift)\n\
         \x20 --header-file F       Cabecera C++/ObjC extra (cocos2d-x)\n\
         \x20 --source-file F       Código fuente C++ extra (cocos2d-x)\n\
         \x20 --spriteids-file F    Lista de ids de sprites extra (amethyst)\n\
         \x20 --template F          Plantilla de metadatos propia\n\
         \x20 --gdx-filter T        Filtro del data format de LibGDX: linear | nearest\n\
         \x20 --key CLAVE           Cifrar texturas con AES-256-GCM\n\
         \x20 --key-name NOMBRE     Usar la clave global guardada con ese nombre\n\
         \x20 --save-key NOMBRE     Guardar --key en el almacén global y usarla\n\
         \x20 --custom-exporters-directory DIR  Carga <id>.hbs propios como formatos de\n\
         \x20                        datos, válidos en --format y --template-format\n\
         \x20 --convert-texture FICHERO  Convierte una imagen a --texture-format y termina\n\
         \x20                        (aplica también --pixel-format, --dpi y --scale)\n\
         \x20 --css-sprite-prefix P  Prefijo de las clases CSS (p.ej. icon-)\n\
         \x20 --css-media-query-2x Q  Envuelve en esta media query la CSS de variantes >1\n\
         \x20 --plain-string-property TEXTO\n\
         \x20                        exporterProperties.string_property de la plantilla\n\
         \x20 --plain-bool-property BOOL  exporterProperties.bool_property (true|false)\n\
         \x20 Del original, registradas pero rechazadas con su motivo porque no escribimos\n\
         \x20 esa salida: easeljs-framerate, zim-framerate,\n\
         \x20 gamemaker-texturegroup-frame-speed, classfile-file, spine-legacy-output,\n\
         \x20 libgdx-legacy-output, spritestudio-writePivots, orx-includeComments,\n\
         \x20 orx-keepInCache, orx-keyDuration, orx-optimizeSectionNames, orx-pixelSnap\n\
         \n\
         MAPAS DE NORMALES:\n\
         \x20 --pack-normalmaps     Empaquetar los mapas de normales (defecto)\n\
         \x20 --no-normals          No empaquetar mapas de normales\n\
         \x20 --normalmap-suffix T  Sufijo del mapa de normales (defecto _normal)\n\
         \x20 --normalmap-filter T  Ficheros con esta subcadena en la ruta son normales\n\
         \x20 --normalmap-sheet N   Nombre base de la hoja de normales (defecto <imagen>_normal)\n\
         \x20 --normalmap-detect    Detectar mapas de normales por su color\n\
         \n\
         ANIMACIONES:\n\
         \x20 --no-auto-animations  No agrupar sprites walk_001..N como animaciones\n\
         \n\
         INFORMACIÓN:\n\
         \x20 --help                Esta ayuda (alias: -h)\n\
         \x20 --version             Versión del programa (alias: -V)\n\
         \x20 --exporter-list       Ids de data formats aceptados por --format de datos\n\
         \x20 --verbose             Detalle extra de la entrada y la configuración usada\n\
         \x20 --quiet               Sólo errores (calla el resumen de empaquetado)",
        env!("CARGO_PKG_VERSION")
    )
}

/// Texto de la ayuda de `decrypt`. Está separada de [`help_text`] por la
/// misma razón: `tp-cli decrypt --help` imprimía antes la ayuda de `pack`,
/// con un centenar de opciones que decrypt ni siquiera lee.
fn decrypt_help_text() -> String {
    format!(
        "TexturePacker-RS {}\n\
         \n\
         USO:\n\
         \x20 tp-cli decrypt <archivo.tpenc> --key CLAVE [-o salida.png] [--pixel-format F]\n\
         \n\
         OPCIONES DE DECRYPT:\n\
         \x20 --key CLAVE           Clave con la que se cifró el .tpenc (obligatoria)\n\
         \x20 -o, --out FICHERO     Imagen de salida (por defecto: <entrada>.dec.png)\n\
         \x20 --pixel-format F      Formato de la imagen decodificada (png por defecto)\n\
         \x20 --verbose             Detalle extra del proceso\n\
         \x20 --quiet               Sólo errores\n\
         \x20 --help, -h            Esta ayuda (la general: tp-cli --help)",
        env!("CARGO_PKG_VERSION")
    )
}

pub(crate) fn usage() -> ! {
    println!("{}", help_text());
    std::process::exit(0);
}

pub(crate) fn decrypt_usage() -> ! {
    println!("{}", decrypt_help_text());
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{DECRYPT_FLAGS, DECRYPT_VALUES, PACK_FLAGS, PACK_VALUES, SHORT_ALIASES};

    /// Todas las `--opciones` que aparecen en el texto de la ayuda.
    fn help_options(text: &str) -> Vec<String> {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == b'-' && bytes[i + 1] == b'-' {
                let start = i + 2;
                let mut j = start;
                // Mayúsculas también: `--orx-keyDuration` lleva las suyas.
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-') {
                    j += 1;
                }
                if j > start {
                    out.push(text[start..j].to_string());
                }
                i = j;
            } else {
                i += 1;
            }
        }
        out
    }

    /// Las opciones **cortas** de un solo carácter que la ayuda promete
    /// (`-h`, `-o`). Solo cuenta un `-` suelto tras espacio o paréntesis y
    /// seguido de una única letra: los guiones de palabras compuestas
    /// (`-hd`, `TexturePacker-RS`) son sufijos o nombres, no opciones.
    fn help_short_options(text: &str) -> Vec<String> {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        for i in 0..bytes.len() {
            if bytes[i] != b'-' || bytes.get(i + 1) == Some(&b'-') {
                continue;
            }
            let anterior = if i == 0 { b' ' } else { bytes[i - 1] };
            if anterior != b' ' && anterior != b'(' {
                continue;
            }
            let Some(&letra) = bytes.get(i + 1) else {
                continue;
            };
            if !letra.is_ascii_alphabetic() {
                continue;
            }
            if bytes.get(i + 2).is_some_and(|c| c.is_ascii_alphanumeric()) {
                continue; // dos letras seguidas: `-hd` y compañía
            }
            out.push((letra as char).to_string());
        }
        out.sort();
        out.dedup();
        out
    }

    #[test]
    fn exporter_list_and_version_are_stable() {
        let list = exporter_list_text();
        assert!(list.contains("phaser"), "{list}");
        assert!(list.contains("libgdx"), "{list}");
        assert!(list.ends_with('\n'));
        assert!(!list.lines().any(|line| line == "png"));
        assert!(version_line().starts_with("TexturePacker-RS "));
    }

    #[test]
    fn every_option_in_the_help_is_known() {
        // La ayuda prometió cosas que el parser no leía (--no-auto-animations
        // era una de ellas): ya no puede volver a pasar.
        for opt in help_options(&help_text()) {
            // Comparación exacta: la ayuda tiene que citar la grafía del
            // registro, que es la que el parser distingue (`--orx-keyDuration`).
            let known = PACK_VALUES.contains(&opt.as_str()) || PACK_FLAGS.contains(&opt.as_str());
            assert!(known, "la ayuda documenta --{opt} y el parser no la conoce");
        }
    }

    /// La ayuda de decrypt es propia y completa: todo lo que documenta lo
    /// lee el parser de decrypt, y nada que acepta se queda sin documentar.
    #[test]
    fn decrypt_help_documents_exactly_the_decrypt_options() {
        let texto = decrypt_help_text();
        for opt in help_options(&texto) {
            let known =
                DECRYPT_VALUES.contains(&opt.as_str()) || DECRYPT_FLAGS.contains(&opt.as_str());
            assert!(
                known,
                "la ayuda de decrypt documenta --{opt} y el parser no la conoce"
            );
        }
        for aceptada in DECRYPT_VALUES.iter().chain(DECRYPT_FLAGS.iter()) {
            assert!(
                texto.contains(&format!("--{aceptada}")),
                "decrypt acepta --{aceptada} y su ayuda no lo documenta"
            );
        }
        // La general sigue documentando pack y menciona decrypt, para que
        // -h desde decrypt siga llevando a alguna parte.
        let general = help_text();
        assert!(
            general.contains("decrypt <archivo.tpenc>"),
            "sin mención a decrypt"
        );
        assert!(general.contains("--max-size"));
    }

    /// Igual que la de arriba, pero para `-h`, `-o` y demás alias cortos:
    /// solo se extraían tokens `--x`, así que `-h` (prometido en la propia
    /// ayuda) pasó el review rechazado dentro de los subcomandos (M18).
    #[test]
    fn every_short_option_in_the_help_is_known() {
        let cortas = help_short_options(&help_text());
        assert!(
            !cortas.is_empty(),
            "el barrido no ha encontrado ni -h ni -o"
        );
        for corta in cortas {
            if let Some((_, largo)) = SHORT_ALIASES.iter().find(|(c, _)| *c == corta) {
                // Un alias tiene que apuntar a una opción que exista.
                assert!(
                    PACK_VALUES.contains(largo) || PACK_FLAGS.contains(largo),
                    "la ayuda promete -{corta} como alias de --{largo}, que el parser no conoce"
                );
                continue;
            }
            let known = PACK_VALUES.contains(&corta.as_str())
                || PACK_FLAGS.contains(&corta.as_str())
                || DECRYPT_VALUES.contains(&corta.as_str())
                || DECRYPT_FLAGS.contains(&corta.as_str());
            assert!(
                known,
                "la ayuda documenta -{corta} y el parser no la conoce"
            );
        }
    }

    #[test]
    fn help_documents_the_packing_options() {
        let help = help_text();
        for opt in [
            "input",
            "output",
            "sheet",
            "data",
            "format",
            "texture-format",
            "trim-mode",
            "height",
            "webp-quality",
            "tolerance",
            "template",
            "no-trim",
            "no-auto-animations",
            "version",
            "quiet",
            "verbose",
            "exporter-list",
            "common-divisor-x",
            "default-pivot-point",
            "force-identical-layout",
            "trim-sprite-names",
            "enable-rotation",
            "pack-normalmaps",
            "max-width",
            "max-height",
            "background-color",
            "dpi",
            "ignore-files",
            "replace",
            "scale",
            "save",
            "heuristic-mask",
            "force-publish",
            "custom-exporters-directory",
            "convert-texture",
            "css-sprite-prefix",
            "css-media-query-2x",
            "plain-string-property",
            "plain-bool-property",
            "disable-rotation",
            "enable-cache-busting",
        ] {
            assert!(
                help.contains(&format!("--{opt}")),
                "falta --{opt} en la ayuda"
            );
        }
    }
}
