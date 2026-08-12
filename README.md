# TexturePacker-RS 🧩

Una aplicación de escritorio **en Rust** para generar atlas de texturas con todas
las funcionalidades de TexturePacker, implementada según el documento de
especificación técnica *"Engine TexturePacker-RS"* (entrada: CLI / GUI / archivo
de proyecto; pipeline: ingesta → polígonos → empaquetado → VRAM → exportación).

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
| `tp-app`     | App de escritorio (egui/eframe): editor de configuración, preview del atlas, mallas, pivots, log. |
| `tp-cli`     | Interfaz de línea de comandos para headless/CI.                          |

## Compilar y ejecutar

Requiere Rust ≥ 1.93 y un compilador C/C++ solo si se habilita ASTC.

```bash
# App de escritorio
cargo run -p tp-app

# CLI
cargo run -p tp-cli -- --help

# Con soporte ASTC (compila ARM astcenc desde fuente; ~1-2 min extra)
cargo build --features gpu-formats

# Tests
cargo test --workspace
```

## Uso rápido (GUI)

1. Pulsa **«Cargar proyecto»** o rellena **Directorio de entrada** (carpeta con PNG/WebP/JPEG).
2. Ajusta tamaño de atlas, padding/extrude, rotación, recorte, polígonos, profundidad de color, formato y cifrado.
3. Pulsa **«📦 Empaquetar»**. El resultado aparece en las pestañas *Atlas* (con marcos, pivots y zoom),
   *Sprites*, *Malla*, *Pivots* (editable), *Salida* y *Log*.
4. Guarda la configuración como `.tpproj` para reutilizarla.

## Uso rápido (CLI)

```bash
# Proyecto guardado
tp-cli pack proyecto.tpproj

# Todo por parámetros
tp-cli pack --input sprites/ --output build/ \
    --max-size 4096 --padding 2 --extrude 1 --format etc2 \
    --strategy bssf --variants 1.0,0.5 --key secreto --template-format json

# Descifrar una textura cifrada
tp-cli decrypt build/atlas_0.png.tpenc --key secreto -o atlas.png
```

## Cobertura de la especificación

| Módulo de la especificación                    | Estado | Detalles |
|------------------------------------------------|--------|----------|
| Carga paralela de imágenes                     | ✅     | Rayon; PNG/WebP/JPEG |
| Alpha trimming + bounding box                  | ✅     | Umbral configurable (0-255) |
| Deduplicación por hashing (aliases)            | ✅     | xxh3 + comparación byte-exacta; los aliases no ocupan espacio |
| Auto-downscaling (@2x/@1x)                     | ✅     | `scale_variants` (p. ej. `1.0, 0.5`) con metadatos escalados |
| Marching Squares + ambigüedad                  | ✅     | Resuelta con análisis de conectividad 8-dir |
| Ramer–Douglas–Peucker                          | ✅     | Tolerancia en píxeles |
| Triangulación Earcut                           | ✅     | Contornos exteriores; agujeros detectados (contorno `is_hole`) |
| MaxRects BSSF / BAF / BLSF                     | ✅     | Seleccionable |
| Guillotine                                     | ✅     | Seleccionable |
| Rotación 90°                                   | ✅     | Píxeles y mallas rotados coherentemente |
| Multi-atlas auto-split                         | ✅     | Nueva página cuando no cabe; límite VRAM |
| Encaje poligonal por bitmap / raster grid      | ✅     | Occupancy grid por página |
| Padding & Extrude                              | ✅     | Anti-bleeding |
| Pivots                                         | ✅     | Por defecto + `pivots.json` + edición en GUI |
| Co-packing de normal maps                      | ✅     | `*_normal.png` en el mismo frame/página/rotación |
| Cuantización (RGBA4444/RGB565)                 | ✅     | Aplicada a toda la página |
| Dithering (Floyd–Steinberg / Atkinson)         | ✅     | Error diffusion por canal |
| PNG / WebP                                     | ✅     | PNG y WebP lossless |
| ASTC 4x4                                       | ✅*    | *Con `--features gpu-formats` (ARM astcenc oficial) |
| ETC2 RGBA (EAC + ETC2)                         | ✅     | Encoder propio validado contra decodificador independiente |
| PVRTC 4BPP                                     | ✅     | Encoder propio PVRTC1 4bpp (block-fit + búsqueda de modulación) en `.pvr` v3, validado contra `texture2ddecoder` |
| Cifrado simétrico de textura (AES-GCM)         | ✅     | AES-256-GCM; archivos `*.tpenc`, `tp-cli decrypt` |
| Motor de plantillas (Mustache)                 | ✅     | handlebars; JSON, XML (libgdx), Plist (cocos2d), C++ header, TSV, texto + plantillas personalizadas |

## Formato de proyecto (`.tpproj`)

TOML con los campos de `ProjectConfig` (ver `crates/tp-core/src/config.rs`):

```toml
input_directory = "sprites"
output_directory = "build"
max_texture_size = 2048
padding = 2
extrude = 1
allow_rotation = true
enable_trim = true
trim_threshold = 1
enable_polygon = false
polygon_tolerance = 1.5
enable_aliasing = true
color_depth = "RGBA8888"
dithering_algorithm = "FloydSteinberg"
gpu_format = "PNG"
encryption_key = "mi-clave"
template_format = "JSON"
packing_strategy = "BSSF"
scale_variants = [1.0, 0.5]
enable_normal_maps = true
```

## Metadatos

El JSON generado sigue la convención de TexturePacker (filosofía `meta`/`frames`):

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
      "page": 0,
      "aliased": false,
      "polygon": null,
      "mesh": null
    }
  ]
}
```

En modo polígono, cada frame incluye `polygon` (puntos del contorno) y `mesh`
(`vertices`, `indices`, `uvs`) en coordenadas locales del sprite recortado.

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

## Pruebas

`cargo test --workspace` ejecuta 68 tests: algoritmos (trim, hash, pack, earcut,
dithering, cuantización), round-trip de ETC2 (5 modos + EAC) y PVRTC 4BPP
(opaco, gradientes, transparencia y layout morton) contra un decodificador
independiente (`texture2ddecoder`), cifrado/descifrado, plantillas, y 5
pruebas end-to-end que generan sprites reales en disco y verifican aliases,
rotación, normal maps, variantes, multi-atlas y los tres formatos GPU
(ETC2/PVRTC/ASTC).

---

Hecho con 🦀 · Licencia MIT
