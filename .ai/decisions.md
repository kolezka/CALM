# Decisions

Settled decisions with the reason and the cost if wrong. Newest at the bottom. Reversing
one means a new entry, not an edit.

## Product

- **Rust + GPUI, not Swift.** The user chose it after comparing alternatives. Cost: GPUI
  is pre-release (`gpui-pre 0.3.4`), so the entry crate and APIs can shift.
- **No tmux integration.** A terminal panel can run tmux itself. Cost: no session survival
  across app quit, which the user accepted.
- **Processes need not survive app quit.** Fresh canvas after reboot, optional saved
  layouts whose terminals start empty. The "fresh canvas" part is superseded by "Restore
  the last session on launch" (Canvas persistence, below) once that plan lands; processes
  still do not survive.
- **Plugins are HTML/JS (TSX) and trusted.** Minimal effort, no sandbox, explicitly
  installed. Cost: a malicious plugin has host access; mitigated by explicit install only.
- **GitHub through `gh` on the host.** Plugin JS never sees a token. Cost: users must have
  `gh` logged in.
- **Customization first (user, 2026-10-31).** Every user-visible feature exposes its look
  and behavior (colors, icons, titles, sizes, fonts, keys, behaviors) as settings or
  manifest fields with sensible defaults and strict validation. A hard-coded choice needs
  its own entry here. Security rules still win over any setting. Reason: the user wants
  CALM shaped to their own workflow, not to the author's taste. Cost: more settings to
  validate, test and document, and more states to support. If it proves wrong, fold rarely
  used settings back into fixed defaults.

## Architecture

- **`gpui-pre-platform` as the GPUI entry crate.** `gpui-pre 0.3.4` has no
  `Application::new`; `gpui_platform::application()` with `font-kit` is the working entry.
- **Metal Toolchain installed instead of the `runtime_shaders` feature.** The user chose
  the install. Reversible via Xcode settings.
- **Native child = clip view + unflipped zoom view.** A flipped container broke Ghostty's
  shim y math and did not clip. The zoom view's bounds always equal the logical size, so
  the surface never sees the camera. Supersedes the earlier flipped-container ruling.
- **Zoom scales the view, never the grid.** `surface.set_frame` runs each placement with
  logical size and display scale; the surface skips unchanged frames, so only a resize
  reflows.
- **Native z-order follows scene order.** `bring_to_front` restacks only when the order
  changed; `set_frame` runs on every placement and the crate dedupes.
- **Context menu is a native NSMenu.** A GPUI popover would be covered by native views.
  "Add" creates a panel at the click point with a default size (terminal 640x400, browser
  800x600). The toolbar shrank to a status strip (since replaced by the bottom status pill,
  below).
- **Preview cadence is one snapshot per 2 s per terminal.** Entering preview does not
  force a snapshot; a failed refresh keeps the last image marked `preview stale`. The smoke
  enforces 1 to 3 snapshots in its window.
- **Superseded by "Dot and cross grids are back" below.** **Grid is drawn as lines, not dots.** About 1500 dot quads stalled paint, which stopped
  preview refresh. Cost: a different look than the brief; the user can veto.
- **Default snap grid is 8 world units, not the requested 3 pixels.** Terminal cells are
  about 8 px wide at zoom 1, so 3 is not a cell multiple. Edge snap within 8. At the
  user's request, Cmd+Shift replaces alt to disable snapping; alt alone snaps again. The
  gesture is hard-coded like the other gesture keys, with no setting yet. One constant
  changes the grid if the user wants finer.
- **Dot and cross grids are back, as tiled textures (reverses "Grid is drawn as lines, not
  dots").** The user asked for grid styles (lines, dots, crosses, none), mark size and a
  major grid. Lines stay one quad per line. Dots and crosses are rasterized once per
  style, color and zoom bucket (12 per doubling) into a tile of at least 256 points and
  drawn tiled, so a frame draws at most `mark_primitive_bound` images (60 on 1280x800)
  however many marks are visible; a test fails if a frame draws one image per dot. The
  minor grid fades out between 16 and 8 screen points while the major grid stays. Cost if
  wrong: a cached tile is stretched up to about 6 % before the next bucket, so marks can
  look slightly soft, and a new bucket rasterizes on the UI thread (about 1.8 ms for the
  largest tile in a debug build, plus upload).
- **Settings live in a separate native NSWindow, not a GPUI modal.** Option (a) of two:
  a GPUI modal would have to hide every live native view while open, and GPUI has no color
  picker, slider or file dialog. A separate AppKit window never shares a rect with Ghostty
  or WKWebView, terminals stay live while it is open, and `NSColorWell` and `NSOpenPanel`
  come free. The form is data (`calm_core::settings_form`), so `calm-native` stays a
  generic renderer and the logic is unit tested. Cost: about 300 lines of objc2 with
  manual frame layout, and the window looks like AppKit, not like the GPUI canvas.
- **Backgrounds render once per change, off the UI thread, into a cached texture.**
  Gradients raster at most 512 px on the long edge and the GPU scales them up; images are
  capped at 2048 px, then blur (`fast_blur`), color filters and tile repetition run once.
  Paint draws one image (or a few tiles of at least 256 px). The image is read from its
  path and not copied: a moved file shows an error in Settings and the fill color.
- **Settings version 2 migrates version 1 and fails loud.** A version 1 preset becomes
  its exact solid color. A malformed, out of range or unknown-version file is an error
  from `load_settings_from`, never quiet defaults; at startup the app reports it, moves
  the file to `settings.json.invalid-<n>` so the next save cannot overwrite it, uses
  defaults and shows a notice at the top of the Settings window.
- **Invalid layout files fail loudly.** Malformed JSON or out-of-range values are
  `LayoutError::Malformed` with path and field; tiny rects are clamped to the minimum size.
- **Plugin entry must be a regular file under the canonical plugin directory.** Symlinked
  entries outside the folder are refused.
- **SDK includes `canvas.createPanel` and `process.exec`.** Plugins are trusted and the
  user wants agents to spawn terminals. Documented as trusted-only.
- **Status bar is a native translucent pill at the bottom; no top strip.** The GPUI
  status strip on top is gone. Zoom, FPS and the save flash live in a centered,
  rounded `NSVisualEffectView` (`calm_native::StatusBar`, material Popover, blending
  WithinWindow) floating 14 pt above the canvas bottom. It is native because terminals
  draw above GPUI, so a GPUI bar would be hidden by any panel passing under it. It stays
  the topmost sibling: `ensure_front` runs after every native restack, and the smoke
  fails if a panel ends up above it. Rect math is `calm_core::status_bar`. The window
  title bar is transparent with a hidden title (`TitlebarOptions::appears_transparent`),
  so the canvas fills the window and the traffic lights float over it. Rejected: a GPUI
  bar with native views clipped above it, which keeps a hard edge where terminals end.
  Cost if wrong: canvas clicks in the top 28 pt strip drag the window instead (native
  title bar drag), and clicks on the bar fall through to the canvas.

- **Joined panels leave a 1 unit gap, filled by the divider.** Panels join when facing
  edges are 0 to 2 world units apart (`JOIN_GAP`) and their spans overlap. Edge snapping
  puts facing edges exactly 1 unit apart (`JOIN_SNAP_GAP`), never flush, because GPUI
  cannot paint over a native view: a 0 gap leaves the divider no room beside two live
  terminals. The divider fills the gap, so it is 1 px at zoom 1 and scales with zoom.
  Over the title bars it is at least 1 px wide, so a pair at gap 0 (only reachable with
  `cmd-shift`) still shows a seam in the bar, but none over the content. All panel kinds join.
  Resizing a shared edge moves one panel; snapping keeps it joined. Rejected: a 0 gap
  with the divider drawn on chrome only, which leaves two terminals visually fused.
  Cost if wrong: the 1 unit offset breaks exact grid alignment of the second panel.

## TUI app plugins (2026-10-31)

- **Superseded 2026-11-02: a TUI app is no longer `PanelKind::Tui`.** See "TUI apps are
  terminals (2026-11-02)". The rule that a layout stores only the plugin id still holds.
- **Commands are built as an argv and joined with POSIX single quotes.** Ghostty takes one
  string and on macOS runs `bash -c "exec -l <string>"`. `shell: true` adds
  `$SHELL -lc 'exec "$0" "$@"'` so the login profile sets `PATH`; env goes through
  `/usr/bin/env K=V` after it, since 0.3.1 has no env option. A leading `/usr/bin/env`
  absorbs the `-` that `exec -l` adds to argv[0]. Cost: the login shell must speak POSIX sh
  (fish users set `shell: false`), and env values show in `ps`.
- **Exit codes come from a status wrapper, not Ghostty.** 0.3.1 drops `SHOW_CHILD_EXITED`
  and has no exit code accessor. A `/bin/sh` wrapper runs the command, writes `$?` to a
  per-launch file and exits 0. 127 means command not found and never auto-restarts. Cost:
  a 127 from the program itself reads as not found; a killed wrapper gives "code unknown".
  The Ghostty fork planned in Plugin API v2 could replace this.
- **TUI look comes from the manifest; confirmation colors stay fixed.** `accentColor`,
  `titleBarColor` and `titleColor` override the theme per panel, but a close confirmation
  always paints the theme's warning bar, border and text, so a plugin cannot hide it. The
  accent marks focus only, so the unfocused border stays the theme's. Image icons are not
  tinted. Cost: a plugin cannot restyle the confirmation.
- **`terminalColors` needs all 16 palette colors.** gpui-libghostty 0.3.1 takes per-surface
  colors only as a full `TerminalTheme` (background, foreground, 16 colors) laid over the
  user config, with no way to read the user's palette back. A partial set would silently
  replace the rest with made-up values. Cost: verbose manifests.
- **`fontSize`, `fontFamily` and `padding` are rejected, not ignored.** 0.3.1 has no
  per-surface font or padding option; they wait for the libghostty shim patch. An explicit
  "planned" error beats a field that does nothing. Cost: manifests written for the future
  fail today.

## Process

- **Agent Teams with disjoint write scopes.** SDD forbids parallel implementers; lanes run
  in separate worktrees per crate and are reviewed per lane.
- **Gate before push.** A broken merge was pushed once (737b762). Merges land only after
  workspace tests, clippy, fmt and smoke pass on the merged tree.
- **No agent drives the GUI beyond `--smoke`.** Synthetic input landed in other windows.
  The user owns manual GUI acceptance.
- **GNU Make 4 required.** macOS Make 3.81 ignores `.SHELLFLAGS`, so the strict shell
  contract was inert. The Makefile errors on old Make; use `gmake`.

## CI and release

- **Linux CI tests portable crates only.** `calm-native` and `calm-app` are AppKit-only.
  No Linux app artifact is promised. The job runs in an `archlinux:latest` container on the
  self-hosted runner with bun installed into a cached volume.
- **Hosted macOS CI is opt-in (`CALM_MACOS_HOSTED=true`).** GitHub-hosted macOS minutes
  failed on account billing. Cost: no automated macOS build until billing or a Mac runner.
- **Release runs on the Linux runner.** It bumps, tags, creates the release with a source
  tarball and checksum. `dry_run` defaults to true. The macOS zip is built locally with
  `gmake release-upload VERSION=x.y.z`, which refuses unless HEAD is the tag and the tree is
  clean, and only targets the release bound to `origin`.
- **Fork pull requests never run on the self-hosted runner.** It has Docker socket access.
  Every self-hosted job carries the same-repo guard.

## Plugin API v2 (2026-10-31)

Design in `docs/superpowers/specs/2026-10-31-plugin-api-v2-design.md`. Not implemented yet.
(Since then phase 1 and the phase 6 core have landed, see "Plugin API v2 phase 1" and
"Event coalescer" below.)

- **Plugin panels are free-form HTML/TSX surfaces with native browser, terminal and
  plugin-to-plugin APIs.** The user asked for full customization. Cost: plugins get much
  more reach, all of it behind declared capabilities.
- **Reversed: script injection into browser panels is now allowed** for plugins with
  `browser.script`, and cookies and site data for `browser.data`. It replaces the draft
  line "no script injection into browser panels". Browser panels still never get the
  `calm` bridge; plugin scripts run in a separate content world. Cost: such a plugin can
  act as the user on every site open in a browser panel. That fits trusted plugins and is
  documented next to the capability.
- **Superseded by "capabilities are declarative only" below.** **Capabilities are enforced: undeclared calls are rejected** with `capability_missing`.
  The manifest list is only useful if it is true. It is a declaration, not a sandbox.
  Manifests without `apiVersion` get the implicit v1 set. Cost: author friction.
- **External control socket approved, with PR #9's allowlist.** Default is read-only plus
  `panel.setStatus` and `events.*`. Spawning with a command, `terminals.write` and
  `process.exec` are one unit, refused by default and enabled only together, because
  refusing one while allowing another is no limit. `github.api` stays plugin-only. Build
  after PRs #5 and #6 merge. Cost: any process running as the user that can read the
  token file can drive CALM.
- **Background pages are opt-in hidden pages** (`"background": true`), one WKWebView each.
  They serve commands, events and RPC with no panel open. Cost: one web content process
  per plugin that opts in.
- **Ghostty shim patch is a planned phase.** gpui-libghostty 0.3.1 drops `SET_TITLE`,
  `PWD` and other actions and does not expose `read_text`, `size`, `foreground_pid` or
  `binding_action`. A CALM-owned fork through `[patch.crates-io]` adds them. Cost: upkeep
  on every upstream bump.
- **Plugins may use the full Node and Bun API, unrestricted.** The user: "what someone
  installs and uses is their business". A plugin may declare `main`, which CALM runs as a
  bun child process with the same `calm.*` host API, talking to the plugin's pages and
  serving `exports`. Backends cover most headless needs, so background pages move to the
  last phase. Cost: an installed plugin has the same power as any program the user runs;
  install stays explicit.
- **Reversed: capabilities are declarative only, never enforced.** This replaces
  "capabilities are enforced" above. The manifest list is shown at install and in a plugin
  info view; no call is rejected for an undeclared capability, and `capability_missing` is
  gone. Reason: a plugin with a backend bypasses any bridge gate, so the gate only slowed
  honest plugins. Cost: the list can drift from the code; unknown words still warn.
- **Still holds after the reversal:** the external socket allowlist (it governs outside
  processes), no `calm` bridge in browser panels, CALM never hands its own secrets to
  plugins, and the API never closes a live terminal without the user's confirmation (a
  guard for the user's work against plugin bugs; a backend could still kill the process).
- **Bun runtime source: the user's installed bun (decided 2026-10-31).** Lookup order: the
  `plugins.bun_path` setting, then a one-time login shell `$SHELL -lc 'command -v bun'`,
  then `~/.bun/bin`, `/opt/homebrew/bin` and `/usr/local/bin`. A missing bun, or one outside
  `engines.bun`, fails loud with an install hint in the panel and the status bar error count.
  Reason: developers already need bun; bundling adds size, signing and delayed security
  fixes. Cost if wrong: a bundled copy can be added later as the last lookup step.

## Panel resize (2026-10-31)

- **One resize policy for every kind, in `calm-core::resize`.** `ResizePolicy::derive`
  takes the kind and, for plugins, the manifest's `resizable`, `minSize` and `maxSize`.
  Press, drag clamp and the hover cursor all read it through `resize_target` and
  `InputHandler::hover_resize`. Terminals and browsers are always free. Bad manifest values
  are errors in `calm-plugins`, never defaults. Cost: `calm-plugins` depends on `calm-core`.
- **Resize handles live outside the panel, in GPUI pixels.** A band of 8 screen px past the
  right and bottom edges, plus the old 12 unit band inside (which only reaches GPUI over
  terminals and previews). The user decided on 2026-11-02 that resize handles take
  priority over content moves. Presses with the configured move modifier on the inner
  band are captured as resizes even over a live web view; unmodified web presses still
  go to the web view. Rejected: insetting the web view from its edges, which would
  break "zoom view bounds equal the panel's logical size", and painting over the native
  rect. Cost: a press just outside a panel resizes it instead of hitting what is behind,
  and over a joined or overlapping neighbor that is a live web view, the web view still
  wins the click.
- **On a joined edge the outer band stops at the neighbor.** Merged with PR #12: the band
  skips any point inside a panel joined to this one, so a press on the neighbor's title bar
  or content goes to the neighbor. The inner band and the 1 unit join gap still resize the
  left or top panel, and only that panel moves. Unjoined edges keep the full 8 px band.
  Cost: over a joined web view the shared edge is grabbable only in the 1 unit gap and the
  inner band, which a web view keeps for itself unless the move modifier is held.
- **Only right, bottom and corner handles.** Left and top edges would move the origin and
  touch `snap_resize`, which PR #12 rewrites. Cost: no resize from the left or top yet.
- **No `aspectRatio`.** Snapping and two clamped axes make it more than cheap.

## Brand assets (2026-10-31)

- **The shipped app identity uses a fixed pixel palette.** The user requested a cyberpunk,
  pixel-art CALM wordmark and icon. The source pixel grids, palette and all export sizes live
  in `assets/brand/build.py`, so anyone can rebuild or change the artwork without a runtime
  setting. This is a release asset, not an in-canvas feature or a plugin appearance. Cost: a
  custom icon needs a rebuild of the app bundle rather than a setting.

## Built-in plugin dirs (2026-10-31)

- **First-party plugins load without install.** A dev build (debug, or a binary in a cargo
  `target` dir) searches the workspace `plugins/`; a bundle searches
  `Contents/Resources/plugins`. Both come after the user dirs, so a user copy with the same id
  wins and the shadowing is logged. Off with `plugins.builtin=false` or
  `CALM_NO_BUILTIN_PLUGINS=1`. Reason: `gmake plugins && cargo run` showed no plugins. Cost:
  the dev path is baked in at compile time, so a moved checkout needs a rebuild.

## Agents (2026-10-31)

- **Shut down idle agents at once.** When a teammate or subagent has reported a verified
  result, or failed, the lead stops it and confirms it left the agent list. A new task gets
  a new session. The user's rule (PR #14); no reason was recorded with it. Cost: a
  follow-up question needs a fresh agent that rebuilds context.

## Canvas persistence (2026-10-31, plan, not built)

Plan in `docs/superpowers/plans/2026-10-31-canvas-persistence.md`, approved by the user.

- **Restore the last session on launch, on by default.** `session.restore_on_launch` is
  `on`, `off` or `ask`; `--layout` and `--no-restore` override it. Only the arrangement comes
  back (kind, rect, order, settings); processes start fresh. Reverses the spec line "a fresh
  launch starts with an empty canvas"; the plan's first PR amends the spec. Cost if wrong:
  a bad session file greets every launch, so a failed restore moves it aside and starts empty.
- **Layouts live in a standard File menu.** New Canvas, Open Layout, Open Recent, Save
  Layout, Save Layout As, Restore Previous Session, Reveal Layouts in Finder, before View. It
  replaces a Layouts menu from an earlier draft. Cost: `cmd-n`, `cmd-o` and `cmd-s` are taken.
- **Browser URLs lose query and fragment on save.** `session.browser_query` defaults to
  `strip_all`; `keep` is opt-in. Reason: the "no credentials in layouts" rule, and tokens
  often ride in query strings. A name list (`strip_sensitive`) was rejected as best effort.
  Cost: pages that need their query reopen without it.

## Session restore (2026-10-31, approved by the user, implementation pending)

The user approved each entry below on 2026-10-31 (canvas persistence plan, Task 1 and
"Resolved 2026-10-31"). None of it is built yet. Status lives in `.ai/status.md`.

- **Reversal: restore the last session on launch by default.** This reverses the spec
  sentence "A fresh app launch after a computer restart starts with an empty canvas; users
  can explicitly load a saved layout." The spec now reads "A fresh app launch restores the
  last session's panels with fresh processes, unless restore is off". Processes still start
  fresh: terminals get new shells, browsers reload their url, plugins reload their page.
  "Processes need not survive app quit" stands. Reason: the user's goal, "when I start the
  canvas the same plugins and elements load". Cost if wrong: a bad or unwanted session file
  greets every launch; mitigated by moving a failed file aside and starting empty,
  `restore_on_launch` set to `off` or `ask`, and `--no-restore`.
- **Opening a named layout replaces the canvas after one confirmation.** New Canvas, Open
  Layout, Open Recent and Restore Previous Session ask once, with a native alert, when live
  terminals exist or the canvas has changes not saved to a named layout. Open never merges.
  Reason: one shared rule is simpler than a merge mode, and the confirmation keeps the rule
  that only an explicit, confirmed close ends a process. Cost if wrong: users who want to
  add a layout's panels to the current canvas cannot, and the alert can feel like friction.
- **Browser urls are saved with query and fragment stripped by default.**
  `session.browser_query` is `strip_all` by default; `keep` is an opt-in that saves the url
  as is. Reason: query strings and fragments often carry tokens (magic links, OAuth `code`,
  presigned signatures), and layouts must hold no credentials; the user asked for the lowest
  effort, and one strip function needs no name list. Cost if wrong: search results and pages
  that keep state in the query or fragment reopen without it; with `keep`, a token can land
  in the file, which the docs must say.
- **A standard File menu, not a Layouts menu.** Seven items, after the app menu and before
  View: New Canvas, Open Layout..., Open Recent, Save Layout, Save Layout As..., Restore
  Previous Session, Reveal Layouts in Finder. Reason: the user asked for a menu bar File menu
  to save and load layouts, and it is the standard macOS place for them. Cost if wrong:
  `cmd-n`, `cmd-o`, `cmd-s` and `cmd-shift-s` are taken, and whether they fire while a
  terminal has focus is unverified.

## Platform (2026-10-31)

- **A Linux port stays a plan only.** The app is macOS only; a Linux port plan exists as a
  proposal and nothing is built. Linux CI keeps testing the portable crates. Reason: the
  native layer (Ghostty parenting, WKWebView, NSMenu, NSWindow) has no Linux backend, and the
  macOS prototype is not done. Cost: no Linux users until a backend is designed.

## Terminal profiles (2026-10-31)

- **Terminal options live in `settings.json` under `terminal`, snake_case like the rest of the
  file.** Defaults at the top level, named `profiles` beside them. No version bump: a file
  without the section loads as before. Cost: TUI manifests say `accentColor`, settings say
  `accent_color`.
- **Unknown keys in `terminal` are errors.** Serde flatten collects them so `validate` can
  name them. Other sections still ignore unknown keys. Cost: inconsistent strictness.
- **`use_ghostty_user_config` is per profile; the global switch stays
  `panels.terminal_user_config`.** One global knob, not two. `terminal_colors` lays over the
  user config when it is on, over Ghostty defaults when off, same as TUI.
- **`command` and `args` are one unit.** The layer that sets `command` gives the args; args
  alone are an error. `env` merges key by key. Other fields are taken whole.
- **Env goes through `/usr/bin/env` in argv.** gpui-libghostty 0.3.1 has no env option.
  Values are never printed, and Settings masks them, but `ps` shows them. Documented.
- **Live recolor uses `TerminalThemeState::apply`.** It exists in 0.3.1 and keeps the process.
  It cannot remove a theme or flip the user config, so those apply to new terminals.
- **The panel's profile is kept beside the scene, not in `PanelKind`.** Layouts are not
  wired and other branches edit `PanelKind` users. Move it into the kind with layout save.

## Blur on outside click (2026-10-31)

- **A click on nothing drops focus.** A left press and release on empty canvas within
  `CLICK_SLOP` (4 points), or a right click there, emits `Effect::ClearFocus`: the scene
  forgets the focused panel and the GPUI host view becomes the AppKit first responder, so a
  Ghostty surface, web view or address field stops getting keys. Drawing with a tool clears
  focus on press. Pans (space, middle button, scroll, pinch) never do. Setting
  `canvas.click_empty_clears_focus`, default true. Reason: the focus ring and the keyboard
  stayed with the last panel after clicking away. Cost: a click on empty canvas while typing
  into a terminal moves the keyboard away, like any other app.
- **Escape clears focus only when no native view has the keyboard.** The canvas `Cancel`
  action checks the first responder; when a terminal, page or address field holds it,
  Escape stays theirs and focus stays. Same setting. Reason: never steal Escape from a
  running program. Cost: Escape cannot blur a panel whose native view holds the keyboard.

## Plugin context menu items (2026-10-31)

- **Flat `contributes.menus` items, not v2's `commands` plus `menus.canvas/panel`.** Each item
  carries its own `id`, `title` and `location`, and `when` is a struct (`kind`, `plugin`,
  `minimized`) instead of the `kind == terminal` string. Reason: the lead's brief for the
  user's request, one place per item, and strict per-field validation without a parser.
  Cost: a later `contributes.commands` (for key bindings) needs items to reference commands
  or keep both shapes.
- **The payload key is `id`, not v2's `command`,** and it adds `location`, the full `panel`
  info and `world`. The event name stays `command`.
- **Built-in items first, plugins cannot remove them.** Plugin sections follow a separator,
  one per plugin in discovery order. Reason: plugins are trusted, but a broken plugin must
  not hide Delete. Users hide plugin items with `plugins.menus.hidden`.
- **Submenu above 3 items per plugin, a setting (`plugins.menus.submenu_above`).** Three
  items inline keep a short menu flat; more nest under the plugin name.
- **No background page target.** v2 sends commands to a background page first; those do not
  exist, so the order is clicked panel, focused panel, frontmost panel, else a new panel.
  `"open": "new"` skips reuse. The prelude queues commands until a handler registers, and
  undeliverable commands are logged.
- **`icon` is text before the title, no `shortcut`.** NSMenu images and key equivalents would
  need `MenuItem` changes across the menu code; a key shown in a context menu that does not
  work outside it would mislead. Key bindings belong to v2 `keybindings`.
- **No runtime `calm.menus.register` yet.** It needs a per-plugin registry the bridge
  dispatcher can reach, which is being changed in parallel for `github.api`.
- **`apiVersion` is not required for `contributes`.** The host does not read `apiVersion`
  yet. (Second sentence superseded by #35: the host now reads `apiVersion`. `contributes`
  still does not need it.)

## Plugin API v2 refresh (2026-10-31)

- **Layouts move into plugin API v2 as phase 16.** The v2 design listed layout save and
  load as a non-goal; the canvas persistence plan needs `layouts.list`, `save`, `open`,
  `delete` and `session.saveNow`, so the design now has them. `layouts.open` closes live
  terminals only through the user's confirmation, and the write methods sit in the
  external socket's `layout` group. The plan's File menu and restore on launch are
  decided. Cost: phase 16 waits on the plan's PRs 1 to 7.

## Runtime app icon and About panel (2026-10-31)

- **An unbundled run sets the brand icon at runtime.** `calm_native::app_icon` embeds
  `CALM.iconset/icon_512x512.png` and calls `setApplicationIconImage` at startup, so
  `cargo run` shows it in the Dock, cmd-tab, About and any NSAlert. When the main bundle
  names `CFBundleIconFile` it does nothing: the bundle's `CALM.icns` has the hand-tuned 16
  and 32 px sizes, and one 512 px image would replace them. gpui-pre 0.3.4 has no app icon
  API, so this is raw AppKit. Cost: the binary carries a 3 KB PNG.
- **A broken icon logs and keeps the default.** Bytes are fully decoded with the `image`
  crate first, because NSImage reads only the header and would accept corrupt pixel data.
  Never a panic, never silent. AppKit stores a re-rendered copy with no name and other
  bytes, so the install reads the icon back and errors if it still equals the default; the
  smoke fails unless that readback passed.
- **"About CALM" is the first app menu item** and opens the standard About panel with the
  brand icon, the Cargo version and one fixed credits line. No settings: it is app identity,
  like the brand palette above. "Settings…" now comes after About.
- **Alerts inherit the icon.** gpui-pre's `prompt` builds `NSAlert::new` without an icon, and
  NSAlert then shows the application icon. CALM itself shows no NSAlert today (close
  confirmation is drawn in GPUI), so no `alert.icon` is set. macOS windows have no per-window
  icon, so the Settings window and status bar get nothing.

## CPU and memory readout (2026-10-31)

- **CPU percent is of one core,** like Activity Monitor and `top`, so it can pass 100%.
  It is user plus system time over all threads (`ri_user_time + ri_system_time` from
  `proc_pid_rusage`), converted from Mach ticks with `mach_timebase_info` (on Apple
  silicon the counters are ticks, not ns; a unit test fails without the conversion),
  averaged over the sampling interval. No extra smoothing: the interval is the window.
- **Memory is phys_footprint,** the Activity Monitor number, not RSS. Binary units
  (1 MB = 1024 * 1024 bytes); `auto` switches to GB at 1 GB.
- **Scope is CALM alone by default.** `developer.stats.include_children` adds every process
  below CALM (shells, TUI apps, plugin backends), walked with `proc_listchildpids` and
  capped at 512. Processes seen in only one of two samples do not count toward CPU.
  libghostty runs in-process, so terminal rendering is in the self number either way.
- **cmd-shift-f stays a per-launch toggle** for the whole readout; the persisted
  `developer.stats.show_fps|show_cpu|show_memory` pick the items. FPS frames are only
  forced while `show_fps` is on, so CPU-only mode does not measure its own 60 fps repaint.
  Sampling runs on the background executor on a timer, never in paint, and stops when
  CPU and memory are both hidden or the readout is off.

## Plugin services (2026-10-31, PR #23)

- **`github.api`, `storage.*` and `openExternal` run on the background executor.** They are
  parsed on the UI thread and answered from a fresh GPUI task, never from inside the WebKit
  message callback. Reason: a slow `gh` must not stall the canvas. Cost: two dispatch paths
  (`plugin_services` and `dispatch`) that `served_methods` must keep in step; a test checks it.
- **Storage is one JSON file per plugin** in `$CALM_HOME/plugin-data`, written to a temp
  file and renamed. Keys up to 256 bytes, file up to 5 MiB. A corrupt file rejects every
  call and is left alone, never replaced by `{}`. Cost: a corrupt file blocks that plugin's
  storage until the user fixes or removes it.
- **`openExternal` takes `http` and `https` only,** through `/usr/bin/open`. Cost: no
  `mailto:` yet.
- **`gh` may get a token from `.env`.** Order: `GH_TOKEN` or `GITHUB_TOKEN` in CALM's
  environment, `$CALM_HOME/.env`, the checkout's `.env` in dev builds, else `gh`'s own
  login. Only those two keys are read. The token reaches `gh` as `GH_TOKEN` in its child
  environment, never in argv, and is replaced with `<redacted>` in output, errors and
  Debug. A world-readable file warns. Reason: users without `gh auth login` could not use
  the GitHub plugin. Cost: a token sits in a plain file the user must keep private.

## Layout v2 (2026-10-31, PR #31)

- **Unknown panel kinds keep their exact JSON bytes** (`PanelKind::Unsupported` with a
  `RawValue`, never reserialized through `Value`) and save back unchanged. A known type with
  a bad shape stays `Malformed`. Reason: a layout from a newer CALM or a missing plugin must
  survive a load and save. Cost: such a panel shows chrome only.
- **v1 files migrate on read and are never rewritten;** versions 0 and 3 and up fail with
  `UnsupportedVersion`. Cost: an old file stays v1 on disk until something saves it.
- **The resize policy is not saved;** it is derived from the kind on load, and ids are fresh
  on every load. Reason: one source of truth in `calm-core::resize`. Cost: none known.
- **Overrides are `title`, `icon`, `accent_color`,** field set agreed with the plugin API v2
  owner. An empty `overrides` writes no key, and unknown override keys still load.

## Plugin API v2 phase 1 (2026-10-31, PR #35)

- **Every host reject carries a code.** `BridgeResponse::Err` holds an `ApiError`, so the
  compiler rejects a bare string. The prelude maps a v1 string reject to `host_error`.
  Cost: none known; plugins that matched on message text still get the same text.
- **`settings.*` and `storage.*` on a non-plugin panel reject with `wrong_kind`,** not
  `host_error`, as the spec table's closest code. Differs from the brief.
- **`calm.has` is synchronous and reads `window.__calmHost`,** injected before the prelude
  from `served_methods`. Reason: feature detection before any call. Cost: the list must
  match what the host serves; a test checks every method against `route`.
- **TUI manifests keep `contributes`** (follows #28) and refuse `main`, `backend`, `engines`
  and `background`, each with its own error.
- **`engines.bun` is syntax checked with a hand-written npm range grammar,** full
  prerelease and build included. The `semver` crate was rejected: its `VersionReq` is Cargo
  syntax, without `||`, hyphen ranges or space-separated comparators. Cost: a parser to
  maintain.
- **`storage.set` without `value` rejects with `invalid_params`; explicit `null` is
  stored.** Reason: `calm.storage.set('k', undefined)` arrived as `{"key":"k"}` and
  overwrote `k` with null. Cost: none known.
- **A `main` that points at a missing file skips the plugin,** like `entry`. Cost: a plugin
  whose backend is not built does not load.
- **Not decided (open question 3):** `backend.restart` says `on-failure` and `never`, TUI
  `restart` says `prompt`, `on-exit` and `never`.

## Event coalescer (2026-10-31, PR #34)

- **`bus` events are discrete,** never merged. The spec table lists no coalescing for them,
  and merging would lose messages. Differs from the brief.
- **Per-event default windows are left to the host** through `set_window`. Cost: the host
  wiring must set `terminalOutput` 250 ms, `terminalBell` 1 s and `panelFocused` per frame.
- **`emit_batch_js` has its own copy of the JS escaper.** Phase 2 unifies it with
  `bridge.rs::js_literal`. Cost: two copies until then.

## Session storage (2026-10-31, PR #36)

- **One durable write path.** `write_durable`: exclusive 0600 temp in the same directory,
  fsync, rename, directory fsync, temp removed on failure, new dirs 0700. Layouts and
  `settings.json` use it, so `settings.json` is now 0600 too. Cost: a few fsyncs per save.
- **Backups rotate once per writer, by copy.** `session.json` is copied to `.1`, never
  moved, so the live file stays in place through rotation. Nothing rotates when
  `session.json` is missing, so a failed restore that was moved aside never enters the
  backups. Cost: lowering `backup_count` leaves older backups in place.
- **The write path scrubs urls inside browser titles too.** A new browser panel is titled
  with its url, so the query leaked through `title`. Urls over 8 KiB save as `about:blank`
  in both modes.
- **The setting is `session.debounce_ms`,** as the task brief asked; the plan table follows.

## Smoke diagnostics (2026-10-31, PR #37)

- **Diagnose before fixing the stats flake.** The stats step now separates "sampler has no
  CPU reading" from "pill does not show it", without weakening the check. Reason: the old
  step already polled for 5 s, so it was not an early read. Cost: the flake remains until
  the paint stall is understood.

## Panel overrides contract (2026-10-31, agreed by the persistence and API v2 lanes)

- **`PanelOverrides` is `{title, icon, accent_color}`, each `Option<String>`,** skipped
  when `None`, with no `deny_unknown_fields` (`calm-core::scene`). Later appearance fields
  grow this struct through the persistence plan, not through API v2. The persistence plan
  owns the layout file format and this struct; `calm-plugins::api` owns the wire types;
  API v2 phase 16 maps one to the other and adds nothing to the file on its own. Reason: one
  owner per format, and an older CALM still opens a file with more override fields. Cost
  if wrong: an appearance field API v2 needs waits on a persistence plan change.

## Autosave (2026-10-31, PR #39)

- **Autosave ships behind `APP_AUTOSAVE_ACTIVE = false` until restore lands.** Reason:
  without restore, a launch starts empty, and the first change would replace the last
  session on disk. Cost: no user gets autosave until Task 5 (#41) turns the gate on.
  Superseded: #41 merged and set the gate to `true`.
- **Read-only bridge calls do not take `get_mut`.** Only `panel.setTitle` and
  `settings.set` mark the scene dirty. Reason: every `get_mut` bumps the scene revision,
  so a read caused a write. Cost: a new mutating bridge method must remember `get_mut`.

## Restore at launch (2026-10-31, persistence plan Task 5)

- **`ask` restores with a notice until the dialog exists.** The NSAlert needs new
  `calm-native` code outside this task's files. `ask` behaves like `on` and the status pill
  and stderr say so, so it never silently picks a side. Cost: `ask` users get no choice yet.
- **The session file is read on every launch, whatever the source.** Autosave writes it
  even with `--layout`, `--no-restore` or restore off, so a malformed one is moved aside in
  those cases too, before any write could rotate it into the backups. Cost: a file the user
  did not ask to restore can be renamed to `.invalid-<n>`, with a notice.
- **A terminal whose saved cwd is gone keeps that cwd in its saved kind.** It starts in
  home with a notice, and the next launch tries the path again (an unmounted drive comes
  back). Cost: the notice repeats until the panel is closed or the path returns.
- **Placeholders are an app-side mark, not a new kind.** The saved `PanelKind` stays as
  loaded, so every save writes it back unchanged and an install brings the panel back at
  the next launch. The plugin version is not restamped on restore.

## Session restore review rulings (2026-11-01, PR #41)

These shipped with #41, next to "Restore at launch" above.

- **A failed launch read closes the autosave gate for the whole launch**
  (`AutosaveStart::launch_gate`). A Settings toggle cannot start the writer afterwards.
  Reason: the file on disk may be good but unreadable, and any write would replace it.
  Cost: no autosave for that launch, even after the user fixes the cause.
- **A restored panel keeps its saved title until the panel sets its own.** Reason: the
  user should see the title they left, not the template. Cost: a plugin that fails to load
  never sets a title, so `(not built)` stays hidden, and a terminal settings change still
  re-renders the title (`.ai/backlog.md` items 11 and 12).
- **A saved cwd that is gone starts the shell in home, not in the profile's cwd**
  (`restore::restored_cwd`). The chrome shows "cwd missing, started in ~", which is never
  saved, and the saved kind keeps the old path for the next launch. Reason: home always
  exists, and the profile cwd may be gone too. Cost: the user lands in home, not in the
  profile's folder.
- **An invalid saved browser url opens `about:blank`** (`browser_url::restored_url`).
  Reason: the panel keeps its place and the bad url never loads. Cost: the user sees a
  blank page and has to type the address again.

## Bridge control (2026-11-01, `feat/plugin-bridge-control`)

- **Window work leaves `dispatch` as `HostEffect`s.** `dispatch` stays pure over a
  `HostContext` and the canvas applies `Open`, `Focus` and `Close` with the window. The
  bridge task runs through `spawn_in`, so effects apply in the same turn instead of a
  queued `Effect::CreatePanel` at the next render (the spec's fallback). Cost:
  `CanvasView::new` takes the window.
- **A bridge close is a request, never a removal.** `HostEffect::Close` maps only to
  `Effect::ClosePanelRequested`; a test fails if it maps to `ClosePanel`. In v1 only
  `panel.close` on the calling plugin panel exists, which has no process and closes at
  once, as the spec says. The live terminal confirmation with a pending reply and `busy` is
  `panels.close(id)`, phase 5, not built. Cost if wrong: none today; phase 5 still owes the
  pending reply.
- **Focusing a plugin panel gives its page the keyboard.** `Effect::FocusPanel` used to
  focus only terminals and browsers, so `canvas.focusPanel` and a focused `createPanel`
  of a plugin left the keys where they were. `WebPanel::focus` makes the web view first
  responder. This also applies to a title bar click and the Add menu. Cost: the canvas
  shortcuts (`t`, `b`) stop while a plugin page has the keyboard, as with a terminal.
- **`panel.minimize` sets, it does not toggle.** A second call changes nothing. Reason: a
  plugin that calls it twice must not restore itself.
- **`canvas.createPanel` defaults `focus` to true,** as the Add menu does. The spec gives
  the field and no default. Cost: a plugin that opens many panels must pass `false`.
- **`canvas.createPanel` without `rect` opens at the viewport center at the kind's default
  size, unsnapped.** Spec text for "neither `rect` nor `near`". A locked plugin with a
  `rect` of another size rejects `invalid_params` instead of silently taking its manifest
  size. v2-only spec fields reject `unavailable`, unknown ones `invalid_params`.
- **`canvas.listPanels` returns the v1 fields plus the scene-only v2 fields** (`z`,
  `focused`, `pluginId`, `url`, `resize`). `mode`, `joined`, `appearance`, `createdBy` and
  `tui` need host state this change does not collect. An `Unsupported` panel lists with
  kind `"unsupported"`, a word the spec does not have. Cost: the d.ts kind union grew.
- **`process.exec` has no shell and its own process group.** A timeout kills the group with
  `/bin/kill -KILL -<pgid>`, so a grandchild cannot outlive it and no unsafe code is needed
  in `calm-app`. Waiting on the pipes shares the same deadline, so a background grandchild
  holding stdout ends as `timeout`, not a hang. A stream over the cap ends the call at once
  with `too_large` and kills the group too, even while the child still runs (review
  finding: a writer that ignores SIGPIPE used to run on until the timeout). Limits: 30 s default and 4 MiB per stream
  (spec), 1 to 600000 ms (`EXEC_MAX_TIMEOUT_MS`, chosen here). A signal reports
  `128 + n`. Output is UTF-8 lossy. The child gets CALM's environment and stdin is empty.
  Cost: a bare name resolves in CALM's short Dock `PATH`; docs say to use `/bin/sh -lc`.
- **Fixed labels in the canvas menu (2026-11-01).** The "Native" and "Plugins" section
  headers in Add and the "Settings…" item below it are hard-coded, like "Add", "Terminal"
  and "Browser" before them. This is an exception to Customization first. Reason: they
  name built-in structure, not a look or behavior a user tunes, and a rename setting per
  label adds validation and docs for little gain. Cost: a user cannot rename them. If that
  is wanted, add one `menus.labels` settings block for all built-in menu labels rather than
  one setting per label.

## Native occlusion (2026-11-01, `fix/native-occlusion`)

- **Native views are masked by the panels above them.** Native views are subviews of the
  GPUI host, so they draw over all GPUI paint, and a back panel's content covered a front
  panel's title bar. Each placement now carries the visible pieces of its container
  (`calm_core::occlusion`, the container minus each higher panel's rect or chip plus the
  1 px ring, shadows ignored). The clip view gets a mask of those pieces, one
  `CAShapeLayer` path (one layer per piece left a hairline where two pieces meet), with
  implicit animations off, and a `hitTest:` override so masked areas take no clicks. No
  mask when nothing covers it; hidden when everything does. Rejected: drawing chrome as
  native views (a second chrome implementation and a native view per panel), and hiding
  overlapped panels (a partly covered terminal would vanish). Cost if wrong: a mask layer
  per overlapped panel per paint while it moves.
- **A join divider is an occluder.** Paint fills the whole gap of a join right after the
  later of its two panels, and the 1 px rings do not close a gap wider than 2 px on screen
  (a 1 unit gap at zoom 4 left a 2 px strip of a rear native view over the divider). So the
  screen rect of `join.divider` occludes every panel behind either joined panel. A panel's
  own dividers lie outside its content, so they never cut into it. The thin title bar seam
  of a near-zero gap is not added: it sits inside the two panels' own occluders.
- **Rounded corners stay a known limitation.** Occluders are rectangles, so at a front
  panel's rounded corner a rear native view is cut square, so the small notch outside the
  rounding shows the GPUI paint under the rear view instead of its content. Exact cutouts would need a curved mask path per corner.
- **Pointer input follows the mask.** A partly covered terminal takes a press only on its
  visible part. Motion and the release follow a press it took, so a drag that starts inside
  keeps tracking over the panel in front, and Ghostty never gets a release without its
  press (`calm_core::occlusion::PointerGate`). A move event with no button held drops any
  remembered press, since GPUI only sends a release to hovered views.

## Per-panel settings (2026-11-01, `feat/panel-settings`)

- **Each field is inherited or set.** The panel Settings window shows every field with its
  inherited value until the user sets it. Clearing a field puts it back to inherited, and
  Reset to global clears every override. Only set fields are saved in `overrides`.
- **Order: panel, then profile, then defaults, then theme.** A panel override wins over its
  terminal profile, which wins over the `terminal` defaults, which win over the Ghostty
  theme.
- **An override title is literal.** It is shown as typed; `{n}` and `{cwd}` are not
  expanded, unlike a profile title template.
- **Icon files are copied into the settings folder.** A chosen image goes to
  `panel-icons/<hash>.<ext>` under the settings folder, so the layout never points outside
  it. Symlinks are refused. Built-in icons are stored as `builtin:<name>`.
- **Move modifier is a setting.** `canvas.move_modifier`, default `cmd`, with no `off`
  value: a drag on content without it always goes to the native view.
- **Content gesture: topmost panel only, never on chrome.** The monitor takes a press only
  over the content of the topmost panel at that point (`content_gesture_target`); chrome
  and chips keep their own handling.
- **Click replay posts the up, then sends the down.** A modifier click within the slop is
  replayed to the native view: the later events go to the front of the queue first, then
  the press is sent, so a view that tracks the mouse in its own loop still gets its up. At
  the up the app answers exactly what `ContentGesture::release` gives, never Hold, which
  would drop the click.
- **Borderless keeps the content rect.** A borderless panel loses its title bar and ring,
  but its content keeps the same world rect, so a native view does not jump.
- **One Settings window per panel.** Opening it again brings the open one to the front; it
  closes with the panel. A global settings change rebuilds every open panel window so
  inherited values stay current.
- **Settings windows are child windows of the canvas.** Both the global and the panel
  Settings windows are added to the canvas NSWindow with `addChildWindow:ordered:Above`
  when shown, so a click on the canvas, a panel taking focus or fullscreen never buries
  them. They also follow the canvas into its fullscreen Space and move with it. Hide and
  close detach first; the title bar close detaches through AppKit (smoke checks both).
  Not a setting: a Settings window under its canvas has no use.
- **Duplicate copies overrides.** The copy gets the same overrides and terminal profile, so
  it looks like the original; borderless geometry stays consistent.
- **A one-well color edit seeds from Ghostty Default.** Setting one terminal color when
  nothing is set yet fills the other wells from the Ghostty Default palette.
- **Terminal colors are inherited and reset one by one.** A panel stores only the colors it
  sets (each of background, foreground and color 0 to 15 is optional); a session file with a
  full palette loads as every color set. Reason: the spec says each field is inherited or set,
  and the window has a Reset per color. Cost: builds before fa8ab43 cannot read the partial form.
- **Untouched colors fall back to Ghostty Default on a live terminal when nothing else defines a palette.**
  libghostty takes a full theme only, so the panel's colors are merged over Ghostty Default,
  not over the user's Ghostty config. The form does not say so yet. Cost: a user with Ghostty
  config colors sees the other colors change after a one-color edit.
- **Cmd on panel content wins over a resize handle inside the content rect,** in both the
  native and the GPUI paths. Plain drag still resizes. Reason: the spec gives Cmd+drag anywhere
  on content to moving.
- **A restored terminal opens its saved profile.** `PanelKind::Terminal.profile` is saved
  with the session. An unknown profile id opens with the defaults and one notice; the saved
  kind keeps the id, so it comes back if the profile does.
- **Panel shortcuts: Cmd+Shift+Backspace deletes, Cmd+Shift+, opens Settings.** Both
  are hard-coded like the other canvas bindings until key bindings become settings. Plain
  `cmd-backspace` stays with shell and web fields, where Ghostty and WKWebView use it to
  delete to the start of the line. The Settings key is written `cmd-<` in code because GPUI
  reports the Cmd+Shift+, press that way (with Cmd held it drops Shift for punctuation and
  keeps the shifted character); a `cmd-shift-,` binding would never match. On layouts where
  `<` is on another key, the shortcut moves with it.

## TUI apps are terminals (2026-11-02, `feat/tui-extends-terminal`)

- **A TUI app is `PanelKind::Terminal` with `app: Some(TerminalApp { plugin_id,
  plugin_version })`.** Built by `PanelKind::app_terminal`, read with `.app()`.
  `PanelKind::Tui` is gone. Why: one terminal path for spawn, refresh, recolor and exit.
  Run Command would otherwise have needed a third path, and apps get every per-panel
  terminal feature (Settings sections, presets, live recolor, profiles) for free.
- **The saved format stays.** An app panel still saves as `type=tui` with `plugin_id` and
  `plugin_version`, plus an optional `profile` (left out when unset, so old files round-trip
  unchanged). An app panel never saves `command`, `cwd` or env.
- **Looks stack in four layers, later wins:** terminal defaults, profile, manifest (title,
  icon, `accentColor`, `titleBarColor`, `titleColor`, `terminalColors`), panel overrides.
  Resetting one panel color falls back to the manifest, not the profile.
- **Env stacks the same way, key by key:** defaults, profile, manifest. The use of the Ghostty
  user config follows the profile, else `panels.terminal_user_config`. Command, args, cwd
  (else home), `shell` and restart come from the manifest only.
- **`{name}` in title templates.** An app's manifest name, a typed command's first word
  without its folder, or the shell's first word for a plain terminal. A typed command's
  default title is `{name}`; a panel title override wins. Every terminal, apps too, counts
  toward `{n}`.
- **Run Command: typed commands are saved in the session file.** The panel is a terminal
  with `command`, run in the login shell with defaults and profile env, under the status
  wrapper. A restore runs it again. The prompt says "The command is saved in the session
  file. Do not put secrets in it."
- **`terminal.command_restart` defaults to `"prompt"`.** Values `"never"`, `"on_exit"`,
  `"prompt"`, validated strictly. Why "prompt": it is the TUI manifest default too, it never
  restarts a command on its own, and Enter or the panel menu still restarts it once it has
  exited. A change applies at the next exit and restarts nothing.
- **A terminal's tick loop starts at spawn, not at its first paint.** The loop services the
  surface and reports the exit. Started on first render, a window that did not paint
  (occluded, stalled frame) never saw a new or restarted terminal exit, which was the
  `tui-exit` smoke flake, also seen on `main`. The exit check runs only from the loop, after
  the caller subscribed.
- Costs: apps now inherit defaults and profile env (before: manifest env only), so a profile
  variable reaches every app run in it. Any terminal kind with a `command` is now supervised
  with exit handling: plugin API `panels.create` with kind `terminal` and a `command`, and
  saved terminals with a `command`. Their exit now shows "[calm] <name> exited with code N"
  and follows `terminal.command_restart`. Plugin API kind `tui` and `PanelInfo.kind` "tui"
  are unchanged.
