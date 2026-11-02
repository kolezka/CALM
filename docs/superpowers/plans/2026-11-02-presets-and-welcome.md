# Presets and Welcome Screen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A first launch opens a built-in `welcome` preset; users can save and load presets; a Developer item replays the welcome preset.

**Architecture:** A preset is a `LayoutFile` (layout v2). A new `calm_core::preset` module finds and lists presets across the user `layouts/` dir and built-in `presets/` dirs (user shadows built-in). `plan_restore` gains a first-launch branch. The app adds a File menu (Save Preset…, Load Preset…) and a Developer item, with native NSAlert dialogs added to `calm_native::prompt` (from PR #63). Two first-party web plugins, `calm-welcome` and `calm-keys`, are hidden from the Add menu by a new `addMenu` manifest field.

**Tech Stack:** Rust (calm-core, calm-plugins, calm-native via objc2, calm-app on GPUI), plain HTML/CSS/JS plugins.

**Spec:** `docs/superpowers/specs/2026-11-02-presets-and-welcome-design.md`

**Base:** `main` at `d21a7c9` (PR #63 `feat: TUI panels extend terminals, add Run Command`, merged 2026-11-02; it also carries #60's shortcuts). Line numbers below were taken at #63's head `ba4834a` and can be off by a few lines; find each spot by the symbol named next to it. What this plan uses from #63:

- `PanelKind::Tui` is gone. A TUI app panel is `PanelKind::Terminal { app: Some(TerminalApp { plugin_id, .. }), .. }`; build one with `PanelKind::app_terminal(id, version)`.
- `crates/calm-native/src/prompt.rs` has `prompt_text(title, message, note, placeholder) -> Result<Option<String>, NativeError>` (Run and Cancel buttons) and `prompt_value`. Task 5 extends this file; it does not add a second prompt.
- Modal prompts run in `cx.spawn_in(window, async move |this, cx| ..)` outside any GPUI update, then `this.update_in(..)`; see `prompt_run_command` (`crates/calm-app/src/canvas_view.rs:977`). Task 6 follows that pattern.
- The Add menu has a "Run Command…" item (`menu::RUN_COMMAND`). `addMenu` does not touch it.

## Global Constraints

- Preset names follow the layout rule `^[A-Za-z0-9_-]+$` (`layout_path`, `crates/calm-core/src/layout.rs:525`).
- Default preset name: `welcome`. Setting: `session.first_launch_preset`, string, default `"welcome"`, `""` = empty first launch.
- Website URL in the preset: `https://calm.raqz.link`.
- Closing line in the welcome panel, exact: `Open Source for life - Long Live Tech`.
- Logo: copy of `assets/brand/logo-animated-transparent.svg`.
- No music, no audio element, no network fonts.
- Plugin ids: `calm-welcome`, `calm-keys`. Both manifests carry `"addMenu": false`.
- Native views draw above GPUI: every prompt is a native NSAlert.
- A process ends only after one explicit confirmation; cancel changes nothing.
- No credentials in presets; saves go through `write_layout_file` (URL scrub).
- Do not call plugins sandboxed. No long dashes in new prose or comments.
- Each crate's unsafe stays in calm-native with a SAFETY comment.
- While iterating run `cargo test -p <crate>`; the full gate (`cargo clean -p`, workspace check, `gmake ci`, smoke) runs once, in Task 9.

## Review Focus

1. **`session.json` present with zero panels.** Expect restore of the empty canvas, never the preset. Test in Task 3 (`plan_restore_keeps_an_empty_session`).
2. **User preset named `welcome`.** Expect it to shadow the built-in on first launch, Load Preset and `--layout welcome`. Test in Task 1 (`user_preset_shadows_builtin`).
3. **Built-in preset file malformed or missing** (bad bundle, built-ins off). Expect an empty first launch with one notice, never an exit. Test in Task 3 (`first_launch_with_unusable_preset_starts_empty_with_notice`).
4. **Load Preset cancelled at the confirmation or at the list.** Expect the scene unchanged and no process ended. Test in Task 6 (`load_preset_cancel_changes_nothing` on the pure decision fn).
5. **Save Preset with an invalid name** (`my preset`, `../x`, empty). Expect a native error and no file written. Test in Task 6 (`preset_name_rejects_unsafe_names`).

---

### Task 1: Preset lookup and the first-launch setting (calm-core)

**Files:**
- Create: `crates/calm-core/src/preset.rs`
- Modify: `crates/calm-core/src/lib.rs` (add `pub mod preset;`)
- Modify: `crates/calm-core/src/layout.rs:525` (expose the name rule)
- Modify: `crates/calm-core/src/settings.rs:120-160` (field, default, validate)
- Modify: `crates/calm-core/src/settings_form.rs:306-345` and `:731-765` (Session field and apply)

**Interfaces:**
- Produces:
  - `pub fn valid_layout_name(name: &str) -> bool` in `layout.rs`; `layout_path` uses it.
  - `pub enum PresetOrigin { User, Builtin }` (Debug, Clone, Copy, PartialEq, Eq).
  - `pub struct PresetEntry { pub name: String, pub origin: PresetOrigin }`.
  - `pub fn find_preset(name: &str, user_dir: &Path, builtin_dirs: &[PathBuf]) -> Result<(LayoutFile, PresetOrigin), LayoutError>`: user dir first, then each built-in dir in order; `NotFound` carries the user-dir path; a malformed user file is an error, it does not fall through.
  - `pub fn list_presets(user_dir: &Path, builtin_dirs: &[PathBuf]) -> Vec<PresetEntry>`: user presets sorted by name, then built-ins not shadowed, sorted; skips files whose stem fails `valid_layout_name`; unreadable dirs give no entries.
  - `SessionSettings.first_launch_preset: String`, default `"welcome"`.

- [ ] **Step 1: Write the failing tests** in `preset.rs` `mod tests` (temp dirs via `tempfile`, as `layout.rs` tests do):
  - `user_preset_shadows_builtin`: `welcome.json` in both dirs with different panel counts; `find_preset("welcome", ..)` returns the user one and `PresetOrigin::User`; `list_presets` lists `welcome` once, origin `User`.
  - `builtin_found_when_user_missing`: origin `Builtin`.
  - `missing_everywhere_is_not_found`: `matches!(err, LayoutError::NotFound(_))`.
  - `malformed_user_preset_does_not_fall_back`: user `welcome.json` = `"{"`, built-in valid; expect `LayoutError::Malformed { .. }`.
  - `list_skips_unsafe_names_and_sorts`: files `b.json`, `a.json`, `bad name.json`, `notes.txt`; expect `["a", "b"]`.
  - In `settings.rs` tests: `first_launch_preset_defaults_to_welcome`; `first_launch_preset_rejects_unsafe_name` (`"../x"` fails validation, error text contains `session.first_launch_preset`); `""` is valid.
  - In `settings_form.rs` tests: the Session section has field id `session.first_launch_preset` with `Control::Text { value: "welcome", .. }`; applying `FieldValue::Text("demo")` sets it; applying `"bad name"` leaves the old value.

- [ ] **Step 2: Run** `cargo test -p calm-core preset settings` and confirm the new tests fail (missing module or field), not something unrelated.

- [ ] **Step 3: Implement** the interfaces above. Read files with the existing `load_layout_from`. Form label: `"First launch preset"`, placeholder `"empty canvas"`.

- [ ] **Step 4: Run** `cargo test -p calm-core` → all pass.

- [ ] **Step 5: Commit** `feat(core): find and list presets, add session.first_launch_preset`

### Task 2: Built-in preset dirs and bundling (calm-plugins, scripts)

**Files:**
- Modify: `crates/calm-plugins/src/manifest.rs:590-660`
- Modify: `scripts/bundle-macos.sh:33-45`
- Create: `presets/.gitkeep` (removed in Task 8 when `welcome.json` lands)

**Interfaces:**
- Produces: `pub fn builtin_preset_dirs() -> Vec<PathBuf>` in `calm_plugins::manifest` (re-export from the crate root like `plugin_search`). Dev build: workspace `presets/` (`CARGO_MANIFEST_DIR/../../presets`); bundle: `Contents/Resources/presets`. Not gated by `plugins.builtin` or `CALM_NO_BUILTIN`: presets are data.

- [ ] **Step 1: Write the failing test** `builtin_preset_dirs_cover_dev_build_and_bundle`, mirroring `builtin_dirs_cover_dev_build_target_dir_and_bundle` (`manifest.rs:1508`), against the generalized private fn.
- [ ] **Step 2: Run** `cargo test -p calm-plugins builtin_preset` → FAIL.
- [ ] **Step 3: Implement** by generalizing `builtin_dirs_from(debug_build, exe, workspace_dir, resource: &str)` so plugins pass `"plugins"` and presets pass `"presets"`. Existing plugin tests stay unchanged and green. In `bundle-macos.sh`, copy `presets/*.json` into `Contents/Resources/presets/` (create the dir even when empty).
- [ ] **Step 4: Run** `cargo test -p calm-plugins` → pass; `bash -n scripts/bundle-macos.sh` → rc 0.
- [ ] **Step 5: Commit** `feat(plugins): find built-in presets in the workspace and the bundle`

### Task 3: First launch opens the preset (calm-core restore, calm-app launch)

**Files:**
- Modify: `crates/calm-core/src/restore.rs:60-160`
- Modify: `crates/calm-app/src/main.rs:77-98` (`launch_plan`), both callers (`rg -n 'launch_plan\(' crates/calm-app/src/main.rs`), `smoke_launch` (`:2897`) and the tests at `:3188+`
- Modify: `crates/calm-app/src/canvas_view.rs:347-375` (`apply_restore_plan`), `:511` (`start_autosave`), `:679` (`save_on_quit`)

**Interfaces:**
- Consumes: `find_preset`, `builtin_preset_dirs`, `SessionSettings.first_launch_preset`.
- Produces:
  - `pub struct FirstLaunch { pub name: String, pub read: Result<LayoutFile, LayoutError> }` in `restore.rs`.
  - `plan_restore(source, notice, session, layout, first_launch: Option<FirstLaunch>) -> Result<RestorePlan, LaunchExit>`.
  - `RestorePlan.save_at_start: bool`: true only when a first-launch preset was applied.
  - `launch_plan(flags, setting, first_launch_preset: &str, session, layouts, builtin_presets: &[PathBuf])`.

Rules (spec table, "Launch behavior"): `first_launch` is read and used only when the source is `Session` and the session read is `NotFound`. Session `Ok` (any panel count), `Malformed`, `UnsupportedVersion` or `Io` keep today's behavior. The preset's `layout_name` stays `None` (it becomes the session, not a named layout). An unusable preset (any error) is an empty start with the notice `first launch preset NAME: <error>`. `--layout NAME` reads through `find_preset`, so built-ins count; its error handling is unchanged.

- [ ] **Step 1: Write the failing tests** in `restore.rs`:
  - `first_launch_opens_the_preset_when_no_session`: session `Err(NotFound)`, `FirstLaunch` with a 3-panel layout; plan has that layout, `layout_name == None`, `save_at_start == true`, no notices.
  - `plan_restore_keeps_an_empty_session`: session `Ok` with 0 panels plus a `FirstLaunch`; plan layout has 0 panels, `save_at_start == false`.
  - `malformed_session_never_opens_the_preset`: session `Malformed`; quarantine set, layout `None`, `save_at_start == false`.
  - `first_launch_with_unusable_preset_starts_empty_with_notice`: `FirstLaunch.read = Err(NotFound)` and, separately, `Err(Malformed)`; layout `None`, one notice containing `first launch preset welcome`, `Ok(_)` (no exit).
  - `no_restore_and_off_ignore_the_preset`: `Source::Empty`; layout `None`.
  - In `main.rs` tests: `launch_plan_opens_builtin_preset_on_first_launch` (temp CALM dirs, built-in dir holding `welcome.json`, no session file) and `launch_plan_layout_flag_finds_builtin_preset` (`--layout welcome`, only the built-in exists).
- [ ] **Step 2: Run** `cargo test -p calm-core restore` and `cargo test -p calm-app launch_plan` → the new tests fail at their assertions (after a stub signature change compiles).
- [ ] **Step 3: Implement.** In `apply_restore_plan`, when `save_at_start` and autosave started, submit one session write right away through the same writer as `save_on_quit` (factor the body of `save_on_quit` into `fn write_now(&mut self, force: bool)`; `save_on_quit` calls it with `false`, first launch with `true`). With autosave off, write nothing (accepted edge case in the spec).
- [ ] **Step 4: Run** `cargo test -p calm-core` and `cargo test -p calm-app` → pass.
- [ ] **Step 5: Add a smoke line.** In `smoke_launch` (`main.rs:2897`), add a run on an empty temp dir with a built-in dir containing a 1-panel test preset; assert the scene has 1 panel and `session.json` exists right after. Print `smoke: first_launch source=preset panels=1 saved=true`. Run `cargo run -p calm-app -- --smoke; echo rc=$?` → rc=0 and the line present.
- [ ] **Step 6: Commit** `feat: open the first launch preset when no session exists`

### Task 4: `addMenu` manifest field

**Files:**
- Modify: `crates/calm-plugins/src/manifest.rs:39-60` (field), `:97-104` and `:120+` (`ApiFields`, `parse_api_fields`)
- Modify: `crates/calm-plugins/src/tui.rs:47` (`FIELDS` gets `"addMenu"`)
- Modify: `crates/calm-app/src/plugin_host.rs:104-118` (`menu_entries`)
- Modify: `docs/plugins.md`, `packages/calm-plugin-sdk/README.md` (manifest field list)

**Interfaces:**
- Produces: `PluginManifest.add_menu: bool` (default `true`). `menu_entries` leaves a plugin out of `web` and `apps` when `add_menu` is false, and still lists its size in `sizes`.

- [ ] **Step 1: Write the failing tests:** in `manifest.rs`: `add_menu_defaults_to_true`, `add_menu_false_parses_for_web_and_tui`, `add_menu_rejects_non_bool` (`"addMenu": "no"` → error text contains `addMenu must be a boolean`). In `plugin_host.rs`: `menu_entries_skip_add_menu_false` (two plugins, one hidden; `web` has one, `sizes` has two).
- [ ] **Step 2: Run** `cargo test -p calm-plugins add_menu` and `cargo test -p calm-app menu_entries` → FAIL.
- [ ] **Step 3: Implement** in `parse_api_fields` so web and TUI share it. Docs: one table row, default `true`, "hides the plugin from Add; presets and sessions still open it".
- [ ] **Step 4: Run** both crates' tests → pass.
- [ ] **Step 5: Commit** `feat(plugins): addMenu hides a plugin from the Add menu`

### Task 5: Native dialogs (extend #63's `prompt.rs`)

**Files:**
- Modify: `crates/calm-native/src/prompt.rs` (from #63)
- Modify: `crates/calm-native/src/lib.rs:13` (re-exports)
- Modify: `crates/calm-app/src/canvas_view.rs:977-1000` (`prompt_run_command`, the one existing caller)

**Interfaces:**
- Consumes: `NativeError` (`crates/calm-native/src/lib.rs:36`), `prompt_value`.
- Produces (all `NSAlert runModal`; `Err(NativeError::NotMainThread)` off the main thread; call them outside any GPUI update):
  - `pub struct TextPrompt<'a> { pub title: &'a str, pub message: &'a str, pub note: &'a str, pub placeholder: &'a str, pub initial: &'a str, pub ok: &'a str }`.
  - `pub fn prompt_text(prompt: &TextPrompt) -> Result<Option<String>, NativeError>`: replaces #63's four-argument form; the first button is `prompt.ok`, then `Cancel`; the field starts with `initial`; answers go through `prompt_value`. Run Command passes `ok: "Run"`, `initial: ""` and keeps its behavior.
  - `pub fn confirm(title: &str, message: &str, ok: &str) -> Result<bool, NativeError>`: buttons `ok`, then `Cancel`; true on `ok`.
  - `pub fn choose(title: &str, message: &str, options: &[String]) -> Result<Option<usize>, NativeError>`: NSPopUpButton accessory; `None` on Cancel; `Ok(None)` without showing anything when `options` is empty.
  - `pub fn alert(title: &str, message: &str) -> Result<(), NativeError>`: one OK button.

- [ ] **Step 1: Write the failing tests** in `prompt.rs`: `choose_with_no_options_is_none`; `dialogs_refuse_off_the_main_thread` (from a spawned thread, each of `prompt_text`, `confirm`, `choose` with one option, `alert` returns `Err(NativeError::NotMainThread)`). Keep #63's `blank_answers_are_none_and_others_are_trimmed`.
- [ ] **Step 2: Run** `cargo test -p calm-native prompt` → the new tests fail.
- [ ] **Step 3: Implement**, then update `prompt_run_command` to the struct form.
- [ ] **Step 4: Run** `cargo test -p calm-native`, `cargo test -p calm-app` and `cargo check -p calm-native --examples` → pass. Showing a real modal is not part of smoke (agents never drive the GUI); Run Command's prompt is a by-eye check.
- [ ] **Step 5: Commit** `feat(native): confirm, choice and alert dialogs next to the text prompt`

### Task 6: Save Preset, Load Preset, Show Welcome Screen (calm-core + calm-app)

**Files:**
- Modify: `crates/calm-core/src/preset.rs` (pure decisions)
- Create: `crates/calm-app/src/preset_menu.rs` (actions and handlers)
- Modify: `crates/calm-app/src/dev_menu.rs:9-45` (File menu, Developer item; `menus` at `:23`)
- Modify: `crates/calm-app/src/canvas_view.rs` (close-all through `Effect::ClosePanel` at `:812`, apply preset)
- Modify: `crates/calm-app/src/main.rs` (install)

**Interfaces:**
- Consumes: `find_preset`, `list_presets`, `valid_layout_name`, `save_layout_in`, `calm_native::{prompt_text, TextPrompt, confirm, choose, alert}`, `builtin_preset_dirs`.
- Produces in `preset.rs`:
  - `pub enum ClosingKind { Terminal, App, Browser, Plugin }`.
  - `pub struct ClosingPanel { pub kind: ClosingKind, pub confirms: bool }`. `kind` is `App` for `PanelKind::Terminal { app: Some(_), .. }`, `Terminal` for `app: None`. `confirms` = what `ClosePanelRequested` computes (`canvas_view.rs:784`: a live terminal, with or without an app, or any browser); factor that into `fn close_confirms(&self, id: PanelId, cx: &App) -> bool` and use it in both places.
  - `pub fn load_confirmation(preset: &str, closing: &[ClosingPanel], confirm_close: bool) -> Option<String>`: `None` when `confirm_close` is false or no panel confirms; else `Load preset NAME? This closes ` + counts in the order terminals, apps, browsers, plugins, each `N terminal(s)`-style with correct singular, joined by `, ` and ` and `, ending `.` Example: `Load preset welcome? This closes 3 terminals and 1 browser.`
  - `pub fn decide_load(confirmation: Option<&str>, user_said_ok: bool) -> bool`: true when there is no confirmation or the user said OK.
  - `pub fn save_name(input: &str) -> Result<String, String>`: trims; error `Preset names use letters, digits, - and _.` when `!valid_layout_name`.
- In calm-app: actions `SavePreset`, `LoadPreset`, `ShowWelcome`; `CanvasView::apply_preset(&mut self, layout: LayoutFile, window, cx)` closes every panel through the `Effect::ClosePanel` path (no per-panel prompt), clears focus, then calls `restore_layout` and sets `layout_name = None`; the next autosave tick records it.
- Menus: new `File` menu after the app menu: `Save Preset…`, `Load Preset…`. Developer gets `Show Welcome Screen` after a separator. No key bindings.

Flows (each runs in `cx.spawn_in` like `prompt_run_command`, never inside an update):
- Save: `prompt_text(&TextPrompt { title: "Save Preset", message: "", note: "Letters, digits, - and _.", placeholder: "my-layout", initial: layout_name or "", ok: "Save" })` → `save_name` → if a user preset of that name exists, `confirm("Replace preset NAME?", .., "Replace")`; if only a built-in exists, the message says it will shadow the built-in → `save_layout_in(layouts_dir(), name, &scene.to_layout(), browser_query)`. Errors go to `alert`.
- Load: `list_presets` → `choose` (labels `name` or `name (built-in)`) → `find_preset` → `load_confirmation` → `confirm(.., "Load")` when `Some` → `apply_preset`.
- Show Welcome: as Load with name = `first_launch_preset`, or `welcome` when it is empty.

- [ ] **Step 1: Write the failing tests** in `preset.rs`: `load_confirmation_counts_and_plurals` (3 terminals + 1 browser → the example string exactly; 1 terminal, 1 app, 2 browsers, 1 plugin → `Load preset x? This closes 1 terminal, 1 app, 2 browsers and 1 plugin.`), `load_confirmation_none_when_nothing_confirms` (only plugins with `confirms: false`), `load_confirmation_none_when_confirm_close_off`, `load_preset_cancel_changes_nothing` (`decide_load(Some(..), false)` is false, `decide_load(None, false)` is true; and in the app, a false decision returns before any panel is closed), `preset_name_rejects_unsafe_names` (`""`, `"my preset"`, `"../x"` → Err; `" demo "` → `Ok("demo")`).
- [ ] **Step 2: Run** `cargo test -p calm-core preset` → FAIL.
- [ ] **Step 3: Implement** the pure fns, then the app wiring.
- [ ] **Step 4: Run** `cargo test -p calm-core` and `cargo test -p calm-app` → pass.
- [ ] **Step 5: Smoke.** Add a step `presets` to `smoke_runner::STEPS` that calls `apply_preset` with a 1-panel test layout over the smoke's terminals (bypassing dialogs) and asserts the scene then holds exactly that panel and the closed terminals' processes are gone (`is_alive() == false` on the old handles). Also assert the File menu and the Developer item exist in `menus(..)`. Print `smoke: presets applied=1 closed=N`. Run smoke → rc=0.
- [ ] **Step 6: Commit** `feat(app): Save Preset, Load Preset and Show Welcome Screen`

### Task 7: Key bindings data and the `calm-keys` plugin

**Files:**
- Create: `crates/calm-core/src/keymap.rs` (help table), `plugins/calm-keys/{calm-plugin.json,index.html,keys.json}`
- Modify: `crates/calm-core/src/lib.rs`, `crates/calm-app/src/canvas_view.rs:109-128` (test only)

**Interfaces:**
- Produces: `pub struct KeyHelp { pub keys: &'static str, pub action: &'static str, pub binding: Option<&'static str> }` and `pub const KEY_HELP: &[KeyHelp]`. `binding` is the GPUI keystroke text for real bindings (`"cmd-shift-h"`) and `None` for gestures (`"Space+drag"`, `"Pinch"`). Entries, in order, from `CLAUDE.md` "In the app": t then drag (terminal), b then drag (browser), right-click (menu), Cmd+drag on content (move panel), Space+drag or middle-drag (pan), Pinch or Cmd+scroll (zoom), cmd-= / cmd-- / cmd-0 (zoom in, out, reset), cmd-shift-h (center origin), cmd-shift-0 (fit all), cmd-shift-backspace (delete the focused panel), cmd-shift-, (the focused panel's Settings; registered as `cmd-<`, so `binding: Some("cmd-<")` with `keys: "cmd-shift-,"`), cmd-ctrl-f (fullscreen), cmd-shift-f (stats), alt while dragging (no snapping), cmd-, (Settings), cmd-q (quit). Escape and Enter are internal and not listed.
- `plugins/calm-keys/keys.json`: `[{"keys": "...", "action": "..."}]` generated from `KEY_HELP`.

- [ ] **Step 1: Write the failing tests:**
  - `calm-core`: `keys_json_matches_key_help`: serializes `KEY_HELP` to the `keys.json` shape and compares with the checked-in file (`include_str!("../../../plugins/calm-keys/keys.json")`); with `UPDATE_KEYS=1` it rewrites the file instead. Failure message names `UPDATE_KEYS=1 cargo test -p calm-core keys_json`.
  - `calm-app`: `key_help_covers_every_registered_binding`: every binding from `canvas_view::key_bindings()`, `dev_menu::key_bindings()` and `settings_menu::key_bindings()` whose context is not the terminal `NoAction` and whose keystroke is not `escape`/`enter` appears as some `KEY_HELP.binding`, and every `Some(binding)` in `KEY_HELP` is registered. Read keystroke text from `gpui::KeyBinding` (check the `gpui-pre` API for the accessor; format to the same `cmd-shift-h` form).
- [ ] **Step 2: Run** both → FAIL.
- [ ] **Step 3: Implement** `keymap.rs`, generate `keys.json`, write the plugin: manifest `id: "calm-keys"`, `name: "Key Bindings"`, `version: "0.1.0"`, `entry: "index.html"`, `defaultSize: {width: 360, height: 420}`, `addMenu: false`. `index.html` fetches `keys.json` and renders a two-column list, colors from `calm.theme.get()` with a dark fallback.
- [ ] **Step 4: Run** `cargo test -p calm-core keys_json` and `cargo test -p calm-app key_help` → pass. Mutation check: delete one `KeyBinding::new` from `canvas_view::key_bindings` locally, confirm `key_help_covers_every_registered_binding` fails at its assertion, restore, rerun green.
- [ ] **Step 5: Commit** `feat: key bindings plugin generated from the keymap`

### Task 8: `calm-welcome` plugin and the `welcome` preset

**Files:**
- Create: `plugins/calm-welcome/{calm-plugin.json,index.html,style.css,logo.svg,font/…}`
- Create: `presets/welcome.json`; delete `presets/.gitkeep`
- Modify: `crates/calm-app/src/main.rs` (smoke check)

**Interfaces:**
- Consumes: `addMenu`, `calm-keys`, `find_preset`, `builtin_preset_dirs`.
- Welcome manifest: `id: "calm-welcome"`, `name: "Welcome"`, `version: "0.1.0"`, `entry: "index.html"`, `defaultSize: {width: 560, height: 420}`, `addMenu: false`.
- Page: `logo.svg` (copy of `assets/brand/logo-animated-transparent.svg`), title `CALM`, instruction default `Press t and drag to draw a terminal. Right-click for more.`, closing line `Open Source for life - Long Live Tech`. Keygen look (dark background, bitmap-style font bundled under `font/` with its license file, glow or scroller); the visual details are the builder's. `calm.settings.get` keys `title`, `instruction`, `closing` override the three texts when set to non-empty strings.
- `presets/welcome.json` (version 2): camera `{"offset": {"x": -640, "y": -400}, "zoom": 1}`; panels back to front: browser `{"type": "browser", "url": "https://calm.raqz.link"}` at `{-620, -380, 560, 420}` titled `calm.raqz.link`; `calm-keys` at `{380, -380, 360, 420}`; `calm-welcome` at `{-40, -60, 560, 420}` in front. All rects within the 1280x800 viewport the camera shows. Plugin kinds as `{"type": "plugin", "plugin_id": ID, "plugin_version": "0.1.0", "settings": {}}`.

- [ ] **Step 1: Write the failing test** in `main.rs` tests: `builtin_welcome_preset_is_valid`: `find_preset("welcome", <empty dir>, &[workspace presets])` loads, has 3 panels, last is plugin `calm-welcome`, one is plugin `calm-keys`, one is browser `https://calm.raqz.link`, and each plugin id is found by built-in plugin discovery with `add_menu == false`.
- [ ] **Step 2: Run** `cargo test -p calm-app builtin_welcome` → FAIL.
- [ ] **Step 3: Implement** the plugin and the preset.
- [ ] **Step 4: Run** the test → pass; `gmake plugins-check` → rc 0; smoke → rc 0 and the Add menu in `smoke: plugin_menus` output lists neither plugin.
- [ ] **Step 5: Commit** `feat: welcome plugin and the built-in welcome preset`

### Task 9: Docs and status

**Files:**
- Modify: `CLAUDE.md` ("In the app", "Not wired yet": layout save is now wired through Save Preset), `docs/plugins.md` (first-party plugins list), `docs/architecture.md` (presets in the restore flow), `.ai/status.md`, `.ai/decisions.md` (mark the 2026-11-02 section built, with PR number).

- [ ] **Step 1:** Update each file; no claim beyond what Tasks 1 to 8 verified. By-eye items (dialogs, welcome look, first launch on a clean `CALM_HOME`) listed as unverified in `.ai/status.md`.
- [ ] **Step 2: Final gate:** `cargo clean -p calm-core -p calm-plugins -p calm-native -p calm-app`, `cargo check --workspace --tests --examples`, `gmake ci`, then smoke; each exit code 0, kept in the PR body.
- [ ] **Step 3: Commit** `docs: presets, welcome screen and layout save`
