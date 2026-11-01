# CALM architecture

CALM is one GPUI window with an infinite canvas. GPUI paints the background, grid, panel
chrome and previews. Live content runs in real AppKit views (Ghostty terminals and WKWebViews)
placed over the canvas. The crate split and the module list are in `.ai/system.md`; this
page explains how the parts work together.

## Crates

- `calm-core`: the pure model. Geometry, camera, scene, input state machine, snapping,
  joins, resize policy, grid, preview policy, settings, terminal profiles, stats math,
  layout v2 and the session storage primitives, TUI launch logic. No GPUI, no AppKit, unit
  tested.
- `calm-native`: every AppKit call and every `unsafe` block. Native child views, web views,
  the browser view, the context menu, the Settings window, the status pill, the runtime app
  icon, the process stats sampler.
- `calm-plugins`: plugin discovery and manifests (web and TUI), menu contributions, the
  API v2 error codes, the JS bridge and prelude, the event coalescer (not wired), the
  `gh api` client and its token lookup.
- `calm-app`: the GPUI binary. Maps events to `calm-core`, places native views, hosts each
  panel kind, serves the plugin bridge and runs the smoke gate.

## Panel kinds

| Kind | Content | Host in `calm-app` |
| --- | --- | --- |
| Terminal | Ghostty surface | `terminal/` |
| Tui | Ghostty surface running a TUI plugin's command | `tui_host.rs` (terminal code underneath) |
| Browser | native address bar plus WKWebView, no bridge | `browser_panel.rs` |
| Plugin | the plugin's HTML entry in a WKWebView with `window.calm` | `plugin_host.rs` |

Every kind has the same chrome, menu actions, joins, resize, preview and close
confirmation. Only explicit confirmed close ends a panel's process.

## Native hosting

Each live panel owns a `NativeChild`: a clip view whose frame is the panel's clipped screen
rect, holding a zoom view whose bounds always equal the panel's logical size and whose frame
is that size times the camera zoom. Ghostty and WKWebView attach to the zoom view at
`(0, 0, logical size)`, so they never see the camera.

- Every paint computes `place_native(content_rect, camera, canvas)` per panel and applies
  it. `None` (minimized or off screen) hides the view; the process keeps running.
- Zoom only changes frames. `surface.set_frame(0, 0, logical size, scale)` runs on every
  placement; the surface skips unchanged frames, so only a panel resize reflows the grid.
- Native views draw above everything GPUI paints. GPUI never paints over a live native rect.
  Menus are native NSMenus; Settings is a separate NSWindow.

## Native z-order

Terminals, TUI panels, browsers and plugins share one native stack. When the scene's
back-to-front order changes, the canvas calls `bring_to_front` on each native view, back to
front. The status pill then moves itself back to the top, so a panel never covers it. The
smoke compares native order with scene order and fails on a mismatch.

## Paint

1. Background: one cached texture, rendered off the UI thread once per settings change.
2. Grid: lines as quads; dots and crosses as a tile cached per style, color and zoom bucket,
   drawn tiled. A frame draws at most `mark_primitive_bound` grid primitives.
3. Panels in scene order: shadow per join group, title bar, ring, dividers, and for
   non-live panels a preview image or a flat rect with the title.
4. Native views placed as above.

## Input

GPUI events go to `CanvasView`, which converts to world coordinates and feeds
`InputHandler`. It returns effects (create, focus, close, minimize, menus, redraw) that the
view applies. Ghostty's view returns nil from `hitTest`, so clicks over a terminal reach
GPUI; a WKWebView keeps its clicks. That is why resize handles reach 8 screen px outside
the right and bottom edges.

A move-modifier press on a native view's content (Cmd by default, `canvas.move_modifier`)
is taken by `calm_native::ContentGestureMonitor`, an NSEvent local mouse monitor. It asks
the app whether it wants the press, holds it, and reports each event; the app's
`content_move` module feeds `calm_core::gesture::ContentGesture`. Past the click slop the
gesture becomes `InputHandler::begin_content_move` and the usual drag; released inside the
slop it is a click, and the monitor replays the held events to the native view.
Cmd+right-click opens the panel menu. Only the topmost panel's content counts, never chrome.

Inside a terminal, keys stop propagating and `t`, `b`, `escape`, `enter` are bound to
`NoAction` in the `Terminal` key context, so canvas shortcuts do not steal them. Browser
pages get editing and navigation shortcuts through an NSEvent local monitor, since GPUI
takes key equivalents first.

## Preview and minimize

- Each paint, each native panel gets `decide_mode(zoom, rect on screen, viewport,
  minimized, modal_open)`: `Live`, `Preview` (zoom below 50 % or a modal up) or `Hidden`
  (minimized or off screen).
- Terminals in `Preview` take a snapshot when `refresh_due` says so (at most one per
  `PREVIEW_REFRESH_SECS`, 2 s), hide the native view and paint the image with GPUI. A
  failed refresh keeps the last image marked `preview stale`. Browsers and plugins paint a
  flat rect with the title; there are no page snapshots yet.
- Minimized panels draw only the 200x28 chip.
- The process keeps running in every mode.

## Joins and resize

Panels whose facing edges are 0 to 2 world units apart with overlapping spans join into a
block: covered corners turn square, the ring drops on joined sides and one divider fills
the gap. Edge snapping leaves exactly 1 unit, so the divider has room beside two live views.

`calm-core::resize` derives one policy per panel: terminals and browsers resize freely,
plugins follow `resizable`, `minSize` and `maxSize` from the manifest. The outer band stops
at a joined neighbor. Only right, bottom and corner handles exist.

## Plugins

Discovery runs at startup over user folders, then built-in folders (the repo's `plugins/`
in a dev build, `Contents/Resources/plugins` in a bundle). Web plugins load in a `WebPanel`
with `window.__calmHost` and the prelude at document start. TUI plugins run their command
through a `/bin/sh` status wrapper that records the exit code. Both kinds may add
right-click menu items; a web plugin gets a `command` event, a TUI item opens a new panel.

A bridge call goes through `plugin_host::route`. `github.api`, `storage.*`, `openExternal`
and `process.exec` run on a background thread (`plugin_services.rs`) and answer from a GPUI
task, so a slow program never stalls the canvas. The rest is served on the UI thread by
`dispatch`, which sees the scene through a `HostContext`. Calls that need the window
(opening a panel's native view, focus, close) come back as `HostEffect`s that the canvas
applies; a close is `Effect::ClosePanelRequested`, the path a click takes, so a live
process still asks first. An unknown method rejects with `unknown_method`. Every reject is
`{ code, message }`, and the prelude turns it into an `Error` with `code`. See
[plugins.md](plugins.md).

## Settings and storage

Settings live in one `settings.json`, written privately through a temp file and rename.
`calm-core::layout` holds the layout v2 format and the session storage primitives (durable
writes, backup rotation, url scrubbing). The app does not call them yet: there is no
autosave and no restore, and a launch starts an empty canvas.

## Not built

Session autosave and restore, layout save and load, `--layout`, `--bench`, most of the
plugin API v2 surface (phase 1 and the coalescer core are built) and the Ghostty shim
patch. See `.ai/status.md`.

## Verified on

macOS 27.0.1, Apple M4 Pro, 2026-10-31: the smoke passes; terminals draw frames and stay
alive at zoom 1.5, including one clipped by the window edge. Manual GUI passes per feature
are still owed (list in `.ai/status.md`).
