#!/usr/bin/env python3
"""Build CALM's pixel-grid wordmark, banner and macOS icon from source data."""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent
ICONSET = ROOT / "CALM.iconset"

NIGHT = "#090D1D"
PLATE = "#101831"
GRID = "#25335A"
EDGE = "#455A88"
GLOW = "#344A72"
CYAN = "#48F5E7"
MAGENTA = "#FF4FD8"
WHITE = "#E7F6FF"
LAVENDER = "#B8A8FF"
DARK_INK = "#172042"

# Five by seven, every visible letter is drawn as pixels rather than a font.
GLYPHS = {
    " ": ("00000",) * 7,
    "A": ("01110", "10001", "10001", "11111", "10001", "10001", "10001"),
    "B": ("11110", "10001", "10001", "11110", "10001", "10001", "11110"),
    "C": ("01111", "10000", "10000", "10000", "10000", "10000", "01111"),
    "D": ("11110", "10001", "10001", "10001", "10001", "10001", "11110"),
    "E": ("11111", "10000", "10000", "11110", "10000", "10000", "11111"),
    "F": ("11111", "10000", "10000", "11110", "10000", "10000", "10000"),
    "G": ("01111", "10000", "10000", "10111", "10001", "10001", "01111"),
    "H": ("10001", "10001", "10001", "11111", "10001", "10001", "10001"),
    "I": ("11111", "00100", "00100", "00100", "00100", "00100", "11111"),
    "J": ("00111", "00010", "00010", "00010", "10010", "10010", "01100"),
    "K": ("10001", "10010", "10100", "11000", "10100", "10010", "10001"),
    "L": ("10000", "10000", "10000", "10000", "10000", "10000", "11111"),
    "M": ("10001", "11011", "10101", "10101", "10001", "10001", "10001"),
    "N": ("10001", "11001", "10101", "10101", "10011", "10001", "10001"),
    "O": ("01110", "10001", "10001", "10001", "10001", "10001", "01110"),
    "P": ("11110", "10001", "10001", "11110", "10000", "10000", "10000"),
    "Q": ("01110", "10001", "10001", "10001", "10101", "10010", "01101"),
    "R": ("11110", "10001", "10001", "11110", "10100", "10010", "10001"),
    "S": ("01111", "10000", "10000", "01110", "00001", "00001", "11110"),
    "T": ("11111", "00100", "00100", "00100", "00100", "00100", "00100"),
    "U": ("10001", "10001", "10001", "10001", "10001", "10001", "01110"),
    "V": ("10001", "10001", "10001", "10001", "10001", "01010", "00100"),
    "W": ("10001", "10001", "10001", "10101", "10101", "10101", "01010"),
    "X": ("10001", "10001", "01010", "00100", "01010", "10001", "10001"),
    "Y": ("10001", "10001", "01010", "00100", "00100", "00100", "00100"),
    "Z": ("11111", "00001", "00010", "00100", "01000", "10000", "11111"),
    ">": ("10000", "01000", "00100", "00010", "00100", "01000", "10000"),
    "_": ("00000", "00000", "00000", "00000", "00000", "00000", "11111"),
    ",": ("00000", "00000", "00000", "00000", "00110", "00100", "01000"),
}


def pixels(text, x, y, unit):
    for index, char in enumerate(text.upper()):
        for row, bits in enumerate(GLYPHS[char]):
            for col, bit in enumerate(bits):
                if bit == "1":
                    yield x + (index * 6 + col) * unit, y + row * unit, unit


def paint_text(draw, text, x, y, unit, color):
    for px, py, size in pixels(text, x, y, unit):
        draw.rectangle((px, py, px + size - 1, py + size - 1), fill=color)


def rect_svg(x, y, width, height, color):
    return f'<rect x="{x}" y="{y}" width="{width}" height="{height}" fill="{color}"/>'


def logo_svg(light=False):
    bg = "#F2F7FF" if light else NIGHT
    main = DARK_INK if light else WHITE
    muted = "#C8D5E9" if light else GRID
    unit, width, height, x, y = 12, 356, 132, 30, 24
    shapes = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" shape-rendering="crispEdges" role="img" aria-label="CALM">', rect_svg(0, 0, width, height, bg)]
    for gx in range(8, width, 24):
        for gy in range(8, height, 24):
            shapes.append(rect_svg(gx, gy, 2, 2, muted))
    ring = "#B3C9DA" if light else GLOW
    for px, py, size in pixels("CALM", x, y, unit):
        shapes.append(rect_svg(px - 2, py - 2, size + 4, size + 4, ring))
    for dx, dy, color in ((-3, 0, MAGENTA), (3, 3, CYAN), (0, 0, main)):
        for px, py, size in pixels("CALM", x + dx, y + dy, unit):
            shapes.append(rect_svg(px, py, size, size, color))
    shapes += [rect_svg(29, 115, 120, 3, CYAN), rect_svg(158, 115, 60, 3, MAGENTA), rect_svg(228, 115, 98, 3, muted), "</svg>"]
    return "\n".join(shapes) + "\n"


def make_logo_png():
    image = Image.new("RGB", (356, 132), NIGHT)
    draw = ImageDraw.Draw(image)
    for gx in range(8, 356, 24):
        for gy in range(8, 132, 24):
            draw.rectangle((gx, gy, gx + 1, gy + 1), fill=GRID)
    for px, py, size in pixels("CALM", 30, 24, 12):
        draw.rectangle((px - 2, py - 2, px + size + 1, py + size + 1), fill=GLOW)
    for dx, dy, color in ((-3, 0, MAGENTA), (3, 3, CYAN), (0, 0, WHITE)):
        paint_text(draw, "CALM", 30 + dx, 24 + dy, 12, color)
    for start, end, color in ((29, 148, CYAN), (158, 217, MAGENTA), (228, 325, GRID)):
        draw.rectangle((start, 115, end, 117), fill=color)
    image.save(ROOT / "logo@1x.png")
    image.resize((1424, 528), Image.Resampling.NEAREST).save(ROOT / "logo@4x.png")
    return image


def make_banner(logo):
    banner = Image.new("RGB", (1280, 320), NIGHT)
    d = ImageDraw.Draw(banner)
    for x in range(16, 1280, 32):
        for y in range(16, 320, 32):
            d.rectangle((x, y, x + 1, y + 1), fill=GRID)
    d.rectangle((38, 34, 1238, 284), outline=EDGE, width=2)
    d.rectangle((38, 34, 242, 37), fill=MAGENTA)
    d.rectangle((38, 281, 498, 284), fill=CYAN)
    banner.paste(logo.resize((534, 198), Image.Resampling.NEAREST), (62, 28))
    # The tagline also uses the hand-drawn glyph grid, without installed fonts.
    tagline = "INFINITE CANVAS FOR TERMINALS, BROWSERS AND PLUGINS"
    paint_text(d, tagline, 80, 246, 2, WHITE)
    paint_text(d, ">_", 1116, 74, 9, CYAN)
    d.rectangle((1095, 55, 1229, 175), outline=MAGENTA, width=3)
    d.rectangle((1095, 55, 1229, 60), fill=MAGENTA)
    d.rectangle((1060, 193, 1186, 195), fill=CYAN)
    banner.save(ROOT / "banner.png")


def icon_base(size):
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(image)
    if size == 16:
        d.rounded_rectangle((0, 0, 15, 15), radius=3, fill=PLATE)
        for pos in (5, 10):
            d.line((pos, 2, pos, 13), fill=GRID)
            d.line((2, pos, 13, pos), fill=GRID)
        # A deliberate one-pixel glyph; the prompt is omitted at this size.
        for row, bits in enumerate(GLYPHS["C"]):
            for col, bit in enumerate(bits):
                if bit == "1":
                    d.point((col + 5, row + 4), fill=CYAN)
        d.rectangle((11, 4, 12, 5), fill=MAGENTA)
        d.point((11, 11), fill=MAGENTA)
    elif size == 32:
        d.rounded_rectangle((1, 1, 30, 30), radius=6, fill=PLATE, outline=EDGE)
        for pos in (10, 20):
            d.line((pos, 4, pos, 28), fill=GRID)
            d.line((4, pos, 28, pos), fill=GRID)
        d.rectangle((4, 5, 21, 18), fill=NIGHT, outline=CYAN)
        d.rectangle((4, 5, 21, 7), fill=CYAN)
        paint_text(d, ">_", 6, 9, 1, WHITE)
        d.rectangle((23, 7, 28, 16), fill=NIGHT, outline=MAGENTA)
        d.rectangle((6, 21, 25, 28), fill=NIGHT, outline=EDGE)
        paint_text(d, "C", 13, 21, 1, MAGENTA)
    else:
        d.rounded_rectangle((2, 2, 61, 61), radius=12, fill=PLATE, outline=EDGE, width=2)
        for pos in (15, 30, 45):
            d.line((pos, 6, pos, 58), fill=GRID)
            d.line((6, pos, 58, pos), fill=GRID)
        d.rectangle((8, 11, 37, 35), fill=NIGHT, outline=CYAN, width=2)
        d.rectangle((8, 11, 37, 14), fill=CYAN)
        paint_text(d, ">_", 11, 18, 2, WHITE)
        d.rectangle((41, 13, 56, 35), fill=NIGHT, outline=MAGENTA, width=2)
        d.rectangle((43, 16, 54, 18), fill=MAGENTA)
        d.rectangle((43, 22, 51, 24), fill=EDGE)
        d.rectangle((12, 39, 53, 56), fill=NIGHT, outline=EDGE, width=2)
        paint_text(d, "C", 26, 41, 2, MAGENTA)
        d.rectangle((15, 44, 19, 46), fill=CYAN)
        d.rectangle((44, 48, 49, 50), fill=CYAN)
    return image


def make_icons():
    ICONSET.mkdir(exist_ok=True)
    bases = {s: icon_base(s) for s in (16, 32, 64)}
    bases[64].resize((1024, 1024), Image.Resampling.NEAREST).save(ROOT / "icon-1024.png")
    for size in (16, 32, 64, 128, 256, 512):
        source = bases[min(size, 64)]
        for scale, suffix in ((1, ""), (2, "@2x")):
            edge = size * scale
            source.resize((edge, edge), Image.Resampling.NEAREST).save(
                ICONSET / f"icon_{size}x{size}{suffix}.png"
            )


def main():
    ROOT.joinpath("logo.svg").write_text(logo_svg(), encoding="utf-8")
    ROOT.joinpath("logo-light.svg").write_text(logo_svg(light=True), encoding="utf-8")
    make_banner(make_logo_png())
    make_icons()
    print("Built CALM wordmark, banner and 12 iconset PNGs")


if __name__ == "__main__":
    main()
