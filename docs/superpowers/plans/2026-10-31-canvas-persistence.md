# Canvas persistence plan

Status: plan, not implemented. Written 2026-10-31 against `origin/main` at `024ce4c`. The
user answered the open questions on 2026-10-31; the answers are folded in below.

## Goal

The user: "Plan saving the whole canvas. It doesn't need to be the exact state, but when I
start the canvas the same plugins and elements load."

So CALM keeps one session file, restores it at launch, and keeps named layouts on the side.
Panels come back with the same kind, place, size, order and configuration. Processes do not
come back: terminals start fresh shells, browsers reload their url, plugins reload their
page with their saved settings.

## What this changes in the binding spec

The design spec says: "A fresh app launch after a computer restart starts with an empty
canvas; users can explicitly load a saved layout." Restoring the last session automatically
reverses that sentence. PR 1 below therefore lands a `decisions.md` entry (reversal, with
reason and cost) and amends that line of the spec to "a fresh launch restores the last
session's panels with fresh processes, unless restore is off". The decision "processes need
not survive app quit" stays as it is: only the arrangement is restored.

The user approved this amendment on 2026-10-31, with restore on by default (resolved question
1), so PR 1 records an approved change, not a proposal.

## What exists today

Verified by reading the code on `origin/main`:

- `calm_core::layout`: `LayoutFile { version: 1, camera, panels }`, `LayoutPanel { kind,
  rect, title, minimized }`, panels back to front. `save_layout_in` writes a temp file and
  renames it, without `fsync` and without setting permissions. `load_layout_from` checks the
  version before the shape and returns `NotFound`, `UnsupportedVersion`, `Malformed` (path
  and message) or `Io`. `validate_layout` checks zoom range and finite numbers. Names match
  `^[A-Za-z0-9_-]+$`. Path `$CALM_HOME/layouts` or `~/.config/calm/layouts`.
- `Scene::to_layout` and `Scene::from_layout` exist; ids are fresh on load.
- `PanelKind` (serde tag `type`, snake case): `Terminal { command, cwd }`,
  `Browser { url }`, `Plugin { plugin_id, settings }`. The browser url already follows the
  page (`canvas_view.rs`). Plugin `settings.get/set` read and write `PanelKind::Plugin.settings`.
- `Panel` has `id`, `kind`, `rect`, `title`, `minimized`. The title is one string: a default
  per kind, replaced by the page title for browsers and by `panel.setTitle` for plugins.
  There is no separate user title, icon or accent yet. Rename is not offered.
- `Camera { offset, zoom }` with zoom in `[0.1, 4.0]`.
- Settings: version 2, every section `#[serde(default)]`, a bad file is moved to
  `settings.json.invalid-<n>` and the Settings window shows a notice.
- The app has no `--layout` flag and never calls the layout functions. `main.rs` parses only
  `--smoke` and `--fullscreen`. The smoke already runs with a temporary settings directory.
- `gpui-pre 0.3.4` has an `on_app_quit` hook (found in `src/app.rs`); its exact behavior on
  cmd-q versus window close is unverified.
- Plugin manifests carry `id`, `name` and `version`.
- Plugin API v2 design: `calm.storage` is per plugin, not per panel, in
  `$CALM_HOME/plugin-data/<plugin-id>.json` (phase 7). Browser page zoom is phase 12.
  Terminal cwd from Ghostty `PWD` is phase 10. The design lists layout save and load as a
  non-goal that "leaves room", so a `layouts.*` namespace is new design work.

Open work and what it adds to the saved state:

- PR #12 (`feat/panel-join`, open): joins are computed from rects each frame
  (`JoinLayout::compute`). Nothing extra to save; restoring the rects restores the joins.
- `fix/panel-resize` (uncommitted in its worktree): `Panel.resize: ResizePolicy`, derived
  from the kind and the plugin manifest. Not saved; recomputed on restore. A restored rect
  goes through the same clamp, so a manifest that tightened its limits wins.
- `feat/tui-plugins`: no commits yet, so the TUI app panel kind has no shape in code. This
  plan assumes a new kind `tui_app { plugin_id, plugin_version, overrides }` and keeps the
  PR that adds it last, so it adopts whatever that branch defines.

## Format decision: extend layout v1 into v2, one format for both uses

The session file and a named layout are the same `LayoutFile`, version 2. One reader, one
validator, one migration path, and "Save session as layout" is a file copy. Rejected: a
separate session schema, which would duplicate validation and let the two drift.

Paths, both under `$CALM_HOME` or `~/.config/calm`:

- `session.json`: the autosaved last session.
- `session.json.1` to `session.json.<n>`: rotating backups, newest is `.1`.
- `session.json.invalid-<k>`: a rejected file moved aside (same helper as settings).
- `layouts/<name>.json`: named layouts, unchanged location.

Version 2 shape:

```json
{
  "version": 2,
  "saved_at_unix": 1793450000,
  "camera": { "offset": { "x": -40.0, "y": 12.5 }, "zoom": 0.75 },
  "panels": [
    {
      "kind": { "type": "terminal", "command": null, "cwd": "/Users/me/src/calm" },
      "rect": { "origin": { "x": 0, "y": 0 }, "size": { "width": 640, "height": 400 } },
      "title": "Terminal",
      "minimized": false,
      "overrides": { "title": null, "icon": null, "accent": null }
    },
    {
      "kind": { "type": "browser", "url": "https://docs.rs/gpui", "page_zoom": null },
      "rect": { "origin": { "x": 700, "y": 0 }, "size": { "width": 800, "height": 600 } },
      "title": "gpui - Rust",
      "minimized": false
    },
    {
      "kind": {
        "type": "plugin",
        "plugin_id": "github-prs",
        "plugin_version": "0.1.0",
        "settings": { "repos": ["kolezka/CALM"] }
      },
      "rect": { "origin": { "x": 0, "y": 500 }, "size": { "width": 420, "height": 300 } },
      "title": "Pull requests",
      "minimized": true
    }
  ]
}
```

Rules:

- `panels` order is the z-order, back to front, as in v1.
- `title` is the last shown title, used only until the panel sets its own. `overrides` holds
  what the user chose and always wins. Missing `overrides` means none.
- Panel ids are not saved. Ids are fresh after a restore, so a plugin that remembered a panel
  id across a quit gets `not_found`. Documented in `docs/plugins.md`.
- `migrate_v1`: a v1 file becomes v2 with `overrides` empty, `plugin_version` null and
  `page_zoom` null. Nothing else changes. The file on disk is rewritten as v2 only on the
  next save, and a v1 named layout is never rewritten unless the user saves it again.
- A version above 2 stays `UnsupportedVersion` and fails loud.
- A panel with an unknown `type` (a file written by a newer CALM, or a TUI panel read by a
  build without TUI support) loads as `PanelKind::Unsupported { raw }`. It keeps its rect and
  the raw JSON, draws as a placeholder, and saves back byte for byte. One unknown panel never
  rejects the whole file.

## What is saved per panel kind

| Kind | Saved | Not saved |
| --- | --- | --- |
| Terminal | rect, title, `cwd`, `command` if spawned with one | scrollback, process, env, shell history |
| Browser | url (after the query rule below), title, `page_zoom` once phase 12 exists | history, back and forward list, cookies, form state |
| Plugin | `plugin_id`, `plugin_version`, panel `settings`, rect | page state, `localStorage`; `calm.storage` stays in its own file |
| TUI app | `plugin_id`, `plugin_version`, user overrides (title, icon, accent) | the TUI process and its screen |
| All | rect, z-order, `minimized`, `overrides` | panel id, focus, hover, preview images |
| Global | camera offset and zoom | window size and fullscreen (out of scope, see below) |

Terminal cwd: until the Ghostty shim patch (plugin API v2 phase 10) delivers `PWD`, the saved
cwd is the spawn cwd, which is `None` for a terminal drawn on the canvas (the shell starts in
home). After phase 10 the app updates `PanelKind::Terminal.cwd` on each `PWD` action, so the
save carries the last known directory. The `command` is the one the panel was spawned with
(the API's `panels.create` with `command`), never a command typed into the shell.

Plugin storage: there is no per-panel storage reference to save, because `calm.storage` is
keyed by plugin id. The panel's `plugin_id` is the reference. If a later design adds
per-panel storage, it adds a `storage_key` field to the plugin kind in a v3.

## When it saves

- **Dirty signal.** `Scene` gets a `revision: u64` that every mutation bumps (add, remove,
  move, resize, z-order, minimize, title, kind change such as a browser url or plugin
  settings write) plus the camera. The app compares it with the last saved revision.
- **Debounce.** A pure `AutosaveClock` in calm-core with injected time: save when the scene
  has been quiet for `debounce_ms` (default 1000) or has been dirty for 10 s without a save
  (so a long continuous pan still saves). Tested like the API v2 `Coalescer`.
- **Off the UI thread.** The app snapshots `scene.to_layout()` on the UI thread (cheap), then
  writes on the background executor. One write in flight at a time; a newer snapshot replaces
  a queued one.
- **On quit.** `on_app_quit` writes synchronously with the latest snapshot. If the hook does
  not fire on some quit path (to check in PR 4), the last debounced save is at most about 1 s
  old.
- **Atomic write.** Temp file in the same directory, created with mode 0600, write,
  `fsync` the file, rename over the target, `fsync` the directory. The existing
  `save_layout_in` gains the `fsync` steps and the mode, so named layouts get them too.
- **Backups.** Rotation happens once per launch, at the first save, not on every autosave:
  `.1` becomes `.2` and so on up to the backup count, `session.json` is copied to `.1`, then
  the new file replaces `session.json` atomically. `session.json` is never moved, so a failed
  write still leaves it to restore. Rotating on every autosave would wipe all backups within seconds of a bad
  session. A launch whose restore failed never rotates the moved-aside file into the backups.
- **Empty start guard.** With restore off, `--no-restore` or a failed restore, the old
  `session.json` is not overwritten until the scene has its first change, and that first save
  rotates it into `.1`. Starting CALM once with an empty canvas must never lose the last
  real session.
- **Status.** A save failure shows in the status bar pill (`⚠ session not saved: <reason>`)
  and on stderr, and the next change retries. No silent catch.

## When and how it restores

At launch, after settings load and plugin discovery, before the window shows panels:

1. Pick the source: `--layout <name>` wins, then `--no-restore` (empty), then the
   `session.restore_on_launch` setting (`on`, `off`, `ask`). `ask` shows a native NSAlert
   ("Restore last session? N panels" with Restore and Start empty) before any native panel is
   created, so no GPUI drawing overlaps a live view.
2. Load with the strict reader. Results:
   - `NotFound`: empty canvas, no notice. A first run is normal.
   - `Malformed` or `UnsupportedVersion`: move the file to `session.json.invalid-<k>`, start
     empty, show a notice in the pill and on stderr naming the moved file. Never start empty
     while the bad file is still in place where the next autosave would overwrite it. For a
     named layout given with `--layout`, print the error and exit 2 (the Task 13 contract),
     without moving the file, since the user asked for it by name.
   - `Io` (permissions, disk): start empty, notice, and disable autosave for this launch, so
     an unreadable file is not replaced blindly.
3. Build the scene with `Scene::from_layout` (fresh ids, rect clamp), then create panels back
   to front:
   - Terminal: spawn a fresh shell in `cwd`, with `command` if set. If `cwd` no longer exists
     or is not a directory, spawn in home and write a line in the terminal's title area
     ("cwd /x missing, started in ~") plus one status bar notice for the whole restore.
   - Browser: create the view and load the saved url. A url that fails
     `browser_url::normalize_address` checks loads `about:blank` with a notice.
   - Plugin: if the plugin id is installed, load its entry and serve the saved `settings`.
     A `plugin_version` that differs from the manifest only logs a line; the plugin owns
     migration of its settings. If the id is not installed, the panel becomes a placeholder.
   - TUI app: same rule as plugins.
   - Unsupported kind: placeholder.
4. Camera last, so placement math runs once with the saved zoom.

Placeholders. A placeholder is a GPUI-drawn panel with no native view, so it can never cover a
live native rect. It shows the kind icon, the saved title and "Plugin github-prs is not
installed" (or "Unknown panel type x, saved by a newer CALM"). It keeps its `PanelKind`
unchanged, so every save writes the saved data back and a later install restores it at the
next launch. Its panel menu offers Delete only. No runtime re-discovery is planned; that
belongs to the plugin manager, not here.

A restore of 20 terminals spawns 20 shells at once. That cost is accepted; if it turns out
slow, minimized and offscreen terminals can spawn lazily on first placement, which keeps the
"process runs while the app is open" rule because they never ran before.

## Customization

Settings section `session` in `settings.json`, added with `#[serde(default)]`, so v2 files
keep loading and no settings version bump is needed. `Settings::validate` checks the ranges.

| Setting | Values | Default |
| --- | --- | --- |
| `session.restore_on_launch` | `on`, `off`, `ask` | `on` |
| `session.autosave` | bool | `true` |
| `session.debounce_ms` | 250 to 10000 | 1000 |
| `session.backup_count` | 0 to 20 | 3 |
| `session.browser_query` | `strip_all`, `keep` | `strip_all` |

The debounce key is `debounce_ms`, not `autosave_debounce_ms`: it already sits in the
`session` section, and validation rejects unknown keys.

The Settings window gets a Session section with these controls (data in `settings_form`).

Menus: a standard macOS File menu, placed right after the app menu and before View (GPUI
menu, items fire actions). It replaces the Layouts menu an earlier draft planned.

| Item | Key | Behavior |
| --- | --- | --- |
| New Canvas | `cmd-n` | Replaces the canvas with an empty one and forgets the current layout name. |
| Open Layout... | `cmd-o` | Native NSOpenPanel, starting in `layouts/`, `.json` only. Replaces the canvas. |
| Open Recent | none | Submenu of the last 10 layouts opened or saved, plus Clear Menu. Replaces the canvas. |
| Save Layout | `cmd-s` | Saves to the current named layout, else acts as Save Layout As. |
| Save Layout As... | `cmd-shift-s` | Native NSAlert with a name field. |
| Restore Previous Session | none | Submenu of `session.json.1` to `.n` with their `saved_at`. Replaces the canvas. |
| Reveal Layouts in Finder | none | Opens `layouts/` in Finder, creating it (0700) if missing. |

Rules for these items:

- **Replace, with one confirmation (resolved question 2).** Every item that replaces the
  canvas (New Canvas, Open Layout, Open Recent, Restore Previous Session) asks once, with a
  native alert, when live terminals exist or the canvas has changes not saved to a named
  layout (scene revision differs from the revision at the last named open or save; autosave
  to `session.json` does not count, and an empty canvas never asks). The alert lists how many
  live terminals would close. Only an explicit confirm closes them (the close rule); Cancel
  leaves the canvas as it was. Open never merges panels into the current canvas. The lead
  brief named only live terminals for New Canvas; one shared rule for all four is simpler and
  asks in a superset of those cases.
- **Open Layout uses NSOpenPanel, not a submenu.** It is the standard macOS Open... and Open
  Recent already gives one-click access, so a second submenu of the same files would be a
  duplicate. A file picked outside `layouts/` loads with the same strict reader; Save Layout
  then acts as Save Layout As, so CALM never writes outside `layouts/` by name.
- **Save Layout As** validates the name against the name rule; an existing name asks before
  replacing. Save flashes `Saved <name>` in the pill (Task 13).
- **Open Recent** lives in `$CALM_HOME/recent-layouts.json` (paths only, 0600). A missing file
  is dropped from the list when the menu is built.
- After any open, autosave keeps writing `session.json`; the named file changes only on Save.
- No Delete item: Reveal Layouts in Finder covers it, and Finder has its own Trash and undo.
  `layouts.delete` in the plugin API still moves to `layouts/.trash/`.

Key bindings checked on `origin/main` (`rg 'KeyBinding::new\('` over `crates/`): the existing
bindings are `cmd-shift-f`, `cmd-q`, `cmd-,`, `cmd-=`, `cmd--`, `cmd-0`, `cmd-shift-h`,
`cmd-shift-0`, `cmd-ctrl-f`, and the unmodified `t`, `b`, `escape`, `enter`. `cmd-n`, `cmd-o`,
`cmd-s` and `cmd-shift-s` are free, so there is no conflict. One thing to check in PR 7: a
focused terminal sends every key to Ghostty and stops propagation (`terminal/adapter.rs`
`key_down`). The new bindings are global like `cmd-q` and `cmd-,`, and the menu key
equivalents should fire before the view sees the key, but that is unverified with a terminal
focused.

CLI:

- `--layout <name>`: restore that named layout instead of the session. Missing file: empty
  canvas with the name remembered for `cmd-s`. Malformed: print the error, exit 2.
- `--no-restore`: start empty for this launch, with the empty start guard above.
- `--smoke` never reads or writes the real session or layouts; it already uses a temp dir.

Plugin API v2: a `layouts` namespace is not in the v2 design, so PR 8 first amends the design,
then implements:

| Method | Result | External socket default |
| --- | --- | --- |
| `layouts.list()` | names with `saved_at` and panel count | allowed (read-only) |
| `layouts.save(name)` | saves the current canvas as `name` | refused by default, `layout` group |
| `layouts.open(name)` | replaces the canvas; live panels close only through the user's confirmation, like `panels.close`; resolves after the confirm or rejects with `cancelled` | refused by default, `layout` group |
| `layouts.delete(name)` | moves to `.trash` | refused by default |
| `session.saveNow()` | forces an autosave | allowed |

Plugins are trusted, so these are not gated for installed plugins (capabilities are
declarative only), but `layouts.open` keeps the confirmation, the same guard the API has for
closing a live terminal. Events: `layoutSaved`, `layoutOpened`.

## Security and privacy

- No credentials in layout or session files. CALM writes none. Two leaks remain possible and
  are documented in `docs/plugins.md` and the new `docs/sessions.md`: plugin panel settings
  (a plugin could put a secret there; plugin authors must use `calm.storage` or the keychain
  for that, never panel settings), and browser urls.
- Browser urls (resolved question 3). Query strings and fragments can carry tokens (magic
  links, OAuth `code` and `state`, presigned `X-Amz-Signature`), so saving urls verbatim would
  break the "no credentials in layouts" rule. The user asked for the lowest effort; the
  lowest-effort choice that keeps that rule is to drop the whole query string and fragment.
  Decision: `session.browser_query` defaults to `strip_all`, one function with no name list
  to maintain. `keep` saves the url as is, for users who opt in; the docs say that a kept url
  can carry a token into the file. Cost: search results and pages that keep state in the
  query or fragment reopen without it. Rejected: a `strip_sensitive` name list, which is best
  effort, needs upkeep, and still misses tokens under unlisted names.
- Urls longer than 8 KiB (large `data:` pages) are saved as `about:blank` with the title kept.
- Terminal `command` is saved because the user or a plugin chose it at spawn. The docs warn
  not to pass secrets in a spawn command; they would be in the file.
- Files are created 0600 and the directory 0700 if CALM creates it. A file found with wider
  permissions is read anyway and tightened on the next save, with a stderr line. Rotated backups
  are set to 0600 and `.1` is written 0600; `.invalid-*` files keep their mode because they
  are renamed.
- No session contents in logs. Errors name the path and the field, never values.

## Testing strategy

calm-core, all pure, with temp dirs:

- Round trip per kind: terminal with and without `command` and `cwd`; browser with url and
  `page_zoom`; plugin with nested settings and version; TUI app with overrides; unsupported
  kind kept byte for byte through load and save. Assert kind, rect, title, minimized,
  overrides and order equal.
- Migration: a checked-in v1 fixture (the current `sample_scene` output) loads as v2 with
  the expected defaults; a v1 file is not rewritten by a load.
- Malformed, as separate cases next to a valid positive control from the same fixture: bad
  JSON, missing version, version 3, zoom out of range, a non-object panel, an unknown field
  type. Each must return the specific error, never an empty layout. Missing file returns
  `NotFound`, which is a different case from malformed and has its own test.
- Restore decision as a pure function: `(source flags, setting, load result) -> RestorePlan`
  with a test for every row, including "malformed moves aside and does not rotate" and the
  empty start guard.
- Atomic write: temp file gone after success and after a forced rename failure; mode 0600;
  backup rotation for counts 0, 1 and 3 across two simulated launches.
- `AutosaveClock`: quiet period, max delay, no save without a revision change.
- Url scrubbing: a table of urls in and out for both modes: no query, query only, fragment
  only, both, a presigned S3 url, a fragment token, `about:blank` and a `data:` url. In
  `strip_all` the scheme, host, port and path stay unchanged.
- File menu logic in core: the replace confirmation predicate (live terminals, unsaved
  changes, empty canvas never asks), the Open Recent list (cap 10, dedupe, missing files
  dropped), and "Save acts as Save As" for no name or a file outside `layouts/`.
- Each regression or rule test is checked to fail when the rule is removed (one mutation per
  rule), per the conventions.

calm-plugins: manifest version comparison helper, `layouts.*` request parsing (PR 8).

Smoke (PR 5): in the smoke temp dir, build a scene with a terminal (cwd set to the temp dir),
a browser on the existing `data:` fixture, a fixture plugin with settings and a plugin id that
is not installed; save the session; close every panel through the normal path and drop the
scene in process; restore from the file; print

```
smoke: session saved=4 restored=4 kinds=terminal,browser,plugin,placeholder order=match settings=match
smoke: session malformed moved=session.json.invalid-0 started_empty=true
```

and exit nonzero on any mismatch. The malformed line uses a copy of the saved file with one
byte corrupted, so the same fixture is the positive control. The terminal's shell is new,
which the smoke checks by its frame count starting at zero.

Manual GUI pass (the user's): quit with cmd-q and relaunch, kill the process with `kill -9`
during a drag and relaunch, every File menu item and its shortcut (also with a terminal
focused), the ask dialog.

## Tasks

Small PRs, in order. Each runs `cargo test -p <crate>` while iterating and `gmake ci` plus
the smoke before its push.

1. **docs: session restore decision.** Files: `.ai/decisions.md`, the spec line, `.ai/status.md`.
   Accept: the reversal entry with reason and cost, marked approved by the user on
   2026-10-31; the spec sentence amended; the three resolved answers below recorded in
   `decisions.md` (restore on by default, Open replaces, urls saved with `strip_all`).
2. **feat(core): layout v2 with migration and unsupported kinds.** Files:
   `calm-core/src/layout.rs`, `scene.rs` (`overrides`, `plugin_version`, `page_zoom`,
   `Unsupported`), fixtures under `calm-core/tests/fixtures/`. Tests: round trip per kind,
   v1 migration, malformed set with positive control, unsupported kind byte for byte.
   Accept: all existing layout tests still pass unchanged except the key-set test, updated
   for `overrides`. Depends on `fix/panel-resize` landing first or rebasing onto it, since
   both touch `Panel`.
3. **feat(core): durable writes, session paths, backups.** Files: `layout.rs` (fsync, 0600,
   `session_path`, `rotate_backups`, `move_aside` shared with settings), `settings.rs`
   (the `session` section and validation), `settings_form.rs` (Session section). Tests:
   atomic write, mode, rotation, settings ranges. Accept: settings v2 files without
   `session` load with defaults.
4. **feat(core,app): autosave.** Files: `calm-core/src/autosave.rs` (`AutosaveClock`),
   `scene.rs` (`revision`), `canvas_view.rs` (dirty check, background write, quit hook,
   pill error). Tests: clock, revision bump per mutation kind. Accept: a manual drag writes
   `session.json` about 1 s later; the `on_app_quit` behavior is checked and written in
   `.ai/status.md` as verified or not.
5. **feat(app): restore at launch with placeholders.** Files: `main.rs` (`--layout`,
   `--no-restore`, restore plan), `canvas_view.rs` (create panels from a layout, placeholder
   paint), `calm-core/src/restore.rs` (pure `RestorePlan`), smoke. Tests: restore plan table,
   smoke lines above. Accept: Task 13's Step 2 checks pass; a missing plugin shows its
   placeholder and comes back after install and relaunch. Depends on PR #12 only for
   rebasing `canvas_view.rs`.
6. **feat(core,app): browser url scrubbing.** Files: `calm-core/src/browser_url.rs`
   (`scrub_for_save`, two modes: `strip_all` drops query and fragment, `keep` returns the
   input), the save path. Tests: the url table. Accept: the default `strip_all` saves no
   query or fragment; `keep` round trips; docs page written (`docs/sessions.md`).
7. **feat(app): File menu and Settings controls.** Files: a new `file_menu.rs` (items and
   `cmd-n`, `cmd-o`, `cmd-s`, `cmd-shift-s` bindings), `settings_menu.rs` (`menus()` inserts
   File after the app menu), `calm-native` (NSOpenPanel, NSAlert with text field,
   confirmation alert, reveal in Finder), `calm-core` (replace predicate, recent list).
   Tests: name validation, the replace predicate, the recent list. Accept: New Canvas, Open
   Layout..., Open Recent, Save Layout, Save Layout As..., Restore Previous Session, Reveal
   Layouts in Finder and `ask` on launch work; the shortcuts fire with a terminal focused (or
   the gap is written in `.ai/status.md`); manual pass by the user.
8. **feat(plugins): `layouts.*` and `session.saveNow`.** Files: the plugin API v2 design
   (amendment first), `calm-plugins` bridge and request parsing, `calm.d.ts`, `docs/plugins.md`,
   the external socket allowlist. Depends on API v2 phases 1, 2, 4 and 5 (`layouts.open`
   reuses the close confirmation), and phase 11 for the socket rows.
9. **feat: terminal cwd from Ghostty PWD.** Files: the terminal adapter, `canvas_view.rs`.
   Depends on API v2 phase 10 (shim patch). Accept: `cd /tmp`, quit, relaunch, the shell
   starts in `/tmp`.
10. **feat: browser page zoom saved.** Depends on API v2 phase 12. Accept: zoom survives a
    relaunch.
11. **feat: TUI app panels in the session.** Depends on `feat/tui-plugins`. Adopts that
    branch's kind shape, maps it to `tui_app` with overrides, and turns the unsupported
    placeholder test into a real round trip.
12. **feat: user overrides in chrome.** Rename, icon and accent through the panel menu,
    written to `overrides`. Needs a native text input (the reason Rename is not offered
    today). Can run after PR 2 in parallel with 4 to 8.

Out of scope: window size, position and fullscreen state; per-panel browser data stores;
terminal scrollback; runtime plugin re-discovery.

## Resolved 2026-10-31

1. Restore at launch: on by default. The user's goal is "when I start the canvas the same
   plugins and elements load", so `session.restore_on_launch` defaults to `on` and the spec
   amendment is approved. The user also asked for a menu bar File menu to save and load
   layouts; it replaces the planned Layouts menu (see Customization).
2. Opening a named layout replaces the canvas, after one confirmation when live terminals or
   unsaved changes exist. It never merges.
3. Browser urls: the user asked for the lowest effort. Keeping urls verbatim would break the
   "no credentials in layouts" rule, so the default is `strip_all` (drop query and fragment),
   with `keep` as an opt-in. `strip_sensitive` and its name list are dropped.
