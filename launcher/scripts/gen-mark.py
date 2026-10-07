#!/usr/bin/env python3
"""Writes the lsuite launcher's mark and app icon: a lowercase l cut square beside the suite, four
tiles, the last one dissolving into ordered dither (the grain of the interface), in one colour.
A sibling of the apps' marks (ryolune, kimchi, zenith, nori, folio).

    brand/mark.svg                                ink on paper
    brand/icon.svg                                the app icon (macOS icon grid)
    brand/icon.png                                1024 px, rendered here with Pillow
    crates/lsuite-desktop/resources/lsuite.png    512 px (window icon, Linux)
    crates/lsuite-desktop/assets/icons/mark.svg   the window's copy, in currentColor
"""
import os

from PIL import Image, ImageDraw

# On a 64-unit grid, centred like the apps' marks (x 13-51, y 10-54).
STEM = [(13, 17), (20, 10), (24, 10), (24, 54), (13, 54)]  # l, its top-left corner cut
TILES = [[(29, 32), (39, 32), (39, 42), (29, 42)], [(41, 32), (51, 32), (51, 42), (41, 42)], [(29, 44), (39, 44), (39, 54), (29, 54)]]
LAST = [(41, 44), (51, 44), (51, 54), (41, 54)]  # the fourth tile, dissolving toward its corner
B = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]  # 4x4 Bayer matrix


def dots(cell=2.0, size=1.6):
    out = []
    j = 0
    y = 44.0
    while y < 54 - 0.01:
        i = 0
        x = 41.0
        while x < 51 - 0.01:
            t = ((x - 41) + (y - 44)) / 20  # 0 at the top-left corner, 1 at the bottom-right
            level = 1 - 0.85 * t ** 1.2
            if level > (B[j % 4][i % 4] + 0.5) / 16:
                out.append((x, y, size))
            x += cell
            i += 1
        y += cell
        j += 1
    return out


def pts(p):
    return " ".join(f"{x},{y}" for x, y in p)


def mark(fill="currentColor"):
    return (f'<g fill="{fill}">' + "".join(f'<polygon points="{pts(p)}"/>' for p in [STEM, *TILES])
            + "".join(f'<rect x="{x:.2f}" y="{y:.2f}" width="{s}" height="{s}"/>' for x, y, s in dots()) + "</g>")


def corner_cells():
    out = []
    cell = 16
    for j in range(26):
        for i in range(26):
            d = ((i / 26) ** 2 + (j / 26) ** 2) ** 0.5 / 1.2
            level = max(0, 1 - d * 1.5) ** 1.4
            if level > (B[j % 4][i % 4] + 0.5) / 16:
                out.append((100 + i * cell, 100 + j * cell, cell - 5))
    return out


def icon_svg():
    corner = "".join(f'<rect x="{x}" y="{y}" width="{s}" height="{s}"/>' for x, y, s in corner_cells())
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <!-- The lsuite launcher's icon: an 824 px tile on 1024 in near black, a corner of dithered
       light, and the mark in white. scripts/gen-mark.py writes this file and the PNGs. -->
  <defs><clipPath id="c"><rect x="100" y="100" width="824" height="824" rx="185"/></clipPath></defs>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="#0b0b0b"/>
  <g clip-path="url(#c)" fill="#fff" fill-opacity="0.16">{corner}</g>
  <rect x="100" y="100" width="824" height="824" rx="185" fill="none" stroke="#fff" stroke-opacity="0.16" stroke-width="3"/>
  <g transform="translate(512 512) scale(10.86) translate(-32 -32)">{mark("#fff")}</g>
</svg>'''


def icon_png(size):
    k = 4  # supersampling
    S = 1024 * k
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    tile = Image.new("L", (S, S), 0)
    ImageDraw.Draw(tile).rounded_rectangle([100 * k, 100 * k, 924 * k, 924 * k], radius=185 * k, fill=255)
    img.paste((11, 11, 11, 255), (0, 0), tile)
    light = Image.new("L", (S, S), 0)
    dl = ImageDraw.Draw(light)
    for x, y, s in corner_cells():
        dl.rectangle([x * k, y * k, (x + s) * k - 1, (y + s) * k - 1], fill=int(255 * 0.16))
    light = Image.composite(light, Image.new("L", (S, S), 0), tile)
    img.paste((255, 255, 255, 255), (0, 0), light)
    m = Image.new("L", (S, S), 0)
    dm = ImageDraw.Draw(m)
    f = lambda x, y: ((512 + (x - 32) * 10.86) * k, (512 + (y - 32) * 10.86) * k)
    for p in [STEM, *TILES]:
        dm.polygon([f(x, y) for x, y in p], fill=255)
    for x, y, s in dots():
        a, b = f(x, y), f(x + s, y + s)
        dm.rectangle([a[0], a[1], b[0] - 1, b[1] - 1], fill=255)
    img.paste((255, 255, 255, 255), (0, 0), m)
    return img.resize((size, size), Image.LANCZOS)


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as fh:
        fh.write(text + "\n")


if __name__ == "__main__":
    os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    write("crates/lsuite-desktop/assets/icons/mark.svg", '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">' + mark() + "</svg>")
    write("brand/mark.svg", '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">\n'
          "  <!-- The lsuite launcher's mark: a lowercase l cut square beside four tiles, the last one\n"
          "       dissolving into dither. One colour. scripts/gen-mark.py writes it. -->\n  " + mark("#0a0a0a") + "\n</svg>")
    write("brand/icon.svg", icon_svg())
    os.makedirs("crates/lsuite-desktop/resources", exist_ok=True)
    big = icon_png(1024)
    big.save("brand/icon.png", optimize=True)
    big.resize((512, 512), Image.LANCZOS).save("crates/lsuite-desktop/resources/lsuite.png", optimize=True)
    big.save("crates/lsuite-desktop/resources/lsuite.ico", sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
