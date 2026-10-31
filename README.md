# CALM

An infinite pan and zoom canvas for macOS where live terminals sit side by side like notes on a
whiteboard.

## What it is

CALM gives you one big canvas instead of a stack of windows and tabs. You draw a rectangle and get
a real Ghostty terminal in it. Move panels around, zoom out to see everything, zoom in to work.
Every terminal keeps running while you pan, zoom or minimize it.

## Features

- Live Ghostty terminals on a canvas you can pan and zoom without limits.
- Zooming scales the view, not the terminal: the grid of rows and columns stays the same.
- Move and resize panels, with snapping to an 8 point grid and to nearby panel edges.
- Distant panels turn into still previews that refresh every 2 seconds.
- Minimize and restore panels from the right-click menu.
- Closing a running terminal asks first (can be turned off in Settings).
- Settings menu: background, grid on or off, close confirmation, and whether terminals use your
  own Ghostty config. Saved to `~/.config/calm/settings.json`.
- Fullscreen mode and an FPS counter.

## Requirements

- macOS 14 or newer
- Apple Silicon (arm64)

## Install and run

There is no packaged release yet. Build it from source:

```sh
git clone https://github.com/kolezka/CALM.git
cd CALM
gmake setup-check                 # checks Rust, Zig 0.16, Xcode tools, gh, bun
cargo run --release -p calm-app   # add -- --fullscreen to start fullscreen
```

The build needs Rust 1.95 or newer, Zig 0.16, the Xcode command line tools with the Metal
Toolchain, and GNU Make 4. [docs/dev-setup.md](docs/dev-setup.md) lists every prerequisite and
how to install it.

When releases exist, each one will carry a macOS zip. The app is ad-hoc signed, not notarized,
so macOS blocks it on first launch: right-click it and pick Open. See
[docs/release.md](docs/release.md).

## Keyboard and mouse

| Action                          | How                                 |
| ------------------------------- | ----------------------------------- |
| Draw a terminal                 | `t`, then drag on the canvas        |
| Cancel drawing                  | `Esc`                               |
| Add a panel, minimize or close  | Right-click                         |
| Pan                             | Space and drag, middle-drag, scroll |
| Zoom                            | Pinch, or `cmd` and scroll          |
| Zoom in, out, reset             | `cmd =`, `cmd -`, `cmd 0`           |
| Move or resize without snapping | Hold `alt` while dragging           |
| Fullscreen                      | `cmd ctrl f`                        |
| Show FPS                        | `cmd shift f`                       |

Scrolling over empty canvas pans it. Scrolling over a terminal scrolls the terminal.

## Plugins

Plugins are small HTML and JavaScript apps that run in a canvas panel and talk to CALM through
`window.calm`. They are trusted code that you install yourself, with full access, so only install
plugins you have read or whose author you trust. The plugin format and API are described in
[docs/plugins.md](docs/plugins.md); showing plugin panels in the app is still in progress.

## Status

CALM is an early prototype. Nothing has been released, and some parts are not finished.

Working today: terminals, pan and zoom, previews, snapping, settings, fullscreen.

In progress, not on `main` yet:

- Live browser panels with an address bar. Today a browser panel is an empty frame.
- Plugins listed in the Add menu and shown as panels.
- Panel actions in the right-click menu and redesigned panel title bars.
- A translucent status bar at the bottom of the window.
- Keys to jump back to the origin or fit all panels on screen.
- A Settings window with color, gradient and image backgrounds.

Not built yet: saving and loading layouts, and a benchmark mode.

## Docs

- [Developer setup](docs/dev-setup.md): prerequisites, running, tests, make targets.
- [Architecture](docs/architecture.md): how terminals, native views and previews fit together.
- [Plugins](docs/plugins.md): manifest, `window.calm` API and the trust model.
- [Releases and CI](docs/release.md): CI jobs, cutting a release, the macOS bundle.
- [Design spec](docs/superpowers/specs/2026-10-31-calm-design.md) and
  [prototype plan](docs/superpowers/plans/2026-10-31-calm-prototype.md).
