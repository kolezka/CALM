# Status

Last updated: 2026-10-31. `main` with PRs #1 (recenter), #2 (panel menu actions), #3
(panel chrome) and #4 (bottom bar) merged. Facts are verified unless marked otherwise.

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
- Bottom bar (PR #4): the top status strip is removed; the title bar is transparent with a
  hidden title; zoom, FPS and the save flash show in a native translucent pill at the
  bottom center (`calm_native::StatusBar`, rect from `calm_core::status_bar`). Smoke
  checks the bar is visible and above both terminals after a restack.

## Verified on hardware

macOS 27.0.1, Apple M4 Pro: smoke passes; two terminals render in zoom views at zoom 1.5;
z-order follows scene order; preview snapshots about one per 2 s; settings persist;
CI run on `main` green (linux-arch success, macos skipped).

## Not wired yet

- Browser panels in the canvas (`WebPanel` exists; Task 10 app wiring pending).
- Plugin host in the app: 17-method dispatcher, `canvas.createPanel`, settings and
  storage, `gh` dispatch. Today no `window.calm` call resolves (Tasks 11 and 12 app parts).
- Layout save and load in the app (`--layout`, Task 13). The core format exists.
- Benchmark mode (`--bench`, Task 14) and `docs/benchmarks.md`.
- `docs/architecture.md` refresh for the whole system (Task 15; the graph part is done).
- Plan file lacks the text of Task 17 (Makefile) and Task 16b (release on Linux); both
  exist only as briefs in the git-ignored SDD workspace.

## Unverified

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
- Smoke fails on `main` (7d53cbb and later) under heavy load (load average 17 to 40) with
  only `preview_snapshots_in_3s=[0,0]`; every other smoke line passes. Cause unconfirmed.
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
