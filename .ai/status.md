# Status

Last updated: 2026-11-01, against `origin/main` at `188d7a5` (PRs #1 to #41 merged; #11
merged into a branch, see #20). This file describes `main`, not branches. Facts are
verified against the code unless marked otherwise.

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
- `fix/term-program-ghostty`: every terminal exports `TERM_PROGRAM=ghostty` (built-in env
  layer, overridable), so slk and similar apps pick kitty graphics. The smoke `tui` step
  checks the value inside a real surface. Kitty images on a zoomed canvas: not yet seen.
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
- Canvas menu: Add, then "Settings…" (dispatches `OpenSettings`, same as `cmd-,`; smoke
  step `settings_dispatch` picks it). In Add, Terminal and Browser sit under a "Native"
  header; TUI apps and web plugins share one section under a "Plugins" header
  (`MenuItem::Header`, `NSMenuItem.sectionHeaderWithTitle`, macOS 14+). The shipped examples
  no longer contribute canvas items (`new-panel`, `slow-refresh`): they duplicated Add.
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
- #30 Session restore decision: restore on launch by default, Open replaces after one
  confirmation, browser urls saved with `strip_all`, a seven item File menu. Recorded in
  `decisions.md` and the spec. Decision only.
- #32 `.ai/backlog.md`.
- #31 Layout v2 (persistence plan Task 2): `saved_at_unix`, per-panel `overrides`
  (`title`, `icon`, `accent_color`), `plugin_version` on plugin and TUI kinds, browser
  `page_zoom`, and `PanelKind::Unsupported`, which keeps an unknown kind's exact JSON bytes.
  v1 files load as v2 and are never rewritten; version 0 and 3 and up fail. Format only,
  see "Not wired yet".
- #33 CPU and memory next to FPS in the status pill
  (`FPS 60.0  p95 16.7ms  ·  4.2% CPU  ·  312 MB`). cmd-shift-f (Developer > Show Stats)
  shows the readout; `developer.stats.*` picks the items, the interval (250 to 10000 ms,
  default 1000), child processes and the memory unit, also in the Settings window.
  `calm_native::process_stats` reads `proc_pid_rusage` (V2) on the background executor;
  the math is in `calm_core::process_stats`. About 1.2 us per sample for CALM alone, about
  19 us with the child walk (unit test on the dev machine). Smoke prints
  `stats fps=... cpu=...% mem=...MB` parsed from the pill.
- #34 Plugin API v2 phase 6 core: `calm_plugins::events` has a pure `Coalescer` (injected
  `Instant`, per `(event, panel)` latest wins, trailing send, discrete events in order,
  `panelRemoved` drops a pending `panelChanged`, windows clamped 16 to 5000 ms) and
  `emit_batch_js`. Not wired: the host sends no events through it.
- #35 Plugin API v2 phase 1: `calm_plugins::api` (`API_VERSION` 2, `ErrorCode` with all 22
  spec codes, `ApiError`, `Caller`, `Capability`). `BridgeResponse::Err` carries `ApiError`
  and every host reject has a code; the prelude rejects with an `Error` that has `code` and
  maps a v1 string to `host_error`. The host injects `window.__calmHost` before the prelude
  and `calm.has` reads it (`plugin_host::served_methods`). Manifest `apiVersion` (above 2
  skips), `capabilities` (unknown words warn, the status pill counts warnings), `main`
  (checked on disk), `backend`, `engines.bun` (npm range syntax, full prerelease and build
  grammar) and `background` are parsed; nothing runs a backend or background page.
  `storage.set` without `value` rejects `invalid_params`; `null` is stored. Smoke prints
  `plugin errors unknown=unknown_method unserved=not_implemented has=[true,false]`.
- #36 Session storage primitives (persistence plan Task 3), see "Not wired yet" for what
  is not active: `layout::write_durable` (exclusive 0600 temp, fsync, rename, directory
  fsync, temp removed on failure, created dirs 0700), used by layouts and `settings.json`;
  `write_layout_file`, the one layout write path, stamps `saved_at_unix` and applies
  `browser_url::scrub_for_save` (urls over 8 KiB save as `about:blank` in both modes) and
  `scrub_text_for_save` (every url inside a browser title); `session_path`,
  `rotate_backups` (copies `session.json` to `.1`, never moves it; backups 0600),
  `SessionWriter` (rotates once per writer, no-op when `session.json` is missing, so a
  moved-aside failed restore never enters the backups); `move_aside`, shared with the
  settings quarantine. Settings section `session` (`restore_on_launch`, `autosave`,
  `debounce_ms` 250 to 10000, `backup_count` 0 to 20, `browser_query`) with a Session
  section in the Settings window.
- #37 Smoke diagnostics: `CanvasView::process_samples` counts sampler readings; the stats
  step waits up to 5 s for two samples and a CPU reading, then up to 2 s for the pill. It
  does not fix the flake; it says which side failed (see "Known small issues").
- #38 Docs and `.ai` refresh after #22 to #37.
- #39 Autosave (persistence plan Task 4), built behind an off gate that #41 turned on.
  Review fixes: session settings apply live from the Settings form, duplicates carry the
  installed `plugin_version`, read-only bridge calls no longer dirty the scene, and a quit
  saves when the last change was not yet ticked.
- #40 Native z-order without paint. Root cause of the z-order flake: the native restack ran
  only in paint, and an occluded window does not paint. The app now restacks at scene and
  native mutation boundaries and keeps the paint-time sync. Smoke asserts native order right
  after send to back, bring to front, plugin reorder and a TUI restart. The preview and
  stats paint stalls stay open (`.ai/backlog.md` item 6).
- #41 Restore at launch (persistence plan Task 5), autosave on (`APP_AUTOSAVE_ACTIVE` is
  `true`, so the real `~/.config/calm/session.json` is written). `calm_core::restore`
  decides the launch (`pick_source`: `--layout NAME`, then `--no-restore`, then
  `session.restore_on_launch`; `plan_restore`, `after_quarantine`, `placeholder_reason`,
  `restored_cwd`, `version_note`). A missing session starts empty; a malformed or
  unsupported one moves to `session.json.invalid-<n>` with a path-only notice and autosave
  stays on; an unreadable one turns autosave off for the launch; a missing named layout
  starts empty with the name kept; a malformed one exits 2 and is not moved.
  `CanvasView::apply_restore_plan` builds panels back to front through the normal open
  paths, then the camera, restacks native views, then starts autosave on the result (the
  empty start guard). A missing plugin, TUI app or unknown kind is a GPUI-only placeholder
  with a Delete-only menu that keeps its saved kind. `ask` has no dialog: it restores like
  `on` with a notice. Review fixes: an autosave-off restore closes the gate for the whole
  launch (`AutosaveStart::launch_gate`), so a Settings toggle cannot start the writer;
  restored panels keep their saved title until the panel sets one; a gone cwd starts in
  home with a chrome note "cwd missing, started in ~" that is never saved; an invalid saved
  browser url opens `about:blank` (`browser_url::restored_url`). Missing-plugin settings
  round trip as a JSON value, not byte for byte; unknown kinds are byte for byte. Smoke
  lines `session saved=4 restored=4 ...`, `session malformed moved=session.json.invalid-0
  started_empty=true` and `session edges titles=saved cwd=home`; the session step also
  checks native order against scene order after a restore.
- #43 Smoke runs every step, prints PASS, FAIL or SKIPPED per step and a summary, labels
  known flakes (`smoke_runner::KNOWN_FLAKES`) and takes `CALM_SMOKE_ONLY`. rc stays 1 on any
  failure. The `zorder` step raises the panel through `apply_menu_choice` and checks native
  order at once, without waiting for a paint. Autosave, restore and session edge checks run
  as the one `session` step. Merged without review or a smoke run, at the user's request.
- #44 Plugin bridge control: the 8 remaining v1 bridge methods are served, so all 17
  answer. `dispatch` takes a `HostContext` and returns `Route::Apply` with `HostEffect`s
  (open, focus, close) that the canvas applies with the window; the bridge task now runs
  through `spawn_in`. `panel.getBounds` (size with the title bar, zoom), `panel.minimize`
  (sets, never toggles), `panel.close` (`Effect::ClosePanelRequested`, the click path),
  `canvas.focusPanel` (restore, raise, focus; `not_found`), `canvas.listPanels` (every
  panel, v1 fields plus `z`, `focused`, `pluginId`, `url`, `resize`), `canvas.createPanel`
  (terminal, browser, plugin, TUI; `rect` or viewport center at the kind's default size;
  `ResizePolicy::clamp`; a locked plugin rejects another size; `focus` default true; v2-only
  fields reject `unavailable`, unknown ones `invalid_params`), `theme.get`
  (`base_color()`), `process.exec` (`plugin_services`, no shell, own process group killed
  on timeout, 30 s default, 1 to 600000 ms, 4 MiB per stream, signal as 128 + n). Smoke
  prints `plugin errors ... bad_param=invalid_params has=[true,false,true]` and
  `plugin reads bounds=true theme=true listed=true exec=true`. Not built from the v2 rows of
  these methods: `near`/`side`/`size`/`gap` placement, `title`, `args`, `env`,
  `appearance`, `PanelInfo.mode`, `joined`, `appearance`, `createdBy`, `tui`, `panels.*`
  names, `panels.close(id)` with a pending reply and `busy`.

## Persistence plan progress

`docs/superpowers/plans/2026-10-31-canvas-persistence.md`, appendix "Remaining tasks".

| Task | State |
| --- | --- |
| 1 Session restore decision | Done, #30 |
| 2 Layout v2 | Done, #31 |
| 3 Durable writes, session paths, backups | Done, #36, with the url scrub, the 8 KiB guard and the title scrub |
| 4 Autosave | Done, #39; active since #41 |
| 5 Restore at launch | Done, #41 |
| 6 Browser url scrubbing | Mostly done in #36; `docs/sessions.md` and leftovers remain |
| 7 File menu and Settings controls | Not started |
| 8 `layouts.*` and `session.saveNow` | Is plugin API v2 phase 16, not started |
| 9 Terminal cwd from Ghostty PWD | Waits on API v2 phase 10 |
| 10 Browser page zoom saved | Waits on API v2 phase 12 |
| 11 TUI app panels in the session | Not started |
| 12 User overrides in chrome | Not started; needs a native text input |

## Plugin API v2 progress

Spec: `docs/superpowers/specs/2026-10-31-plugin-api-v2-design.md`. Worker briefs:
`docs/superpowers/plans/2026-11-01-plugin-api-v2-worker-briefs.md`.

| Phase | State |
| --- | --- |
| 1 Error codes and manifest | Done, #35 |
| 2 Request parsing | Next. Single writer on `bridge.rs` |
| 3 Read-only host | Waits on open question 1 (`PanelInfo.joined`) |
| 4 Control | Waits on open question 2 (`setAppearance` on every kind) |
| 5 Close | After 4 |
| 6 Events | Core `Coalescer` done, #34; wiring not started |
| 7 Storage, bus, process, github, openExternal | Storage, `github.api`, `openExternal` done outside the plan, #23; bus and `process.exec` open |
| 8 Contributions | `contributes.menus` done outside the plan, #28; the rest open |
| 9 to 16 | Not started. 10 needs the user's go for the Ghostty fork; 11 may move earlier (open question 4); 16 is persistence Task 8 |

Open questions: `.ai/open-questions.md`.

## In progress (not on `main`)

- `fix/settings-above-canvas`, no PR yet: global and panel Settings windows stay above the
  canvas as its child windows. Smoke asserts it; not checked by eye in fullscreen yet.
- `feat/panel-settings`, no PR yet: per-panel settings. Settings… in every panel menu opens
  one native Settings window per panel (tabs, each field inherited or set with Reset to
  global), saved as the panel's `overrides`: title, icon (built-in or an image copied to
  `panel-icons/`), accent, title bar and title colors, terminal colors, borderless.
  Cmd+drag on a terminal's or page's content moves the panel (`canvas.move_modifier`,
  default `cmd`); a Cmd+click inside the slop is replayed to the native view;
  Cmd+right-click opens the panel menu. Terminals save their profile in the session and a
  restore reopens it (an unknown id gives one notice). Duplicate copies overrides and
  profile. Policy is unit tested (`calm_core::gesture`, `calm-app` `content_move`); the
  AppKit monitor is tested on its state machine only.
- No PR yet: a Linux port plan.
- `feat/tui-extends-terminal`, PR #63, not merged: a TUI app is a terminal with an app
  attached (`PanelKind::app_terminal`, `.app()`), still saved as `type=tui`. One spawn,
  refresh, recolor and exit path for shells, typed commands and apps. App panels get the
  Settings sections "Title bar colors" and "Terminal colors" with presets, and inherit
  defaults and profile env. Looks stack defaults, profile, manifest, panel overrides. Add >
  Run Command… (after Terminal and Terminal Profiles) asks for a command in an NSAlert and
  opens a terminal running it in the login shell, supervised, titled `{name}`, saved in the
  session and run again on restore. `terminal.command_restart` (`never`, `on_exit`,
  `prompt`, default `prompt`) sets what a typed command does on exit. A terminal's tick loop
  starts at spawn, which removes the `tui-exit` smoke flake. Core rules are unit tested. The
  app-side spawn, exit and restart of typed commands and app recolor are checked by the smoke
  only; the NSAlert prompt is never driven by agents (manual pass owed).
- `fix/native-occlusion`, PR #54: a back panel's native view (Ghostty, WKWebView, the
  browser's address bar, a plugin page) no longer covers the title bar and ring of a panel
  in front of it. `calm_core::occlusion` computes the visible part of each native container
  (the container minus every panel above it, ring included, chip when minimized, and the
  divider of every join painted above it);
  `terminal_placement` fills `NativePlacement::visible`, and `NativeChild::apply` masks the
  clip view's layer to it and makes `hitTest:` return nil outside it. A fully covered view
  is hidden; its process keeps running. Terminal presses and scrolls outside the visible
  part go on to the canvas; motion there and releases reach the terminal only during a
  press it took (`PointerGate`). Rounded corners of a front panel leave a small square
  notch of the rear view (known limitation). A restack without a paint keeps the old mask until the next
  paint. `native_smoke` checks the mask piece, hit testing and full cover in AppKit; the
  look over real Ghostty and WKWebView content is unverified (manual pass owed).

## Not wired yet

- Plugin bridge: all 17 `BRIDGE_METHODS` are served (#44). `storage.keys` and `mailto:` for
  `openExternal` are not served. The only host event is `command`; the phase 6
  `Coalescer` is not wired.
- Plugin API v2 phases other than 1 and the phase 6 core are not built: no backends, no
  background pages, no `calm.menus.register`, no capability display.
- Clicking inside a plugin page does not focus or raise its panel; AppKit routes the click
  to the WKWebView.
- Layouts and session: restore at launch and autosave are active (#41). Not built: layout
  save (`--layout NAME` only opens), the File menu, the `ask` restore dialog (persistence
  plan Task 7) and the later persistence tasks.
- Layout v2 limits: the resize policy is not saved, overrides are stored but not applied
  in the chrome, `page_zoom` is stored but not applied, an unsupported panel draws chrome
  only, and the terminal cwd is the spawn cwd. Lowering `backup_count` leaves older backups
  above the new count in place.
- Autosave details (persistence plan Task 4, #39; active since #41). `calm_core::autosave` holds `AutosaveClock` (quiet `session.debounce_ms`, max
  dirty age 10 s), `SaveQueue` (one write in flight, the latest snapshot waiting) and
  `OrderedWriter` (a write older than the file on disk is skipped, so a late background
  write never replaces the quit write). `Scene` has a revision: every `Scene` mutator and
  every `get_mut` bump it, and `sync_revision` counts direct camera writes. `CanvasView`
  ticks it on each render and on a timer, writes on the background executor, shows a failed
  write in the status pill and on stderr, and retries on the next change. A launch read
  that fails (I/O or malformed) turns autosave off for that launch. Settings changes are reconciled by `autosave_change`: `session.autosave` off stops the
  timer and the quit write, on starts again through the gate, and `debounce_ms`,
  `backup_count` (at the next rotation) and `browser_query` apply to the running save.
  Plugin bridge calls take `get_mut` only for `panel.setTitle` and `settings.set`, so
  read-only calls do not cause writes. New and duplicated plugin and TUI panels carry the
  installed manifest `plugin_version`. Unverified: whether `on_app_quit` fires on cmd-q and
  on every quit path (the smoke calls the quit save directly, not through a real quit), and
  the manual "drag writes `session.json` about 1 s later" check.
- `--bench` is not built, and `docs/benchmarks.md` does not exist. The Makefile `bench`
  target calls a flag that is missing.
- The Ghostty shim patch (plugin API v2 phase 10) is not done, so no terminal `PWD`, title
  or exit code from Ghostty, and TUI `fontSize`, `fontFamily` and `padding` are rejected.
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
- Run Command (`feat/tui-extends-terminal`): Add > Run Command…, type `htop`, Run; quit
  htop with `q`, check the "[calm] htop exited with code 0" line, press Enter and see it
  start again. Cancel and blank input open nothing. Open a Top panel's Settings… and change
  its title bar, accent and terminal colors; the panel recolors live. Reset on the accent
  brings back the manifest's orange (`#ff8a3d`), not the profile's accent.
- Terminals: mouse selection at zoom 1.0 and 1.7, cmd-v paste, Polish letters through the
  input method, resize reflow, Enter to confirm close, pinch feel, `htop`.
- Terminal profiles: the live recolor, the profile submenu, the Settings > Terminal controls.
- Blur: a real mouse click on empty canvas, Escape inside a web page.
- Plugin menus: a real right click (the NSMenu), long titles, the github-prs "Open repo
  in browser" item.
- App icon: the Dock, cmd-tab and About panel by eye.
- Stats readout (#33): CPU and memory against Activity Monitor, the Developer settings, the
  child process walk with real shells running.
- Plugin API v2 phase 1 (#35): in the github-prs page's Web Inspector,
  `calm.has("panel.setTitle")` is `true`; the gh logged-out message still shows.
- Bridge control (`feat/plugin-bridge-control`), from a plugin page's Web Inspector:
  `calm.canvas.createPanel({ kind: "terminal" })` opens a focused shell at the view center,
  `calm.canvas.focusPanel(id)` on a minimized panel restores and raises it,
  `calm.canvas.focusPanel(id)` on another plugin panel moves the keyboard into that page,
  `calm.panel.minimize()` keeps the page running, `calm.panel.close()` removes the plugin
  panel, `calm.process.exec("/bin/sh", ["-lc", "command -v gh"])` finds `gh` from the Dock.
- GitHub PRs plugin with a real `gh` login and with a `.env` token (#23).
- Session settings section in the Settings window (#36), by eye.
- Session restore (#41): a manual quit and relaunch, and the `about:blank` fallback for a bad
  saved url in a live view (unit tested only).
- Brand: the logo and app icon by eye (the 16 px mark reads as a C without a prompt).
- Per-panel settings (`feat/panel-settings`), by eye: Cmd+drag over a live terminal and
  over a page, Cmd+click on a link in a terminal and in a page (the click must land),
  Cmd+right-click menu on both, the panel Settings window and its tabs, Reset to global,
  the borderless look. The panel form does not yet explain that untouched terminal colors come
  from Ghostty Default when no palette is set.
- Performance with 20 terminals.
- A real Release run on the runner and the hosted macOS CI steps.

## Verified on hardware

macOS 27.0.1, Apple M4 Pro: smoke passes; terminals render in zoom views at zoom 1.5;
z-order follows scene order; preview snapshots about one per 2 s; settings persist; CI on
`main` green (linux-arch success, macos skipped).

## Known small issues

- Smoke flakes. Root cause found on 2026-11-01: an occluded window does not paint, so
  every paint-driven step stalls (`.ai/conventions.md`, "Smoke flakes"). #40 (merged) fixed
  the z-order case; the preview and stats cases remain. The notes from 2026-10-31 follow:
  - `preview_snapshots_in_3s=[0,0]`, on `main` and branches, more often under heavy load.
  - `window_visible=Some(false)`: the window was occluded, and GPUI stops painting an
    occluded window, so frame checks fail.
  - The native z-order line did not match the scene order (`zorder` vs scene), seen twice.
  - Stats step after the cmd-shift-f toggle. With the #37 diagnostics, one of three runs
    showed the sampler holding a CPU reading while the pill stayed stuck on
    `--% CPU` for 2 s more (`stats: sampler has CPU but the status bar does not show it`),
    right after the FPS step printed fps=123.9. Another run failed one step earlier with
    "only 1 frames with show_stats on". So the sampler works and paint stalls. Suspected
    cause: window occlusion or the display link stopping. Not confirmed.
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
5. Say go for the remaining plan tasks (10, 11, 12 app wiring, 13, 14) and persistence plan
   Tasks 6 onward.
6. Answer the plugin API v2 questions (`.ai/open-questions.md`).
