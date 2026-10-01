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
| **Algoritmo "Guillotine"** y heurística "Guillotine (legacy)" (`crates/tp-app/src/app/settings.rs:425,467`) **[RESUELTO]** | Ahora hay un repartidor de guillotina real: `pack_guillotine` + `split_guillotine` en `crates/tp-core/src/pack.rs`, seleccionado desde `place_all` cuando `algorithm == Guillotine`. La etiqueta ya no promete el heurístico "legacy" oculto. |
| **Pivots y bordes 9-patch editados en la GUI** (`crates/tp-app/src/app/sprite_settings.rs:62-151`) **[RESUELTO]** | La GUI escribe `pivot_overrides` / `border_overrides` en el `.tpproj` (`crates/tp-core/src/config.rs`) y `pipeline::run_grouped` los aplica con precedencia sobre sidecar y `default_pivot`, así que llegan al atlas y al fichero publicado sin botón aparte. |
| **"▶ Animación: vista previa de animación de los sprites seleccionados"** (`crates/tp-app/src/app/toolbar.rs:96-101`) **[RESUELTO]** | `collect_frames` (`crates/tp-app/src/app/animation.rs`) filtra por `selected_paths` cuando hay selección; sin selección reproduce todos. El grupo elegido se recalcula con `effective_group` cuando la selección lo deja fuera. |
| **"Align to grid"** (`align_to_grid`) **[RESUELTO]** | Ahora el packer sube cada origen de frame al múltiplo (`snap_pos` en `crates/tp-core/src/pack.rs`, aplicado en MaxRects/Guillotine/Polygon, Grid, Basic y Manual) y `align_to_grid` dejó de entrar en `effective_divisors`: a la par que el original, alinea **sin estirar** los sprites (solo el *common divisor* los estira). Tests `align_to_grid_snaps_every_corner_without_stretching` (8 casos) y `lote5_align_to_grid_rounds_padding_and_positions`. |
| **Hoja principal: "← Selección" / arrastrar al nodo `(hoja principal)`** **[RESUELTO]** | `run_groups` (`crates/tp-core/src/pipeline.rs`) lleva un `owner` por id: el primer grupo que lista un sprite es su dueño y el grupo por defecto recoge los suyos propios y los no listados, así que asignar un sprite ya no lo saca del atlas. |
| **Pestaña "Archivos" del panel inferior** **[RESUELTO]** | La vista previa lista ahora **exactamente** los ficheros que escribirá la publicación — hojas, normales y metadatos, con el `.tpenc` cuando hay cifrado (`execute` en `crates/tp-core/src/pipeline.rs`) — y la pestaña distingue «Archivos que se publicarán» de «Archivos generados» según `App::files_written`. Test `preview_lists_exactly_the_files_publish_writes`. |

## 2) Parciales — existen, pero muy por debajo del original

- **Exportadores** **[RESUELTO] 63 de los 60+ del original**: `crates/tp-core/src/dataformats.rs`
  enumera 63 exportadores agrupados en 6 categorías — Genéricos (5: `json`, `json-array`,
  `xml`, `plain`, `spritesheet-only`), Atlas de texto (6: `libgdx`, `spine`, `css`,
  `css-simple`, `less`, `sass-mixins`), JSON de motores (26: `phaser`, `pixijs4`, `egret`,
  `spriter`, `melonjs`, `unity`, `unity-texture2d`, `unreal-paper2d`, `monogame`,
  `easeljs`, `zim`, `amethyst`, `godot-spritesheet`…), plist y XML (7: `cocos2d`,
  `cocos2d-v2`, `cocos2d-x`, `spritekit`, `spritekit-swift`, `sparrow`, `uikit`), Motores
  minoritarios (17: `2dtoolkit`… `wave-engine-1`, `shiva3d`, `caat`) y Extras del propio
  clon (2: `cpp-header`, `tsv`) —, cada uno con familia de plantilla, extensión del fichero
  de datos y valores recomendados (rotación, algoritmo, auto-detección de animaciones) en
  `DATA_FORMATS`, con `find_data_format`, `data_format_ids` y `data_formats_in_category`.
  Las **15 familias de `TemplateFormat`** generan la salida: `builtin_template` tiene
  plantilla propia para catorce (el JSON con `frames` en lista lo serializa `render` con
  su contrato de siempre) y la plantilla Mustache propia (`export_template`/`--template`)
  sigue teniendo prioridad. `templates::data_file_extension` usa la extensión del preset
  cuando el proyecto declara uno y la de la familia si no; la familia «solo la hoja»
  (`spritesheet-only`) no escribe fichero de datos: `pipeline::execute` lo salta y la
  vista previa no lo anuncia.
  La **conversión de formato de datos** (el «Data format… / Update to recommended values»
  del original) vive en `ProjectConfig::apply_data_format` (cambia familia + extensión +
  recomendados) y `apply_data_format_defaults` (sólo recomendados), sobre el campo
  `data_format` (`#[serde(default)]`, validado en `validate()` y con `data_format_preset()`).
  En la GUI: combo «Formato de metadatos» con los 63 presets agrupados por categoría y
  botón «Valores recomendados». En el CLI: `apply_template_format` acepta los tokens
  legados `json | xml | plist | cpp | tsv | text` **y** los ids de exportador.
  Contexto nuevo: `frameX`/`frameY`/`frameWidth`/`frameHeight` (Starling), `offset`
  centrado «x,y» y `offsetBottomLeft` (atlas de texto y plist), `cssClass` (clase CSS
  saneada) y `sheets` con los frames agrupados por hoja (Phaser, CSS y atlas de texto);
  el JSON lista descarta esas claves para no mover su contrato. **Desviaciones**: los
  motores minoritarios salen con la estructura `JsonHash` y extensión `json` en vez de la
  propia de cada motor; el id del exportador de Sparrow/Starling es `sparrow` (el del
  original), no `starling`; `json` legado sigue siendo el array, así que el hash del
  original se pide como `json-hash` en el CLI (en la GUI es el preset «JSON (hash)»); el
  `offset` del plist pasó a ser el centrado que publica el original (antes copiaba el
  `spriteSourceSize` top-left) y lleva `alias` sólo cuando hay alias; en Starling
  `width`/`height` es el rect almacenado con la rotación ya aplicada y el original va en
  `frameWidth`/`frameHeight`; y las texturas citadas siguen el cache busting global del
  proyecto, no sólo el de Pixi/Phaser. Los ficheros extra por framework ya estaban:
  `class_file`, `header_file`, `source_file` y `spriteids_file` en el `.tpproj` (vacío =
  no escribir), generados por `templates::extra_files` (`crates/tp-core/src/templates.rs`)
  — ids C++/Swift saneados (ruta y signos → `_`, dígito inicial con prefijo `_`,
  colisiones con `_` extra), cabecera con guard `ATLAS_SPRITES_H`, fuente con `#include`
  de la cabecera configurada o `<ns>.h`, enum Swift `public enum` con `public static let`
  y lista de ids una por línea —, escritos junto a los metadatos y anunciados en la vista
  previa (pestaña «Archivos»), con sección «Ficheros extra por framework» en la GUI y
  flags `--class-file`, `--header-file`, `--source-file` y `--spriteids-file`. Tests: 5
  de `dataformats`, el de conversión de `config`,
  `template_format_accepts_families_and_exporters` (CLI),
  `json_array_keeps_its_original_contract`, `extra_files_generate_cpp_swift_and_id_list`,
  `extra_data_file_settings_roundtrip_and_default_empty`, `extra_data_file_flags_parse` y
  los e2e `lote10_extra_data_files_export`, `data_format_presets_publish_their_own_data_file`
  y `data_format_families_render_their_own_shape`.
- **Formatos de textura de salida** **[RESUELTO]**: los **17 de los 17** que enumera la
  documentación vigente del original (el `--help` antiguo sumaba `atf` y `pvr2`, que el
  propio TexturePacker retiró en la 6.0.2, de ahí la vieja cuenta «16 de los 19»). A
  PNG/PNG8/JPG/WebP/ETC2/PVRTC se les han sumado **BMP, TGA y TIFF** (codificador de
  `image`), **DDS** (cabecera legacy de 124 bytes con máscaras de canal y payload crudo),
  **ZKTX** (KTX v1 en zlib), **PVR3GZ y PVR3CCZ** (el PVR3 de PVRTC en gzip y en el
  contenedor CCZ de Cocos2D) y **ETC1** en contenedor PKM (`.pkm`) y en KTX
  (`glInternalFormat 0x8D60`), con un codificador ETC1 propio que reutiliza los modos
  individual + diferencial de `etc2.rs` y descarta T/H/planar (son extensiones ETC2, y
  un bloque así se leería mal en hardware ETC1); y ahora también **KTX2** (contenedor
  2.0 sin comprimir: identificador de 12 bytes, índice de un solo nivel, DFD [KDF14]
  con modelo RGBSDA/BT.709/transferencia lineal y una muestra de 8 bits por canal,
  `KTXorientation` = `rd` y `KTXwriter` en la KVD ordenadas por clave, vkFormat
  37/10/9 para RGBA8/RGB8/R8) y **Basis Universal** en `.basis` (ETC1S vía la crate
  `basis-universal`, que se aísla en el crate **`tp-basis`**, la única excepción del
  workspace a `unsafe_code = "forbid"`: sus dos llamadas `unsafe` de FFI viven ahí).
  Compresión con `flate2`, variantes en `GpuFormat` (+ `GpuFormat::parse` para el CLI),
  combo de «Formato de publicación», flags
  `--format bmp|tga|tiff|dds|zktx|pvr3gz|pvr3ccz|pkm|ktx|ktx2|basis` y `--basis-quality`.
  Tests: `bmp_tga_tiff_roundtrip_the_rgba_pixels`,
  `dds_writes_a_valid_legacy_header_and_raw_pixels`, `zktx_is_a_zlib_compressed_ktx`,
  `pvr3_gz_and_ccz_wrap_the_pvr3_file`, `etc1_pkm_and_ktx_decode_close_to_the_source`,
  `output_format_tokens_and_extensions`, `ktx2_writes_a_spec_container_with_dfd_and_kvd`,
  `ktx2_pixel_formats_round_trip_through_the_reader`,
  `ktx2_reports_pixel_formats_it_cannot_describe`, `basis_file_transcodes_back_to_rgba`,
  los 3 de `tp-basis` y los e2e `lote9_software_and_container_formats_export` y
  `lote13_ktx2_container_and_basis_export`. Desviaciones: la calidad Basis se llama
  `basis_quality`/`--basis-quality` y va 0-100 (defecto 50), no `--basisu-quality` 0-4
  como el original; KTX2 solo describe RGBA8/RGB8/R8 crudos (sin `BASISU_*` dentro del
  contenedor ni supercompresión) y no entra en el conjunto de flip-y, mientras que el
  original lo lista; y `basis` es solo ETC1S (sin UASTC ni la opción de transcodificar
  a la hora de publicar).
- **ASTC** **[RESUELTO]**: `tp-app` y `tp-cli` activan `tp-core/gpu-formats` en su
  `Cargo.toml`, así que `GpuFormat::is_supported()` es `true`, astcenc se compila y el
  test `etc2_pvrtc_and_astc_export_paths` exporta `atlas.astc` de verdad. La advertencia
  de `settings.rs:1059` sigue ahí para construcciones sin la feature.
- **Scaling variants** **[RESUELTO]**: además del campo de escalas y `variant_names`, cada
  variante tiene `variant_options` (`crates/tp-core/src/config.rs`): filtro de sprites con
  comodines, `max_texture_size` propio y `force_identical_layout`. `plan_variants`
  (`crates/tp-core/src/pipeline.rs`) decide qué variantes reescalan la hoja base y cuáles
  empaquetan por su cuenta (con su filtro y su tope), y la GUI lo edita en «Opciones por
  variante» (`crates/tp-app/src/app/settings.rs`). Están también los **presets** del
  original — `VARIANT_PRESETS` + `ProjectConfig::apply_variant_preset` (Ninguna,
  iPad+iPhone del ejemplo de la documentación, Retina @1x/@2x/@3x, Android
  mdpi…xxxhdpi y Descuentos 1/2-1/3-1/4) con combo y «Aplicar» que sobrescribe la
  lista, como el diálogo original — y **«accept fractional values»**:
  `accept_fractional` por variante la deja **fuera del común divisor**,
  `variant_common_divisor` (LCM de los denominadores de las escalas idénticas vía
  `scale_denominator`, tope 2048) lo suma en `effective_divisors` y `execute` alinea con
  ese valor los orígenes, de modo que una hoja idéntica a 0.5 sale exactamente a la
  mitad sin redondeos. La CLI entiende ya la sintaxis completa del original
  `--variant <escala>[:<nombre>[:<filtro>[:allowfraction[:<ancho>:<alto>]]]]`
  (repetible o por comas; un filtro con comas no se parte). Desviación conocida: una
  escala fraccionaria se **redondea** al píxel y se avisa, mientras que el original
  guarda coordenadas con decimales. Tests: `scale_denominator_covers_the_usual_and_rejects_the_odd`,
  `variants_extend_the_common_divisor_unless_they_opt_out`,
  `variant_presets_overwrite_the_variant_list`,
  `accept_fractional_roundtrips_and_defaults_off`, `variant_flag_parses_filter_allowfraction_and_size`
  y el e2e `variant_common_divisor_keeps_the_base_sheet_on_integer_coordinates`.
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
- **Pixel format / calidades — hecho**: los 9 formatos de software más `RGBA4444`/`RGB565`
  y los pixel formats de GPU (`PVRTCI_*`, `ETC1/ETC2_RGB`, `ETC2_RGBA`, `DXT1/5`, `ASTC_*`
  con sus 14 bloques), con el criterio del original «solo se pueden elegir los formatos de
  píxel que soporta el formato de textura» (`PixelFormat::is_compatible_with`, validado en
  `ProjectConfig::validate`, filtrado en el combo de la GUI y comprobado por la CLI), y las
  calidades del original: `--pvr-quality 0-7` (3), `--etc1-quality 0-100` (70),
  `--etc2-quality 0-100` (70), `--astc-quality 0-4` (2) y `--dxt-mode
  DXT_LINEAR|DXT_PERCEPTUAL`. Compresores propios: BC1/BC3 (`dxt.rs`), PVRTC1 2/4bpp
  (`pvrtc.rs`), ETC1/ETC2 con esfuerzo escalonado (`etc2.rs`) y ASTC con `astcenc-rs`;
  BASIS/BasisU también está (ETC1S por `tp-basis`, puntos 11 y 14).
- **Content protection — hecho**: AES-GCM propio (más general que el original, que es
  solo Cocos2D + `pvr.ccz`) más el gestor de clave global que faltaba: `tp_core::keys`
  guarda `nombre -> clave` en `keys.toml` dentro del directorio de configuración del
  usuario (`TEXTUREPACKER_KEYS_FILE`/`_DIR` apuntan a otro sitio, que es lo que usan los
  tests), el proyecto guarda `encryption_key_name` y la publicación resuelve la clave
  antes de escribir nada — la clave explícita `encryption_key` gana, y si el nombre no
  existe solo falla al publicar: la vista previa sigue listando los mismos nombres.
  Flags `--key-name` y `--save-key` (con `--key`), y «Clave global» con combo +
  Guardar/Borrar en Ajustes. Tests: `stored_keys_roundtrip_and_are_sorted`,
  `empty_names_and_keys_are_rejected_and_broken_files_are_ignored`,
  `resolve_prefers_the_explicit_key_and_fails_on_a_missing_name`,
  `global_key_name_roundtrip_and_legacy_default`, `global_key_flags_parse` y el e2e
  `global_key_name_publishes_encrypted_files`.

## 3) Prácticamente inexistentes

- **Entrada de imágenes — hecho**: `pbm`/`pgm`/`ppm` (y `pnm`) entran por la feature `pnm`
  de `image`; `xbm`, `xpm`, `astc`, `psd`, `svg` y los contenedores `ktx`/`ktx2` se
  decodifican en el nuevo `crates/tp-core/src/reader.rs` — XBM con `#define *_width/_height`
  y bits LSB-first (verificado contra el `atobm` de X11: bit encendido = negro opaco),
  XPM1/XPM2 con paleta `None`/hex/X11, `.astc` por cabecera de 16 bytes +
  `texture2ddecoder::decode_astc`, KTX1 (RGBA8/RGB8 crudos y ETC1/ETC2/DXT/ASTC vía
  `texture2ddecoder`) y KTX2 (RGBA8/BGRA8/RGB8/R8, con supercompresión zlib; Basis
  Universal se rechaza con un mensaje claro porque necesita un transcoder, igual que su
  escritura). La detección KTX1/KTX2 es por contenido (los dos comparten identificador: el
  byte 12 es la endianness `01 02 03 04` en v1), así que un `.ktx` con payload v2 también
  se lee. `psd` se abre con el crate `psd` (compuesto aplanado, igual que la vista previa
  de Photoshop) y `svg` se rasteriza con `resvg` a tamaño intrínseco (o 100×100 por
  defecto), con las fuentes del sistema cargadas una sola vez (`OnceLock`) y la salida
  des-premultiplicada: tiny-skia compone en alfa premultiplicado y el pipeline trabaja con
  alfa recta, comprobado con un `fill-opacity="0.5"`. `is_image_file` y los filtros del
  diálogo de la GUI/hoja están ampliados. `basis` se lee con el transcoder de la misma
  crate que lo escribe (`tp_basis::transcode_rgba`: firma `sB`, primer nivel de la
  primera imagen, tamaño validado contra el buffer) tras la feature `gpu-formats`;
  sin ella el error lo dice en lugar de ignorar el fichero, igual que en la salida.
  En total la entrada cubre **19 formatos** (24 extensiones) de los ~29 que enumera la
  documentación del original: esa es la única diferencia que queda abierta aquí.
  Tests: 19 unitarios en `reader.rs` (incluido un `.astc` real exportado por nuestro
  codificador, embebido en hexadecimal, un PSD RGB mínimo construido a mano, la
  comprobación alfa/recta del SVG y el round-trip `.basis` → RGBA con error medio
  < 5) y los e2e
  `lote11_ingests_xbm_xpm_ppm_astc_and_ktx_inputs`,
  `lote12_ingests_psd_and_svg_inputs` y `basis_input_is_ingested_and_packed`.
- **Extras de data format — hecho**: `cache_busting` añade `?v=<hash>` a la textura citada
  en los metadatos (hash de 8 hex del fichero publicado, `hash::hash_bytes_short`), igual
  que los data formats de Pixi/Phaser; `gdx_filter` declara `filter: Linear, Linear` /
  `Nearest, Nearest` en el atlas XML de LibGDX; `shape_debug` dibuja el rectángulo visible
  y los polígonos de cada sprite en magenta sobre la hoja, con la misma rotación 90° CW
  que `polygon::compute_uvs`, así la vista previa y el fichero publicado salen idénticos.
  Flags `--cache-busting`, `--gdx-filter linear|nearest` y `--shape-debug`, y la sección
  «Extras del data format» en Ajustes. Tests: `data_format_extras_roundtrip_and_default_off`,
  `cache_busting_appends_the_file_version_to_the_texture_reference`,
  `gdx_filter_is_declared_on_the_xml_atlas`, `shape_debug_draws_the_borders_and_keeps_the_interior`,
  `shape_debug_clips_out_of_canvas_shapes`, `data_format_extra_flags_parse` y el e2e
  `cache_busting_and_shape_debug_reach_the_published_files`.
- **Tutoriales** **[RESUELTO]**: el botón «Tutorial» de la toolbar del original
  («Opens TexturePacker's tutorial page in your web browser») existe ahora en la
  toolbar de la app y abre <https://www.codeandweb.com/texturepacker/tutorials>
  con `ctx.open_url(OpenUrl::new_tab(…))`, que egui-winit resuelve con la crate
  `webbrowser` (feature `links`, ya activa), y deja constancia en el Log. El enlace
  vive en `TUTORIAL_URL` (`crates/tp-app/src/app/toolbar.rs`), con test de contrato.

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

Pendientes fuera del orden acordado: ninguno — la lista de §2/§3 está cerrada,
salvo «Scale mode», que queda **[PARCIAL]** a propósito (el Hq2x se omitió por su
licencia LGPL, decisión acordada).

Puerta de calidad (se reejecuta en cada punto de §2/§3): `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings` y
`cargo test --workspace` en verde (246 tests tras el tercer punto, 250 tras el cuarto,
272 tras el quinto, 279 tras el sexto, 285 tras el séptimo, 291 tras el octavo,
305 tras el noveno, 310 tras el décimo, 318 tras el undécimo, 327 tras el duodécimo,
328 tras el decimotercero, **332 tras el decimocuarto**).

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
   [RESUELTO] 17/17»): BMP/TGA/TIFF/DDS/ZKTX/PVR3GZ/PVR3CCZ/ETC1(PKM)/ETC1(KTX)/
   KTX2/BASIS, con `GpuFormat::parse`, combo de la GUI, `--format` ampliado y e2e
   propio.
4. **Exportadores (ficheros extra)** — **hecho** (ver §2 «Exportadores [RESUELTO] 63 de
   los 60+»): los cuatro campos del `.tpproj`, `templates::extra_files` + `ident`,
   escritura junto a los metadatos con anuncio en la vista previa, flags CLI y sección de
   la GUI. Los 63 presets de formato de datos y la conversión son el punto 12.
5. **Calidades y pixel formats GPU** — **hecho** (ver §2 «Pixel format / calidades»):
   `PixelFormat` ampliado con `RGBA4444`/`RGB565` y todos los de GPU (serde idéntico al
   original), `is_compatible_with` + `validate()`, campos `pvr_quality`/`etc1_quality`/
   `etc2_quality`/`astc_quality` y `DxtMode` en `ProjectConfig`, compresores nuevos
   (`dxt.rs` BC1/BC3 con modo lineal/perceptual, PVRTC 2bpp y calidad en `pvrtc.rs`,
   esfuerzo escalonado en `etc2.rs`, `.astc` con bloque y preset según calidad),
   dispatch por pixel format en `export.rs` (DDS con fourcc `DXT1`/`DXT5`, KTX ETC2
   RGB/RGBA, cabecera PVR con el `pixelFormat` correcto), flags
   `--pvr-quality --etc1-quality --etc2-quality --astc-quality --dxt-mode` con rangos y
   compatibilidad validados, y GUI con combo filtrado, calidades por formato y modo DXT.
   BASIS/BasisU queda cerrado por completo (salida en el punto 11, entrada en el
   punto 14).
6. **Extras de data format** — **hecho** (ver §3): `cache_busting`/`gdx_filter`/`shape_debug`
   en `ProjectConfig` (con `serde(default)`, así un `.tpproj` anterior sigue cargando),
   `PageInfo::cache_version` para que los metadatos citen el hash del fichero recién
   escrito, `GdxFilter` con `filter` en el contexto de la plantilla, `draw_shape_debug`
   (`export.rs`, Bresenham con recorte al lienzo) aplicado sobre las páginas justo después
   de montar los `SpriteAsset` — así lo ven la vista previa y la publicación —, flags CLI
   validados y la sección «Extras del data format» en la GUI.

7. **Gestor de clave global** — **hecho** (ver §2 «Content protection»): `tp_core::keys`
   con `list`/`get`/`put`/`remove` (y sus variantes con ruta explícita para los tests),
   fichero `keys.toml` con permisos 600 y directorio de configuración por plataforma;
   `ProjectConfig::encryption_key_name` (con `serde(default)`), resolución al inicio de
   `pipeline::execute` vía `keys::resolve_config` (estricta al publicar, tolerante en la
   vista previa), flags `--key-name`/`--save-key` en el CLI y combo «Clave global» con
   Guardar/Borrar en Ajustes. Tests: los tres de `tp_core::keys`,
   `global_key_name_roundtrip_and_legacy_default`, `global_key_flags_parse` y el e2e
   `global_key_name_publishes_encrypted_files`.
8. **Presets de variantes + fractional values** — **hecho** (ver §2 «Scaling variants»):
   `VARIANT_PRESETS`/`apply_variant_preset` con los cinco presets y «Aplicar» en Ajustes,
   `accept_fractional` en `VariantOptions` (serde default), el común divisor derivado de
   las escalas idénticas (`scale_denominator`, `variant_common_divisor`,
   `variant_fractional_scales`, `effective_divisors`) con los orígenes alineados a ese
   valor en `pipeline::execute`, avisos cuando la hoja idéntica se redondea y la sintaxis
   completa de `--variant` (filtro, `allowfraction`, tamaño) en el CLI, con
   `apply_variant_flags` extraído para poder testearlo. Tests: 4 de config, 1 de CLI y el
   e2e.
9. **Entrada de imágenes** — **hecho** (ver §3 «Entrada de imágenes»): `pnm` en las
   features de `image` y el módulo `reader` con los decoders escritos a mano (XBM, XPM,
   ASTC, KTX1, KTX2, PSD, SVG) más el despacho por extensión en `ingest::is_image_file`/
   `ingest::load_image_rgba`, los filtros de la GUI y el e2e de ingestión. La lectura
   de `.basis` como entrada se cierra en el punto 14.
10. **PSD y SVG** — **hecho** (ver §3 «Entrada de imágenes»): `psd` (crate del mismo
    nombre, `Psd::from_bytes` → `rgba()`, el compuesto aplanado) y `svg` (`resvg` 0.48 +
    `usvg` + `tiny-skia`, fuentes del sistema en un `OnceLock` compartido, pixmap
    des-premultiplicado) en `reader.rs`, con `is_image_file`/diálogos de la GUI
    ampliados. Tests: `psd_reads_the_flattened_rgb_composite`,
    `psd_reports_broken_documents`, `svg_rasterizes_shapes_with_straight_alpha`,
    `svg_reports_malformed_markup` y el e2e `lote12_ingests_psd_and_svg_inputs`.
11. **Salidas KTX2 y Basis** — **hecho** (ver §2 «Formatos de textura de salida
    [RESUELTO] 17/17»): variantes `GpuFormat::Ktx2`/`GpuFormat::Basis` con su
    `file_extension`/`is_hardware`/`is_supported` (Basis, como ASTC, depende de
    `gpu-formats`), el campo `basis_quality` (0-100, defecto 50) con `#[serde(default)]`
    y su validación, `encode_ktx2` en `export.rs` (cabecera de 80 bytes + índice de un
    nivel + DFD [KDF14] con RGBSDA/BT.709 lineal y una muestra por canal + KVD
    `KTXorientation`/`KTXwriter` ordenadas por clave, vkFormat 37/10/9) y `encode_basis`
    (ETC1S) tras `gpu-formats`, con el FFI aislado en el crate **`tp-basis`** (única
    excepción del workspace a `unsafe_code = "forbid"`, API segura `encode_etc1s` y
    `transcode_rgba`). Flags `--basis-quality` con rango validado, combo con las dos
    entradas nuevas y DragValue de calidad en Ajustes. Tests: los 4 de `export.rs`
    (`ktx2_writes_a_spec_container_with_dfd_and_kvd`,
    `ktx2_pixel_formats_round_trip_through_the_reader`,
    `ktx2_reports_pixel_formats_it_cannot_describe`,
    `basis_file_transcodes_back_to_rgba`, más `basis_requires_the_gpu_formats_feature`
    sin la feature), los 3 de `tp-basis`, los casos de config/CLI actualizados y el e2e
    `lote13_ktx2_container_and_basis_export`.

12. **Presets de formato de datos + conversión** — **hecho** (ver §2 «Exportadores
    [RESUELTO] 63 de los 60+»): `crates/tp-core/src/dataformats.rs` con
    `DataFormatPreset` (id, etiqueta, familia, extensión, categoría y recomendados),
    `DATA_FORMATS` (63 en 6 categorías), `find_data_format`, `data_format_ids`,
    `data_formats_in_category` y sus 5 tests; `TemplateFormat` ampliado a **15 familias**
    (JSON hash, Phaser, PixiJS, Starling, plist UIKit, atlas libGDX, atlas Spine, CSS y
    «solo la hoja») con plantilla propia en `builtin_template`; `ProjectConfig::data_format`
    (`#[serde(default)]`) con `apply_data_format` / `apply_data_format_defaults` /
    `data_format_preset` y validación en `validate()`; `templates::data_file_extension`
    (la extensión del preset manda) y salta el bloque de metadatos de
    `pipeline::execute` en «solo la hoja»; contexto con `frameX`/`frameY`/`frameWidth`/
    `frameHeight`, `offset` centrado, `offsetBottomLeft`, `cssClass` y `sheets` agrupados
    por hoja (el JSON lista descarta esas claves); combo «Formato de metadatos» agrupado
    por categoría con botón «Valores recomendados» en la GUI, y `apply_template_format`
    en el CLI (tokens legados `json|xml|plist|cpp|tsv|text` + ids de exportador).
    Tests: 5 de `dataformats`, 1 de `config`, 1 de CLI, `json_array_keeps_its_original_contract`
    y los e2e `data_format_presets_publish_their_own_data_file` y
    `data_format_families_render_their_own_shape`.

13. **Botón «Tutorial»** — **hecho** (ver §2 «Tutoriales [RESUELTO]»): `TUTORIAL_URL` en
    `crates/tp-app/src/app/toolbar.rs` (la página de tutoriales del original), el botón
    «Tutorial» de la toolbar que la abre con `ctx.open_url(OpenUrl::new_tab(…))` —
    egui-winit la resuelve con la crate `webbrowser`, ya activa vía la feature `links` —,
    y aviso en el Log. Sólo toca la GUI: el test de contrato del enlace deja el total en
    328.

14. **Lectura de `.basis` como entrada** — **hecho** (ver §3 «Entrada de imágenes»):
    `decode_basis` en `crates/tp-core/src/reader.rs`, llamado por el despacho por
    extensión en las dos ramas: con `gpu-formats` usa `tp_basis::transcode_rgba`
    (firma `sB` comprobada, tamaño del buffer validado) y sin ella devuelve
    `«Basis requiere la feature "gpu-formats"»`, en línea con lo que hace la
    exportación. `ingest::is_image_file` y los filtros de los diálogos de la
    toolbar y de «Dividir hoja» aceptan `basis`. Tests: `basis_files_are_read_back_as_rgba`
    (ETC1S a calidad 80 y error medio < 5 contra el original), `basis_files_report_a_broken_file`,
    el caso sin la feature, `is_image_file_accepts_every_documented_input_extension` y el
    e2e `basis_input_is_ingested_and_packed` (un `.basis` 16×8 y un PNG en el mismo atlas,
    con el id sin extensión y el tamaño original en `sourceSize`). Deja la entrada sin
    carencias documentadas; el total queda en **332**.
