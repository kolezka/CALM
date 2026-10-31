# Status

Last updated: 2026-10-31. `main` at the CLAUDE.md refresh after fd41d94. Facts are
verified unless marked otherwise.

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

## On branch `feat/settings-modal` (not merged)

- Settings window, a native NSWindow with sections Background, Grid, Canvas and
  Snapping, Panels, Developer. Opened by `cmd-,`, CALM > Settings… and Settings > Open
  Settings…, all through one global `OpenSettings` handler; a second open focuses the
  same window. A top-level menu bar item cannot fire an action in GPUI (each top-level
  `Menu` becomes an item with a submenu), so the Settings menu stays a menu.
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
- `docs/architecture.md` refresh for the whole system and `repo-intel build` (Task 15).
  The build script currently fails with an unbound `tmp` variable.
- Plan file lacks the text of Task 17 (Makefile) and Task 16b (release on Linux); both
  exist only as briefs in the git-ignored SDD workspace.

## Unverified

- Manual GUI pass: mouse selection at zoom 1.0 and 1.7, cmd-v paste, Polish letters through
  the input method, panel resize reflow, Enter to confirm close, pinch feel, `htop`.
- Performance with 20 terminals.
- A real Release run on the runner (dry run not yet triggered) and the hosted macOS CI
  steps.

## Known small issues

- `docs/dev-setup.md` lists `--layout work` and `--bench 20` as run commands; neither flag
  exists yet.
- Escape on the close confirmation does not trigger a redraw until the next event.
- A transitive `block 0.1.6` future-incompatibility warning from a dependency.

## User-owned next steps

1. `gh workflow run release.yml --ref main -f bump=patch -f dry_run=true`, then a real run.
2. `gmake release-upload VERSION=x.y.z` on the Mac after the release exists.
3. Set `CALM_MACOS_HOSTED=true` once billing allows hosted macOS CI.
4. Say go for the remaining plan tasks (10, 11, 12 app wiring, 13, 14, 15).
