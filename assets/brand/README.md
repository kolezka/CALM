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
```

`CALM.iconset/` contains every macOS required size (16 through 1024 pixels). The bundle script copies `CALM.icns` into `Contents/Resources`.
