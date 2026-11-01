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

- `crates/calm-core`: pure model. Geometry, camera, scene, interaction, chrome, joins,
  resize policy, grid, layout v2 and session storage (durable writes, backups, url scrub),
  viewport placement, preview policy, snapping, settings and the settings form, terminal
  profiles, backgrounds, status bar rect, TUI launch, menus and plugin menus, frame and
  process stats math. No GPUI, no AppKit. Everything here has unit tests.
- `crates/calm-native`: AppKit. Clip view plus zoom view for each native child, Ghostty
  parenting, WKWebView with the `calm` message handler, the browser view, the NSMenu context
  menu, the Settings window, the status pill, the runtime app icon, the process stats
  sampler. All `unsafe` lives here, each block with a SAFETY comment.
- `crates/calm-plugins`: manifest discovery (user and built-in dirs), TUI manifests,
  `contributes.menus`, API v2 error codes (`api.rs`), JS bridge (`CALM_PRELUDE_JS`,
  `BRIDGE_METHODS`), event coalescer (`events.rs`, not wired), `gh api` client and token
  lookup (`gh_token.rs`).
- `crates/calm-app`: the GPUI binary. Thin: maps events to `calm-core`, places native views,
  hosts terminals, TUI apps, browsers and plugins, owns the Developer and Settings menus and
  the libghostty adapter.
- `plugins/hello-world`, `plugins/github-prs`: first-party web plugins (`github-prs` is Preact
  TSX built with bun). `plugins/top-example`: a TUI app. `templates/plugin-tsx`,
  `templates/plugin-tui`: starters. `packages/calm-plugin-sdk/calm.d.ts`: the `window.calm` API.
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
- Before any pre-merge build, check or test, `cargo clean -p` the touched crates: a stale
  rlib once failed a test with missing symbols. Always run
  `cargo check --workspace --tests --examples`; `cargo build` skips tests and examples.
- Build and check after every merge, even a clean one. Text merges hide semantic conflicts.
- Known smoke flakes come from paint stalls in an occluded window (`window_visible=Some(false)`):
  `preview_snapshots_in_3s=[0,0]`, "only N frames with show_stats on", the stats pill stuck
  at `--% CPU`. A z-order mismatch is no longer a flake since #40. A run with a flake is not a pass.
  Run a control on `origin/main` before blaming a branch.
- Plugin tests that need bun fail unless bun is installed; `CALM_SKIP_BUN_TESTS=1` skips them.
- Not wired yet: `--bench`, layout save (`--layout` only opens), the `ask` restore dialog, the event
  coalescer, and plugin events other than `command` (all 17 bridge methods answer; the v2
  namespaces are design only). The Makefile `bench` target exists; the flag does not. Do not
  document them as working. Browser, plugin and TUI app panels and plugin menu items are
  wired.
- Benchmarks go to `docs/benchmarks.md` with hardware and method. Never quote a number
  without a run.

## In the app

`t` then drag draws a terminal, `b` then drag a browser. Right-click opens Add and panel
actions, plus plugin menu items. Space+drag or
middle-drag pans. Pinch, cmd+scroll, `cmd-=`, `cmd--`, `cmd-0` zoom. `cmd-shift-h`
centers the world origin, `cmd-shift-0` fits all panels (both also in the View menu).
`cmd-ctrl-f` fullscreen, `cmd-shift-f` stats (FPS, CPU, memory; items in `developer.stats`). A click on empty canvas clears panel focus
(`canvas.click_empty_clears_focus`). `alt` while dragging disables snapping (default
grid 8 world units, edge snap within 8). `cmd-,` opens the native Settings window
(background, grid, snapping, panels, terminal, session, developer). Settings persist in
`~/.config/calm/settings.json`; `CALM_HOME` overrides the directory. A launch restores the
last session from `session.json` (new shells, browsers at their url, plugins with their
settings) and autosaves it; `--no-restore` starts empty, `--layout NAME` opens
`layouts/NAME.json`, and `session.restore_on_launch` `off` turns restore off (`ask` has no
dialog yet and restores with a notice). Distant panels become previews refreshed every 2 s.

## Rules that are easy to break

- Native views (Ghostty, WKWebView) draw above everything GPUI paints. Nothing GPUI draws may
  overlap a live native rect. Modal UI is native (NSMenu) or hides native views first.
- Camera zoom scales the zoom view. It never changes a terminal's logical grid. Only a panel
  resize does. The zoom view's bounds always equal the panel's logical size.
- Native z-order follows scene order. Every placement calls `set_frame` and `bring_to_front`.
- A panel's process keeps running through pan, zoom, preview, minimize and culling. Only an
  explicit, confirmed close ends it.
- Grid lines are quads. Dots and crosses are a cached texture drawn tiled, never one quad
  or image per dot: about 1500 dot quads once stalled paint and stopped preview refresh.
  The grid's draw count must not grow with the number of marks (`mark_primitive_bound`).
- No credentials in layouts, manifests, docs, logs or tests. GitHub goes through `gh api` on
  the host. Plugin JS never sees a token.
- Plugins are trusted, explicitly installed code. Do not call them sandboxed anywhere.
- Agents never drive the GUI beyond `--smoke`. Synthetic keystrokes land in other windows.
- Agents never run the Release workflow, tag, create releases or push `main`. Merging to
  `main` needs the user's current go-ahead each time. A denied command is reported, never
  reshaped to get past the gate.
- Every non-trivial PR gets a cross-family review before merge. Each one so far found real
  defects (`.ai/conventions.md`, "Review and merging").
- Retarget a stacked PR to `main` before merging it, then confirm with
  `git merge-base --is-ancestor <merge-commit> origin/main`.
- CI runs on a self-hosted Linux runner with Docker socket access. Every self-hosted job keeps
  the same-repo guard so fork pull requests never run there. Hosted macOS CI is opt-in
  (`CALM_MACOS_HOSTED`). The macOS zip is built locally with `gmake release-upload`.
- Code changes go through a git worktree under `.worktrees/`, never the main checkout.
- After structural changes run `repo-intel build` and check its exit code. The graphify graph
  (graphify only, backend litellm, scope in `.graphifyignore`) answers concept and
  architecture questions; `rg` answers literal and call-site questions. See `AGENTS.md`.
- Customization first. Every user-visible feature exposes its look and behavior as settings
  or manifest fields (colors, icons, titles, sizes, fonts, keys, behaviors) with sensible
  defaults, validated strictly. A hard-coded choice needs a reason in `.ai/decisions.md`.
  Security rules above still apply.

## Process

Plan execution uses superpowers subagent-driven development with Agent Teams; details in
`.ai/conventions.md`. The ledger with rulings lives at
`.superpowers/sdd/2026-10-31-calm-prototype/progress.md` (git-ignored, main checkout).
Work state persists in Outline under the task record `calm-canvas-design`.

- Implementation and docs changes each go to a new agent session. The lead coordinates and
  verifies.
- `kolezka-bootstrap:route-gpt-astra` and `kolezka-bootstrap:route-fable` cannot spawn
  workers. As orchestrators they return ordered briefs and the lead dispatches.

- Shut down idle agent sessions. When a teammate or subagent has finished its task and is
  only idle (its result is reported and verified, or it failed), the lead sends it a
  shutdown request at once, or stops it, and confirms it is gone from the agent list. Do not
  keep finished agents around for later; a new task gets a new session.

<!-- repo-intel:begin -->
@AGENTS.md
<!-- repo-intel:end -->
