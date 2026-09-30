# Comparativa: TexturePacker original vs. este clon

Auditoría de funcionalidades: qué ofrece el TexturePacker original y qué hay en este
repositorio, con especial atención a lo que **solo está de adorno** (la UI lo promete y
el código no lo cumple) o **apenas está implementado**.

Fuentes del original:
<https://www.codeandweb.com/texturepacker/documentation>,
`/documentation/user-interface-overview`,
`/documentation/texture-settings`.

Cada afirmación está verificada contra el código (`file:line`), no contra la intención.

---

## 1) De adorno — la UI lo promete, el código no lo cumple

Las filas marcadas **[RESUELTO]** ya están corregidas; el detalle, en «Orden de
corrección» al final.

| Lo que ofrece | Qué pasa realmente |
| --- | --- |
| **Algoritmo "Guillotine"** y heurística "Guillotine (legacy)" (`crates/tp-app/src/app/settings.rs:277,305`) **[RESUELTO]** | Ahora hay un repartidor de guillotina real: `pack_guillotine` + `split_guillotine` en `crates/tp-core/src/pack.rs`, seleccionado desde `place_all` cuando `algorithm == Guillotine`. La etiqueta ya no promete el heurístico "legacy" oculto. |
| **Pivots y bordes 9-patch editados en la GUI** (`crates/tp-app/src/app/sprite_settings.rs:62-105`) **[RESUELTO]** | La GUI escribe `pivot_overrides` / `border_overrides` en el `.tpproj` (`crates/tp-core/src/config.rs`) y `pipeline::run_grouped` los aplica con precedencia sobre sidecar y `default_pivot`, así que llegan al atlas y al fichero publicado sin botón aparte. |
| **"▶ Animación: vista previa de animación de los sprites seleccionados"** (`crates/tp-app/src/app/toolbar.rs:93`) **[RESUELTO]** | `collect_frames` (`crates/tp-app/src/app/animation.rs`) filtra por `selected_paths` cuando hay selección; sin selección reproduce todos. El grupo elegido se recalcula con `effective_group` cuando la selección lo deja fuera. |
| **"Align to grid"** (`align_to_grid`) **[RESUELTO]** | Ahora el packer sube cada origen de frame al múltiplo (`snap_pos` en `crates/tp-core/src/pack.rs`, aplicado en MaxRects/Guillotine/Polygon, Grid, Basic y Manual) y `align_to_grid` dejó de entrar en `effective_divisors`: a la par que el original, alinea **sin estirar** los sprites (solo el *common divisor* los estira). Tests `align_to_grid_snaps_every_corner_without_stretching` (8 casos) y `lote5_align_to_grid_rounds_padding_and_positions`. |
| **Hoja principal: "← Selección" / arrastrar al nodo `(hoja principal)`** **[RESUELTO]** | `run_groups` (`crates/tp-core/src/pipeline.rs`) lleva un `owner` por id: el primer grupo que lista un sprite es su dueño y el grupo por defecto recoge los suyos propios y los no listados, así que asignar un sprite ya no lo saca del atlas. |
| **Pestaña "Archivos" del panel inferior** **[RESUELTO]** | La vista previa lista ahora **exactamente** los ficheros que escribirá la publicación — hojas, normales y metadatos, con el `.tpenc` cuando hay cifrado (`execute` en `crates/tp-core/src/pipeline.rs`) — y la pestaña distingue «Archivos que se publicarán» de «Archivos generados» según `App::files_written`. Test `preview_lists_exactly_the_files_publish_writes`. |

## 2) Parciales — existen, pero muy por debajo del original

- **Exportadores** **[PARCIAL]**: 6 plantillas nativas (JSON, XML, Plist, C++ header, TSV,
  PlainText — `crates/tp-core/src/config.rs:621-630`) + plantilla Mustache propia, frente a
  los 60+ del original, y sigue sin existir el botón de conversión de formato de datos.
  **Los ficheros extra por framework ya están**: `class_file`, `header_file`, `source_file`
  y `spriteids_file` en el `.tpproj` (vacío = no escribir), generados por
  `templates::extra_files` (`crates/tp-core/src/templates.rs`) — ids C++/Swift saneados
  (ruta y signos → `_`, dígito inicial con prefijo `_`, colisiones con `_` extra), cabecera
  con guard `ATLAS_SPRITES_H`, fuente con `#include` de la cabecera configurada o
  `<ns>.h`, enum Swift `public enum` con `public static let` y lista de ids una por línea —,
  escritos junto a los metadatos y anunciados en la vista previa (pestaña «Archivos»), con
  sección «Ficheros extra por framework» en la GUI y flags `--class-file`, `--header-file`,
  `--source-file` y `--spriteids-file`. Tests: `extra_files_generate_cpp_swift_and_id_list`,
  `extra_data_file_settings_roundtrip_and_default_empty`, `extra_data_file_flags_parse` y el
  e2e `lote10_extra_data_files_export`.
- **Formatos de textura de salida** **[PARCIAL]**: 16 de los 19 del original. A PNG/PNG8/
  JPG/WebP/ETC2/PVRTC se les han sumado **BMP, TGA y TIFF** (codificador de `image`),
  **DDS** (cabecera legacy de 124 bytes con máscaras de canal y payload crudo),
  **ZKTX** (KTX v1 en zlib), **PVR3GZ y PVR3CCZ** (el PVR3 de PVRTC en gzip y en el
  contenedor CCZ de Cocos2D) y **ETC1** en contenedor PKM (`.pkm`) y en KTX
  (`glInternalFormat 0x8D60`), con un codificador ETC1 propio que reutiliza los modos
  individual + diferencial de `etc2.rs` y descarta T/H/planar (son extensiones ETC2, y
  un bloque así se leería mal en hardware ETC1). Compresión con `flate2`, variantes en
  `GpuFormat` (+ `GpuFormat::parse` para el CLI), combo de «Formato de publicación»,
  flags `--format bmp|tga|tiff|dds|zktx|pvr3gz|pvr3ccz|pkm|ktx`. Tests:
  `bmp_tga_tiff_roundtrip_the_rgba_pixels`, `dds_writes_a_valid_legacy_header_and_raw_pixels`,
  `zktx_is_a_zlib_compressed_ktx`, `pvr3_gz_and_ccz_wrap_the_pvr3_file`,
  `etc1_pkm_and_ktx_decode_close_to_the_source`, `output_format_tokens_and_extensions`
  y el e2e `lote9_software_and_container_formats_export`. Siguen sin existir **`ktx2`**
  (contenedor con su DFD obligatorio) y **`basis`** (necesita un codificador Basis
  Universal externo, como astcenc lo fue para ASTC).
- **ASTC** **[RESUELTO]**: `tp-app` y `tp-cli` activan `tp-core/gpu-formats` en su
  `Cargo.toml`, así que `GpuFormat::is_supported()` es `true`, astcenc se compila y el
  test `etc2_pvrtc_and_astc_export_paths` exporta `atlas.astc` de verdad. La advertencia
  de `settings.rs:766` sigue ahí para construcciones sin la feature.
- **Scaling variants** **[RESUELTO]**: además del campo de escalas y `variant_names`, cada
  variante tiene `variant_options` (`crates/tp-core/src/config.rs`): filtro de sprites con
  comodines, `max_texture_size` propio y `force_identical_layout`. `plan_variants`
  (`crates/tp-core/src/pipeline.rs`) decide qué variantes reescalan la hoja base y cuáles
  empaquetan por su cuenta (con su filtro y su tope), y la GUI lo edita en «Opciones por
  variante» (`crates/tp-app/src/app/settings.rs`). Siguen sin existir los presets del
  original y «accept fractional values».
- **Dithering** **[RESUELTO]**: los 9 `--dither-type` del original están cubiertos:
  `None`, `NearestNeighbour` (redondeo por píxel sin difusión) y `Linear` (el error
  viaja al píxel de la derecha, distribución lineal con más contraste) en
  `DitheringAlgorithm` (`crates/tp-core/src/config.rs`), Floyd–Steinberg/Atkinson con y
  sin alfa como ya estaban, y los tres `PngQuantLow/Medium/High` en `png8_dither`
  (`PngDither`, solo PNG-8, igual que en el original). GUI, CLI (`--dither nn|linear|…`)
  y `pixels::distribute_error` cubren los nuevos.
- **Scale mode** **[PARCIAL]**: ahora hay 6 de los 7 del original — `Smooth`, `Fast`,
  `Scale2x`, `Scale3x`, `Scale4x` (= Scale2x dos veces) y `Eagle`
  (`crates/tp-core/src/config.rs` → `scale_rgba` en `crates/tp-core/src/export.rs`).
  Los modos de pixel art solo corren en su factor entero exacto y, si la variante pide
  otra escala, caen a `Smooth` con un aviso (`pipeline::execute` y los avisos de la
  GUI). Sigue faltando **`Hq2x`**: su tabla de 256 patrones solo existe en
  implementaciones LGPL 2.1 (el algoritmo original y el crate `hqx`), así que **se ha
  decidido omitirlo** para no meter código LGPL en un proyecto MIT.
- **Normal maps** **[RESUELTO]**: sufijo, filtro de ruta y detección por color son
  configurables (`normal_map_suffix`, `normal_map_filter`, `normal_map_auto_detect` en
  `crates/tp-core/src/config.rs`, clasificados en `ingest.rs` por `is_normal_map` y
  `looks_like_normal_map`; el auto-detect está **apagado por defecto** y emite un aviso
  con el nº de imágenes clasificadas) y la hoja de normales admite nombre propio
  (`normal_map_sheet`, rechazado por `validate()` si es una ruta). El emparejamiento
  difusa↔normal cubre sufijo, filtro, el último grupo del nombre (`hero-det.png` →
  `hero.png`) y, de último recurso, el mismo nombre de fichero en otra carpeta solo si
  no está duplicado; las normales sin difusa siguen avisando. Nuevos flags
  `--normalmap-suffix/--normalmap-filter/--normalmap-sheet/--normalmap-detect` y campos
  en Ajustes. Tests: `normal_maps_honor_suffix_filter_and_color`,
  `color_heuristic_accepts_normals_and_rejects_art`,
  `normal_map_settings_roundtrip_and_validate`,
  `normal_sheet_name_honors_the_custom_base`, `normalmap_flags_parse` y el e2e
  `full_pipeline_with_aliases_rotation_and_normals` (incluye el nombre de hoja propio).
- **Pixel format / calidades**: 9 formatos de píxel y calidad solo para JPG/WebP; faltan
  las calidades de PVRTC/ETC/ASTC/BASIS/DXT y los pixel formats de GPU
  (`PVRTCI_*`, `ETC1_*`, `DXT1/5`, `ASTC_*`, `BASISU_*`).
- **Content protection**: AES-GCM propio (más general que el original, que es solo
  Cocos2D + `pvr.ccz`), pero sin gestor de clave global reutilizable.

## 3) Prácticamente inexistentes

- **Entrada de imágenes**: faltan `psd, svg, ktx, ktx2, pbm, pgm, ppm, xbm, xpm, astc, basis`
  (`crates/tp-core/src/ingest.rs:135-144`); el original lista ~29 formatos.
- **Extras de data format**: cache busting (Pixi/Phaser), filtering (LibGDX), `shape-debug`
  (contorno de shapes dibujado en la hoja resultante).
- **Tutoriales**: el botón de la toolbar del original no existe aquí (menor).

## 4) Verificado como completo y bien cableado

Uso real en el motor comprobado campo a campo: los 5 trim modes, los 4 alpha handling, las
6 heurísticas MaxRects, size constraints / fixed size / force squared, pack mode,
basic sort/order, extrude, border/shape padding, common divisor, multipack con placeholders
`{v}`/`{n}`/`{n1}`, `trim_sprite_names`, `prepend_folder_name`, `texture_path`,
auto-detect animations, aliasing, png opt level, flip-y, dithering, color depth,
`basic_sort_by`, `scale_variants`, `default_pivot`, `encryption_key`.

---

## Orden de corrección (estado)

1. **Hoja principal** (borra sprites del atlas) — **hecho**:
   `run_groups` con `owner` por id + test `sprites_listed_in_the_main_sheet_stay_in_the_atlas`
   (`crates/tp-core/tests/pipeline_e2e.rs`).
2. **Pivots/bordes no publicados** (los edits de la GUI no llegaban al fichero exportado) —
   **hecho**: `pivot_overrides` / `border_overrides` en el proyecto, precedencia
   config > sidecar > default, sin campos de memoria en la GUI; tests
   `pivot_and_border_overrides_survive_toml_roundtrip` y
   `gui_overrides_win_over_sidecar_and_default_pivot`.
3. **Guillotine falso** (implementarlo de verdad) — **hecho**:
   `pack_guillotine` + `split_guillotine` en `crates/tp-core/src/pack.rs`, con 6 tests
   propios (sin solapes, segunda página, rotación, Best nunca peor, mejor que rejilla,
   partición de la lista libre).
4. **ASTC** (habilitar la compilación) — **hecho** (ver §2).
5. **Animación** (que use la selección) — **hecho**:
   `collect_frames` filtra por selección + `effective_group`; tests
   `animation_selection.rs` (2) y `effective_group_falls_back_when_the_group_is_gone`.
6. **Scaling variants** (filter / max size / identical layout) — **hecho**:
   `VariantOptions` + `plan_variants` + corridas propias por variante (con
   `scale_ingested_sprites` para empaquetar a su escala), la tabla «Opciones por
   variante» en los ajustes y 7 tests (`variant_*` en `pipeline_e2e.rs` y
   `variant_options_*`/`variant_filter_*` en `config.rs`).

7. **Align to grid** (que coloque las esquinas en múltiplos, como el original) —
   **hecho**: `snap_pos` en `crates/tp-core/src/pack.rs` para todos los algoritmos
   (MaxRects/Guillotine/Polygon, Grid, Basic, Manual) + `align_grid` en
   `PackerOptions`, y `effective_divisors` ya no suma `align_to_grid` (así alinear
   mueve los sprites y no los estira); tests `align_to_grid_snaps_every_corner_without_stretching`
   y `effective_divisors_ignore_align_to_grid`.

8. **Pestaña "Archivos"** (lista mentirosa en vista previa) — **hecho**: la
   preview predice los mismos nombres que escribe la publicación (imagen,
   normales, metadatos y `.tpenc`) y la pestaña indica si los ficheros están
   escritos o por escribir; test `preview_lists_exactly_the_files_publish_writes`.

Pendientes fuera del orden acordado: el resto de la lista de §2/§3.

Puerta de calidad (se reejecuta en cada punto de §2/§3): `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings` y
`cargo test --workspace` en verde (246 tests tras el tercer punto, **250 tras el cuarto**).

---

## Progreso de §2/§3 (orden elegido)

1. **Scale modes + dithering** — **hecho** (`Hq2x` omitido por licencia, ver §2):
   `Scale2x`/`Scale3x`/`Scale4x`/`Eagle` en `scale_rgba`
   (`crates/tp-core/src/export.rs`), que solo corren en su factor entero y caen a
   `Smooth` con aviso (`pipeline::execute` + avisos de la GUI); `NearestNeighbour` y
   `Linear` en `DitheringAlgorithm` con sus ramas en `pixels::distribute_error`;
   combos en «Escalado de variantes»/«Dithering», `--scale-mode` y `--dither` en el
   CLI. Tests: `scale2x_corner_takes_the_diagonal_colour`,
   `scale3x_edges_follow_the_corner_rules`, `eagle_corner_takes_the_three_equal_neighbours`,
   `pixel_art_scalers_keep_the_palette_and_the_right_size`,
   `pixel_art_modes_fall_back_to_smooth_off_their_factor`,
   `nearest_neighbour_dither_rounds_without_diffusion`.
2. **Normal maps** — **hecho** (ver §2 «Normal maps [RESUELTO]»): sufijo/filtro/auto-detect
   por color con aviso (`ingest::is_normal_map` + `looks_like_normal_map`), emparejamiento
   por sufijo → filtro → último grupo del nombre → nombre de fichero inequívoco,
   `normal_map_sheet` para la hoja (validado en `ProjectConfig::validate`), UI de Ajustes
   (sufijo, filtro, auto-detect y nombre de la hoja bajo el checkbox) y flags
   `--normalmap-*` en el CLI. Tests citados en §2.
3. **Formatos de salida faltantes** — **hecho** (ver §2 «Formatos de textura de salida
   [PARCIAL] 16/19»): BMP/TGA/TIFF/DDS/ZKTX/PVR3GZ/PVR3CCZ/ETC1(PKM)/ETC1(KTX) nuevos,
   con `GpuFormat::parse`, combo de la GUI, `--format` ampliado y e2e propio. Faltan
   `ktx2` y `basis`.
4. **Exportadores (ficheros extra)** — **hecho** (ver §2 «Exportadores [PARCIAL]»): los cuatro
   campos del `.tpproj`, `templates::extra_files` + `ident`, escritura junto a los metadatos
   con anuncio en la vista previa, flags CLI y sección de la GUI. Quedan sin cubrir los 60+
   presets de plantilla del original y la conversión de formato de datos.
5. **Calidades y pixel formats GPU** — pendiente.
