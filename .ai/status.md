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

## Verified on hardware

macOS 27.0.1, Apple M4 Pro: smoke passes; two terminals render in zoom views at zoom 1.5;
z-order follows scene order; preview snapshots about one per 2 s; settings persist;
CI run on `main` green (linux-arch success, macos skipped).

## On branch `feat/browser-links` (not merged)

- Browser panels are live: `calm_native::BrowserView` puts a native address bar (back,
  forward, reload, text field) and a WKWebView in the panel's zoom view, so the bar scales
  and clips with the page and GPUI draws nothing over it. App side in
  `crates/calm-app/src/browser_panel.rs`. No `calm` message handler on browser pages.
- Address input: `calm_core::browser_url::normalize_address`. Kept schemes load as typed,
  `localhost` and IPv4 get `http://`, dotted hosts get `https://`, the rest searches
  DuckDuckGo (`SEARCH_URL_PREFIX`).
- Links navigate in place. `target=_blank` and `window.open` load in the same panel (UI
  delegate returns no new web view).
- While the page or the bar has the keyboard: cmd-[ back, cmd-] forward, cmd-r reload,
  cmd-l address bar, plus cmd-c/x/v/a/z and cmd-shift-z (an NSEvent local monitor, since
  GPUI takes key equivalents first and the app has no Edit menu).
- A click on the page raises and focuses the panel. Title and URL follow the page into the
  chrome and into `PanelKind::Browser.url`. A new panel opens `about:blank` with the
  address bar focused. Closing a browser asks for confirmation like a running terminal.
- Preview and modal UI hide the page and paint a flat rect with the title. No page
  snapshot yet.
- Smoke adds one browser that loads a `data:` page which clicks its own link, and checks
  `smoke: browser loaded=true link=followed url=about:blank#clicked mode=live`. Frame-check
  failures now print `window_visible=`; failures seen during this work all had the window
  occluded (`Some(false)`), and GPUI stops painting an occluded window.
- Not done: cmd+click on a URL in a terminal, non-web schemes (`mailto:` and the like do
  nothing), page snapshots for previews, downloads. Manual GUI pass pending.

## Not wired yet

- Plugin host in the app: 17-method dispatcher, `canvas.createPanel`, settings and
  storage, `gh` dispatch. Today no `window.calm` call resolves (Tasks 11 and 12 app parts).
- Layout save and load in the app (`--layout`, Task 13). The core format exists.
- Benchmark mode (`--bench`, Task 14) and `docs/benchmarks.md`.
- `docs/architecture.md` refresh for the whole system and `repo-intel build` (Task 15).
  The build script currently fails with an unbound `tmp` variable.
- Plan file lacks the text of Task 17 (Makefile) and Task 16b (release on Linux); both
  exist only as briefs in the git-ignored SDD workspace.

## Unverified

- Browser panels by hand: typing in the bar at zoom 0.5 and 2, link clicks, `target=_blank`,
  the keyboard shortcuts, scroll over a page, pinch over a page.
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
