# CALM brand assets

The logo, banner and macOS icon are generated from pixel grids in `build.py`. No fonts
or image files are inputs. The 16 and 32 pixel icon compositions are drawn separately so
small sizes stay readable. `logo-light.svg` uses dark letters on a light plate; `logo.svg`
has a dark plate and reads on either page background.

| Role | Hex |
| --- | --- |
| Night | `#090D1D` |
| Plate | `#101831` |
| Grid | `#25335A` |
| Edge | `#455A88` |
| Pixel glow | `#344A72` |
| Neon cyan | `#48F5E7` |
| Neon magenta | `#FF4FD8` |
| Letter white | `#E7F6FF` |
| Light-plate ink | `#172042` |

From the repository root:

```sh
uv run --with pillow python3 assets/brand/build.py
iconutil -c icns assets/brand/CALM.iconset -o assets/brand/CALM.icns
python3 assets/brand/animate.py
```

`CALM.iconset/` contains every macOS required size (16 through 1024 pixels). The bundle script copies `CALM.icns` into `Contents/Resources`.

## Animated wordmark

`animate.py` emits two SVG files from the same glyphs and palette as `build.py`:

- `logo-animated.svg`: same 356x132 frame as `logo.svg`, night background.
- `logo-animated-transparent.svg`: no background plate, so it sits on any surface.

The letters are static, a neon sign already lit. Motion lives around them: a scan wave
across the dot grid, pixel rain behind the word, a scrolling data tape under it, blinking
corner brackets, a terminal cursor block, and flickering debris. CSS keyframes inside the
file, no JavaScript. `prefers-reduced-motion` stops every animation.

The script imports `build.py` with a stub for `PIL`, so it needs no third-party packages:

```sh
python3 assets/brand/animate.py
```

GitHub does not play SVG animation in a README, so a README banner needs a GIF or MP4
rendered from these files.

## Social preview card

`social_preview.py` renders `social-preview.png` at 1280x640, the size GitHub asks for
(minimum 640x320). It uses the same glyph grid and palette as `build.py`:

```sh
uv run --with pillow python3 assets/brand/social_preview.py
```

The PNG is generated, not exported by hand. GitHub takes the image from the repository
settings page (Settings, General, Social preview), so uploading it stays a manual step.

