# See-through Terminals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Terminals let the canvas or the panel below show through: Ghostty's own `background-opacity` is no longer hidden by CALM, and each terminal gets a CALM fade (content opacity) with a global default.

**Architecture:** The fade is a new inherited terminal field resolved like the #57 colors (panel override, profile, `terminal` defaults, then 1.0) and applied as `alphaValue` on the panel's native clip view. CALM stops painting fills under live terminal content, and a back panel's native view is no longer masked under a live terminal's content (only under its title bar and ring), while input keeps the old region.

**Tech Stack:** Rust, GPUI (`gpui-pre` 0.3.4), objc2 / objc2-app-kit 0.3, stock gpui-libghostty 0.3.1.

**Spec:** `docs/superpowers/specs/2026-11-02-terminal-transparency-design.md`

## Global Constraints

- Stock `gpui-libghostty = "=0.3.1"`. No `[patch]`, no fork, no calls into its private fields or C symbols.
- Fade range `0.2..=1.0`, default `1.0`. Values outside the range or NaN are a validation error in files; the form clamps.
- Terminals only. A fade override on a browser or plugin panel is a validation error, like `terminal_colors`.
- No GPUI drawing over a live native view. No setting change restarts or kills a process.
- Customization first: the fade is a setting with a default, validated strictly.
- `.ai/status.md` and `.ai/decisions.md` are updated in the task that changes the behavior.
- Code changes in a worktree under `.worktrees/`. Before the final push: `cargo clean -p calm-core -p calm-native -p calm-app`, `cargo check --workspace --tests --examples`, `gmake ci`, and `cargo run -p calm-app -- --smoke; echo rc=$?` on a visible window.
- Coordination: the in-flight `tui-extends-terminal` work turns TUI panels into terminals. Rebase on it if it lands first. If it has not landed, TUI panels get the fade through `PanelKind::Tui` in Task 1 and Task 3 (spec: TUI counts as a terminal).

## Review Focus

1. A session file with `content_opacity: 0.05` or `"0.5"` (string): load fails with a named field error, the way other overrides fail, never a silent clamp.
2. A fade change while the panel is a preview: the next snapshot and the return to live both show the new fade; nothing resets it to 1.0.
3. Two overlapping terminals: a click in the overlap goes to the front terminal only, even though the back one now paints there.
4. A minimized or preview front panel over a live back terminal: the back terminal is still cut out under it, since the front is GPUI-drawn.
5. Reset to global on a faded panel brings it back to the inherited value at once, live.

---

### Task 1: The `content_opacity` field and its resolution (calm-core)

**Files:**
- Modify: `crates/calm-core/src/terminal_profile.rs` (`TerminalFields`, its `validate`, `ResolvedTerminal`, `resolve`, `resolve_for_panel`)
- Modify: `crates/calm-core/src/scene.rs` (`PanelOverrides`, `PanelOverrides::validate`)
- Test: the `tests` modules of both files, and `crates/calm-core/src/layout.rs` tests for save and restore

**Interfaces:**
- Produces: `pub const MIN_CONTENT_OPACITY: f64 = 0.2;` and `pub const DEFAULT_CONTENT_OPACITY: f64 = 1.0;` in `terminal_profile.rs`.
- Produces: `TerminalFields.content_opacity: Option<f64>` and `PanelOverrides.content_opacity: Option<f64>`, both `#[serde(skip_serializing_if = "Option::is_none")]`.
- Produces: `ResolvedTerminal.content_opacity: f64`, panel override over profile over `terminal` defaults over `DEFAULT_CONTENT_OPACITY`.
- Produces: `pub(crate) fn check_content_opacity(path: &str, value: Option<f64>) -> Result<(), String>`, error text `"\"{path}\" is {value}, expected 0.2 to 1"`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn content_opacity_resolves_panel_over_profile_over_defaults() {
    // defaults 0.9, profile "dim" 0.6, panel override 0.4
    // resolve_for_panel(None, true, &no_override).content_opacity == 0.9
    // resolve_for_panel(Some("dim"), true, &no_override).content_opacity == 0.6
    // resolve_for_panel(Some("dim"), true, &override_0_4).content_opacity == 0.4
    // nothing set anywhere: == DEFAULT_CONTENT_OPACITY
}

#[test]
fn content_opacity_out_of_range_is_rejected_with_its_path() {
    // 0.19, 1.01, f64::NAN as overrides.content_opacity -> Err containing "overrides.content_opacity"
    // 0.2 and 1.0 -> Ok
}

#[test]
fn content_opacity_on_a_browser_is_rejected() {
    // PanelOverrides { content_opacity: Some(0.5), .. }.validate(&browser())
    // -> Err("\"overrides.content_opacity\" applies only to terminal panels")
}
```

In `layout.rs` tests: a session with `"content_opacity": 0.5` on a terminal round-trips; a session without the field loads with `None`; `"content_opacity": "0.5"` fails to load (Review Focus 1).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p calm-core content_opacity`
Expected: FAIL at the assertions (add the fields as `Option<f64>` stubs first if needed so the run compiles; a compile error is not the RED).

- [ ] **Step 3: Implement** the fields, `check_content_opacity`, the terminal-only rule in `PanelOverrides::validate` (accept `PanelKind::Tui` too while it exists), and the resolution order in `resolve` and `resolve_for_panel`.

- [ ] **Step 4: Run them to see them pass**

Run: `cargo test -p calm-core`
Expected: PASS, whole crate.

- [ ] **Step 5: Commit**

```bash
git add crates/calm-core/src/terminal_profile.rs crates/calm-core/src/scene.rs crates/calm-core/src/layout.rs
git commit -m "feat(core): add inherited terminal content opacity"
```

### Task 2: Form rows for the fade (calm-core)

**Files:**
- Modify: `crates/calm-core/src/panel_form.rs` (terminal section of the panel form, `apply_panel_field`)
- Modify: `crates/calm-core/src/settings_form.rs` (`terminal_fields`, the `terminal.` arm of apply; make `slider` `pub(crate)`)
- Test: both files' `tests` modules

**Interfaces:**
- Consumes: Task 1 fields and constants.
- Produces: panel field id `"panel.content_opacity"`, label `"Opacity"`, a slider over `MIN_CONTENT_OPACITY..=1.0` with 2 decimals, wrapped in `with_state(.., overrides.content_opacity.is_some())`, in a terminal-only section titled `"Opacity"`. `FieldValue::Number(v)` sets the clamped, rounded value; `FieldValue::Reset` sets `None`; `panel.reset_all` clears it with the rest.
- Produces: global field id `"terminal.content_opacity"`, same label and range, in the Terminal tab, plus a note `"terminal.background_opacity_note"`: `"Background opacity comes from background-opacity in your Ghostty config, and only with the Ghostty user config on."`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn panel_opacity_field_sets_clamps_and_resets() {
    // apply "panel.content_opacity" Number(0.05) -> overrides.content_opacity == Some(0.2)
    // apply Number(0.456) -> Some(0.46)
    // apply Number(NAN) -> Some(0.2)   (same clamp rule as other sliders)
    // apply Reset -> None
}

#[test]
fn browser_form_has_no_opacity_field() {
    // field ids of the browser form do not contain "panel.content_opacity"
}

#[test]
fn global_terminal_tab_has_opacity_and_the_ghostty_note() {
    // ids contain "terminal.content_opacity" and "terminal.background_opacity_note"
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p calm-core opacity`
Expected: FAIL at the assertions.

- [ ] **Step 3: Implement** the rows and the apply arms. Add `PanelEffective.content_opacity: f64`, filled in `panel_effective` from `resolve_for_panel(..).content_opacity` (`DEFAULT_CONTENT_OPACITY` for other kinds); the panel row shows it when inherited.

- [ ] **Step 4: Run to see them pass**

Run: `cargo test -p calm-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/calm-core/src/panel_form.rs crates/calm-core/src/settings_form.rs
git commit -m "feat(core): add opacity rows to panel and terminal settings"
```

### Task 3: Apply the fade to the native view (calm-native, calm-app)

**Files:**
- Modify: `crates/calm-native/src/child.rs` (`NativeChild`)
- Modify: `crates/calm-app/src/terminal/adapter.rs` (`TerminalPanel`)
- Modify: `crates/calm-app/src/canvas_view.rs` (`refresh_terminal`, terminal creation and restore, duplicate)
- Modify: `crates/calm-app/src/smoke_runner.rs` (the `panel_overrides` step)

**Interfaces:**
- Consumes: `ResolvedTerminal.content_opacity`.
- Produces: `NativeChild::set_opacity(&self, opacity: f64)` (calls `setAlphaValue` on the clip view) and `NativeChild::opacity(&self) -> f64` (reads `alphaValue`).
- Produces: `TerminalPanel::set_content_opacity(&mut self, opacity: f64)`, stored so the preview path (Task 5) can read it, and `TerminalPanel::content_opacity(&self) -> f64`.
- Produces: smoke line `smoke: terminal_opacity=[..]`, one value per terminal from `NativeChild::opacity`.

- [ ] **Step 1: Extend the smoke step first.** In `panel_overrides`, set `content_opacity = Some(0.5)` on the first terminal, call `apply_panel_overrides`, and fail the step unless that terminal reports `0.5` (within 0.001) and the others report `1.0`. Then reset it and require `1.0`.

- [ ] **Step 2: Run smoke to see the step fail**

Run: `cargo run -p calm-app -- --smoke; echo rc=$?`
Expected: `panel_overrides` FAIL naming `terminal_opacity`. Needs a visible window; with `window_visible=Some(false)` the run proves nothing.

- [ ] **Step 3: Implement.** `refresh_terminal` calls `set_content_opacity(resolved.content_opacity)` every time, not only on color change. New and restored terminals get it at spawn. A global settings change refreshes every terminal through the existing path. Duplicate already copies `overrides` (check `canvas_view.rs` around `add_with_overrides`); add a test or smoke assertion only if it does not.

- [ ] **Step 4: Run smoke to see it pass**

Run: `cargo run -p calm-app -- --smoke; echo rc=$?`
Expected: `rc=0`, the `terminal_opacity` line shows `0.5` then `1.0`.

- [ ] **Step 5: Commit**

```bash
git add crates/calm-native/src/child.rs crates/calm-app/src/terminal/adapter.rs crates/calm-app/src/canvas_view.rs crates/calm-app/src/smoke_runner.rs
git commit -m "feat(app): fade terminal content per panel"
```

### Task 4: No fill under live terminals, and the panel below shows through

**Files:**
- Modify: `crates/calm-core/src/occlusion.rs` (`native_visible_region`, new `native_paint_region`)
- Modify: `crates/calm-core/src/viewport.rs` (`NativePlacement`)
- Modify: `crates/calm-native/src/child.rs` (`apply`, `apply_mask` use the paint region)
- Modify: `crates/calm-app/src/canvas_view.rs` (`terminal_placement`, `paint_panel`)
- Test: `occlusion.rs` tests

**Interfaces:**
- Produces: `NativePlacement.paint: Vec<Rect>`, same space as `visible`. `visible` stays the input region (unchanged meaning); `paint` is the mask and decides `occluded`.
- Produces: `pub fn native_paint_region(scene: &Scene, joins: &[Join], id: PanelId, container: Rect, offset: Point, live_native: &dyn Fn(PanelId) -> bool) -> Vec<Rect>`. A front panel for which `live_native` is true occludes only its title bar strip and ring, not its content rect. Any other front panel (preview, minimized, hidden) occludes as today. Join dividers occlude as today.

- [ ] **Step 1: Explain the user's observation first.** By the code, the back terminal should have been cut out under the front one, yet the user saw it. Find out why with superpowers:systematic-debugging (read-only first: a unit test on `native_visible_region` for two overlapping panels, then the `apply_mask` path for a stale mask after a panel is added). Record the finding in `.ai/status.md`. If it is a stale-mask bug, its regression test lands in this task.

- [ ] **Step 2: Write the failing tests**

```rust
#[test]
fn paint_region_keeps_a_back_panel_under_a_live_front_terminal_content() {
    // back 0..200, front 100..300 overlapping; live_native(front) == true
    // paint region of back contains a point inside front's content rect
    // paint region of back does not contain a point in front's title bar strip or ring
}

#[test]
fn paint_region_still_cuts_under_a_preview_or_minimized_front() {
    // same rects, live_native(front) == false -> equals native_visible_region
}

#[test]
fn input_region_is_unchanged_by_live_front_panels() {
    // native_visible_region result for back is identical to before (Review Focus 3)
}
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p calm-core occlusion`
Expected: FAIL at the paint region assertions.

- [ ] **Step 4: Implement** `native_paint_region`, `NativePlacement.paint`, the mask switch in `NativeChild`, and `live_native` from the canvas (a terminal panel that is placed live this frame). In `paint_panel`, for a live terminal: skip the `CONTENT_PLACEHOLDER` fill, paint the ring quad with a transparent background, and paint the title bar fill as its own quad over the bar rect with the same top corner radii. Borderless terminals paint no bar fill. Keep the shadow as is (by-eye item).

- [ ] **Step 5: Run tests and smoke**

Run: `cargo test -p calm-core && cargo run -p calm-app -- --smoke; echo rc=$?`
Expected: PASS and `rc=0`; the `zorder` and preview steps still pass.

- [ ] **Step 6: Commit**

```bash
git add crates/calm-core/src/occlusion.rs crates/calm-core/src/viewport.rs crates/calm-native/src/child.rs crates/calm-app/src/canvas_view.rs .ai/status.md
git commit -m "feat(app): let the canvas and the panel below show through terminals"
```

### Task 5: Faded previews and color wells without opacity

**Files:**
- Modify: `crates/calm-core/src/preview.rs` (pure byte helper)
- Modify: `crates/calm-app/src/terminal/adapter.rs` (`take_snapshot`)
- Modify: `crates/calm-native/src/settings_window.rs` (where the first Settings window is created)
- Modify: `.ai/status.md`, `.ai/decisions.md`
- Test: `preview.rs` tests

**Interfaces:**
- Consumes: `TerminalPanel::content_opacity` (Task 3).
- Produces: `pub fn fade_pixels(pixels: &mut [u8], opacity: f64, premultiplied: bool)`: scales the alpha byte of each 4-byte pixel by `opacity`, and the three color bytes too when `premultiplied`. A length not divisible by 4 leaves the tail untouched.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn fade_pixels_scales_alpha_and_premultiplied_color() {
    // [200, 100, 50, 255] at 0.5, premultiplied -> [100, 50, 25, 128]
    // same at 0.5, straight -> [200, 100, 50, 128]
    // opacity 1.0 leaves bytes unchanged
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p calm-core fade_pixels`
Expected: FAIL at the assertion.

- [ ] **Step 3: Implement** `fade_pixels`. In `take_snapshot`, apply it to the frame's bytes when `content_opacity < 1.0`. Find out from `gpui-pre` 0.3.4's `RenderImage` path whether its BGRA is premultiplied, and pass that; write the answer in a one-line comment.

- [ ] **Step 4: Color wells.** Every CALM color well drops alpha today (`rgb_from_ns`, `BindingKind::Color`), the chrome wells included. So call `NSColor::setIgnoresAlpha(true, mtm)` once, before the first Settings or Panel Settings window opens. Record in `.ai/decisions.md`: app-wide because no well honors alpha; per-well opacity returns when chrome alpha is wired (the gap stays in `.ai/status.md`). This replaces the spec's "terminal wells only" with one app-wide switch, decided at plan time.

- [ ] **Step 5: Run tests**

Run: `cargo test -p calm-core && cargo check --workspace --tests --examples`
Expected: PASS, no errors.

- [ ] **Step 6: Commit**

```bash
git add crates/calm-core/src/preview.rs crates/calm-app/src/terminal/adapter.rs crates/calm-native/src/settings_window.rs .ai/status.md .ai/decisions.md
git commit -m "feat(app): fade terminal previews and hide unused color opacity"
```

### Task 6: Gates and the by-eye list

**Files:**
- Modify: `.ai/status.md` (by-eye list), `docs/architecture.md` if it describes occlusion or the paint of panel bodies

- [ ] **Step 1:** Add the by-eye items to `.ai/status.md`: canvas through a terminal with Ghostty `background-opacity = 0.5`; a back terminal through a front one, and a click in the overlap reaching the front one; the fade at 20%, 70% and 100%; title bar solid; preview of a faded terminal; no opacity slider in any color picker; shadow under a see-through terminal.
- [ ] **Step 2:** Run `cargo clean -p calm-core -p calm-native -p calm-app && cargo check --workspace --tests --examples && gmake ci && cargo run -p calm-app -- --smoke; echo rc=$?`. Expected: every step exits 0 on a visible window. A flake is not a pass; run a control on `origin/main` before blaming the branch.
- [ ] **Step 3:** Commit, then a cross-family review of the whole branch before merge (`.ai/conventions.md`, "Review and merging").
