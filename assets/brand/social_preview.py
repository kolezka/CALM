#!/usr/bin/env python3
"""Render CALM's GitHub social preview card (1280x640).

GitHub shows this image when the repo link is shared. It is uploaded by hand in
the repository settings; this script only produces the PNG so the result is
reproducible instead of a one-off export.

Glyphs and palette come from build.py. Only letters, space, '>' and '_' exist in
that glyph grid, so no digits or punctuation in the copy.
"""

from pathlib import Path

from PIL import Image, ImageDraw

import build as brand

ROOT = Path(__file__).resolve().parent

WIDTH = 1280
HEIGHT = 640
WORD = "CALM"
TAGLINE = "INFINITE CANVAS FOR TERMINALS, BROWSERS AND PLUGINS"
FOOTER = "RUST GPUI MACOS"


def text_width(text, unit):
    """Pixel width of a glyph run: each char is 5 cells wide, 1 cell gutter."""
    return (len(text) * 6 - 1) * unit


def draw_wordmark(d, unit, x, y):
    ring = 3
    for px, py, size in brand.pixels(WORD, x, y, unit):
        d.rectangle((px - ring, py - ring, px + size + ring, py + size + ring), fill=brand.GLOW)
    for dx, dy, color in ((-5, 0, brand.MAGENTA), (5, 5, brand.CYAN), (0, 0, brand.WHITE)):
        brand.paint_text(d, WORD, x + dx, y + dy, unit, color)


def draw_dots(d):
    for x in range(16, WIDTH, 32):
        for y in range(16, HEIGHT, 32):
            d.rectangle((x, y, x + 1, y + 1), fill=brand.GRID)


def build_card():
    image = Image.new("RGB", (WIDTH, HEIGHT), brand.NIGHT)
    d = ImageDraw.Draw(image)

    draw_dots(d)

    # Outer frame with two accent segments, same language as the README banner.
    d.rectangle((40, 40, WIDTH - 40, HEIGHT - 40), outline=brand.EDGE, width=2)
    d.rectangle((40, 40, 240, 43), fill=brand.MAGENTA)
    d.rectangle((40, HEIGHT - 43, 440, HEIGHT - 40), fill=brand.CYAN)

    # Corner pixel brackets.
    for x, y, sx, sy, color in (
        (64, 64, 1, 1, brand.CYAN),
        (WIDTH - 64, 64, -1, 1, brand.MAGENTA),
        (64, HEIGHT - 64, 1, -1, brand.MAGENTA),
        (WIDTH - 64, HEIGHT - 64, -1, -1, brand.CYAN),
    ):
        d.rectangle((min(x, x + sx * 48), min(y, y + sy * 4), max(x, x + sx * 48), max(y, y + sy * 4)), fill=color)
        d.rectangle((min(x, x + sx * 4), min(y, y + sy * 48), max(x, x + sx * 4), max(y, y + sy * 48)), fill=color)

    unit = 22
    word_width = text_width(WORD, unit)
    word_x = (WIDTH - word_width) // 2
    word_y = 120
    draw_wordmark(d, unit, word_x, word_y)

    # Underline bar under the wordmark: cyan, magenta, then a muted tail.
    bar_y = word_y + 7 * unit + 44
    bar_x = word_x + 20
    segments = ((260, brand.CYAN), (110, brand.MAGENTA), (140, brand.GRID))
    cursor = bar_x
    for length, color in segments:
        d.rectangle((cursor, bar_y, cursor + length, bar_y + 5), fill=color)
        cursor += length + 12

    # Tagline must fit inside the frame at 40 px inset. 54 chars at unit 4 is
    # 1292 px, wider than the card, so 3 is the size that fits.
    tagline_unit = 3
    tagline_x = (WIDTH - text_width(TAGLINE, tagline_unit)) // 2
    brand.paint_text(d, TAGLINE, tagline_x, bar_y + 52, tagline_unit, brand.WHITE)

    footer_unit = 3
    footer_x = (WIDTH - text_width(FOOTER, footer_unit)) // 2
    brand.paint_text(d, FOOTER, footer_x, bar_y + 116, footer_unit, brand.EDGE)

    return image


def main():
    image = build_card()
    target = ROOT / "social-preview.png"
    image.save(target)
    print(f"Wrote {target.name} {image.width}x{image.height}")


if __name__ == "__main__":
    main()
