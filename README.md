# TexturePacker-RS 🧩

Una aplicación de escritorio **en Rust** para generar atlas de texturas,
desarrollada de forma independiente (entrada: CLI / GUI / archivo de proyecto;
pipeline: ingesta → polígonos → empaquetado → VRAM → exportación).

```
Capa de Entrada (CLI / GUI / .tpproj)
        │
        ▼
1. Ingesta & Preprocesamiento   (carga paralela, trimming, hashing, aliases, variantes)
        │
        ▼
2. Motor Geométrico & Mallas    (Marching Squares → RDP → Earcut)
        │
        ▼
3. Empaquetado Espacial         (MaxRects BSSF/BAF/BLSF, Guillotine, rotación 90°, multi-atlas)
        │
        ▼
4. Procesamiento de Píxeles     (extrude, rotación, normal maps, cuantización, dithering)
        │
        ▼
5. Exportación & Cifrado        (PNG/WebP/ASTC/ETC2, AES-256-GCM, plantillas Mustache)
```

## Componentes

| Crate        | Descripción                                                              |
|--------------|--------------------------------------------------------------------------|
| `tp-core`    | Motor: pipeline completo, paralelo con Rayon, sin estado global mutable. |
| `tp-app`     | App de escritorio (egui/eframe): árbol de sprites, preview del atlas con zoom, ajustes (básicos/avanzados), pestañas Log/Salida/Sprites/Malla, pivots. |
| `tp-cli`     | Interfaz de línea de comandos para headless/CI.                          |

## Compilar y ejecutar

Requiere Rust ≥ 1.93 y un compilador C/C++ solo si se habilita ASTC.

```bash
# App de escritorio
cargo run -p tp-app

# Abre un proyecto directamente
cargo run -p tp-app -- ruta/al/proyecto.tpproj

# CLI
cargo run -p tp-cli -- --help

# Con soporte ASTC (compila ARM astcenc desde fuente; ~1-2 min extra)
cargo build --features gpu-formats

# Tests
cargo test --workspace
```

## Uso rápido (GUI)

La ventana organiza el flujo de trabajo en cuatro zonas:

| Zona | Contenido |
|------|-----------|
| Barra superior | **Abrir** / **Guardar** / **↺** (restablecer), **➕** añadir sprites, **➖** quitar seleccionados, **Carpeta** (carpeta inteligente), **⚙** ajustes de sprite (pivots y bordes 9-patch), **Publicar**, **✂** (dividir hoja) y **▶** (vista previa de animación), ruta del proyecto |
| Panel izquierdo | Árbol **Sprites**: carpetas y archivos; selección simple o múltiple (Ctrl/Shift), arrastrar y soltar, «Restaurar (N)» para deshacer exclusiones |
| Centro | Vista previa del atlas: zoom (−/slider/+/1:1/Ajustar), contornos, pivots, **bordes 9-patch** (barras verdes **arrastrables** en el sprite seleccionado), selección de página y de sprite; debajo, las pestañas **Log**, **Salida**, **Sprites** y **Malla** |
| Panel derecho | **Ajustes**: Datos (directorios, nombre base con placeholders `{n}`/`{n1}`/`{v}`, formato de metadatos, quitar extensión de los nombres, anteponer carpeta de la carpeta inteligente, ruta de la textura en los metadatos), Composición (tamaño, **multipack**, padding, **padding de borde**, **divisor común**, **alinear a rejilla**, extrude, rotación, recorte, **algoritmo/heurística/modo de empaquetado**, **restricción de tamaño, tamaño fijo y atlas cuadrado**), Procesamiento (color, dithering, **transparencia/alpha handling**, **escalado de variantes**, formato de salida) y, con el interruptor **Avanzados**, polígonos, alias, variantes, cifrado y plantillas |
| Ventanas flotantes | **✂ Dividir hoja**: elige una hoja, corta en rejilla (columnas × filas) o por tamaño fijo con margen/espaciado, previsualiza la rejilla y escribe los PNG, los añade al proyecto y publica. **▶ Vista previa de animación**: grupos por nombre de archivo, FPS, repetir, transporte (⏮ ⏸ ⏭), fondo (damas/alfa/negro), escala y sprites rotados |

1. Añade sprites con **➕** / **Carpeta** o rellena **Directorio de entrada**
   (PNG, WebP, JPEG, TGA, BMP, GIF, ICO, TIFF, DDS y QOI).
2. Ajusta tamaño de atlas, padding/extrude, rotación, recorte, polígonos, profundidad de color, formato y cifrado.
3. Pulsa **«Publicar»**: la vista previa muestra el atlas con marcos, pivots, bordes 9-patch y zoom automático,
   y la pestaña *Log* los tiempos de cada etapa.
4. Edita pivots y bordes 9-patch con **⚙** (se guardan en `pivots.json` y `borders.json` junto a
   los sprites) y guarda la configuración con **Guardar** (`.tpproj`) para reutilizarla.

## Uso rápido (CLI)

```bash
# Proyecto guardado
tp-cli pack proyecto.tpproj

# Todo por parámetros
tp-cli pack --input sprites/ --output build/ \
    --max-size 4096 --padding 2 --extrude 1 --format etc2 \
    --strategy bssf --variants 1.0,0.5 --key secreto --template-format json

# Ajustes de textura: borde, divisor, rejilla, transparencia y ruta
tp-cli pack --input sprites/ --output build/ \
    --border-padding 8 --common-divisor 4 --align 4 \
    --alpha-handling premultiply --scale-mode fast \
    --texture-path /assets --keep-extension --prepend-folder-name \
    --dither floyd-alpha

# Empaquetado: algoritmo, heurística, tamaño mínimo y restricciones
tp-cli pack --input sprites/ --output build/ \
    --algorithm basic --basic-sort-by name --basic-order descending \
    --pack-mode best --size-constraints pot --force-squared

# Multipack: placeholders {n}/{n0} (desde 0), {n1} (desde 1) y {v}
tp-cli pack --input sprites/ --output build/ --max-size 1024 \
    --base-name 'hoja{n1}{v}' --variants 1.0,0.5
#   → hoja1.png, hoja1.json, hoja2.png, hoja2.json, hoja1-hd.png, ...
tp-cli pack --input sprites/ --output build/ --no-multipack   # falla si no cabe en una hoja

# Descifrar una textura cifrada
tp-cli decrypt build/atlas_0.png.tpenc --key secreto -o atlas.png
```

## Cobertura de la especificación

| Módulo de la especificación                    | Estado | Detalles |
|------------------------------------------------|--------|----------|
| Carga paralela de imágenes                     | ✅     | Rayon; PNG, WebP, JPEG, TGA, BMP, GIF, ICO, TIFF, DDS y QOI |
| Alpha trimming + bounding box                  | ✅     | Umbral configurable (0-255) |
| Deduplicación por hashing (aliases)            | ✅     | xxh3 + comparación byte-exacta; los aliases no ocupan espacio |
| Auto-downscaling (@2x/@1x)                     | ✅     | `scale_variants` (p. ej. `1.0, 0.5`) con metadatos escalados |
| Marching Squares + ambigüedad                  | ✅     | Resuelta con análisis de conectividad 8-dir |
| Ramer–Douglas–Peucker                          | ✅     | Tolerancia en píxeles |
| Triangulación Earcut                           | ✅     | Contornos exteriores; agujeros detectados (contorno `is_hole`) |
| Algorithm → Polygon                            | ✅     | El *Trim mode Polygon* cambia el algoritmo a *Polygon* automáticamente (mallas + empaquetado por contorno) |
| MaxRects BSSF / BAF / BLSF                     | ✅     | Seleccionable |
| Guillotine                                     | ✅     | Seleccionable |
| Grid / Basic                                   | ✅     | Rejilla (celda = mayor sprite inflado) y filas de izquierda a derecha con `basic_sort_by`/`basic_order` |
| Heurísticas Best / BottomLeft / ContactPoint   | ✅     | `Best` prueba las 5 heurísticas y se queda con el empaquetado más ajustado |
| Pack mode Fast / Good / Best                   | ✅     | Búsqueda binaria del atlas mínimo (presupuesto de 400 ms / 3 s); Fast solo recorta |
| Size constraints (AnySize/POT/Múltiplo4/Palabra) | ✅  | El recorte y la búsqueda respetan la restricción sin superar `max_texture_size`; WordAligned usa el ancho de palabra de `color_depth` |
| Tamaño fijo y atlas cuadrado                   | ✅     | `fixed_width`/`fixed_height` (0 = automático) y `force_squared` |
| Rotación 90°                                   | ✅     | Píxeles y mallas rotados coherentemente |
| Multi-atlas auto-split                         | ✅     | Nueva página cuando no cabe; límite VRAM |
| Multipack on/off                               | ✅     | `multipack` (por defecto activo; GUI + `--no-multipack`); si está desactivado y no cabe en una hoja, error claro |
| Placeholders multipack                         | ✅     | `{n}`/`{n0}` (índice desde 0), `{n1}` (desde 1) y `{v}` (sufijo de variante) en el nombre base; cada hoja escribe su data file; aviso si faltan |
| Encaje poligonal por bitmap / raster grid      | ✅     | Occupancy grid por página |
| Padding & Extrude                              | ✅     | Anti-bleeding |
| Border padding                                 | ✅     | Margen entre sprites y borde del atlas; se redondea con la rejilla |
| Common divisor (x/y)                           | ✅     | Los sprites se estiran (con transparencia) hasta ser divisibles; LCM con la rejilla |
| Align to grid (0 = off)                        | ✅     | Padding, borde y posiciones redondeados al múltiplo; aviso en la GUI |
| Transparencia (alpha handling)                 | ✅     | keep / clear / reduce-border-artifacts (bleed) / premultiply-alpha; paso propio del pipeline |
| Escalado de variantes                          | ✅     | smooth (bilineal) o fast (vecino más cercano) |
| Ruta de la textura en los metadatos            | ✅     | Prefijo aplicado a `meta.image` y a `pages[].file` (`--texture-path`) |
| Nombres de sprite                              | ✅     | Ids relativos con subcarpeta; extensión opcional (`trim_sprite_names`) y carpeta inteligente opcional |
| Pivots                                         | ✅     | Por defecto + `pivots.json` + edición en GUI |
| Bordes 9-patch / 3-patch                    | ✅     | `[izq, arriba, der, abajo]` en píxeles vía `borders.json`, editor en la GUI (presets 9-patch/3-patch), **detección automática** de filas/columnas de color sólido (botón «🔎 Detectar»), guías verdes arrastrables con el ratón en la vista previa y metadato `border` por frame |
| Co-packing de normal maps                      | ✅     | `*_normal.png` en el mismo frame/página/rotación |
| Cuantización (RGBA4444/RGB565)                 | ✅     | Aplicada a toda la página |
| Pixel format RGBA5551 / RGBA5555 / BGRA8888    | ✅     | `pixel_format = "RGBA5551"` cuantiza a la rejilla 5-5-5-1 (expansión por replicación de bits, alfa 0/255); `"RGBA5555"` (20 bits) mantiene el alfa también en 5 bits; `"BGRA8888"` intercambia R/B en el archivo |
| Dithering (Floyd–Steinberg / Atkinson)         | ✅     | Error diffusion por canal; variantes `*-Alpha` difunden también al alfa |
| PNG / WebP                                     | ✅     | PNG y WebP lossless |
| ASTC 4x4                                       | ✅*    | *Con `--features gpu-formats` (ARM astcenc oficial) |
| ETC2 RGBA (EAC + ETC2)                         | ✅     | Encoder propio validado contra decodificador independiente |
| PVRTC 4BPP                                     | ✅     | Encoder propio PVRTC1 4bpp (block-fit + búsqueda de modulación) en `.pvr` v3, validado contra `texture2ddecoder` |
| Cifrado simétrico de textura (AES-GCM)         | ✅     | AES-256-GCM; archivos `*.tpenc`, `tp-cli decrypt` |
| Motor de plantillas (Mustache)                 | ✅     | handlebars; JSON, XML (libgdx), Plist (cocos2d), C++ header, TSV, texto + plantillas personalizadas |
| Auto-detect animations                         | ✅     | `walk_001..walk_003` se agrupan como animación `walk` en `meta.animations` (JSON) y en la sección `animations` del Plist; desactivable (`enable_auto_detect_animations`, `--no-auto-animations`) |

## Formato de proyecto (`.tpproj`)

TOML con los campos de `ProjectConfig` (ver `crates/tp-core/src/config.rs`):

```toml
input_directory = "sprites"
output_directory = "build"
max_texture_size = 2048
padding = 2                 # espacio ENTRE sprites
border_padding = 8          # sprites ↔ borde de la hoja
common_divisor_x = 4        # estira hasta ser divisible entre 4 (x)
common_divisor_y = 4        # ... y entre 4 (y); 0 = sin divisor
align_to_grid = 4           # alinea esquinas/padding/borde a múltiplos de 4; 0 = off
extrude = 1
allow_rotation = true
enable_trim = true
trim_threshold = 1
trim_sprite_names = true    # false = los ids conservan la extensión
prepend_folder_name = false # anteponer el nombre de la carpeta inteligente
alpha_handling = "Keep"     # Keep | Clear | ReduceBorderArtifacts | PremultiplyAlpha
scale_mode = "Smooth"       # Smooth (bilineal) | Fast (vecino más cercano)
texture_path = "/assets"    # prefijo de meta.image / pages[].file (opcional)
enable_polygon = false
polygon_tolerance = 1.5
enable_aliasing = true
color_depth = "RGBA8888"
dithering_algorithm = "FloydSteinberg"
gpu_format = "PNG"
encryption_key = "mi-clave"
template_format = "JSON"
packing_strategy = "BSSF"
algorithm = "MaxRects"       # MaxRects | Polygon | Guillotine | Grid | Basic
pack_mode = "Good"           # Fast | Good | Best (búsqueda del tamaño mínimo)
size_constraints = "AnySize" # AnySize | POT | MultipleOf4 | WordAligned
force_squared = false        # atlas cuadrado
fixed_width = 0              # ancho fijo (0 = automático)
fixed_height = 0             # alto fijo (0 = automático)
basic_sort_by = "Best"       # Best | Name | Width | Height | Area | Circumference
basic_order = "Ascending"    # Ascending | Descending (algoritmo Basic)
detect_border_tolerance = 0  # tolerancia por canal al detectar bordes 9-patch
detect_border_max_search = 64 # filas/columnas inspeccionadas por lado (0 = sin límite)
scale_variants = [1.0, 0.5]
variant_names = []           # p. ej. [[1.0, "-ipadhd"], [0.5, "-hd"]]
enable_normal_maps = true
base_file_name = "hoja{n1}" # placeholders {n}/{n0} (desde 0), {n1} (desde 1) y {v}
multipack = true            # false = error si los sprites no caben en una sola hoja
enable_auto_detect_animations = true  # walk_001..N → animación "walk" en los metadatos
```

## Metadatos

El JSON generado organiza la información en dos bloques, `meta` y `frames`:

```json
{
  "meta": { "app": "TexturePacker-RS", "image": "atlas.png", "size": {"w": 2048, "h": 2048}, "scale": "1" },
  "frames": [
    {
      "filename": "hero",
      "frame": {"x": 2, "y": 2, "w": 64, "h": 64},
      "rotated": false,
      "trimmed": true,
      "spriteSourceSize": {"x": 0, "y": 0, "w": 60, "h": 60},
      "sourceSize": {"w": 64, "h": 64},
      "pivot": {"x": 0.5, "y": 0.5},
      "border": {"left": 8, "top": 8, "right": 8, "bottom": 8},
      "page": 0,
      "aliased": false,
      "polygon": null,
      "mesh": null
    }
  ],
  "animations": [
    { "name": "walk", "frames": ["walk_001", "walk_002", "walk_003"] }
  ]
}
```

En modo polígono, cada frame incluye `polygon` (puntos del contorno) y `mesh`
(`vertices`, `indices`, `uvs`) en coordenadas locales del sprite recortado.

### Pivots y bordes 9-patch por sprite

Junto a los sprites pueden convivir dos archivos JSON opcionales:

```json
// pivots.json — pivot normalizado 0..1 por sprite
{ "hero": {"x": 0.5, "y": 1.0} }

// borders.json — bordes 9-patch [izquierda, arriba, derecha, abajo] en píxeles
// de la imagen original (medidos sin recortar)
{ "panel": [8, 8, 8, 8] }
```

La GUI los escribe con **💾 Guardar pivots** de la ventana de ajustes de sprite
(un borde con los cuatro valores a 0 se considera «sin 9-patch» y no se guarda).
En los metadatos el borde aparece como `border` con las cuatro caras; un sprite
con las dos barras horizontales o verticales a 0 equivale a un 3-patch.

Con un placeholder de página (`{n}`/`{n0}`/`{n1}`) en el nombre base, cada hoja
escribe su propio data file (`hoja1.json`, `hoja2.json`, …) con solo sus frames,
`meta.image` de esa hoja y `meta.pages` de un único elemento; sin placeholder se
emite un único fichero con todas las páginas y el campo `page` de cada frame
indica a cuál pertenece.

## Cifrado

`encryption_key` cifra los **bytes completos** de cada archivo de imagen con
AES-256-GCM (clave derivada por SHA-256 de la frase). Formato del archivo:

```
"TPENC1" (6 bytes) | nonce (12 bytes) | ciphertext + tag GCM
```

## Limitaciones conocidas

- **PVRTC 4BPP**: encoder propio PVRTC1 4bpp (`.pvr` v3). Calidad tipo
  *fast/medium* (ajuste de 2 colores por bloque + búsqueda exhaustiva de
  modulación por texel), no iterativa como PVRTexTool HQ. Requiere atlas con
  dimensiones potencia de dos; un `scale_variant` no-potencia-de-2 (p. ej.
  0.75) devuelve un error claro. PVRTC interpola toroidalmente: los bordes
  del atlas mezclan color con el borde opuesto, así que deja margen alrededor
  de los sprites del borde si notas bleeding.
- **ETC2**: encoder propio con los **5 modos** (individual, differential,
  T, H y planar) + EAC para alpha. Selección automática del mejor modo por
  bloque; los layouts de bits replican el stuffing de ETCPACK (referencia
  oficial de Ericsson) y se validan contra `texture2ddecoder`
  (individual/differential/T) y decoders propios del layout ETCPACK (H/planar).
  Para máxima calidad usa ASTC.
- La malla poligonal no rellena los agujeros (el agujero es transparente en el
  atlas, por lo que no produce artefactos visibles).
- Los formatos de entrada BMP/GIF/ICO/TIFF/DDS heredan las limitaciones de los
  decodificadores de la crate `image`; los GIF animados se leen como su primer
  fotograma.
- **Formatos de salida**: escribe PNG, PNG8, JPG, WebP y los contenedores de
  hardware ASTC/ETC2(KTX)/PVRTC(.pvr); no escribe BMP/TGA/TIFF/DDS/PVR-gz/
  KTX2/Basis.
- **Heurísticas y formatos de píxel**: el motor implementa el subconjunto de la
  especificación técnica del proyecto (RGBA8888/4444/565, RGBA5551/5555,
  BGRA8888, RGB888, ALPHA/INTENSITY, ASTC 4x4, ETC2 RGBA, PVRTC1 4bpp); otros
  formatos (ETC1, DXT, ASTC de otros tamaños de bloque, Basis Universal, ...)
  no están incluidos.

## Pruebas

`cargo test --workspace` ejecuta 162 tests (entre ellos el del tipo de error
`TpError`, con mensajes en español): algoritmos (trim, hash, pack, earcut,
dithering, cuantización, alpha handling, escalado), **empaquetado del Lote 6**
(algoritmos Grid/Basic, heurísticas Best/BottomLeft/ContactPoint, restricciones
de tamaño POT/múltiplo 4/palabra, tamaño fijo, force squared y búsqueda
Fast/Good/Best), **multipack del Lote 7** (expansión de placeholders
`{n}`/`{n0}`/`{n1}`/`{v}`, extensión sin duplicar, data file por hoja, aviso
cuando faltan placeholders y error con `multipack = false`), round-trip de ETC2 (5 modos
+ EAC) y PVRTC 4BPP (opaco, gradientes, transparencia y layout morton) contra un
decodificador independiente (`texture2ddecoder`), cifrado/descifrado, plantillas,
configuración (round-trip TOML de los ajustes nuevos), CLI (flags y parseo de
argumentos), y 17 pruebas end-to-end que generan sprites reales en disco y
verifican aliases, rotación, normal maps, variantes, multi-atlas, los tres
formatos GPU (ETC2/PVRTC/ASTC), los ajustes de Lote 5 (padding de borde,
divisor común, nombres con subcarpeta/extensión, `texture_path`, premultiply y
alineación a rejilla), los del Lote 6 (Grid con POT y tamaño fijo con Basic) y
los del Lote 7 (hojas nombradas con `{n1}`, un data file por hoja con solo sus
frames, nomenclatura implícita con aviso, variantes `{v}` y multipack
desactivado), además de los del **Lote 8** (JPG/PNG8/WebP y formatos de píxel,
flip vertical solo en formatos GPU e ingesta de TGA/BMP/QOI con deduplicación
por hash) y del **Lote 9** (bordes 9-patch desde `borders.json` hasta los
metadatos `border` del JSON).

## Licencia y marcas

**Licencia.** TexturePacker-RS se distribuye bajo los términos de la **licencia
MIT**: puedes usarlo, estudiarlo, modificarlo y redistribuirlo libremente, en
proyectos personales o comerciales. Las dependencias open source declaradas en
`Cargo.toml` conservan sus propias licencias.

**Proyecto independiente.** TexturePacker-RS es una herramienta original
escrita en Rust, sin afiliación, patrocinio, colaboración ni respaldo de ningún
proveedor de software comercial. Las técnicas que implementa (recorte de
transparencias, padding, empaquetado en varias hojas, sufijos de variantes,
formatos comprimidos de GPU) son convenciones generales del ecosistema de
desarrollo de videojuegos y no pertenecen a nadie en particular.

**Marcas.** Los nombres de productos o empresas que puedan aparecer en esta
documentación se citan, en su caso, únicamente con fines de interoperabilidad
e identificación técnica; cada uno pertenece a su respectivo titular, con el
que este proyecto no mantiene vínculo alguno. El nombre *TexturePacker-RS*
describe el propósito de la herramienta (empaquetar texturas, en Rust); si
algún titular de marca considera que su uso induce a confusión, puede abrir
una issue en el repositorio para resolverlo.

---

Hecho con 🦀 · Licencia MIT
