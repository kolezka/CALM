# CALM system overview

CALM is a macOS app (arm64, macOS 14+) with one nearly infinite pan/zoom canvas, in the
spirit of FigJam, that hosts live Ghostty terminals, WKWebView browser panels, trusted
HTML/JS plugin panels and TUI app panels. Rust workspace, GPUI for the canvas, AppKit for
native children.

## Goals that shape the design

- Many terminals at once (5, 10, 20), each any size, all alive while panned or zoomed.
- Performance matters. Distant or tiny panels become previews instead of live views.
- Processes do not need to survive app quit. Today a launch starts an empty canvas; the
  persistence plan restores the arrangement with fresh processes (not built).
- Plugins are explicitly installed, trusted code. No sandbox.
- Customization first: look and behavior come from settings and manifest fields.

## Panel kinds

`calm_core::scene::PanelKind`:

- `Terminal { command, cwd }`: a Ghostty surface.
- `Browser { url }`: a native address bar plus a WKWebView. No `calm` bridge.
- `Plugin { plugin_id, settings }`: the plugin's HTML entry in a WKWebView with the bridge.
- `Tui { plugin_id }`: a Ghostty surface running a TUI plugin's command. Only the id is
  stored; command, args and env stay in the manifest. Lives in the terminals list, so it
  shares terminal placement, preview, resize and close.

## Crates

```
calm-core      pure model, no GPUI, no AppKit, fully unit tested
calm-native    AppKit via objc2, all unsafe: NativeChild, WebPanel, BrowserView, NSMenu,
               Settings window, status pill
calm-plugins   manifest discovery, TUI manifests, JS bridge types and prelude, gh api client
calm-app       GPUI binary: CanvasView, hosts per panel kind, menus, smoke gate
```

### calm-core

- `geometry`: `Point`, `Size`, `Rect`.
- `camera`: world to screen mapping, zoom limits, zoom about a point, `Camera::fit`.
- `scene`: `Scene` of `Panel`s with `PanelId`, `PanelKind`, back-to-front order, `hit_test`,
  `bring_to_front`, `content_rect`, `chip_rect`.
- `interaction`: `InputHandler` state machine (`Idle`, `Drawing`, `Dragging`, `Resizing`,
  `Panning`) that turns input plus a `Tool` into `Effect`s.
- `chrome`: title bar layout, hit testing, hover, icon and button strokes, and
  `chrome_colors`, `divider_color`, `icon_tint` (theme plus TUI manifest overrides).
- `join`: `JoinLayout::compute` finds joined neighbors (gap 0 to `JOIN_GAP`, overlapping
  spans, minimized skipped): square corners, joined sides, divider rects, groups.
- `resize`: `ResizePolicy::derive(kind, plugin)` and `resize_target`, with the outer band
  past the right and bottom edges that stops at a joined neighbor.
- `snap`: grid and neighbor-edge snapping (grid 8, edge distance 8, facing edges land
  `JOIN_SNAP_GAP` apart).
- `grid`: grid styles (lines, dots, crosses, none), tile keys per zoom bucket, the minor
  fade, `mark_primitive_bound`.
- `viewport`: `place_native` computes a `NativePlacement` for a content rect.
- `preview`: `decide_mode` (Live, Preview, Hidden) and `refresh_due`
  (`PREVIEW_REFRESH_SECS = 2`).
- `status_bar`: the bottom pill's rect.
- `browser_url`: `normalize_address` for the address bar.
- `tui`: TUI launch argv (status wrapper, login shell, POSIX quoting) and the restart
  `Supervisor`.
- `background`: solid, gradient and image backgrounds, raster and filters, fit rects.
- `settings`: persisted options (version 2, migrates version 1), path
  `$CALM_HOME/settings.json` or `~/.config/calm/settings.json`.
- `settings_form`: the Settings window as data and `apply_field`.
- `layout`: layout file format and validation. Not wired into the app.
- `menu`: context menu model.
- `plugin_menu`: plugin menu items (`MenuContribution`, `MenuWhen`), the merged section after
  the built-ins, the `command` payload and which panel a command goes to.
- `frame_stats`, `stats`: FPS and percentiles.

### calm-native

- `child`: `NativeChild`, a clip view (unflipped, clips to bounds) holding a zoom view whose
  bounds equal the logical size and whose frame is logical size times zoom. `apply`,
  `set_visible`, `bring_to_front`, `stack_index`.
- `webview`: `WebPanel`, a WKWebView in a `NativeChild` with the `calm` message handler,
  `load_file`, `eval_js`, user scripts at document start.
- `browser`: `BrowserView`, address bar plus WKWebView in the zoom view, with navigation
  events and the key monitor.
- `menu`: native NSMenu context menu.
- `settings_window`: the Settings NSWindow that renders a `settings_form`.
- `status_bar`: `StatusBar`, a translucent `NSVisualEffectView` pill kept topmost.

### calm-plugins

- `manifest`: `calm-plugin.json`, `plugin_search` (user dirs, then built-in dirs),
  `discover_search`, strict errors, resize fields. Depends on `calm-core`.
- `tui`: `"kind": "tui"` manifests (`TuiSpec`, restart, look fields, icon checks).
- `assets`: checks that an entry page's local scripts exist (the unbuilt `dist/` case).
- `bridge`: request, response and event types, `BRIDGE_METHODS` (17), `CALM_PRELUDE_JS`.
- `github`: `GhClient` wrapping `gh api` on the host. Not dispatched by the app yet.
- `contributes`: `contributes.menus` in either manifest kind, strict per-field errors.

### calm-app

- `main.rs`: args (`--smoke`, `--fullscreen`), window, menus, smoke driver.
- `canvas_view.rs`: `CanvasView` owns the scene, camera, input handler, terminals,
  browsers, plugins and settings. Render builds a paint snapshot; paint draws everything.
- `terminal/`: `TerminalPanel` with the libghostty adapter (CALM copy of gpui-libghostty
  0.3.1, MIT).
- `tui_host.rs`: TUI panels, status wrapper files, restart.
- `browser_panel.rs`: browser panels around `BrowserView`.
- `plugin_host.rs`: plugin discovery report, plugin panels, the bridge dispatcher.
- `grid_paint.rs`: grid lines as quads, dots and crosses as cached tiles.
- `icon_image.rs`: decodes TUI image icons once per path.
- `background.rs`, `theme.rs`, `dev_menu.rs`, `settings_menu.rs`.

## Event and paint flow

1. GPUI delivers mouse and key events to `CanvasView`. Clicks over a live native view go
   to that view, except where it passes them on (Ghostty returns nil from `hitTest`).
2. The view converts them to world coordinates and feeds `InputHandler`, which returns
   `Effect`s. The view applies them to the scene, camera and panels.
3. Each paint: background, grid, then per panel in scene order the chrome (title bar,
   ring, shadow per join group, dividers from `JoinLayout`), previews and flat rects.
4. For every native panel, `decide_mode` picks Live, Preview or Hidden; `place_native`
   computes the placement and `set_frame` runs. Previews and modal UI hide the native view;
   terminals paint a snapshot, browsers and plugins a flat rect with the title.
5. Native views draw above everything GPUI paints. GPUI never paints over a live native
   rect. Modal UI is native (NSMenu, Settings window) or hides native views first.

## Native z-order

Terminals, TUI panels, browsers and plugins share one native stack.
`native_panel_order` lists them in scene order; `sync_native_order` calls `bring_to_front`
back to front when the order changed; the status pill then runs `ensure_front` so no panel
covers it. `native_zorder` reports native and scene order for the smoke, which fails on a
mismatch.

## Smoke gate

`cargo run -p calm-app -- --smoke` opens a window, adds terminals, a browser, fixture
plugins and TUI fixtures, waits for frames, prints one line per check (viewport, canvas,
terminals, zorder, status_bar, fps, settings, preview, background, join, resize, plugins,
browser, tui; the full set is in `main.rs`), then exits 0. It is the only way an agent may run the GUI.

## Plugin model

Web plugins are folders with `calm-plugin.json` and an HTML entry, loaded in a `WebPanel`
with `CALM_PRELUDE_JS` defining `window.calm`. Calls post to the `calm` handler as JSON
strings; the host answers with `eval_js`. The app serves `log`, `panel.setTitle`,
`settings.get` and `settings.set`; the rest reject with `not implemented: <method>`. TUI
plugins have `"kind": "tui"` and run a command in a terminal panel. See `docs/plugins.md`
and `packages/calm-plugin-sdk/calm.d.ts`. API v2 is a design, not built.
