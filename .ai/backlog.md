# Backlog

Agreed follow-ups with remaining unassigned work. Checked against `origin/main` at
`7de2c55` (PRs #1 to #37 merged) on 2026-10-31. Move an item to `status.md` under
"In progress" once all of its remaining work has an owner or branch.

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

- State: storage done in PR #31 (layout v2 `overrides`: `title`, `icon`, `accent_color`).
  Not applied in the chrome and no menu items (`status.md`, "Not wired yet").
- Blocker: needs a native text input (an `NSAlert` with a text field) and agreed ownership
  with the plugin API v2 lane, whose spec has `panels.setAppearance` and `panels.setTitle`.
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
- Native z-order mismatch: `zorder=[1, 2] scene=[2, 1]` and
  `zorder=[1,2,3] scene=[2,3,1]`. Traced to restacking only during paint: an occluded
  window can stop painting while scene changes and new native panels arrive. The
  `fix/native-zorder-desync` branch restacks on scene changes and native creation; pending
  merge. Its smoke checks both reorder directions immediately, without waiting for paint.
- Twice, native z-order not matching the scene: `zorder=[1, 2] scene=[2, 1]` and
  `zorder=[1,2,3] scene=[2,3,1]`.
- On `feat/session-autosave`, with the autosave smoke step run right after the app icon
  check, the plugins step failed 6 of 6 runs with `zorder=[2, 3, 1, 4, 5]
  scene=[3, 1, 4, 5, 2]`, both when the step reordered panels and put them back and when
  it only moved one. Native order matched the scene when the step ended. With the step at
  the end of the smoke, the plugins step passed 2 of 2. Cause unknown; not fixed.
- Stats step: "stats: sampler has CPU but the status bar does not show it" (pill stuck at
  `--% CPU` while the sampler has a reading), and "only 1 frames with show_stats on". Both
  look like paint stalling after the stats toggle, likely the same occlusion cause.

- State: native z-order root cause verified and fix pending merge. The preview, terminal
  mode and stats failures remain open; their causes have not been isolated.
- Fix direction for remaining failures: make their checks independent of window visibility
  or bring the window to the front before checking paint-dependent state.
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

### 8. Show plugin capabilities to the user

Manifests declare `capabilities` (PR #35), and the docs and decisions say the list is
shown at install and in a plugin info view. Nothing shows it today: unknown words only
warn on stderr and in the status pill count.

- State: not started. No install flow and no plugin info view exist.
- Blocker: needs a place to show it (an info item in the Add menu or the panel menu, or a
  Settings section).
- Origin: review of PR #35.

### 9. Fill `plugin_version` when panels are created

Layout v2 (PR #31) has `plugin_version` on plugin and TUI kinds, but panels are created
with `None`, so a saved layout would not say which plugin version made it.

- State: not started. Owned by persistence plan Task 4 (autosave), which first writes
  real layouts.
- Blocker: Task 4.
- Origin: review of PR #31.

### 10. Remaining plugin API v2 phases

Done: phase 1 (PR #35) and the pure phase 6 `Coalescer` (PR #34, not wired). Partly done
outside the phase plan: storage, `github.api` and `openExternal` from phase 7 (PR #23),
`contributes.menus` from phase 8 (PR #28). Open: 2 (request parsing), 3 (read-only host),
4 (control), 5 (close), 6 wiring (subscribe, host timer), the rest of 7 (bus,
`process.exec`), the rest of 8 (commands, key bindings, `calm.menus.register`), 9 (SDK,
docs, examples), 10 (Ghostty shim patch), 11 (external control channel), 12 (native
browser API), 13 (plugin RPC), 14 (plugin backend), 15 (background pages), 16 (layouts
and session, waits on the persistence plan).

- State: not started past the items above.
- Blocker: none for 2 to 5; 16 waits on persistence Tasks 4 to 7.
- Origin: `docs/superpowers/specs/2026-10-31-plugin-api-v2-design.md`, "Phased
  implementation".

## Done

- Smoke diagnostics for the stats flake (item 6): PR #37 splits sampler failure from the
  pill stall. The flake itself stays open.
- Per-panel override storage (item 2): PR #31.

## Waiting on a dependency

- Claude Code mods integration waits on plugin API v2 phase 11
  (`docs/superpowers/specs/2026-10-31-claude-code-mods-integration.md`).
- Prototype plan Task 14, `--bench` and `docs/benchmarks.md`, is not started.
- The Linux port stays a plan only, by user decision.
