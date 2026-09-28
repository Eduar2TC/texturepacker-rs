# packaging/windows

Recursos del instalador `.msi` (WiX v3) que genera el release workflow.

## Contenidos

- `texturepacker-rs.wxs` — manifiesto del instalador. El workflow sustituye
  `@VERSION@`, `@EXE_APP@` y `@EXE_CLI@` antes de invocar `candle`/`light`.
  Instala `tp-app.exe` y `tp-cli.exe` en *Archivos de programa*, crea el
  acceso directo en el menú Inicio y añade `tp-cli` al `PATH` del sistema.

## Ficheros opcionales

El `.wxs` referencia dos ficheros que **no** están en el repositorio por
tamaño/licencia; si existen, el instalador los incluye:

- `icon.ico` — icono de la app (usado en el acceso directo y en
  *Agregar o quitar programas*). Si no existe, el workflow lo elimina del
  manifiesto (`<Icon>`, `ARPPRODUCTICON` y el atributo `Icon` del
  shortcut) y el instalador usa el icono genérico de WiX.
- `licencia.rtf` — licencia mostrada en la página del instalador. Si no
  existe, el workflow elimina el `WixVariable WixUILicenseRtf` y la
  página de licencia se omite.

Para generar un `.ico` desde un PNG existente (p. ej. el logo del
proyecto) se puede usar ImageMagick:

```sh
magick logo.png -define icon:auto-resize=256,128,64,48,32,16 icon.ico
```

## Validación local (opcional)

Requiere .NET SDK (wix v3 se instala como herramienta dotnet):

```sh
dotnet tool install --global wix
wix extension add -g WixToolset.Util.wixext   # no requerido por este wxs
sed -e "s/@VERSION@/0.2.0/" \
    -e "s|@EXE_APP@|target/x86_64-pc-windows-msvc/release/tp-app.exe|" \
    -e "s|@EXE_CLI@|target/x86_64-pc-windows-msvc/release/tp-cli.exe|" \
    packaging/windows/texturepacker-rs.wxs > /tmp/tp.wxs
wix build /tmp/tp.wxs -o /tmp/tp.msi
```

En CI esto lo hace el paso «Instalar WiX» del release workflow, que
instala la extensión UI necesaria (`WixToolset.UI.wixext`) porque el
`.wxs` usa `WixUILicenseRtf`.
