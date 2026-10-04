# Parches de código de terceros

Este directorio contiene copias de dependencias que van **modificadas** en
relación con lo que publica crates.io. Cada subdirectorio tiene aquí su
ficha: qué se cambió, por qué, y cómo se retira.

## `winit/` — winit 0.30.13 con drag-and-drop nativo en Wayland

**Qué es.** Una copia de winit 0.30.13 con aplicado el commit
[`988f0b8`](https://github.com/IndigoCarmine/winit/commit/988f0b8) de la
rama `wayland-native-dnd` de `github.com/IndigoCarmine/winit`.

**Por qué.** winit 0.30 solo emite los eventos de fichero
(`HoveredFile`/`DroppedFile`) vía XDND, el protocolo de X11. En Wayland no
hay API equivalente, así que eframe/egui nunca recibe
`hovered_files`/`dropped_files` y soltar un fichero desde el gestor de
archivos no funciona: el overlay azul de `handle_global_file_drop` solo
aparece bajo X11/XWayland.

**Alcance.** 4 archivos, +370 líneas, puramente aditivos sobre el backend
Wayland (`wl_data_device` → los eventos clásicos de winit). No toca la API
pública ni los backends X11, Win32 o macOS.

**Cómo se conecta.** Desde el `Cargo.toml` de la raíz, que mantiene fuera
del workspace y parchea el registro:

```toml
[workspace]
exclude = ["third_party/winit"]

[patch.crates-io]
winit = { path = "third_party/winit" }
```

La justificación con las mediciones que la motivaron vive en el bloque de
comentarios junto a `[patch.crates-io]` en el `Cargo.toml` de la raíz, y
en el README (sección de limitaciones conocidas).

**Coste.** Al fijar una copia local no se pueden tomar las versiones
`^0.30.14` ni superiores de winit, ni sus fixes de seguridad, hasta que el
parche se retire.

**Retirada.**

1. Esperar a que eframe publique sobre winit 0.31 (el drag-and-drop en
   Wayland ya está fusionado en `master` de winit: rust-lang/winit#4571).
2. Borrar `third_party/winit`.
3. Borrar la sección `[patch.crates-io]` y la línea `exclude` del
   `Cargo.toml` de la raíz.
4. Comprobar que soltar ficheros sigue funcionando bajo Wayland puro.

Cualquier cambio adicional sobre este árbol debe documentarse aquí.
