#!/usr/bin/env python3
"""Generate CALM's animated wordmark.

Letters stay still like a neon sign already lit. Everything around them moves:
the grid carries a slow scan wave, sparse pixel rain falls behind the word, the
dark bar scrolls like a data tape, corner brackets blink, a terminal cursor box
sits on the right and debris pixels flicker.

Glyphs and palette come from build.py so there is one source of truth. PIL is
stubbed out because this script only emits SVG and should not need Pillow.
"""

import importlib.util
import sys
import types
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def load_build():
    if "PIL" not in sys.modules:
        stub = types.ModuleType("PIL")
        stub.Image = object
        stub.ImageDraw = object
        sys.modules["PIL"] = stub
    spec = importlib.util.spec_from_file_location("brand_build", ROOT / "build.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


brand = load_build()

WIDTH = 356
HEIGHT = 132
UNIT = 12
TEXT_X = 30
TEXT_Y = 24
WORD = "CALM"


def rect(x, y, width, height, fill, extra=""):
    return (
        f'<rect x="{x}" y="{y}" width="{width}" height="{height}" '
        f'fill="{fill}"{extra}/>'
    )


def letter_shapes():
    """Static wordmark: glow ring, magenta split, cyan split, white face."""
    shapes = []
    for px, py, size in brand.pixels(WORD, TEXT_X, TEXT_Y, UNIT):
        shapes.append(rect(px - 2, py - 2, size + 4, size + 4, brand.GLOW))
    for px, py, size in brand.pixels(WORD, TEXT_X, TEXT_Y, UNIT):
        shapes.append(rect(px - 3, py, size, size, brand.MAGENTA))
    for px, py, size in brand.pixels(WORD, TEXT_X, TEXT_Y, UNIT):
        shapes.append(rect(px + 3, py + 3, size, size, brand.CYAN))
    for px, py, size in brand.pixels(WORD, TEXT_X, TEXT_Y, UNIT):
        shapes.append(rect(px, py, size, size, brand.WHITE))
    return shapes


def grid_dots():
    """Same dot grid as logo.svg, each dot gets a delayed pulse.

    Every animation period divides 12 s, the loop length the README GIF is
    rendered from. Without a common period the GIF jumps at the wrap.
    """
    dots = []
    columns = list(range(8, WIDTH, 24))
    for column, gx in enumerate(columns):
        for row, gy in enumerate(range(8, HEIGHT, 24)):
            delay = round((column * 0.16 + row * 0.28) % 4.0, 2)
            dots.append(
                rect(gx, gy, 2, 2, brand.GRID, f' class="dot" style="animation-delay:-{delay}s"')
            )
    return dots


def underline_bar():
    """Solid segments from the static logo, then a scrolling tape on the dark one."""
    shapes = [
        rect(29, 115, 120, 3, brand.CYAN),
        rect(158, 115, 60, 3, brand.MAGENTA),
        rect(228, 115, 98, 3, brand.GRID),
    ]
    ticks = []
    for x in range(228, 326, 8):
        ticks.append(rect(x, 115, 2, 3, brand.EDGE))
    shapes.append(f'<g clip-path="url(#barClip)" class="tape">{"".join(ticks)}</g>')
    shapes.append(
        f'<g clip-path="url(#barClip)">'
        f'{rect(29, 115, 12, 3, brand.WHITE, " class=\"tick\"")}'
        "</g>"
    )
    return shapes


def brackets():
    """Pixel corner marks that blink out of rhythm with each other."""
    arms = 18
    thickness = 2
    inset = 10
    corners = [
        (inset, inset, brand.CYAN, "-1s", "12s"),
        (WIDTH - inset - arms, inset, brand.MAGENTA, "-4s", "6s"),
        (inset, HEIGHT - inset - thickness, brand.MAGENTA, "-7s", "4s"),
        (WIDTH - inset - arms, HEIGHT - inset - thickness, brand.CYAN, "-3s", "3s"),
    ]
    shapes = []
    for x, y, color, delay, duration in corners:
        style = f'style="animation-delay:{delay};animation-duration:{duration}"'
        shapes.append(rect(x, y, arms, thickness, color, f' class="bracket" {style}'))
        vertical_y = y if y < HEIGHT / 2 else y - arms + thickness
        shapes.append(
            rect(x, vertical_y, thickness, arms, color, f' class="bracket" {style}')
        )
    return shapes


def fog_blocks():
    """Coarse pixel haze at the edges. Dither, not a gradient: smooth ramps break
    the pixelart look the wordmark depends on."""
    shapes = []
    left_dots = [(2, 0.5), (6, 0.35), (10, 0.22), (14, 0.12), (18, 0.06)]
    right_dots = [(WIDTH - 4 - x, s) for x, s in left_dots]
    for x, strength in left_dots:
        for y in range(4, HEIGHT - 4, 8):
            fill = brand.CYAN if strength > 0.25 else brand.GLOW
            opacity = round(min(0.5, strength), 2)
            shapes.append(rect(x, y, 3, 3, fill, f' opacity="{opacity}"'))
    for x, strength in right_dots:
        for y in range(4, HEIGHT - 4, 8):
            fill = brand.MAGENTA if strength > 0.25 else brand.GLOW
            opacity = round(min(0.5, strength), 2)
            shapes.append(rect(x, y, 3, 3, fill, f' opacity="{opacity}"'))
    return shapes


def pixel_rain():
    """Sparse glyph-free pixels falling behind the word. Deterministic columns so
    the file rebuilds identically."""
    shapes = []
    streams = [
        (22, 0, 6, brand.CYAN),
        (46, -3, 4, brand.GLOW),
        (86, -6, 12, brand.CYAN),
        (128, -1, 6, brand.GLOW),
        (170, -8, 8, brand.CYAN),
        (214, -4, 12, brand.MAGENTA),
        (252, -7, 6, brand.CYAN),
        (300, -2, 4, brand.GLOW),
        (334, -5, 12, brand.MAGENTA),
    ]
    for x, delay, duration, color in streams:
        trail = []
        for step in range(6):
            opacity = round(0.55 - step * 0.08, 2)
            trail.append(
                rect(x, -8 - step * 7, 2, 5, color, f' opacity="{opacity}"')
            )
        shapes.append(
            '<g class="rain" style="--rain-duration: %ss; --rain-delay: %ss">%s</g>'
            % (duration, delay, "".join(trail))
        )
    return shapes


def cursor_block():
    """A block cursor in the empty right side, blinking like a waiting terminal."""
    return [
        rect(316, 44, 22, 26, "none", f' stroke="{brand.CYAN}" stroke-width="2"'),
        rect(316, 44, 22, 4, brand.CYAN),
        rect(322, 53, 10, 2, brand.WHITE, ' class="cursor-bar"'),
        rect(322, 58, 6, 2, brand.WHITE, ' class="cursor-bar slow"'),
        rect(322, 63, 8, 2, brand.WHITE, ' class="cursor-bar"'),
    ]


def debris():
    """Loose pixels that flare and die at irregular spots."""
    spots = [
        (14, 26, -.4, 6, brand.CYAN),
        (96, 118, -2.6, 4, brand.MAGENTA),
        (190, 12, -5.1, 12, brand.CYAN),
        (246, 121, -3.4, 6, brand.GLOW),
        (310, 16, -6.8, 4, brand.MAGENTA),
        (336, 96, -1.2, 12, brand.CYAN),
        (62, 100, -4.9, 6, brand.GLOW),
    ]
    shapes = []
    for x, y, delay, duration, color in spots:
        shapes.append(
            rect(
                x,
                y,
                2,
                2,
                color,
                f' class="debris" style="animation-delay:{delay}s;'
                f'animation-duration:{duration}s"',
            )
        )
    return shapes


def css():
    return """
/* Every period divides 12 s so the README GIF can loop without a jump. */
.dot { opacity: .22; animation: dotWave 6s ease-in-out infinite; }
.bracket { opacity: .3; animation: blink 12s steps(1, end) infinite; }
.tape { animation: tape 2.4s linear infinite; }
.tick { animation: tick 4s linear infinite; }
.scan { opacity: .07; animation: scanDown 12s steps(66, end) infinite; }
.haze-left { animation: fog 12s ease-in-out infinite; }
.haze-right { animation: fog 6s ease-in-out infinite; animation-delay: -3s; }
.debris { animation: flare 6s steps(1, end) infinite; opacity: 0; }
.cursor-bar { animation: cursorFlicker 1.2s steps(1, end) infinite; }
.cursor-bar.slow { animation-duration: 3s; animation-delay: -.5s; }
.rain {
  animation: rain var(--rain-duration, 8s) linear infinite;
  animation-delay: var(--rain-delay, 0s);
}
@keyframes dotWave {
  0%, 100% { opacity: .18 }
  50% { opacity: .8 }
}
@keyframes blink {
  0%, 86%, 100% { opacity: .3 }
  88%, 95% { opacity: 1 }
}
@keyframes tape {
  from { transform: translateX(0) }
  to { transform: translateX(-8px) }
}
@keyframes tick {
  0% { transform: translateX(0); opacity: 0 }
  12% { opacity: .9 }
  88% { opacity: .9 }
  100% { transform: translateX(119px); opacity: 0 }
}
@keyframes scanDown {
  0% { transform: translateY(4px) }
  100% { transform: translateY(128px) }
}
@keyframes fog {
  0%, 100% { opacity: .35 }
  50% { opacity: .8 }
}
@keyframes rain {
  0% { transform: translateY(0) }
  100% { transform: translateY(150px) }
}
@keyframes flare {
  0%, 100% { opacity: 0 }
  4%, 9% { opacity: 1 }
  12% { opacity: 0 }
}
@keyframes cursorFlicker {
  0%, 49% { opacity: 1 }
  50%, 100% { opacity: .12 }
}
@media (prefers-reduced-motion: reduce) {
  .dot, .bracket, .tape, .tick, .scan, .haze-left, .haze-right,
  .debris, .cursor-bar, .rain {
    animation: none;
  }
  .scan, .tick, .debris { opacity: 0; }
  .cursor-bar { opacity: 1; }
}
"""


def animated_svg(transparent=False):
    defs = (
        '<defs>'
        '<clipPath id="barClip"><rect x="29" y="113" width="297" height="7"/></clipPath>'
        f"<style>{css()}</style>"
        "</defs>"
    )

    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" '
        f'viewBox="0 0 {WIDTH} {HEIGHT}" shape-rendering="crispEdges" role="img" '
        'aria-label="CALM animated wordmark">',
        "<title>CALM, infinite canvas for terminals, browsers and plugins</title>",
        defs,
    ]
    if not transparent:
        parts.append(rect(0, 0, WIDTH, HEIGHT, brand.NIGHT))
    parts.extend([
        '<g class="grid">',
        *grid_dots(),
        "</g>",
        '<g class="rain-layer">',
        *pixel_rain(),
        "</g>",
        '<g class="haze-left">',
        *fog_blocks(),
        "</g>",
        '<g class="letters">',
        *letter_shapes(),
        "</g>",
        *underline_bar(),
        *cursor_block(),
        *brackets(),
        *debris(),
        rect(0, 0, WIDTH, 1, brand.CYAN, ' class="scan"'),
        "</svg>",
    ])
    return "\n".join(parts)


def main():
    outputs = (
        ("logo-animated.svg", False),
        ("logo-animated-transparent.svg", True),
    )
    for name, transparent in outputs:
        (ROOT / name).write_text(animated_svg(transparent=transparent), encoding="utf-8")
        print(f"Wrote {name}")


if __name__ == "__main__":
    main()
