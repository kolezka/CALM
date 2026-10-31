# Status

Last updated: 2026-10-31, against `origin/main` at `c327cb1` plus #29 (PRs #1 to #29 merged). This
file describes `main`, not branches. Facts are verified against the code unless marked
otherwise.

## Done and merged to `main`

Foundation (before the PR series):

- Workspace, toolchain, Makefile (`gmake`), CI on the self-hosted Arch runner, Release
  workflow on the runner, local macOS bundle and upload script.
- `calm-core` model: camera, scene, interaction, snapping, viewport placement, preview
  policy, layout format, settings, menu model, frame stats.
- `calm-app`: canvas with live Ghostty terminals, previews and minimize, right-click Add
  menu, pinch and keyboard zoom, Developer menu with FPS, snapping, fullscreen, smoke gate.
- Plugins: `hello-world`, `github-prs` (TSX), `templates/plugin-tsx`, SDK `calm.d.ts`.
- Graphify graph: `repo-intel build` (graphify only, backend litellm, scope in
  `.graphifyignore`). `AGENTS.md` carries the routing block. A git post-commit hook
  rebuilds it in the background.

Pull requests:

- #1 Camera jumps: `cmd-shift-h` centers the world origin, `cmd-shift-0` fits all panels
  (32 px margin, zoom clamped), both in the View menu. One `Camera::fit` serves Fit All and
  the panel's Zoom to Fit.
- #2 Panel context menu on the title bar or a minimized chip: Minimize or Restore, Zoom to
  Fit, Bring to Front, Send to Back, Duplicate (terminals only), Delete. Delete uses the
  normal close path with confirmation. No Rename (needs a native text input).
- #3 Panel chrome: geometry in `calm-core::chrome`, shared by paint and hit testing. 28 unit
  title bar, rounded top corners, ring and shadow, accent ring on focus, a vector icon per
  kind, hover buttons, a red bar during "Close? enter / esc". Colors in `calm-app/src/theme.rs`.
- #4 Bottom status pill: the top strip is gone, the title bar is transparent, and zoom, FPS,
  the save flash and the plugin error count show in a native translucent pill
  (`calm_native::StatusBar`, rect from `calm_core::status_bar`).
- #5 Plugin host: discovery at startup fills the Add menu; a Plugin panel is a `WebPanel` in
  a `NativeChild` with the prelude at document start (`calm-app/src/plugin_host.rs`). An
  unbuilt entry shows an error page and a `(not built)` title.
- #6 Browser panels: `calm_native::BrowserView` with a native address bar and a WKWebView in
  the zoom view (`calm-app/src/browser_panel.rs`). Address rules in
  `calm_core::browser_url::normalize_address`. Links and `target=_blank` load in place.
  Editing and navigation shortcuts through an NSEvent local monitor. No `calm` bridge on
  browser pages.
- #7 Settings window: native NSWindow (Background, Grid, Canvas and Snapping, Panels,
  Developer), form as data in `calm_core::settings_form`. Backgrounds: solid, linear or
  radial gradient, image with fit and filters. Settings file version 2 with version 1
  migration and strict errors.
- #8, #19 README as a product overview.
- #9 Claude Code mods integration research (`docs/superpowers/specs/2026-10-31-claude-code-mods-integration.md`).
- #10 Plugin API v2 design (`docs/superpowers/specs/2026-10-31-plugin-api-v2-design.md`).
  Design only, not implemented.
- #12 Joined panels: `calm-core::join` joins facing edges 0 to `JOIN_GAP` (2) apart with
  overlapping spans, all kinds, minimized chips excepted. Square covered corners, one
  divider in the gap, one shadow per group. Edge snap lands `JOIN_SNAP_GAP` (1) apart.
- #13 Open Settings from `cmd-,`, CALM > Settings… and Settings > Open Settings… works
  again: the global handler defers the open with `cx.defer`. Smoke line `settings_dispatch`.
- #14 Recorded the bun decision, the customization-first rule and the idle-agent rule.
- #15 Canvas persistence plan (`docs/superpowers/plans/2026-10-31-canvas-persistence.md`).
  Plan only.
- #16 Resize policy: `calm-core::resize` (`ResizePolicy::derive`, `resize_target`), an outer
  grab band 8 screen px past the right and bottom edges so web views can be resized,
  resize cursors, manifest `resizable`, `minSize`, `maxSize`. The band stops at a joined
  neighbor.
- #17 Built-in plugin dirs: user dirs first, then the workspace `plugins/` in a dev build or
  `CALM.app/Contents/Resources/plugins` in a bundle. A user plugin shadows a built-in with
  the same id. Off with `plugins.builtin=false` (file only) or `CALM_NO_BUILTIN_PLUGINS=1`.
  Empty Add menu says "No plugins found" and lists the searched dirs.
- #18 TUI app plugins: `"kind": "tui"` manifests (`calm-plugins::tui`),
  `PanelKind::Tui { plugin_id }`, launch argv and restart supervisor in `calm-core::tui`,
  app glue in `calm-app/src/tui_host.rs`. Exit codes from a `/bin/sh` status wrapper.
  `plugins/top-example`, `templates/plugin-tui`.
- #20 Grid styles (re-land of #11, which merged into `feat/settings-modal` by mistake):
  lines, dots, crosses or none, mark size, a major grid. Dots and crosses are a cached tile
  (`calm-core::grid`, `calm-app/src/grid_paint.rs`); the smoke fails above
  `mark_primitive_bound`.
- #21 TUI look fields: `title` with `{name}`, image `icon` (`.png`, `.svg`), `accentColor`,
  `titleBarColor`, `titleColor`, `terminalColors`. Colors from
  `calm-core::chrome::chrome_colors`; image icons decoded once in `calm-app/src/icon_image.rs`.
- #27 Pixel-art brand: `assets/brand/build.py` generates the wordmarks, README banner,
  hand-tuned 16 and 32 px icons, a 1024 px master and an iconset; `iconutil` builds
  `CALM.icns`. The bundle script copies it to `Contents/Resources` and the plist names it
  with `CFBundleIconFile`.
- #24 Terminal profiles: a `terminal` settings section (`calm-core::terminal_profile`) with
  defaults, named `profiles` and `default_profile`. Look (title template with `{n}` and
  `{cwd}`, icon, chrome colors, `terminal_colors`, `use_ghostty_user_config`) and launch
  (`command`, `args`, `cwd`, `env`), validated strictly. Add > Terminal Profiles submenu;
  `t` and plain duplicates use `default_profile`. Running surfaces recolor live through
  `TerminalThemeState::apply`. Settings > Terminal edits defaults only (no text fields yet).
- #25 Blur on outside click: a click on empty canvas clears panel focus and makes the GPUI
  host view the first responder; pans and zooms keep focus; Escape clears it only when no
  native view has the keyboard. Setting `canvas.click_empty_clears_focus` (default true).
- #28 Plugin context menu items: `contributes.menus` in web and TUI manifests
  (`calm-plugins::contributes`, strict per-field errors), the merged menu model and
  `command` payload in `calm-core::plugin_menu`, settings `plugins.menus.hidden` and
  `plugins.menus.submenu_above`, glue in `calm-app/src/canvas_view/plugin_menus.rs`. Web
  plugins get a `command` event in an open panel or a new one; TUI items open a new panel
  with `run` args appended. Not built: `calm.menus.register`, `shortcut`,
  `contributes.commands`.
- #23 Plugin services: `github.api`, `storage.get/set/remove` and `openExternal` (http and
  https only) are served by `plugin_services` on the background executor. `GhClient` kills
  `gh` after 30 s and refuses output over 8 MiB. `calm_plugins::gh_token` takes
  `GH_TOKEN`/`GITHUB_TOKEN` from CALM's env, then `$CALM_HOME/.env`, then (dev builds) the
  workspace `.env`, else `gh`'s own login; passed as child env `GH_TOKEN`, never argv, and
  redacted in output and Debug. Smoke `github.api ok` runs against a fake `gh` script.
- #22 Plugin API v2 design refresh: layouts (`layouts.*`, `session.saveNow`) added as phase
  16. Design only.
- #29 Runtime app icon: `calm_native::app_icon` sets the embedded 512 px brand PNG as the
  application icon at startup when no bundle icon is present. A new "About CALM" item, first
  in the app menu, opens the standard About panel (icon, Cargo version, credits line). Smoke
  prints `app_icon set=... size=WxH` and fails unless the brand image is set.

## In progress (not on `main`)

- `feat/cpu-ram-stats`: CPU and memory next to FPS in the status pill
  (`FPS 60.0  p95 16.7ms  ·  4.2% CPU  ·  312 MB`). cmd-shift-f (Developer > Show Stats)
  shows the readout; `developer.stats.*` picks the items, the interval (250 to 10000 ms,
  default 1000), child processes and the memory unit, also in the Settings window.
  `calm_native::process_stats` reads `proc_pid_rusage` (V2) on the background executor:
  about 1.2 us per sample for CALM alone, about 19 us with the child walk (unit test on
  the dev machine). Smoke prints `stats fps=... cpu=...% mem=...MB` parsed from the pill.
  Manual GUI pass owed.

- No PR yet: a Linux port plan.
- `feat/api-v2-p6-coalescer`: plugin API v2 phase 6 core. `calm_plugins::events` has a pure
  `Coalescer` (injected `Instant`, per `(event, panel)` latest wins, trailing send, discrete
  events in order, `panelRemoved` drops a pending `panelChanged`, windows clamped 16 to
  5000 ms) and `emit_batch_js`. Not wired: the host sends no events through it yet.

## Not wired yet

- Plugin bridge: `log`, `panel.setTitle`, `settings.get` and `settings.set` are served in
  `plugin_host.rs`; `github.api`, `storage.get/set/remove` and `openExternal` in
  `plugin_services.rs`. The other 8 of the 17 `BRIDGE_METHODS` reject with
  `not implemented: <method>`: `panel.getBounds/minimize/close`, `canvas.*`,
  `process.exec`, `theme.get`. `storage.keys` and `mailto:` for `openExternal` are not
  served. No host events are sent other than `command`. Errors are still v1 strings; the v2
  `{ code, message }` shape is not done.
- Clicking inside a plugin page does not focus or raise its panel; AppKit routes the click
  to the WKWebView.
- Layouts and session: layout save and load, `--layout`, session autosave and restore are
  not built. Only the core layout format exists. Plugin settings are not saved anywhere.
  The session restore decision (restore on launch by default, Open replaces after one
  confirmation, browser urls saved with `strip_all`, a seven item File menu) is recorded
  in `decisions.md` and the spec; implementation (plan Tasks 3 to 12) is pending.
  Layout v2 (persistence plan Task 2, PR #31): `saved_at_unix`, per-panel
  `overrides`, `plugin_version`, browser `page_zoom`, and `PanelKind::Unsupported` that keeps
  an unknown kind's JSON bytes. v1 files load as v2 and are never rewritten; version 3 and
  up fail. The resize policy is not saved. Overrides are stored but not applied in the
  chrome, page zoom is stored but not applied, and an unsupported panel draws chrome only.
  Session storage primitives (persistence plan Task 3, `feat/session-storage`, not on `main`
  yet): `layout::write_durable` (exclusive 0600 temp, fsync, rename, directory fsync, temp
  removed on failure, created dirs 0700), used by layouts and `settings.json`;
  `write_layout_file`, the one layout write path, stamps `saved_at_unix` and applies
  `browser_url::scrub_for_save` (also to a browser title equal to its url);
  `session_path`, `rotate_backups`, `SessionWriter` (rotates once per writer, no-op when
  `session.json` is missing, so a moved-aside failed restore never enters the backups);
  `move_aside`, shared with the settings quarantine. Settings section `session`
  (`restore_on_launch`, `autosave`, `debounce_ms` 250 to 10000, `backup_count` 0 to 20,
  `browser_query`) with a Session section in the Settings form. Nothing calls the session
  writer yet: autosave is Task 4, restore Task 5. Lowering `backup_count` leaves older
  backups above the new count in place.
- `--bench` is not built, and `docs/benchmarks.md` does not exist. The Makefile `bench`
  target calls a flag that is missing.
- The Ghostty shim patch (plugin API v2 phase 10) is not done, so no terminal `PWD`, title
  or exit code from Ghostty, and TUI `fontSize`, `fontFamily` and `padding` are rejected.
- Per-panel edits after adding (Rename, Change Icon, Accent Color) are not built.
- Bun lookup for plugin backends (`plugins.bun_path`) is decided but not built; no plugin
  backend runs yet.
- Browser: no cmd+click on terminal URLs, no non-web schemes, no page snapshots for
  previews, no downloads.
- Plan file lacks the text of Task 17 (Makefile) and Task 16b (release on Linux); both exist
  only as briefs in the git-ignored SDD workspace.

## Unverified (manual GUI passes owed by the user)

- Panel menu: each action by hand.
- Chrome: the look, hover buttons over a live native view.
- Camera jumps: key delivery with a terminal focused, the View menu shortcuts.
- Status pill: blur over terminals, corners, light and dark mode, window drag from the top strip.
- Browser panels: typing in the bar at zoom 0.5 and 2, links, `target=_blank`, shortcuts,
  scroll and pinch over a page.
- Settings window: color panel, open panel, slider feel, image filters on a real photo,
  open with a terminal or a browser focused.
- Grid styles: dots, crosses and the major grid while zooming.
- Joined panels: the look, partial overlaps at high zoom.
- Resize: cursor and a drag by hand over a live web view, a joined shared edge between two
  web views.
- TUI apps: a real `slk`, fish as login shell, the glyph and image icon at all zooms, the
  accent ring and terminal colors.
- Terminals: mouse selection at zoom 1.0 and 1.7, cmd-v paste, Polish letters through the
  input method, resize reflow, Enter to confirm close, pinch feel, `htop`.
- Terminal profiles: the live recolor, the profile submenu, the Settings > Terminal controls.
- Blur: a real mouse click on empty canvas, Escape inside a web page.
- Plugin menus: a real right click (the NSMenu), long titles, the github-prs "Open repo
  in browser" item.
- App icon: the Dock, cmd-tab and About panel by eye.
- Brand: the logo and app icon by eye (the 16 px mark reads as a C without a prompt).
- Performance with 20 terminals.
- A real Release run on the runner and the hosted macOS CI steps.

## Verified on hardware

macOS 27.0.1, Apple M4 Pro: smoke passes; terminals render in zoom views at zoom 1.5;
z-order follows scene order; preview snapshots about one per 2 s; settings persist; CI on
`main` green (linux-arch success, macos skipped).

## Known small issues

- Smoke flakes seen on 2026-10-31, cause unconfirmed for each:
  - `preview_snapshots_in_3s=[0,0]`, on `main` and branches, more often under heavy load.
  - `window_visible=Some(false)`: the window was occluded, and GPUI stops painting an
    occluded window, so frame checks fail.
  - The native z-order line did not match the scene order (`zorder` vs scene), seen twice.
- Escape on the close confirmation does not redraw until the next event.
- Panel bottom corners are square because the native view is square.
- Chrome hover is tracked from GPUI mouse moves only, so buttons can stay visible over a
  live native view.
- A transitive `block 0.1.6` future-incompatibility warning.
- repo-intel 0.1.8 prints `tmp: nieustawiona zmienna` on its error path. Upstream bug.

## User-owned next steps

1. `gh workflow run release.yml --ref main -f bump=patch -f dry_run=true`, then a real run.
2. `gmake release-upload VERSION=x.y.z` on the Mac after the release exists.
3. Set `CALM_MACOS_HOSTED=true` once billing allows hosted macOS CI.
4. The manual GUI passes listed above.
5. Say go for the remaining plan tasks (10, 11, 12 app wiring, 13, 14) and the persistence plan.
