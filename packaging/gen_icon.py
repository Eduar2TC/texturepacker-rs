#!/usr/bin/env python3
"""Genera el icono de TexturePacker-RS: una hoja/atlas 2x2 con cuatro
sprites de colores sobre fondo de cuadrícula, estilo píxel-art.

Salidas (todo se deriva del mismo dibujo):
  - packaging/icon.png  (1024x1024, RGBA)
  - packaging/windows/icon.ico  (256/128/64/48/32/16)
  - packaging/macos/icon.iconset/  (PNGs para iconutil en macOS)

Uso:  python3 packaging/gen_icon.py [--png|--ico|--iconset|--all]

"""
import argparse
import os

from PIL import Image, ImageDraw

S = 1024
BORDER = 56          # margen exterior de la "hoja"
GRID = 8             # tamaño de celda de la cuadrícula de fondo

# Paleta: verde WhatsApp-like de fondo de canvas (como la vista de la app)
CANVAS = (38, 44, 52, 255)        # gris azulado oscuro
GRIDLINE = (48, 56, 66, 255)      # líneas de la cuadrícula
SHEET = (24, 28, 34, 255)         # hoja del atlas
CORNER = (90, 200, 120, 255)      # acento verde (selección/éxito)

# Los cuatro "sprites" del atlas 2x2, con colores vivos.
SPRITES = [
    ((255, 99, 71, 255),   "hero"),    # rojo tomate
    ((255, 205, 86, 255),  "coin"),    # ámbar
    ((86, 156, 255, 255),  "ui"),      # azul
    ((155, 89, 182, 255),  "fx"),      # violeta
]


def rounded(draw, box, radius, fill):
    draw.rounded_rectangle(box, radius=radius, fill=fill)


ICO_SIZES = [256, 128, 64, 48, 32, 16]
ICONSET_SIZES = [16, 32, 64, 128, 256, 512]


def build_image() -> Image.Image:
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Hoja del atlas: cuadrado redondeado oscuro con borde fino.
    sheet_box = (BORDER, BORDER, S - BORDER, S - BORDER)
    rounded(d, sheet_box, 96, SHEET)
    # Borde exterior de 10 px en verde acento.
    rounded(d, sheet_box, 96, None)
    d.rounded_rectangle(sheet_box, radius=96, outline=CORNER, width=14)

    # Cuadrícula tenue dentro de la hoja.
    x0, y0, x1, y1 = sheet_box
    inner = 40
    for gx in range(x0 + inner, x1 - inner + 1, GRID * 12):
        d.line([(gx, y0 + inner), (gx, y1 - inner)], fill=GRIDLINE, width=4)
    for gy in range(y0 + inner, y1 - inner + 1, GRID * 12):
        d.line([(x0 + inner, gy), (x1 - inner, gy)], fill=GRIDLINE, width=4)

    # Los cuatro sprites 2x2, cada uno un rectángulo redondeado con una
    # marca simple dentro (ojo = héroe, círculo = moneda, botón, chispa).
    pad = 150
    cw = (S - 2 * BORDER - 2 * pad) // 2  # ancho de celda
    ch = cw
    for i, (color, kind) in enumerate(SPRITES):
        cx = x0 + pad + (i % 2) * (cw + pad // 2)
        cy = y0 + pad + (i // 2) * (ch + pad // 2)
        box = (cx, cy, cx + cw, cy + ch)
        rounded(d, box, 48, color)
        m = cw // 5
        inner_box = (cx + m, cy + m, cx + cw - m, cy + ch - m)
        if kind == "hero":
            # Ojo: blanco + pupila oscura.
            rounded(d, inner_box, 40, (255, 255, 255, 235))
            pupil = cw // 5
            d.ellipse(
                (cx + cw // 2 - pupil, cy + ch // 2 - pupil,
                 cx + cw // 2 + pupil, cy + ch // 2 + pupil),
                fill=(30, 34, 40, 255),
            )
        elif kind == "coin":
            # Moneda: anillo.
            d.ellipse(inner_box, outline=(255, 255, 255, 235), width=cw // 10)
        elif kind == "ui":
            # Botón: barra horizontal clara.
            bh = ch // 6
            rounded(
                d,
                (inner_box[0], cy + ch // 2 - bh, inner_box[2], cy + ch // 2 + bh),
                bh,
                (255, 255, 255, 235),
            )
        else:
            # Chispa: rombo.
            mx, my = cx + cw // 2, cy + ch // 2
            r = cw // 3
            d.polygon(
                [(mx, my - r), (mx + r, my), (mx, my + r), (mx - r, my)],
                fill=(255, 255, 255, 235),
            )

    return img


def write_png(img: Image.Image) -> None:
    img.save("packaging/icon.png")
    print("packaging/icon.png", img.size)


def write_ico(img: Image.Image) -> None:
    root = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(root, "windows", "icon.ico")
    img.save(out, format="ICO", sizes=[(s, s) for s in ICO_SIZES])
    print(out, ICO_SIZES)


def write_iconset(img: Image.Image) -> None:
    """PNGs con la nomenclatura de Apple; el workflow de macOS ejecuta
    iconutil -c icns sobre esta carpeta."""
    root = os.path.dirname(os.path.abspath(__file__))
    out_dir = os.path.join(root, "macos", "icon.iconset")
    os.makedirs(out_dir, exist_ok=True)
    for s in ICONSET_SIZES:
        img.resize((s, s), Image.LANCZOS).save(
            os.path.join(out_dir, f"icon_{s}x{s}.png")
        )
        # Variante @2x (retina).
        img.resize((s * 2, s * 2), Image.LANCZOS).save(
            os.path.join(out_dir, f"icon_{s}x{s}@2x.png")
        )
    print(out_dir, "listo para iconutil")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--png", action="store_true", help="solo el PNG maestro")
    ap.add_argument("--ico", action="store_true", help="solo el .ico de Windows")
    ap.add_argument("--iconset", action="store_true", help="solo el iconset de macOS")
    ap.add_argument("--all", action="store_true", help="todo (por defecto)")
    args = ap.parse_args()
    img = build_image()
    if args.png:
        write_png(img)
    if args.ico:
        write_ico(img)
    if args.iconset:
        write_iconset(img)
    if args.all or not (args.png or args.ico or args.iconset):
        write_png(img)
        write_ico(img)
        write_iconset(img)


if __name__ == "__main__":
    main()
