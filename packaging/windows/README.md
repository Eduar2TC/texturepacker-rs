# packaging/windows

**English** | [Español](README.es.md)

Resources for the `.msi` installer (WiX v3) built by the release workflow.

## Contents

- `texturepacker-rs.wxs` — installer manifest. The workflow replaces
  `@VERSION@`, `@EXE_APP@` and `@EXE_CLI@` before invoking `candle`/`light`.
  Installs `tp-app.exe` and `tp-cli.exe` in *Program Files*, creates the
  Start menu shortcut and adds `tp-cli` to the system `PATH`.

## Optional files

The `.wxs` references two files that are **not** in the repository for
size/licensing reasons; if they exist, the installer includes them:

- `icon.ico` — app icon (used in the shortcut and in
  *Add or Remove Programs*). If it doesn't exist, the workflow strips it
  from the manifest (`<Icon>`, `ARPPRODUCTICON` and the shortcut's `Icon`
  attribute) and the installer falls back to WiX's generic icon.
- `licencia.rtf` — license shown on the installer page. If it doesn't
  exist, the workflow removes the `WixVariable WixUILicenseRtf` and the
  license page is skipped.

To generate a `.ico` from an existing PNG (e.g. the project logo)
you can use ImageMagick:

```sh
magick logo.png -define icon:auto-resize=256,128,64,48,32,16 icon.ico
```

## Local validation (optional)

Requires the .NET SDK (WiX v3 installs as a dotnet tool):

```sh
dotnet tool install --global wix
wix extension add -g WixToolset.Util.wixext   # not required by this wxs
sed -e "s/@VERSION@/0.2.0/" \
    -e "s|@EXE_APP@|target/x86_64-pc-windows-msvc/release/tp-app.exe|" \
    -e "s|@EXE_CLI@|target/x86_64-pc-windows-msvc/release/tp-cli.exe|" \
    packaging/windows/texturepacker-rs.wxs > /tmp/tp.wxs
wix build /tmp/tp.wxs -o /tmp/tp.msi
```

In CI this is done by the "Install WiX" step of the release workflow, which
installs the required UI extension (`WixToolset.UI.wixext`) because the
`.wxs` uses `WixUILicenseRtf`.
