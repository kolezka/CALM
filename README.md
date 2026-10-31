![CALM pixel-art logo and infinite canvas tagline](assets/brand/banner.png)

# CALM

An infinite pan and zoom canvas for macOS that hosts live terminals, browsers and plugins
side by side.

![macOS 14+](https://img.shields.io/badge/macOS-14%2B-black?logo=apple)
![Apple Silicon](https://img.shields.io/badge/Apple_Silicon-arm64-555555)
![Rust 1.95+](https://img.shields.io/badge/Rust-1.95%2B-orange?logo=rust)

## What is CALM

CALM replaces a stack of windows and tabs with one large canvas. Draw a rectangle and you get
a real Ghostty terminal or a web browser in it. Arrange panels like notes on a whiteboard,
zoom out to see the whole workspace, zoom in to work. Every panel keeps running while you
pan, zoom or minimize it. Only an explicit, confirmed close ends it.

## Features

### Canvas

- Pan and zoom without limits: pinch, `cmd` and scroll, or the keyboard.
- Zooming scales the view, never the terminal. Rows and columns stay the same.
- Center on the world origin (`cmd-shift-h`) or fit every panel on screen (`cmd-shift-0`),
  both also in the View menu.
- Distant panels turn into still previews that refresh every 2 seconds.
- A translucent status pill at the bottom shows zoom, FPS and plugin errors.
- TUI apps: a plugin folder with one manifest turns a terminal program into an Add menu entry,
  for example "Slack" running `slk`. It runs through your login shell, shows when it exited and
  can restart.
- Fullscreen mode and an FPS counter.

### Panels

- Live Ghostty terminals, optionally using your own Ghostty config.
- Move and resize with snapping to an 8 unit grid and to nearby panel edges.
- Panels placed edge to edge join into one block with a single divider.
- Title bars with a kind icon, focus ring and hover buttons for minimize and close.
- Right-click a title bar for Minimize or Restore, Zoom to Fit, Bring to Front, Send to Back,
  Duplicate (terminals) and Delete.
- Closing a running terminal or a browser asks first. This can be turned off.
- A click on empty canvas, left or right, drops panel focus: the focus ring goes and typing
  goes back to the canvas. Pans and zooms keep focus. Off with
  `canvas.click_empty_clears_focus=false` (Settings, Canvas and Snapping).

### Browser

- Live web pages in a panel, with an address bar, back, forward and reload.
- Type a URL or a host, or anything else to search DuckDuckGo.
- Links and `target=_blank` open in the same panel.
- `cmd-[` back, `cmd-]` forward, `cmd-r` reload, `cmd-l` address bar.

### Plugins

- Installed plugins appear in the right-click Add menu and open as canvas panels.
- Plugins are HTML and JavaScript, or TSX built with bun, talking to CALM through
  `window.calm`.
- Broken plugin folders are skipped and reported in the status pill.

### Customization

- A native Settings window (`cmd-,`) with Background, Grid, Canvas and Snapping, Panels and
  Developer sections.
- Backgrounds: solid color, linear or radial gradients with 2 to 6 stops, or an image with
  cover, contain, tile or center.
- Image filters: blur, brightness, contrast, saturation, grayscale and dim.
- Grid spacing, color and opacity. Snapping on or off, grid step and edge distance.
- Changes show live on the canvas and persist to `settings.json`.

## Requirements

- macOS 14 or newer on Apple Silicon (arm64).
- To build: Rust 1.95 or newer, Zig 0.16, the Xcode command line tools with the Metal
  Toolchain, GNU Make 4 (`gmake`), `gh` and `bun`.

## Install

No release has been published yet. Build from source:

```sh
git clone https://github.com/kolezka/CALM.git
cd CALM
gmake setup-check                 # checks the toolchain
cargo run --release -p calm-app   # add -- --fullscreen to start fullscreen
```

[docs/dev-setup.md](docs/dev-setup.md) lists every prerequisite and how to install it.
Future releases will ship a macOS zip. The app is ad-hoc signed, not notarized, so on first
launch right-click it and pick Open. See [docs/release.md](docs/release.md).

## Usage

| Action                          | How                                       |
| ------------------------------- | ----------------------------------------- |
| Draw a terminal                 | `t`, then drag on the canvas              |
| Draw a browser                  | `b`, then drag on the canvas              |
| Cancel drawing                  | `Esc`                                     |
| Add a panel or plugin           | Right-click empty canvas                  |
| Panel actions                   | Right-click a title bar                   |
| Pan                             | Space and drag, middle-drag, scroll       |
| Zoom                            | Pinch, or `cmd` and scroll                |
| Zoom in, out, reset             | `cmd-=`, `cmd--`, `cmd-0`                 |
| Center on origin                | `cmd-shift-h`                             |
| Fit all panels                  | `cmd-shift-0`                             |
| Move or resize without snapping | Hold `alt` while dragging                 |
| Settings                        | `cmd-,`                                   |
| Fullscreen                      | `cmd-ctrl-f`                              |
| Show FPS                        | `cmd-shift-f`                             |

Scrolling over empty canvas pans it. Scrolling over a terminal scrolls the terminal.

## Plugins

A plugin is a folder with a `calm-plugin.json` manifest and an HTML entry page. CALM finds it
at startup, lists it in the Add menu and runs it in a panel. Plugins are trusted code that you
install yourself, with full access, so only install plugins you have read or whose author you
trust. The bridge is partial today: `log`, `panel.setTitle`, `settings.get` and `settings.set`
answer; the other methods reply `not implemented`.

From a clone, `gmake plugins && cargo run -p calm-app` builds the first-party plugins and shows
them under right-click > Add. A development build reads the repository's `plugins/` folder
directly; installed plugins go in `~/.config/calm/plugins`. Set `CALM_NO_BUILTIN_PLUGINS=1` to
hide the built-in ones.

A TUI app plugin needs no HTML: its manifest names a command, and CALM runs it in a terminal
panel. See "TUI apps" in [docs/plugins.md](docs/plugins.md#tui-apps) and
`templates/plugin-tui`.

```json
{
  "id": "hello-world",
  "name": "Hello World",
  "version": "0.1.0",
  "entry": "index.html",
  "defaultSize": { "width": 320, "height": 200 }
}
```

Read [docs/plugins.md](docs/plugins.md) for the manifest and the `window.calm` API, and start
from the TSX template in [templates/plugin-tsx](templates/plugin-tsx/README.md).

## Configuration

- Settings live in `~/.config/calm/settings.json`.
- `CALM_HOME` replaces `~/.config/calm` for settings and plugins.
- `CALM_PLUGIN_PATH` adds plugin folders, `:` separated like `PATH`. For development:
  `CALM_PLUGIN_PATH=./plugins cargo run -p calm-app`.
- Without it, plugins load from `$CALM_HOME/plugins` or `~/.config/calm/plugins`.

## Project status

CALM is an early prototype. Nothing has been released and the GUI has had limited manual
testing.

In progress, not on `main` yet:

- Grid styles: dots, crosses and a major grid.
- Resizing browser and plugin panels from outside the web view.
- Loading first-party plugins without `CALM_PLUGIN_PATH`.
- TUI apps as plugins that run a command in a terminal panel, and their customization.

Not built yet: saving and restoring layouts or sessions (a plan exists), a benchmark mode,
and most of the plugin bridge.

## Documentation

- [Developer setup](docs/dev-setup.md): prerequisites, running, tests, make targets.
- [Architecture](docs/architecture.md): terminals, native views and previews.
- [Plugins](docs/plugins.md): manifest, `window.calm` API and the trust model.
- [Releases and CI](docs/release.md): CI jobs, cutting a release, the macOS bundle.
- [Design spec](docs/superpowers/specs/2026-10-31-calm-design.md) and
  [prototype plan](docs/superpowers/plans/2026-10-31-calm-prototype.md).
- [Plugin API v2 design](docs/superpowers/specs/2026-10-31-plugin-api-v2-design.md)
  (design only, not implemented).
- [Canvas persistence plan](docs/superpowers/plans/2026-10-31-canvas-persistence.md)
  (plan only, not implemented).
- [Claude Code mods integration](docs/superpowers/specs/2026-10-31-claude-code-mods-integration.md)
  (research).

## Contributing

Set up with [docs/dev-setup.md](docs/dev-setup.md). Work on a branch, run
`cargo test -p <crate>` while iterating, and pass `gmake ci` (clippy, fmt, tests, smoke and
plugin type checks) before opening a pull request.
