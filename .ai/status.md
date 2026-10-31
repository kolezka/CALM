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

## On branch `feat/menu-panel-actions` (not merged)

- Panel context menu on the title strip or a minimized chip: Minimize or Restore, Zoom to
  Fit (hidden while minimized), Bring to Front, Send to Back, Duplicate (terminals only, a
  new terminal of the same size 32 units down and right, fresh process), Delete.
- Delete uses the normal close path: a live terminal shows the close confirmation unless
  `confirm_close` is off in settings.
- Right click on a live Ghostty view goes to Ghostty, not to CALM, so the menu opens only
  from the title strip and the chip. Right click on content in preview mode does nothing.
- Rename is not offered: it needs a native text input, which does not exist yet.
- Manual GUI pass of each action is pending.

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
