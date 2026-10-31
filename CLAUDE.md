# CALM

Infinite pan/zoom canvas for macOS (arm64, macOS 14+) hosting live Ghostty terminals,
WKWebView browser panels and trusted HTML/JS plugins. Rust + GPUI.

## Read first

Knowledge lives in `.ai/`, indexed by `.ai/INDEX.md`. On a cold start read, in order:

- `.ai/system.md`: what CALM is, crate boundaries, event and paint flow, plugin model.
- `.ai/status.md`: what is merged, what is not wired, what is unverified.
- `.ai/conventions.md`: how work is done here (worktrees, gates, tests, agents, security).
- `.ai/decisions.md`: settled decisions with rationale. Check before re-opening one.

Human-facing guides: `docs/dev-setup.md`, `docs/architecture.md`, `docs/plugins.md`,
`docs/release.md`. Spec: `docs/superpowers/specs/2026-10-31-calm-design.md` (binding).
Plan: `docs/superpowers/plans/2026-10-31-calm-prototype.md`.

Keep `.ai/` current: update `status.md` when a task lands or a gap is found, and
`decisions.md` when a decision is made or reversed.

## Workspace

- `crates/calm-core`: pure model. Geometry, camera, scene, interaction, layouts, viewport
  placement, preview policy, snapping, settings, menus, frame stats. No GPUI, no AppKit.
  Everything here has unit tests.
- `crates/calm-native`: AppKit. Clip view plus zoom view for each native child, Ghostty
  parenting, WKWebView with the `calm` message handler, the NSMenu context menu. All
  `unsafe` lives here, each block with a SAFETY comment.
- `crates/calm-plugins`: manifest discovery, JS bridge (`CALM_PRELUDE_JS`, `BRIDGE_METHODS`),
  `gh api` client.
- `crates/calm-app`: the GPUI binary. Thin: maps events to `calm-core`, places native views,
  owns the Developer and Settings menus and the libghostty adapter.
- `plugins/hello-world`, `plugins/github-prs`: first-party plugins (Preact TSX, built with
  bun). `templates/plugin-tsx`: starter. `packages/calm-plugin-sdk/calm.d.ts`: the
  `window.calm` API.
- `scripts/`: version bump, macOS bundle, Arch CI container, release upload.
  `.github/workflows/`: `ci.yml`, `release.yml`.

## Build, run, test

- Toolchain: Rust >= 1.95, Zig 0.16 (`ZIG` set in `.cargo/config.toml`), Xcode CLT with the
  Metal Toolchain, `gh`, `bun`, GNU Make 4 (`gmake`). `gmake setup-check` verifies it.
- Run: `cargo run -p calm-app` (windowed) or `-- --fullscreen`. Release build for real use.
- While iterating: `cargo test -p <crate>`. Before a merge: `gmake ci` (clippy with
  `-D warnings`, fmt check, workspace tests, smoke, plugin type checks).
- GUI gate: `cargo run -p calm-app -- --smoke; echo rc=$?`. A green build is not a running
  window. Smoke prints viewport, canvas, terminals, zorder, fps, settings and preview lines
  and must exit 0. Gate before every push.
- Plugin tests that need bun fail unless bun is installed; `CALM_SKIP_BUN_TESTS=1` skips them.
- Not wired yet: `--bench`, `--layout`, browser and plugin panels in the app, layout save and
  load. The Makefile targets for them exist; the flags do not. Do not document them as working.
- Benchmarks go to `docs/benchmarks.md` with hardware and method. Never quote a number
  without a run.

## In the app

`t` then drag draws a terminal. Right-click opens Add and panel actions. Space+drag or
middle-drag pans. Pinch, cmd+scroll, `cmd-=`, `cmd--`, `cmd-0` zoom. `cmd-ctrl-f`
fullscreen, `cmd-shift-f` FPS. `alt` while dragging disables snapping (default grid 8 world
units, edge snap within 8). Settings persist in `~/.config/calm/settings.json`; `CALM_HOME`
overrides the directory. Distant panels become previews refreshed every 2 s.

## Rules that are easy to break

- Native views (Ghostty, WKWebView) draw above everything GPUI paints. Nothing GPUI draws may
  overlap a live native rect. Modal UI is native (NSMenu) or hides native views first.
- Camera zoom scales the zoom view. It never changes a terminal's logical grid. Only a panel
  resize does. The zoom view's bounds always equal the panel's logical size.
- Native z-order follows scene order. Every placement calls `set_frame` and `bring_to_front`.
- A panel's process keeps running through pan, zoom, preview, minimize and culling. Only an
  explicit, confirmed close ends it.
- Grid is drawn as lines, not dots. The dot grid stalled paint and stopped preview refresh.
- No credentials in layouts, manifests, docs, logs or tests. GitHub goes through `gh api` on
  the host. Plugin JS never sees a token.
- Plugins are trusted, explicitly installed code. Do not call them sandboxed anywhere.
- Agents never drive the GUI beyond `--smoke`. Synthetic keystrokes land in other windows.
- Agents never run the Release workflow, tag, create releases or push `main`. Merging to
  `main` needs the user's explicit instruction each time.
- CI runs on a self-hosted Linux runner with Docker socket access. Every self-hosted job keeps
  the same-repo guard so fork pull requests never run there. Hosted macOS CI is opt-in
  (`CALM_MACOS_HOSTED`). The macOS zip is built locally with `gmake release-upload`.
- Code changes go through a git worktree under `.worktrees/`, never the main checkout.
- After structural changes run `repo-intel build` and check its exit code. The graphs are not
  built yet; `docs/architecture.md` is the current substitute.

## Process

Plan execution uses superpowers subagent-driven development with Agent Teams; details in
`.ai/conventions.md`. The ledger with rulings lives at
`.superpowers/sdd/2026-10-31-calm-prototype/progress.md` (git-ignored, main checkout).
Work state persists in Outline under the task record `calm-canvas-design`.

<!-- repo-intel:begin -->
@AGENTS.md
<!-- repo-intel:end -->
