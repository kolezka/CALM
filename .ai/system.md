# CALM system overview

CALM is a macOS app (arm64, macOS 14+) with one nearly infinite pan/zoom canvas, in the
spirit of FigJam, that hosts live Ghostty terminals, WKWebView browser panels, trusted
HTML/JS plugin panels and TUI app panels. Rust workspace, GPUI for the canvas, AppKit for
native children.

## Goals that shape the design

- Many terminals at once (5, 10, 20), each any size, all alive while panned or zoomed.
- Performance matters. Distant or tiny panels become previews instead of live views.
- Processes do not need to survive app quit. Today a launch starts an empty canvas; the
  persistence plan restores the arrangement with fresh processes. Its storage primitives
  are merged (#36); autosave and restore are not active.
- Plugins are explicitly installed, trusted code. No sandbox.
- Customization first: look and behavior come from settings and manifest fields.

## Panel kinds

`calm_core::scene::PanelKind`:

- `Terminal { command, cwd, profile, app }`: a Ghostty surface. A `command` makes it a typed
  command (Run Command), supervised with exit handling. `app: Some(TerminalApp { plugin_id,
  plugin_version })` makes it a TUI app panel (`PanelKind::app_terminal`, read with
  `.app()`), saved as `type=tui` with only the plugin id, version and optional profile;
  command, args and env stay in the manifest.
- `Browser { url }`: a native address bar plus a WKWebView. No `calm` bridge.
- `Plugin { plugin_id, settings }`: the plugin's HTML entry in a WKWebView with the bridge.

## Crates

```
calm-core      pure model, no GPUI, no AppKit, fully unit tested
calm-native    AppKit via objc2, all unsafe: NativeChild, WebPanel, BrowserView, NSMenu,
               Settings window, status pill, app icon, process stats
calm-plugins   manifest discovery, TUI manifests, menu contributions, API v2 error codes,
               JS bridge types and prelude, event coalescer, gh api client and token
calm-app       GPUI binary: CanvasView, hosts per panel kind, plugin services, menus,
               smoke gate
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
- `browser_url`: `normalize_address` for the address bar; `BrowserQueryMode` (`strip_all`,
  `keep`), `scrub_for_save` and `scrub_text_for_save` for the layout write path.
- `tui`: TUI launch argv (status wrapper, login shell, POSIX quoting) and the restart
  `Supervisor`.
- `background`: solid, gradient and image backgrounds, raster and filters, fit rects.
- `settings`: persisted options (version 2, migrates version 1), path
  `$CALM_HOME/settings.json` or `~/.config/calm/settings.json`. Sections `background`,
  `grid`, `snap`, `panels`, `plugins`, `terminal`, `canvas`, `developer`, `session`. Saved
  with `layout::write_durable`; an invalid file is moved aside with `move_aside`.
- `settings_form`: the Settings window as data and `apply_field`.
- `terminal_profile`: the `terminal` section, profiles, resolution and launch argv.
- `process_stats`: CPU and memory math for the stats readout (ticks to ns, `CpuTracker`,
  `MemoryUnit`, interval range).
- `layout`: layout v2 format (v1 migrates on read, `PanelKind::Unsupported` keeps unknown
  kinds' bytes), validation, and session storage: `write_durable`, `write_layout_file`
  (stamps `saved_at_unix`, scrubs browser urls), `session_path`, `rotate_backups`,
  `SessionWriter`, `move_aside`. Not called by the app yet, except `write_durable` and
  `move_aside` for settings.
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
- `app_icon`: sets the embedded 512 px brand PNG as the application icon when no bundle
  icon exists, and reads it back to check it took.
- `process_stats`: `ProcessSampler`, `proc_pid_rusage` (V2) for CALM and optionally its
  child processes (`proc_listchildpids`, capped at 512).

### calm-plugins

- `manifest`: `calm-plugin.json`, `plugin_search` (user dirs, then built-in dirs),
  `discover_search`, strict errors, resize fields. Depends on `calm-core`.
- `tui`: `"kind": "tui"` manifests (`TuiSpec`, restart, look fields, icon checks).
- `assets`: checks that an entry page's local scripts exist (the unbuilt `dist/` case).
- `bridge`: request, response and event types, `BRIDGE_METHODS` (17), `CALM_PRELUDE_JS`,
  `host_info_js` (the `window.__calmHost` script).
- `api`: plugin API v2 phase 1. `API_VERSION` (2), `ErrorCode` (22 codes), `ApiError
  { code, message }`, `Caller`, `ClientId`, `Capability` and `unknown_capabilities`.
- `events`: the phase 6 `Coalescer` and `emit_batch_js`. Pure, not wired into the app.
- `github`: `GhClient` wrapping `gh api` on the host (30 s timeout, 8 MiB cap, 401 is not
  logged in). Dispatched by `calm-app/src/plugin_services.rs`.
- `gh_token`: `load_token`, the `GH_TOKEN`/`GITHUB_TOKEN` lookup (env, `$CALM_HOME/.env`,
  the dev checkout's `.env`), redacted in Debug and Display.
- `contributes`: `contributes.menus` in either manifest kind, strict per-field errors.

### calm-app

- `main.rs`: args (`--smoke`, `--fullscreen`), window, menus, smoke driver.
- `canvas_view.rs`: `CanvasView` owns the scene, camera, input handler, terminals,
  browsers, plugins and settings. Render builds a paint snapshot; paint draws everything.
- `terminal/`: `TerminalPanel` with the libghostty adapter (CALM copy of gpui-libghostty
  0.3.1, MIT).
- `tui_host.rs`: `CommandRun`, the supervised run of an app or typed command inside a
  terminal: manifest layer, status wrapper files, restart.
- `browser_panel.rs`: browser panels around `BrowserView`.
- `plugin_host.rs`: plugin discovery report, plugin panels, `served_methods`, `route` and
  `dispatch` (the bridge calls served on the UI thread).
- `plugin_services.rs`: `github.api`, `storage.*` and `openExternal`, parsed on the UI
  thread and run on the background executor; storage files in `$CALM_HOME/plugin-data`.
- `canvas_view/plugin_menus.rs`: plugin context menu items and `command` delivery.
- `terminal_profiles.rs`: plain terminals from the `terminal` settings, live recolor.
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
strings; the host answers with `eval_js`. Before the prelude the host injects
`window.__calmHost = { apiVersion, methods }` from `served_methods`, which `calm.has` reads.
The app serves all 17 methods. TUI plugins have
`"kind": "tui"` and run a command in a terminal panel. Both kinds may add context menu
items with `contributes.menus`. See `docs/plugins.md` and
`packages/calm-plugin-sdk/calm.d.ts`. Of API v2, phase 1 (codes, `calm.has`, manifest
fields) and the phase 6 coalescer core are built; the rest is design.

### Error-code flow

1. The page calls `calm.<method>`; the prelude posts `{ id, method, params }`.
2. `plugin_host::route` asks `plugin_services::parse` first. A background call (`gh`,
   storage, `open`, `process.exec`) runs on the executor and its `Result<Value, ApiError>`
   becomes a reply through `service_reply`. A parse failure replies at once. Anything else
   goes to `dispatch`, with a `HostContext` (scene, caller, focus, viewport, base color,
   default terminal size, installed plugins). The bridge task runs with the window
   (`spawn_in`), so `Route::Apply` effects (`HostEffect::Open`, `Focus`, `Close`) apply at
   once. `Close` maps to `Effect::ClosePanelRequested`, never to `ClosePanel`.
3. `dispatch` serves the UI-thread methods and takes `get_mut` only for the ones that change
   the scene, so reads cause no autosave. A known method it did not serve would get
   `ApiError::not_implemented`; every one is served now. An unknown one gets
   `unknown_method`. Other host rejects use `invalid_params`, `not_found`, `wrong_kind`,
   `unavailable`, `not_installed`, `storage_corrupt`, `too_large`, the `gh_*` codes,
   `timeout` or `host_error`.
4. `BridgeResponse::Err(id, ApiError)` renders `window.__calm.resolve(id, false,
   { code, message })`. The prelude rejects with an `Error` carrying `code`; a bare string
   (v1 shape) gets `code: "host_error"`.
