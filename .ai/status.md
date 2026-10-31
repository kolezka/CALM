# Status

Last updated: 2026-10-31. `main` with PRs #1 to #7, #9 and #11 merged: recenter, panel menu actions,
panel chrome, bottom bar, plugin panels, browser panels, the Settings window, the mods
research doc and grid styles. Facts are verified unless marked otherwise.

## Done and merged to `main`

- Workspace, toolchain, Makefile (`gmake`), CI on the self-hosted Arch runner, Release
  workflow on the runner, local macOS bundle and upload script.
- `calm-core` complete for the prototype: camera, scene, interaction, snapping, viewport
  placement, preview policy, layout format, settings, menu model, frame stats.
- `calm-native`: `NativeChild`, `WebPanel`, native context menu, `native_smoke` example.
- `calm-plugins`: manifest discovery, bridge types and prelude, `GhClient`.
- `calm-app`: canvas with live Ghostty terminals, previews and minimize, right-click Add
  menu, pinch and keyboard zoom, Developer menu with FPS, Settings menu with background and
  options persisted to `settings.json`, snapping, fullscreen, smoke gate.
- Plugins: `hello-world`, `github-prs` (TSX), `templates/plugin-tsx`, SDK `calm.d.ts`.
- Docs: `dev-setup.md`, `architecture.md` (terminals, preview), `plugins.md`,
  `release.md`, `CLAUDE.md`, this folder.
- Graphify graph: `repo-intel build` rc=0 on 2026-10-31 (graphify only, backend litellm
  with `deepseek/deepseek-flash`, scope in `.graphifyignore`: `docs/superpowers/`, lock
  files and the plist template excluded). 55 code files, 16 docs, 1247 nodes, 2472 edges.
  `AGENTS.md` carries the routing block, imported from `CLAUDE.md`. A git post-commit hook
  rebuilds it in the background.
- Camera jumps (PR #1): `cmd-shift-h` centers the world origin at the current zoom,
  `cmd-shift-0` fits all panels (32 px margin, zoom clamped, empty canvas centers the
  origin), both in a View menu. One `Camera::fit` serves fit all and the panel Zoom to Fit.
- Panel context menu (PR #2) on the title strip or a minimized chip: Minimize or Restore, Zoom to
  Fit (hidden while minimized), Bring to Front, Send to Back, Duplicate (terminals only, a
  new terminal of the same size 32 units down and right, fresh process), Delete.
- Delete uses the normal close path: a live terminal shows the close confirmation unless
  `confirm_close` is off in settings.
- Right click on a live Ghostty view goes to Ghostty, not to CALM, so the menu opens only
  from the title strip and the chip. Right click on content in preview mode does nothing.
- Rename is not offered: it needs a native text input, which does not exist yet.
- Manual GUI pass of each panel menu action is pending.
- Panel chrome (PR #3). Geometry lives in `calm-core::chrome` (`title_bar_layout`,
  `chrome_hit`, `hover_target`, `details_visible`, icon and button strokes), which paint
  and hit testing share. `TitleBarLayout::middle` is the free area between the kind icon
  and the buttons, meant for a future address field.
- Look: 28 unit title bar, 8 unit rounded top corners, 1 px ring and soft drop shadow
  outside the panel rect, accent ring and brighter bar on focus, vector icon per kind,
  round minimize and close buttons that show on hover (amber and red when pointed at),
  red tinted bar during "Close? enter / esc". Icon, title and buttons hide below 12 px of
  bar height on screen. Colors in `calm-app/src/theme.rs`.
- Unverified: the look itself. Only smoke ran; a manual GUI pass is the user's.
- Joined panels (branch `feat/panel-join`, not merged yet). `calm-core::join` finds panels
  whose facing edges are 0 to `JOIN_GAP` (2) apart with overlapping spans, for all kinds;
  minimized chips never join. Paint squares the covered corners, drops the ring on joined
  sides, fills the gap with one divider, and casts the shadow only around a group's outer
  boundary. Edge snapping now lands facing edges `JOIN_SNAP_GAP` (1) apart instead of
  flush. Smoke prints `join pairs=1 dividers=1 gap=1`. Unverified: the look, and partial
  overlaps at high zoom. Manual GUI pass is the user's.
- Bottom bar (PR #4): the top status strip is removed; the title bar is transparent with a
  hidden title; zoom, FPS and the save flash show in a native translucent pill at the
  bottom center (`calm_native::StatusBar`, rect from `calm_core::status_bar`). Smoke
  checks the bar is visible and above both terminals after a restack.
- Plugin panels (PR #5): discovery at startup fills the right-click Add menu. Skipped
  plugin folders print to stderr and show as `⚠ plugins: N errors` in the status bar pill.
- Built-in plugin dirs (fix/dev-plugin-discovery): user dirs first, then the workspace
  `plugins/` in a dev build and `CALM.app/Contents/Resources/plugins` in a bundle. A user
  plugin shadows a built-in with the same id. Off with `plugins.builtin=false` (file only,
  not in the Settings window) or `CALM_NO_BUILTIN_PLUGINS=1`. Empty Add menu says
  "No plugins found" and lists searched dirs. Smoke prints `plugins builtin=N user=M`.
- Adding a plugin opens a Plugin panel (`crates/calm-app/src/plugin_host.rs`): a `WebPanel`
  in a `NativeChild`, prelude injected at document start, entry loaded with `load_file`.
  Placed like terminals: Live sets the frame each paint, Preview and Hidden hide the view
  and draw the flat title rect.
- Bridge: `log`, `panel.setTitle`, `settings.get`, `settings.set` are served. The other 13
  methods reject with `not implemented: <method>`. `github-prs` renders but its list shows
  that error until `github.api` is dispatched.
- Fixed: the prelude posted plain objects, which WebKit delivers as an NSDictionary whose
  description is not JSON. It now posts `JSON.stringify(...)`; the bun test fake rejects
  non-string bodies.
- An entry page whose local `<script src>` is missing (an unbuilt `dist/`) gets an error
  page naming the file and the bun command, a `(not built)` title, and a stderr line.
- Smoke adds two fixture plugins and prints `plugin loaded=true bridge=ok title=smoke-ok`,
  `plugin unbuilt reported=true` and a five-panel `plugins zorder` line.
- Browser panels (PR #6) are live: `calm_native::BrowserView` puts a native address bar (back,
  forward, reload, text field) and a WKWebView in the panel's zoom view, so the bar scales
  and clips with the page and GPUI draws nothing over it. App side in
  `crates/calm-app/src/browser_panel.rs`. No `calm` message handler on browser pages.
- Address input: `calm_core::browser_url::normalize_address`. Kept schemes load as typed,
  `localhost` and IPv4 get `http://`, dotted hosts get `https://`, the rest searches
  DuckDuckGo (`SEARCH_URL_PREFIX`).
- Links navigate in place. `target=_blank` and `window.open` load in the same panel (UI
  delegate returns no new web view).
- While the page or the bar has the keyboard: cmd-[ back, cmd-] forward, cmd-r reload,
  cmd-l address bar, plus cmd-c/x/v/a/z and cmd-shift-z (an NSEvent local monitor, since
  GPUI takes key equivalents first and the app has no Edit menu).
- A click on the page raises and focuses the panel. Title and URL follow the page into the
  chrome and into `PanelKind::Browser.url`. A new panel opens `about:blank` with the
  address bar focused. Closing a browser asks for confirmation like a running terminal.
- Preview and modal UI hide the page and paint a flat rect with the title. No page
  snapshot yet.
- Smoke adds one browser that loads a `data:` page which clicks its own link, and checks
  `smoke: browser loaded=true link=followed url=about:blank#clicked mode=live`. Frame-check
  failures now print `window_visible=`; failures seen during this work all had the window
  occluded (`Some(false)`), and GPUI stops painting an occluded window.
- Not done: cmd+click on a URL in a terminal, non-web schemes (`mailto:` and the like do
  nothing), page snapshots for previews, downloads. Manual GUI pass pending.
- Native stack: terminals, browsers and plugins share one native stack that restacks with
  the scene order (`native_panel_order`, `sync_native_order`, `native_zorder`). Smoke
  checks it with three native panels, then with five after the plugins.
- Settings window (PR #7), a native NSWindow with sections Background, Grid, Canvas and
  Snapping, Panels, Developer. Opened by `cmd-,`, CALM > Settings… and Settings > Open
  Settings…, all through one global `OpenSettings` handler; a second open focuses the
  same window. A top-level menu bar item cannot fire an action in GPUI (each top-level
  `Menu` becomes an item with a submenu), so the Settings menu stays a menu.
  The handler defers the open: GPUI runs it while it holds the canvas window, so a
  direct `canvas.update` failed and all three entry points did nothing until this fix.
  Smoke line `settings_dispatch` dispatches the action on the canvas window to cover it.
  Manual pass with a terminal and a browser focused is pending.
- Backgrounds: solid color, linear or radial gradient with 2 to 6 stops, image with
  cover, contain, tile or center and blur, brightness, contrast, saturation, grayscale
  and dim. Live on the canvas while editing.
- New options read by the app: grid spacing, color and opacity; snapping on or off, grid
  step and edge distance; default size of a terminal added from the context menu;
  preview below zoom.
- Settings file version 2 with version 1 migration and strict errors.
- Unverified: manual GUI pass of the window (color panel, open panel, slider feel, image
  filters on a real photo). The smoke builds the window and checks every control is
  bound, and renders a gradient background.
- Grid styles (PR #11): lines, dots, crosses or none; mark size 1 to 4 points; major grid every N
  cells (0 off, the default) with its own opacity. Color and opacity apply to every
  style. Controls in the Grid section of the Settings window, live on the canvas.
- New fields load from older version 2 files with defaults that draw the old grid; bad
  values are explicit errors.
- Smoke draws dots by default (`CALM_SMOKE_GRID` picks the style) and fails if a dot or
  cross frame draws more than `mark_primitive_bound` grid primitives.
- Unverified: manual look of dots, crosses and the major grid while zooming.

## On `fix/panel-resize` (PR open, not merged)

- Fixed: browser panels could not be resized. The 12 unit grab band sat inside the
  content rect; Ghostty's view returns nil from `hitTest`, so terminals passed the click to
  GPUI, but a WKWebView keeps it. Plugin panels had the same bug.
- `calm-core::resize`: `ResizePolicy { resizable, min, max }` from
  `ResizePolicy::derive(kind, plugin)`, `resize_target` for press and cursor, an outer grab
  band of 8 screen px outside the right and bottom edges. Drag clamps to the policy.
- Plugin manifest: optional `resizable`, `minSize`, `maxSize`, strictly validated.
  `calm-plugins` now depends on `calm-core`.
- Resize cursor on hover (left-right, up-down, diagonal). The smoke drags a browser and a
  locked plugin through the real mouse handlers and prints `smoke: resize ...`.
- Merged with #12 and #13: the outer band does not reach into a joined neighbor
  (`resize_target`); the shared edge resizes from the join gap or the inner band.
- Unverified: manual GUI pass of the cursor and of a resize by hand over a live web view,
  and of a joined shared edge between two web views.

## On `feat/tui-plugins` (PR open, not merged)

- TUI app plugins: `"kind": "tui"` manifests (`calm-plugins::tui`, strict per-field errors),
  `PanelKind::Tui { plugin_id }`, launch argv and restart state machine in
  `calm-core::tui`, app glue in `calm-app/src/tui_host.rs`. Apps are listed next to Terminal
  in the Add menu. Exited panels offer Restart in the panel menu and on Enter.
- Exit codes come from a `/bin/sh` status wrapper writing a per-launch file under
  `$TMPDIR/calm-<pid>/`, because gpui-libghostty 0.3.1 exposes no exit code.
- Smoke covers a script fixture (ran, exit 0, restart, close without asking) and a missing
  command (`smoke: tui ...` line). `plugins/top-example`, `templates/plugin-tui`.
- Unverified: a manual GUI pass, a real `slk`, fish as login shell (documented as
  unsupported for `shell: true`), and the glyph icon at all zoom levels.
- Not wired: TUI panels in saved layouts (layout save and load are not wired at all).

## On `fix/blur-on-outside-click` (PR open, not merged)

- A click on empty canvas (left or right) clears panel focus in the scene and makes the GPUI
  host view the first responder; pans and zooms keep focus; Escape clears it only when no
  native view has the keyboard. Setting `canvas.click_empty_clears_focus` (default true).
  Smoke line `smoke: blur focused=none responder=canvas` covers a terminal and the browser.
- Unverified: manual GUI pass of a real mouse click, and of Escape inside a web page.

## Verified on hardware

macOS 27.0.1, Apple M4 Pro: smoke passes; two terminals render in zoom views at zoom 1.5;
z-order follows scene order; preview snapshots about one per 2 s; settings persist;
CI run on `main` green (linux-arch success, macos skipped).

## Not wired yet

- Plugin host, rest of the dispatcher: storage, `panel.getBounds/minimize/close`,
  `canvas.*`, `process.exec`, `github.api`, `openExternal`, `theme.get`, and host events.
  Plugin panel settings are not saved anywhere yet (layouts are not wired).
- Clicking inside a plugin page does not focus or raise its panel in the scene; AppKit
  routes the click to the WKWebView directly. Unverified how keys behave while it has focus.
- Layout save and load in the app (`--layout`, Task 13). The core format exists.
- Benchmark mode (`--bench`, Task 14) and `docs/benchmarks.md`.
- `docs/architecture.md` refresh for the whole system (Task 15; the graph part is done).
- Plan file lacks the text of Task 17 (Makefile) and Task 16b (release on Linux); both
  exist only as briefs in the git-ignored SDD workspace.

## Unverified

- Browser panels by hand: typing in the bar at zoom 0.5 and 2, link clicks, `target=_blank`,
  the keyboard shortcuts, scroll over a page, pinch over a page.
- Manual GUI pass: mouse selection at zoom 1.0 and 1.7, cmd-v paste, Polish letters through
  the input method, panel resize reflow, Enter to confirm close, pinch feel, `htop`.
- Camera jumps: math is unit tested in `calm-core`; key delivery with a terminal focused,
  the `cmd-shift-0` match and the View menu shortcuts are untested in the GUI.
- Bottom status bar look: blur over terminals, rounded corners, light and dark mode,
  traffic lights over the canvas, window drag from the top strip.
- Performance with 20 terminals.
- A real Release run on the runner (dry run not yet triggered) and the hosted macOS CI
  steps.

## Known small issues

- `docs/dev-setup.md` lists `--layout work` and `--bench 20` as run commands; neither flag
  exists yet.
- The smoke's preview refresh step is intermittent: only `preview_snapshots_in_3s=[0,0]`
  fails, on `main` and on branches alike, more often under heavy load (load average 17 to
  40); every other smoke line passes. Cause unconfirmed.
- Escape on the close confirmation does not trigger a redraw until the next event.
- Panel bottom corners are square because the native view is square. Rounding them needs a
  corner radius on the clip view layer in `calm-native`.
- Chrome hover is tracked from GPUI mouse moves only. Over a live native view GPUI may get
  no moves, so the buttons can stay visible until the pointer crosses GPUI area again.
- A transitive `block 0.1.6` future-incompatibility warning from a dependency.
- repo-intel 0.1.8 prints `tmp: nieustawiona zmienna` on its error path (for example after
  an interrupted build). Upstream bug; harmless when the build succeeds.

## User-owned next steps

1. `gh workflow run release.yml --ref main -f bump=patch -f dry_run=true`, then a real run.
2. `gmake release-upload VERSION=x.y.z` on the Mac after the release exists.
3. Set `CALM_MACOS_HOSTED=true` once billing allows hosted macOS CI.
4. Push `main` (local commits: CLAUDE.md refresh, `.ai/`, repo-intel config).
5. Say go for the remaining plan tasks (10, 11, 12 app wiring, 13, 14, 15 docs).
