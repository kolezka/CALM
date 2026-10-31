# Backlog

Agreed follow-ups that nobody is working on yet. Checked against `origin/main` at
`3287b21` and the open pull requests on 2026-10-31. When an item gets an owner or a branch,
move it to `status.md` under "In progress" and delete it here.

## Open items

### 1. MIT license

CALM has no license file, so the code is not formally open. Add a `LICENSE` file (MIT),
`license = "MIT"` in every `Cargo.toml` and `package.json`, and a README section.

- State: not started. No `LICENSE` file on `main`, no `license` field in any `Cargo.toml`,
  no license section in `README.md`. One agent failed to start; a second attempt was declined.
- Blocker: none known. Keep the MIT notice at the top of
  `crates/calm-app/src/terminal/adapter.rs`, which is adapted from gpui-libghostty 0.3.1.
- Origin: user request in the prototype session.

### 2. Per-panel overrides (rename, icon, accent)

Let the user rename a panel, change its icon and set its accent color from the panel
right-click menu, stored on the panel. Today only settings and manifests choose these.

- State: not built (`status.md`, "Not wired yet"). Open PR #31 (layout v2) adds the stored
  `overrides` field (`title`, `icon`, `accent_color`) but does not apply it in the chrome.
- Blocker: needs a native text input (an `NSAlert` with a text field) and agreed ownership
  with the plugin API v2 lane, whose spec has `panels.setAppearance` and `panels.setTitle`.
  Depends on #31 for storage.
- Origin: Task 12 of `docs/superpowers/plans/2026-10-31-canvas-persistence.md`. Deferred by
  PR #21 (TUI customization) and PR #24 (terminal profiles).

### 3. Text field control in the Settings window

The native Settings window has checkboxes, sliders, colors, choices, buttons and notes
(`calm-core::settings_form::Control`), but no text field.
So terminal profiles (add, duplicate, rename, delete), title templates, icons and launch
commands can only be edited in `settings.json`.

- State: not built. `settings_window.rs` uses `NSTextField` only for labels and notes.
- Blocker: none known. The same control would serve `plugins.builtin` and the session
  settings.
- Origin: PR #24 follow-up.

### 4. Resize handles on the left and top edges

Panels resize only from the right edge, the bottom edge and the bottom-right corner
(`ResizeEdge` in `calm-core::interaction`). Left and top handles would match what users
expect from windows.

- State: not started.
- Blocker: left and top edges move the origin and touch `snap_resize`, which PR #12
  rewrote. Must keep the joined-edge rule from PR #16.
- Origin: PR #16, which added only right, bottom and bottom-right.

### 5. Focus a plugin panel on a click inside its page

A click inside a web page goes straight to the WKWebView, so scene focus and z-order do
not change.

- State: done for browser panels. `calm-native::browser` watches clicks with a local event
  monitor and sends `BrowserEvent::Clicked`, which `canvas_view.rs` handles (PR #6). Still
  open for plugin panels: `WebPanel` has no such monitor (`status.md`, "Not wired yet").
- Blocker: none known. The browser monitor is the model to copy.
- Origin: noted in `status.md` since PR #5.

### 6. Flaky smoke

Seen several times on 2026-10-31:

- `preview_snapshots_in_3s=[0,0]`, often with `window_visible=Some(false)`. The window is
  occluded or not key, and GPUI stops painting it.
- "terminals not live at zoom 1.5".
- Twice, native z-order not matching the scene: `zorder=[1, 2] scene=[2, 1]` and
  `zorder=[1,2,3] scene=[2,3,1]`.
- Stats step: "stats: sampler has CPU but the status bar does not show it" (pill stuck at
  `--% CPU` while the sampler has a reading), and "only 1 frames with show_stats on". Both
  look like paint stalling after the stats toggle, likely the same occlusion cause.

- State: cause unconfirmed. Load was suspected. Occlusion is likely for the first two. The
  z-order one may be a real ordering bug.
- Fix direction: make smoke independent of window visibility, or bring the window to the
  front before the checks. Add a deterministic z-order check that reorders panels and
  compares native order with scene order right after placement.
- Origin: smoke runs during the prototype merges (`status.md`, "Known small issues").

### 7. Example plugins that use the SDK widely

Plugins that exercise most of `packages/calm-plugin-sdk/calm.d.ts`, as living docs and a
manual test bed. Ideas: Scratchpad (storage, settings, menus), Repo Pulse (`github.api`,
`openExternal`), Canvas Inspector (calls every method with feature detection).

- State: stopped at the user's request. Worktree `.worktrees/example-plugins`, branch
  `feat/example-plugins`, holds one unmerged commit (`4985f5f`, adds the TUI panel kind and
  a null storage miss to `calm.d.ts` and `docs/plugins.md`). No plugin code.
- Blocker: 8 of the 17 bridge methods still reject with `not implemented` (`canvas.*`,
  `process.exec`, `theme.get`, part of `panel.*`), so the Inspector would mostly show
  rejections.
- Origin: example plugins brief in the prototype session.

## Waiting on a dependency

- Claude Code mods integration waits on plugin API v2 phase 11
  (`docs/superpowers/specs/2026-10-31-claude-code-mods-integration.md`).
- Prototype plan Task 14, `--bench` and `docs/benchmarks.md`, is not started.
- The Linux port stays a plan only, by user decision.
