# CALM system overview

CALM is a macOS app (arm64, macOS 14+) with one nearly infinite pan/zoom canvas, in the
spirit of FigJam, that hosts live Ghostty terminals, WKWebView browser panels and trusted
HTML/JS plugin panels. Rust workspace, GPUI for the canvas, AppKit for native children.

## Goals that shape the design

- Many terminals at once (5, 10, 20), each any size, all alive while panned or zoomed.
- Performance matters. Distant or tiny panels become previews instead of live views.
- Processes do not need to survive app quit. A reboot starts a fresh canvas; a saved layout
  can be loaded, and its terminals start empty.
- Plugins are explicitly installed, trusted code. No sandbox. First plugins: Hello World and
  a GitHub pull-request list for user-configured repos (private included).

## Crates

```
calm-core      pure model, no GPUI, no AppKit, fully unit tested
calm-native    AppKit via objc2: NativeChild, WebPanel, NSMenu context menu (all unsafe)
calm-plugins   manifest discovery, JS bridge types and prelude, gh api client
calm-app       GPUI binary: CanvasView, menus, terminal adapter, smoke gate
```

### calm-core

- `geometry`: `Point`, `Size`, `Rect`.
- `camera`: world to screen mapping, zoom limits, zoom about a point.
- `scene`: `Scene` of `Panel`s with `PanelId`, `PanelKind` (Terminal, Browser, Plugin, Tui),
  back-to-front order, `hit_test`, `bring_to_front`, `content_rect`, `chip_rect`.
- `interaction`: `InputHandler` state machine (`Idle`, `Drawing`, `Dragging`, `Resizing`,
  `Panning`) that turns mouse and key input plus a `Tool` into `Effect`s: `CreatePanel`,
  `FocusPanel`, `ClosePanelRequested`, `ConfirmClose`, `ClosePanel`, `ToggleMinimize`,
  `ShowCanvasMenu`, `ShowPanelMenu`, `Redraw`.
- `snap`: grid and neighbor-edge snapping (`SnapConfig`, default grid 8, edge distance 8).
- `viewport`: `place_native` computes a `NativePlacement` (clipped rect, inner origin, zoom)
  for a panel's content rect against the camera and canvas.
- `preview`: `decide_mode` (Live, Preview, Hidden) and `refresh_due` with
  `PREVIEW_REFRESH_SECS = 2`.
- `layout`: layout file format and validation (`LayoutError::Malformed` names path and
  field). Save and load are not wired into the app yet.
- `settings`: persisted options (version 2, migrates version 1), path
  `$CALM_HOME/settings.json` or `~/.config/calm/settings.json`.
- `settings_form`: the Settings window as data (sections, controls) and `apply_field`.
- `background`: solid, gradient and image backgrounds, gradient raster, image filters,
  fit rects.
- `menu`: context menu model (nested Add submenu, panel actions).
- `tui`: TUI launch argv (status wrapper, login shell, POSIX quoting) and the restart
  `Supervisor` (prompt, on-exit with backoff and give-up, never).
- `frame_stats`, `stats`: FPS and percentile helpers.

### calm-native

- `NativeChild`: a clip view (unflipped, `clipsToBounds` on, frame = clipped rect) holding a
  zoom view whose bounds always equal the logical size and whose frame is logical size times
  zoom. Children (Ghostty surface, WKWebView) attach to the zoom view, so their own math
  never sees the camera. `apply(placement)`, `set_visible`, `bring_to_front`, `stack_index`.
- `WebPanel`: WKWebView inside a `NativeChild`. `load_url`, `load_file(entry, read_dir)`,
  `eval_js`, `add_user_script_at_document_start`, `on_message` for the `calm` script
  message handler, `set_logical_size`.
- `ContextMenu` and `show_context_menu`: a native NSMenu popped up at a point; returns the
  chosen item id. Native because anything GPUI paints is under the native views.
- `SettingsWindow`: a separate NSWindow drawing a `settings_form` with AppKit controls;
  `choose_image_file` runs an NSOpenPanel.
- `StatusBar`: translucent rounded `NSVisualEffectView` with one label, kept as the
  topmost subview so panels never cover it. Its rect comes from `calm_core::status_bar`.

### calm-plugins

- `manifest`: `calm-plugin.json` (`PluginManifest`), `plugin_dirs()` from
  `~/.config/calm/plugins/<id>` and `CALM_PLUGIN_PATH`, `discover(dirs)`. The entry file
  must be a regular file under the canonical plugin directory.
- `bridge`: `BridgeRequest`, `BridgeResponse`, `BridgeEvent`, `BRIDGE_METHODS` (17 names),
  `CALM_PRELUDE_JS` injected at document start to define `window.calm`.
- `github`: `GhClient` wrapping `gh api` on the host. Plugin JS never sees a token.
- `tui`: `"kind": "tui"` manifests (`TuiSpec`): a terminal panel running one command.

### calm-app

- `main.rs`: args (`--smoke`, `--fullscreen`), window creation, smoke driver, settings
  startup.
- `canvas_view.rs`: `CanvasView` owns `Scene`, camera, `InputHandler`, terminals and
  settings. Render builds a `PaintSnapshot`; paint draws grid, chrome, previews and
  places native views. Key bindings in `key_bindings()`.
- `terminal/`: `TerminalPanel` with the libghostty adapter (CALM-owned copy of the
  gpui-libghostty 0.3.1 MIT adapter, using `spawn_surface(options, &native_child, scale)`).
- `dev_menu.rs`, `settings_menu.rs` (menu and Settings window glue), `background.rs`
  (renders gradient and image backgrounds once per change), `theme.rs`. The canvas fills
  the window under a transparent title bar; zoom and FPS show in the native `StatusBar` at
  the bottom.

## Event and paint flow

1. GPUI delivers mouse and key events to `CanvasView`.
2. The view converts them to world coordinates and feeds `InputHandler`, which returns
   `Effect`s. The view applies them to `Scene`, camera and terminals.
3. Each paint: for every terminal, `decide_mode` picks Live, Preview or Hidden;
   `place_native` computes the placement; `set_frame` and, if the order changed,
   `bring_to_front` run on the native child. Previews paint a snapshot with GPUI.
4. Native views draw above everything GPUI paints. GPUI never paints over a live native
   rect. Modal UI is native (NSMenu) or hides native views first.

## Smoke gate

`cargo run -p calm-app -- --smoke` opens a window, adds two terminals at zoom 1.5, waits
for frames, and prints `viewport`, `canvas`, `terminals` (frames and alive), `zorder`,
`status_bar`, `fps`, `settings` and `preview` lines, then exits 0. It is the only way an agent may run
the GUI.

## Plugin model

Plugins are directories with `calm-plugin.json` and an HTML entry. The entry loads in a
`WebPanel` with `CALM_PRELUDE_JS` defining `window.calm` (settings, storage, panel, canvas,
process, github, openExternal, log, theme, events). Calls post to the `calm` message
handler as JSON strings; the host answers with `eval_js`. On `feat/menu-plugins` the app
serves `log`, `panel.setTitle` and `settings.get/set`; the rest reject with
`not implemented: <method>`. First-party plugins are Preact TSX built with
bun. See `docs/plugins.md` and `packages/calm-plugin-sdk/calm.d.ts`.
