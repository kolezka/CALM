# CALM web

The product page for CALM. One static Astro page, no subpages.

## Style source

The look comes from `assets/brand`, not from a separate design system:

- Palette: `Night #090D1D`, `Plate #101831`, `Grid #25335A`, `Edge #455A88`,
  `Neon cyan #48F5E7`, `Neon magenta #FF4FD8`, `Letter white #E7F6FF`.
- Wordmark letters: the 5x7 pixel glyph table copied from
  `assets/brand/build.py` into `src/lib/glyphs.js`, drawn as SVG cells by
  `src/components/PixelText.astro`. The table has only A to Z, space, comma,
  underscore and `>`. Add a glyph to both files if you need more.
- Images in `public/` are copied from `assets/brand`. If the brand images
  change, copy them again.

The hero canvas is a CSS illustration of the app, not a screenshot.

## Commands

```sh
bun install
bun run dev      # http://localhost:4321
bun run build    # writes dist/
bun run preview
```

## Copy rules

The page states plainly that no release exists and the app is built from
source. Keep that true. Do not claim a download, a sandbox, or a Linux build.
