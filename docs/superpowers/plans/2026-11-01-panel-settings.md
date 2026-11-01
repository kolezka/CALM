# Per-panel Settings, Borderless and Cmd+drag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Settings item in every panel's context menu opens a native per-panel window (title, icon, borderless; terminals also chrome and terminal colors) whose values override global defaults field by field; global Settings becomes tabbed; Cmd+drag on content moves any panel.

**Architecture:** All rules live in `calm-core` (override fields, resolution, form model, gesture decision) with unit tests. `calm-native` gains tabs, a text control, per-field inherited/reset state and a content-gesture event monitor. `calm-app` wires them: draws overrides, opens one panel Settings window per panel, routes the gesture into `InputHandler`.

**Tech Stack:** Rust 1.95, GPUI, objc2 AppKit bindings, serde.

**Spec:** `docs/superpowers/specs/2026-11-01-panel-settings-design.md`

## Global Constraints

- All `unsafe` stays in `calm-native`, each block with a `// SAFETY:` comment.
- No GPUI drawing over a live native rect. Settings windows are native NSWindows.
- A setting change never restarts or kills a terminal process.
- New override fields: `Option`, `skip_serializing_if = "Option::is_none"`, no `deny_unknown_fields` on `PanelOverrides` (decision "Panel overrides contract").
- Colors are `#rrggbb` or `#rrggbbaa`, parsed with `chrome::parse_hex_color`.
- Title max length `MAX_TITLE_CHARS`; icon text 1 to 4 chars (`MAX_ICON_CHARS = 4`, counted with `chars()`); icon files `.png` or `.svg`.
- Move modifier setting `canvas.move_modifier`, values `"cmd"` (default), `"ctrl"`, `"alt"`, `"cmd+ctrl"`, `"cmd+alt"`, `"ctrl+alt"`. No `"off"`. Unknown value is a validation error.
- Click vs drag threshold is `interaction::CLICK_SLOP` (4 screen points).
- Code comments: short and plain, no em or en dashes.
- While iterating: `cargo test -p <crate>`. Before a PR: `cargo clean -p <touched crates>`, `cargo check --workspace --tests --examples`, `gmake ci`, `cargo run -p calm-app -- --smoke; echo rc=$?` (must be 0, flakes are not passes).

## Review Focus

1. Borderless toggle on a live terminal: the content keeps its screen place and size, so the logical grid does not change. Pinned in Task 2 (`toggling_borderless_keeps_the_content_rect`).
2. Emoji icon: `"🚀"` is accepted, a 5-char string is rejected, empty text means "inherited", not "no icon". Pinned in Task 3.
3. Panel closed while its Settings window is open: the window closes; a late change for a gone id returns an error and changes nothing. Pinned in Task 3 (`apply_to_missing_panel_is_an_error`) and Task 9.
4. Reset to global on a terminal whose global has no custom colors: effective colors become `None`, and the window shows the "applies to new terminals" note because a live theme cannot be removed. Pinned in Task 3.
5. Cmd+press on empty canvas or on a minimized chip is not a content gesture: canvas behavior is unchanged. Pinned in Task 5.

---

## Lanes and PRs

Integration branch `feat/panel-settings` from `origin/main`. Each lane works in its own worktree under `.worktrees/` and opens a PR into `feat/panel-settings`. The lead merges lanes into it, then one PR from `feat/panel-settings` to `main` after cross-family review.

| Lane | Tasks | Worker | Starts |
| --- | --- | --- | --- |
| core | 1, 2, 3, 4, 5 | `kolezka-bootstrap:route-opus` (Task 4 alone: `route-deepseek-flash-edit`) | now |
| native | 6, 7 | `kolezka-bootstrap:route-opus` | after Task 3 lands (needs the form model) |
| app | 8, 9, 10 | `kolezka-bootstrap:route-opus` | after both lanes merge |
| review | whole branch | `kolezka-bootstrap:route-gpt-sol` | after Task 10 |

---

### Task 1: Override fields and terminal profile in the session

**Files:**
- Modify: `crates/calm-core/src/scene.rs` (`PanelOverrides`, `PanelKind::Terminal`, `KnownKind::Terminal`)
- Modify: `crates/calm-core/src/layout.rs` (tests only)
- Modify: every `PanelKind::Terminal { command, cwd }` pattern without `..` (5 sites, find with `rg -n "PanelKind::Terminal \{[^}]*\}" crates | rg -v "\.\."`)

**Interfaces:**
- Produces:
  - `PanelOverrides` gains `title_bar_color: Option<String>`, `title_color: Option<String>`, `terminal_colors: Option<TerminalColorsSetting>`, `borderless: Option<bool>` (existing `title`, `icon`, `accent_color` stay).
  - `PanelOverrides::validate(&self, kind: &PanelKind) -> Result<(), String>`: checks lengths, colors, icon form; terminal-only fields on a non-terminal kind are an error.
  - `PanelKind::Terminal { command, cwd, profile: Option<String> }` with `#[serde(default, skip_serializing_if = "Option::is_none")]` on `profile`.

- [ ] **Step 1: Write failing tests** in `scene.rs` / `layout.rs` test modules:
  - `new_override_fields_roundtrip_through_the_session`: set all seven fields on a terminal, save, load, `assert_eq!` on `overrides`.
  - `file_without_new_fields_loads_with_none`: existing v2 fixture text loads, all new fields `None`.
  - `unknown_override_field_is_ignored`: `"overrides": {"title": "x", "future": 1}` loads with `title == Some("x")`.
  - `terminal_profile_roundtrips_and_is_omitted_when_none`: JSON has `"profile": "neon"` when set and no `profile` key when `None`.
  - `validate_rejects_terminal_fields_on_a_browser` and `validate_rejects_bad_color_and_long_icon` (`"#12"`, `"abcde"`).
- [ ] **Step 2:** `cargo test -p calm-core` and confirm these fail to compile or fail at the assertion.
- [ ] **Step 3:** Add the fields, `validate`, and the `profile` field; fix the 5 pattern sites.
- [ ] **Step 4:** `cargo test -p calm-core` passes; `cargo check --workspace --tests --examples` passes.
- [ ] **Step 5:** Commit `feat(core): add per-panel override fields and terminal profile to the session`.

### Task 2: Resolution and borderless geometry

**Files:**
- Modify: `crates/calm-core/src/terminal_profile.rs`, `crates/calm-core/src/chrome.rs`, `crates/calm-core/src/scene.rs`, `crates/calm-core/src/interaction.rs`

**Interfaces:**
- Consumes: Task 1 `PanelOverrides`.
- Produces:
  - `TerminalSettings::resolve_for_panel(&self, profile: Option<&str>, user_config: bool, overrides: &PanelOverrides) -> ResolvedTerminal`: `resolve(...)` then each set override field replaces the resolved one. An override `title` replaces `title_template` and is taken literally (no `{n}`/`{cwd}` expansion).
  - `chrome::lay_overrides(base: ChromeOverrides, overrides: &PanelOverrides) -> ChromeOverrides`: for TUI, browser and plugin looks.
  - `scene::chrome_height(panel: &Panel) -> f64`: `0.0` when `overrides.borderless == Some(true)`, else `CHROME_HEIGHT`. `content_rect` and every `CHROME_HEIGHT` hit test in `interaction.rs` (lines ~209 and ~489) and `chrome.rs` use it.
  - `scene::set_borderless(panel: &mut Panel, on: bool)`: sets the field and moves `rect.origin.y` by `+CHROME_HEIGHT` and shrinks `rect.size.height` by `CHROME_HEIGHT` when turning on (reverse when off), so `content_rect` is unchanged.

- [ ] **Step 1: Write failing tests:**
  - `panel_override_wins_over_profile_and_defaults`: defaults title bar `#111111`, profile `#222222`, override `#333333` gives `0x333333ff`; with override `None` gives `0x222222ff`.
  - `override_title_is_literal`: override `"{n} logs"` gives `title_template == "{n} logs"` and the app must not expand it (assert a doc comment exists is not a test; instead assert `ResolvedTerminal.title_is_literal == true`, a new `bool` field).
  - `toggling_borderless_keeps_the_content_rect`: `content_rect` before on, after on, after off are equal.
  - `borderless_panel_has_no_chrome_hit_and_no_bar_menu`: `chrome_hit` is `None` and a right click at the old bar position returns no `ShowPanelMenu`.
- [ ] **Step 2:** Run, confirm failures at the assertions.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test -p calm-core` passes.
- [ ] **Step 5:** Commit `feat(core): resolve per-panel overrides and borderless geometry`.

### Task 3: Form model for per-panel Settings

**Files:**
- Modify: `crates/calm-core/src/settings_form.rs`
- Create: `crates/calm-core/src/panel_form.rs` (register in `lib.rs`)

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces:
  - `Control::Text { value: String, placeholder: String }`.
  - `FieldValue::Text(String)` and `FieldValue::Reset`. `FieldValue` loses `Copy` (keep `Clone`); fix call sites.
  - `Field` gains `state: FieldState` with `enum FieldState { Plain, Inherited, Set }`; the `field()` helper sets `Plain`, so the global form is unchanged.
  - `FormCommand::ChoosePanelIcon(PanelId)` for the file chooser.
  - `panel_form(panel: &Panel, effective: &PanelEffective) -> Vec<Section>`, where `PanelEffective { title: String, icon: Option<String>, chrome: ChromeOverrides, colors: Option<TerminalColors>, live_colors_note: bool }` is what the panel shows now. Field ids: `panel.title`, `panel.icon`, `panel.icon.file` (button), `panel.icon.builtin` (choice: "Default" plus `PanelIcon` names), `panel.borderless`, and for terminals `panel.accent`, `panel.title_bar`, `panel.title_color`, `panel.colors.background`, `panel.colors.foreground`, `panel.colors.<0..15>`, `panel.preset.<index>` buttons over `PALETTE_PRESETS`, `panel.reset_all` button.
  - `apply_panel_field(scene: &mut Scene, id: PanelId, field: &str, value: FieldValue) -> Result<FormOutcome, String>`: edits `overrides` through `scene.get_mut` (marks dirty); `FieldValue::Reset` sets that field to `None`; `panel.reset_all` clears every override but keeps `borderless` geometry consistent via `set_borderless`; runs `PanelOverrides::validate` and rejects without changing on error.

- [ ] **Step 1: Write failing tests** in `panel_form.rs`:
  - `browser_form_has_title_icon_borderless_only` (ids exactly `panel.title`, `panel.icon`, `panel.icon.file`, `panel.icon.builtin`, `panel.borderless`, `panel.reset_all`).
  - `terminal_form_adds_colors_and_presets`.
  - `unset_fields_show_inherited_and_set_fields_show_set`.
  - `emoji_icon_is_accepted_and_five_chars_rejected` (`"🚀"` ok; `"abcde"` error, overrides unchanged); `empty_icon_text_resets_to_inherited`.
  - `reset_restores_the_global_value` and `reset_with_no_global_colors_gives_none_and_the_live_note` (`live_colors_note == true`).
  - `apply_to_missing_panel_is_an_error`.
  - Existing `every_control_in_every_form_applies_and_stays_valid` still passes.
- [ ] **Step 2:** Run, confirm failures.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test -p calm-core` passes; `cargo check --workspace --tests --examples` passes.
- [ ] **Step 5:** Commit `feat(core): per-panel settings form with inherited and reset state`. Lead tells the native lane it can start.

### Task 4: Settings item in the panel menu (small, exact)

**Files:**
- Modify: `crates/calm-core/src/menu.rs`

**Interfaces:**
- Produces: `pub const PANEL_SETTINGS: &str = "panel.settings";`, `MenuCommand::Settings`; `panel_menu` puts `Settings` after the Bring/Send group and before Duplicate/Delete, for every kind, minimized included, after its own separator. `placeholder_menu` unchanged. `menu_action("panel.settings", ..) == Some(MenuCommand::Settings)`.

- [ ] **Step 1:** Add test `every_kind_offers_settings` (Terminal, Browser, Plugin, Tui, minimized Terminal each contain `action("panel.settings", "Settings")`) and extend the id table test at `menu.rs:~450` with `("panel.settings", MenuCommand::Settings)`.
- [ ] **Step 2:** `cargo test -p calm-core menu` fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test -p calm-core menu` passes; existing menu tests updated only where the new item shifts positions.
- [ ] **Step 5:** Commit `feat(core): add Settings to every panel menu`.

### Task 5: Move modifier and content gesture

**Files:**
- Modify: `crates/calm-core/src/settings.rs` (`CanvasSettings`), `crates/calm-core/src/settings_form.rs` (Canvas section), `crates/calm-core/src/interaction.rs`
- Create: `crates/calm-core/src/gesture.rs`

**Interfaces:**
- Produces:
  - `enum MoveModifier { Cmd, Ctrl, Alt, CmdCtrl, CmdAlt, CtrlAlt }`, serde names as in Global Constraints, default `Cmd`; `MoveModifier::matches(self, mods: Modifiers) -> bool` (exact set: `Cmd` matches platform only, not platform+shift? Shift is ignored; the other three must match exactly).
  - `CanvasSettings.move_modifier: MoveModifier`; Canvas section gets a `canvas.move_modifier` choice.
  - `gesture::ContentGesture` with `fn press(screen: Point) -> Self`, `fn moved(&mut self, screen: Point) -> GestureStep`, `fn release(self) -> GestureStep`; `enum GestureStep { Undecided, StartMove { from: Point }, Click }`. Past `CLICK_SLOP` gives `StartMove` once, then `Undecided` is never returned again.
  - `InputHandler::begin_content_move(&mut self, id: PanelId, screen: Point, scene: &mut Scene) -> Vec<Effect>`: same state and effects as a title bar press (`Dragging`, bring to front, `FocusPanel`, `Redraw`); later `mouse_move`/`mouse_up` work unchanged.
  - `InputHandler::content_menu(&self, id: PanelId, screen: Point) -> Vec<Effect>`: `[ShowPanelMenu { id, screen }]`.
  - `fn content_gesture_target(scene: &Scene, screen: Point) -> Option<PanelId>`: the topmost non-minimized panel whose `content_rect` holds the point; `None` on empty canvas and on chips.

- [ ] **Step 1: Write failing tests:** `release_within_slop_is_a_click`, `move_past_slop_starts_once`, `cmd_matches_with_shift_but_not_with_ctrl`, `unknown_move_modifier_is_a_settings_error` (`"off"` rejected), `begin_content_move_then_mouse_move_moves_the_panel` (snapping on, `alt` disables it), `no_target_on_empty_canvas_or_chip`.
- [ ] **Step 2:** Run, confirm failures.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test -p calm-core` passes.
- [ ] **Step 5:** Commit `feat(core): move modifier setting and content drag gesture`. Core lane opens its PR into `feat/panel-settings`.

### Task 6: Tabs, text control and field state in the native window

**Files:**
- Modify: `crates/calm-native/src/settings_window.rs`

**Interfaces:**
- Consumes: Task 3 (`Control::Text`, `FieldState`, `FieldValue::Text`, `FieldValue::Reset`).
- Produces: `SettingsWindow::new(title: &str, layout: WindowLayout, on_change: impl FnMut(String, FieldValue) + 'static)` with `enum WindowLayout { Tabs, Single }`. `Tabs` puts each `Section` in its own `NSTabView` tab (scrolling inside); `Single` keeps today's scroll. `Control::Text` is an `NSTextField` reporting `FieldValue::Text` on end editing. `FieldState::Inherited` draws a small grey "inherited" label after the control; `FieldState::Set` draws a "Reset" button reporting `FieldValue::Reset` for that field id. `set_form` keeps the selected tab across rebuilds.
- [ ] **Step 1:** Extend the native example/test that counts controls (`bound_controls`) with a form holding one Text field and one Set field; expected count includes the Reset button.
- [ ] **Step 2:** `cargo test -p calm-native` fails.
- [ ] **Step 3:** Implement. Update the one caller in `calm-app/src/settings_menu.rs:157` to `SettingsWindow::new("Settings", WindowLayout::Tabs, ...)` so the workspace builds.
- [ ] **Step 4:** `cargo test -p calm-native` and `cargo check --workspace --tests --examples` pass.
- [ ] **Step 5:** Commit `feat(native): tabs, text fields and inherited state in settings windows`.

### Task 7: Content gesture monitor

**Files:**
- Create: `crates/calm-native/src/content_gesture.rs` (export from `lib.rs`)

**Interfaces:**
- Consumes: nothing from core except plain types; the monitor stays policy free.
- Produces: `ContentGestureMonitor::install(window: &NSWindow, wants: impl Fn(Point, GestureMods) -> bool + 'static, on_event: impl FnMut(ContentEvent) -> ContentDecision + 'static) -> Result<ContentGestureMonitor, NativeError>`; `enum ContentEvent { Down(Point), Dragged(Point), Up(Point), RightDown(Point) }` in window points (top-left origin, same space the canvas uses); `GestureMods { shift, control, alt, command: bool }`; `enum ContentDecision { Hold, Moving, Click }`. Dropping the monitor removes it.
  - Left down with `wants == true`: swallow it (return null) and keep the event. Dragged and Up while held: swallow and report; `on_event` answers `Hold` (undecided), `Moving` (drop the held down) or `Click` (at Up only).
  - On `Click` at Up: deliver the held down, then the up, to `window.sendEvent` so Ghostty and WKWebView see a normal click (Cmd+click on a link opens it).
  - Right down with `wants == true`: swallow and report `RightDown`.
  - Only events over a native child of this window are candidates; others pass through untouched.
- [ ] **Step 1:** Unit test the pure part: `fn decide_passthrough(over_native: bool, wants: bool) -> bool` and the held-event replay order as a small state struct with a fake sink (`replays_down_then_up_on_click`, `drops_held_down_on_move`).
- [ ] **Step 2:** `cargo test -p calm-native content_gesture` fails.
- [ ] **Step 3:** Implement with `NSEvent::addLocalMonitorForEventsMatchingMask_handler` (precedent: `browser.rs:327`), SAFETY comments on every `unsafe`.
- [ ] **Step 4:** Tests pass; `cargo clippy -p calm-native -- -D warnings` clean.
- [ ] **Step 5:** Commit `feat(native): content gesture monitor with click replay`. Native lane opens its PR.

### Task 8: Draw overrides and borderless

**Files:**
- Modify: `crates/calm-app/src/canvas_view.rs` (`panel_look`, title drawing, terminal recolor, native placement), `crates/calm-app/src/main.rs` (chrome paint near lines 749 and 821, `CHROME_HEIGHT` use near 1193)

**Interfaces:**
- Consumes: Tasks 1, 2.
- Produces: `CanvasView::panel_look` returns override-aware looks for all three sources (terminal via `resolve_for_panel`, TUI and default via `chrome::lay_overrides`); an override `title` shows instead of the panel's own title; borderless panels paint no bar and place native views at `content_rect`; `CanvasView::apply_panel_overrides(id)` re-resolves and calls the existing live recolor (`TerminalThemeState::apply`) for terminals.
- [ ] **Step 1:** Add a smoke line `smoke: panel_overrides styled=<n> borderless=<n>` in `smoke_runner.rs` that sets a title override and borderless on one smoke terminal and reads back `panel_look` and `content_rect`; expected `styled=1 borderless=1` and the terminal's logical size unchanged.
- [ ] **Step 2:** Run smoke, line missing or wrong.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo run -p calm-app -- --smoke; echo rc=$?` prints the line and `rc=0`.
- [ ] **Step 5:** Commit `feat(app): draw per-panel overrides and borderless panels`.

### Task 9: Panel Settings window lifecycle and icon import

**Files:**
- Modify: `crates/calm-app/src/settings_menu.rs` (or create `crates/calm-app/src/panel_settings.rs`), `crates/calm-app/src/canvas_view.rs` (`MenuCommand::Settings`, close path)
- Create: `crates/calm-core/src/panel_icons.rs`

**Interfaces:**
- Consumes: Tasks 3, 4, 6, 8.
- Produces:
  - `panel_icons::import_icon(settings_dir: &Path, source: &Path) -> Result<String, String>`: copies a `.png`/`.svg` into `<settings_dir>/panel-icons/<sha256-prefix>.<ext>` and returns the path relative to `settings_dir`; other extensions are an error. Tested with a temp dir (`imports_png_and_rejects_jpg`, `same_file_twice_gives_same_path`).
  - At most one window per panel (`HashMap<PanelId, SettingsWindow>`); opening again focuses it; `Effect::ClosePanel` closes it; window title `"<effective title> Settings"`.
  - Each change: `apply_panel_field`, then `apply_panel_overrides(id)`, then rebuild the form when `rebuild` is true.
- [ ] **Step 1:** Write the `panel_icons` tests; add a smoke line `smoke: panel_settings windows_open=1 after_close=0` that opens the window for a smoke panel, closes the panel, and counts with `settings_window_count`.
- [ ] **Step 2:** Run, confirm failure.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test -p calm-core panel_icons` and smoke pass, `rc=0`.
- [ ] **Step 5:** Commit `feat(app): per-panel Settings window from the context menu`.

### Task 10: Gesture wiring, restore, duplicate, docs

**Files:**
- Modify: `crates/calm-app/src/canvas_view.rs` (install `ContentGestureMonitor`, restore at ~402, Duplicate at ~894, `open_plain_terminal` stores `profile` in `PanelKind::Terminal`), `.ai/status.md`, `.ai/decisions.md`, `docs/architecture.md`, `CLAUDE.md` ("In the app" paragraph)

**Interfaces:**
- Consumes: Tasks 5, 7.
- Produces: the monitor's `wants` is `settings.canvas.move_modifier.matches(mods) && content_gesture_target(..).is_some()`; `Down` creates a `ContentGesture`; `StartMove` calls `begin_content_move`; `Click` replays; `RightDown` calls `content_menu`. Restore passes the saved `profile` to `open_plain_terminal`; an unknown id gives a notice. Duplicate copies `overrides` and `profile`.
- [ ] **Step 1:** Core test `restored_terminal_uses_its_saved_profile` (restore planning function in `calm-core::restore` returns the saved profile id) and `duplicate_copies_overrides` (if the duplicate logic is in core; else a smoke line `smoke: duplicate overrides_copied=true`).
- [ ] **Step 2:** Run, confirm failure.
- [ ] **Step 3:** Implement; write the decisions entry "Per-panel settings (2026-11-01)" with the lead calls from the spec, and add to the by-eye list in `status.md`: Cmd+drag over a live terminal and a page, Cmd+click on a link in both, Cmd+right-click menu, panel Settings window, tabs.
- [ ] **Step 4:** `cargo clean -p calm-core -p calm-native -p calm-app`, `cargo check --workspace --tests --examples`, `gmake ci`, smoke `rc=0`.
- [ ] **Step 5:** Commit `feat(app): Cmd+drag on content, restore profiles, docs`. App lane opens its PR; then the review lane runs.
