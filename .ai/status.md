# Status

Last updated: 2026-10-31. `main` at the repo-intel config commit after the `.ai/` folder
(0236d0c). Facts are verified unless marked otherwise.

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

## On branch `feat/menu-plugins` (not merged)

- Plugin discovery at startup fills the right-click Add menu. Skipped plugin folders print
  to stderr and show as a red `plugins: N errors` label in the status strip.
- Adding a plugin opens a Plugin panel (`crates/calm-app/src/plugin_host.rs`): a `WebPanel`
  in a `NativeChild`, prelude injected at document start, entry loaded with `load_file`.
  Placed like terminals: Live sets the frame each paint, Preview and Hidden hide the view
  and draw the flat title rect. Terminals and plugins share one native stack that restacks
  with the scene order.
- Bridge: `log`, `panel.setTitle`, `settings.get`, `settings.set` are served. The other 13
  methods reject with `not implemented: <method>`. `github-prs` renders but its list shows
  that error until `github.api` is dispatched.
- Fixed: the prelude posted plain objects, which WebKit delivers as an NSDictionary whose
  description is not JSON. It now posts `JSON.stringify(...)`; the bun test fake rejects
  non-string bodies.
- An entry page whose local `<script src>` is missing (an unbuilt `dist/`) gets an error
  page naming the file and the bun command, a `(not built)` title, and a stderr line.
- Smoke adds two fixture plugins and prints `plugin loaded=true bridge=ok title=smoke-ok`,
  `plugin unbuilt reported=true` and a four-panel `plugins zorder` line.
- Docs: `dev-setup.md`, `architecture.md` (terminals, preview), `plugins.md`,
  `release.md`, `CLAUDE.md`, this folder.
- Graphify graph: `repo-intel build` rc=0 on 2026-10-31 (graphify only, backend litellm
  with `deepseek/deepseek-flash`, scope in `.graphifyignore`: `docs/superpowers/`, lock
  files and the plist template excluded). 55 code files, 16 docs, 1247 nodes, 2472 edges.
  `AGENTS.md` carries the routing block, imported from `CLAUDE.md`. A git post-commit hook
  rebuilds it in the background.

## Verified on hardware

macOS 27.0.1, Apple M4 Pro: smoke passes; two terminals render in zoom views at zoom 1.5;
z-order follows scene order; preview snapshots about one per 2 s; settings persist;
CI run on `main` green (linux-arch success, macos skipped).

## Not wired yet

- Browser panels in the canvas (`WebPanel` exists; Task 10 app wiring pending).
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

- Manual GUI pass: mouse selection at zoom 1.0 and 1.7, cmd-v paste, Polish letters through
  the input method, panel resize reflow, Enter to confirm close, pinch feel, `htop`.
- Performance with 20 terminals.
- A real Release run on the runner (dry run not yet triggered) and the hosted macOS CI
  steps.

## Known small issues

- The smoke's preview refresh step is intermittent: `preview_snapshots_in_3s=[0,0]` failed
  once on unmodified `main` (clean export, no other smoke running) and twice on
  `feat/menu-plugins`, then passed. It predates the branch. Cause not found.

- `docs/dev-setup.md` lists `--layout work` and `--bench 20` as run commands; neither flag
  exists yet.
- Escape on the close confirmation does not trigger a redraw until the next event.
- A transitive `block 0.1.6` future-incompatibility warning from a dependency.
- repo-intel 0.1.8 prints `tmp: nieustawiona zmienna` on its error path (for example after
  an interrupted build). Upstream bug; harmless when the build succeeds.

## User-owned next steps

1. `gh workflow run release.yml --ref main -f bump=patch -f dry_run=true`, then a real run.
2. `gmake release-upload VERSION=x.y.z` on the Mac after the release exists.
3. Set `CALM_MACOS_HOSTED=true` once billing allows hosted macOS CI.
4. Push `main` (local commits: CLAUDE.md refresh, `.ai/`, repo-intel config).
5. Say go for the remaining plan tasks (10, 11, 12 app wiring, 13, 14, 15 docs).
