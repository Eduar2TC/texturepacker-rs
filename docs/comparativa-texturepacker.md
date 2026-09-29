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

- **Exportadores**: 6 plantillas nativas (JSON, XML, Plist, C++ header, TSV, PlainText —
  `crates/tp-core/src/config.rs:621-630`) + plantilla Mustache propia, frente a los 60+ del
  original; sin ficheros extra por framework (`--class-file`, `--header-file`,
  `--source-file`, `--spriteids-file`) ni botón de conversión de formato de datos.
- **Formatos de textura de salida**: PNG/PNG8/JPG/WebP + ETC2/PVRTC, frente a los 19 del
  original (faltan `bmp, tga, tiff, pvr3, pvr3gz, pvr3ccz, pkm, ktx, ktx2, zktx, astc, basis, dds`).
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
- **Dithering**: 5 algoritmos (`crates/tp-core/src/config.rs:40-54`) vs 9 del original
  (faltan `NearestNeighbour`, `Linear` y los tres `PngQuant*`).
- **Scale mode**: Smooth/Fast (`crates/tp-core/src/config.rs:310-318`) vs 7 del original
  (faltan `Scale2x`, `Scale3x`, `Scale4x`, `Eagle`, `Hq2x`).
- **Normal maps**: sí se genera una hoja de normales por página, pero el sufijo es fijo
  `_normal` (`crates/tp-core/src/ingest.rs:380-385`); faltan auto-detect por color, path
  filter, sufijo configurable y nombre propio de la hoja de normales.
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

Puerta de calidad tras los puntos 1-8: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings` y
`cargo test --workspace` en verde (225 tests).
