# Decisions

Settled decisions with the reason and the cost if wrong. Newest at the bottom. Reversing
one means a new entry, not an edit.

## Product

- **Rust + GPUI, not Swift.** The user chose it after comparing alternatives. Cost: GPUI
  is pre-release (`gpui-pre 0.3.4`), so the entry crate and APIs can shift.
- **No tmux integration.** A terminal panel can run tmux itself. Cost: no session survival
  across app quit, which the user accepted.
- **Processes need not survive app quit.** Fresh canvas after reboot, optional saved
  layouts whose terminals start empty.
- **Plugins are HTML/JS (TSX) and trusted.** Minimal effort, no sandbox, explicitly
  installed. Cost: a malicious plugin has host access; mitigated by explicit install only.
- **GitHub through `gh` on the host.** Plugin JS never sees a token. Cost: users must have
  `gh` logged in.

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
  800x600). The toolbar shrank to a status strip.
- **Preview cadence is one snapshot per 2 s per terminal.** Entering preview does not
  force a snapshot; a failed refresh keeps the last image marked `preview stale`. The smoke
  enforces 1 to 3 snapshots in its window.
- **Grid is drawn as lines, not dots.** About 1500 dot quads stalled paint, which stopped
  preview refresh. Cost: a different look than the brief; the user can veto.
- **Default snap grid is 8 world units, not the requested 3 pixels.** Terminal cells are
  about 8 px wide at zoom 1, so 3 is not a cell multiple. Edge snap within 8, `alt`
  disables. One constant to change if the user wants finer.
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

## TUI app plugins (2026-10-31)

- **A TUI app is `PanelKind::Tui { plugin_id }`, not `Terminal` with a launch spec.** The
  manifest owns command, args, env and restart policy, so a layout stores only the id and
  never holds env values (which may be secrets despite the docs). The panel still lives in
  the shared `terminals` list, so placement, z-order, preview, minimize, resize and close are
  the terminal code. Cost: one more arm in each `PanelKind` match.
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

- **Plugin panels are free-form HTML/TSX surfaces with native browser, terminal and
  plugin-to-plugin APIs.** The user asked for full customization. Cost: plugins get much
  more reach, all of it behind declared capabilities.
- **Reversed: script injection into browser panels is now allowed** for plugins with
  `browser.script`, and cookies and site data for `browser.data`. It replaces the draft
  line "no script injection into browser panels". Browser panels still never get the
  `calm` bridge; plugin scripts run in a separate content world. Cost: such a plugin can
  act as the user on every site open in a browser panel. That fits trusted plugins and is
  documented next to the capability.
- **Capabilities are enforced: undeclared calls are rejected** with `capability_missing`.
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
- **Bun runtime source: pending the user.** Recommended: bun on the user's system with a
  fixed lookup order and a loud failure, not a bundled copy.
