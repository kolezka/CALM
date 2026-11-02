# Editable Key Bindings Implementation Plan

> **Execution (user's choice, 2026-11-02):** native, the lead implements every task in one session after #63 merges; one fresh cross-family reviewer checks the whole branch before merge.
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every CALM key binding becomes a setting, listed and recorded in a Keys tab of the Settings window, applied live and saved as overrides in `settings.json`.

**Architecture:** One pure action table in `calm-core/src/keymap.rs` owns ids, labels, defaults, scopes, the key grammar, the GPUI keystroke rule and conflict checks. `calm-core` settings store only overrides and validate them strictly; the settings form gets a Keys section. `calm-app/src/keymap.rs` maps ids to GPUI actions and rebuilds the whole keymap and menus after every settings change. `calm-native` adds a recorder control that turns one AppKit key-down into the string GPUI will report.

**Tech Stack:** Rust 2024, GPUI `gpui-pre 0.3.4` (macOS backend `gpui-pre-macos 0.3.4`), objc2 0.6 / objc2-app-kit 0.3, serde.

**Spec:** `docs/superpowers/specs/2026-11-02-keybindings-settings-design.md` (binding).

**Base:** `origin/main` at `2eb4ea8` (PR #60 merged). Line numbers below are from `2eb4ea8`; every cited file is identical there to the #60 head `2ce1de5`. PR #63 lands first and moves some of them: see "Re-check after #63" at the end.

GPUI sources cited as `events.rs`, `platform.rs` live in
`/Users/me/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-pre-macos-0.3.4/src/`;
`app.rs`, `keystroke.rs`, `binding.rs` in `.../gpui-pre-0.3.4/src/`.

## Global Constraints

- Ids, labels, defaults and scopes are exactly the spec table, plus five unbound menu rows (Task 1).
- `canvas.fit_all` default is `cmd-)` (GPUI's report of Cmd+Shift+0 on US), not `cmd-shift-0`.
- Settings section `keybindings`: optional, overrides only, `null` means unbound, Reset removes the id.
- Grammar: modifiers `cmd ctrl alt shift fn` joined by `-`, then one key. No chords, one key per action.
- Shift with a printable non-letter key is an error. Named keys keep Shift (`cmd-shift-backspace` is valid).
- Reserved, never bindable: the ten browser keys (`calm-native/src/browser.rs:227-238`) and bare `space`.
- Any binding without Cmd or Ctrl also gets `NoAction` in the `Terminal` key context.
- "canvas" scope binds in key context `Canvas`; "global" binds with no context. Every pair of scopes overlaps.
- Recording refuses a key another action uses and shows "Used by <label>" for 3 s; no swap.
- UI shows keys with macOS symbols from one pure function; modifiers in Apple's order `⌃⌥⇧⌘` (lead's call, overriding the spec's `⌘⇧⌫` example), so `cmd-shift-backspace` shows as `⇧⌘⌫`.
- Recording prompt text is exactly `Press a shortcut…`. Escape alone cancels. Focus loss cancels.
- A menu item whose key GPUI cannot show (any `fn`, `enter`, `tab`; `platform.rs:364-402`) gets the row label suffix ` (not shown in the menu)`.
- No synthetic key presses anywhere, including the smoke. `unsafe` only in `calm-native`, each with a SAFETY comment.
- Plain writing in code comments and docs: no en or em dashes.

## Review Focus

1. **Same key, different spelling.** `shift-cmd-h` in the file must conflict with `canvas.center_origin` (`cmd-shift-h`); `CMD-h` is a syntax error, not a silent second spelling. Pinned in Task 1 (`modifier_order_does_not_hide_a_conflict`).
2. **Swapping keys by hand in the file.** `panel.delete: "cmd-="` together with `canvas.zoom_in: "cmd-k"` is valid: conflicts use effective keys, not defaults. Pinned in Task 1 (`conflicts_use_effective_keys_not_defaults`).
3. **Recording the key a row already has, or its default.** Not "Used by <itself>"; recording the default stores no override. Pinned in Task 3 (`recording_the_same_or_default_key_is_not_a_conflict`).
4. **Wrong JSON types.** `"keybindings": 1`, `"keybindings": null`, `{"panel.delete": 1}` are explicit errors naming `keybindings`, never defaults. Pinned in Task 3 (`keybindings_with_wrong_types_are_errors`).
5. **Non-US layouts.** `cmd-Ö` (German) and `cmd-ý` (Czech) are what GPUI reports and must validate; only ASCII uppercase is rejected. Pinned in Task 1 (`grammar_accepts_what_gpui_reports`) and Task 2 (layout cases).

## File map

| File | Task | Responsibility |
|---|---|---|
| `crates/calm-core/src/keymap.rs` (new) | 1, 2 | table, grammar, checks, symbols, `keystroke_from_parts` |
| `crates/calm-core/src/lib.rs` | 1 | `pub mod keymap;` |
| `crates/calm-core/src/settings.rs` | 3 | `keybindings` field and validation |
| `crates/calm-core/src/settings_form.rs` | 3 | Keys section, `Control::Key`, `FieldValue::Key`, `FormOutcome::KeyRefused`, stats label |
| `crates/calm-app/src/keymap.rs` (new) | 4 | id to action map, binding list, `apply` |
| `crates/calm-app/src/{canvas_view,dev_menu,settings_menu,main,panel_settings}.rs` | 3, 4 | remove three `key_bindings()`, live rebind, confirm title |
| `crates/calm-native/src/settings_window.rs` | 3, 5 | recorder, Clear, transient message |
| `crates/calm-native/Cargo.toml` | 5 | objc2-foundation features `NSNotification`, `NSOperation` |
| `crates/calm-app/src/{smoke_runner,main}.rs` | 6 | `keybindings` smoke step |
| `README.md`, `CLAUDE.md`, `docs/architecture.md`, `docs/dev-setup.md`, `.ai/decisions.md`, `.ai/status.md` | 6 | docs |

---

### Task 1: Action table, key grammar and checks (calm-core)

**Files:**
- Create: `crates/calm-core/src/keymap.rs`
- Modify: `crates/calm-core/src/lib.rs` (add `pub mod keymap;` in order)
- Test: `crates/calm-core/src/keymap.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces:

```rust
pub type KeyOverrides = std::collections::BTreeMap<String, Option<String>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope { Canvas, Global }
impl Scope { pub fn overlaps(self, other: Scope) -> bool; } // explicit match, all four pairs true

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group { Canvas, Panels, View, Developer, App }
impl Group { pub fn title(self) -> &'static str; } // "Canvas", "Panels", "View", "Developer", "App"

#[derive(Debug, PartialEq, Eq)]
pub struct ActionSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub group: Group,
    pub scope: Scope,
    pub default: Option<&'static str>,
    /// Listed in a menu bar menu, so its key shows there.
    pub in_menu: bool,
}
pub const ACTIONS: &[ActionSpec];
pub fn action(id: &str) -> Option<&'static ActionSpec>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySpec { pub cmd: bool, pub ctrl: bool, pub alt: bool, pub shift: bool, pub function: bool, pub key: String }
impl KeySpec { pub fn canonical(&self) -> String; } // "cmd-ctrl-alt-shift-fn-<key>", present parts only

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError { Syntax, BareModifier, UppercaseLetter, ShiftedSymbol, FnNeverFires, Reserved(&'static str), Space, UsedBy(&'static str) }
impl KeyError { pub fn message(self) -> String; }

pub const BROWSER_KEYS: &[(&str, &str)]; // (key, label), 10 entries
pub const RESTART_KEY: (&str, &str) = ("enter", "Restart an exited terminal");

pub fn parse_key(text: &str) -> Result<KeySpec, KeyError>;      // grammar and never-fires rules
pub fn check_key(text: &str) -> Result<KeySpec, KeyError>;      // parse_key plus Reserved and Space
pub fn effective_key(overrides: &KeyOverrides, action: &ActionSpec) -> Option<String>;
pub fn bound_keys(overrides: &KeyOverrides) -> Vec<(&'static ActionSpec, String)>; // ACTIONS order, unbound skipped
pub fn check_rebind(overrides: &KeyOverrides, action: &ActionSpec, key: &str) -> Result<KeySpec, KeyError>;
pub fn validate_overrides(overrides: &KeyOverrides) -> Result<(), String>;
pub fn passes_to_terminal(key: &KeySpec) -> bool;               // !cmd && !ctrl
pub fn shows_in_menu(key: &KeySpec) -> bool;                    // false for fn, "enter", "tab"
pub fn display_key(text: &str) -> String;
pub fn confirm_prompt(overrides: &KeyOverrides) -> String;
```

`ACTIONS`, in this order (it is the row order of the Keys tab):

| id | label | group | scope | default | in_menu |
|---|---|---|---|---|---|
| `canvas.draw_terminal` | Draw a terminal | Canvas | Canvas | `t` | no |
| `canvas.draw_browser` | Draw a browser | Canvas | Canvas | `b` | no |
| `canvas.cancel` | Cancel / answer No | Canvas | Canvas | `escape` | no |
| `canvas.confirm` | Confirm close | Canvas | Canvas | `enter` | no |
| `panel.delete` | Delete focused panel | Panels | Canvas | `cmd-shift-backspace` | no |
| `panel.settings` | Focused panel Settings | Panels | Canvas | `cmd-<` | no |
| `canvas.zoom_in` | Zoom in | View | Canvas | `cmd-=` | no |
| `canvas.zoom_out` | Zoom out | View | Canvas | `cmd--` | no |
| `canvas.zoom_reset` | Reset zoom | View | Canvas | `cmd-0` | no |
| `canvas.center_origin` | Center on origin | View | Canvas | `cmd-shift-h` | yes |
| `canvas.fit_all` | Fit all panels | View | Canvas | `cmd-)` | yes |
| `view.fullscreen` | Toggle fullscreen | View | Canvas | `cmd-ctrl-f` | no |
| `view.show_grid` | Show grid | View | Global | none | yes |
| `developer.toggle_stats` | Show stats | Developer | Global | `cmd-shift-f` | yes |
| `developer.reset_fps` | Reset FPS stats | Developer | Global | none | yes |
| `app.settings` | Open Settings | App | Global | `cmd-,` | yes |
| `app.open_config_folder` | Open config folder | App | Global | none | yes |
| `app.reset_settings` | Reset settings to defaults | App | Global | none | yes |
| `app.about` | About CALM | App | Global | none | yes |
| `app.quit` | Quit | App | Global | `cmd-q` | yes |

The five unbound rows are every other action the menus register: `dev_menu.rs:9` (`ResetFps`, `About`) and `settings_menu.rs:19-22` (`ToggleGrid`, `OpenConfigFolder`, `ResetSettings`).

`BROWSER_KEYS`: `("cmd-[","Back") ("cmd-]","Forward") ("cmd-r","Reload") ("cmd-l","Address bar") ("cmd-c","Copy") ("cmd-x","Cut") ("cmd-v","Paste") ("cmd-a","Select all") ("cmd-z","Undo") ("cmd-shift-z","Redo")`.

`KeyError::message`, exact text:

| variant | message |
|---|---|
| `Syntax` | `not a key; write modifiers (cmd, ctrl, alt, shift, fn) and one key joined by "-", like "cmd-shift-h"` |
| `BareModifier` | `only modifiers; add a key, like "cmd-k"` |
| `UppercaseLetter` | `write "shift-" and the lowercase letter, like "cmd-shift-k"` |
| `ShiftedSymbol` | `shift with this key never fires, because macOS reports the shifted character; write that character, like "cmd-<" for Cmd+Shift+comma on a US layout` |
| `FnNeverFires` | `fn with this key never fires; macOS sets fn on it already` |
| `Reserved(label)` | `a fixed browser key ({label})` |
| `Space` | `space alone is the hold-to-pan key` |
| `UsedBy(label)` | `Used by {label}` |

Grammar rules for `parse_key` (narrower than `Keystroke::parse`, `keystroke.rs:120-200`, so every accepted string also parses in GPUI):
- Split on `-`; a trailing `--` or the whole text `-` means the key is `-`. Every part but the last is one of `cmd ctrl alt shift fn`, lowercase, each at most once; otherwise `Syntax`. Empty text, an empty key part, whitespace, more than one key: `Syntax`.
- Last part only a modifier name: `BareModifier`.
- Key is a named key (`space tab enter backspace escape up down left right pageup pagedown home end delete insert f1`..`f35`) or exactly one printable char. A single ASCII uppercase letter: `UppercaseLetter`. Non-ASCII uppercase (`Ö`) is valid.
- `shift` with a one-char key that is not `a`..`z`: `ShiftedSymbol` (`events.rs:466-476`).
- `fn` with `up down left right pageup pagedown home end delete insert f1..f35`: `FnNeverFires` (GPUI clears `function` for chars in `NSUpArrowFunctionKey..=NSModeSwitchFunctionKey`, `events.rs:354-357`).

`check_key` adds: a key equal (as `KeySpec`) to a `BROWSER_KEYS` entry: `Reserved(label)`; key `space` without cmd, ctrl and alt: `Space` (the pan check at `canvas_view.rs:2196` ignores shift).

`check_rebind` checks `check_key`, then compares the parsed key with the effective key of every other action whose scope overlaps; equal `KeySpec` gives `UsedBy(other.label)`. The action itself is skipped.

`validate_overrides` errors, one per case, first found wins, overrides walked in `ACTIONS` order then unknown ids:
- unknown id: `"keybindings.{id}": unknown action`
- bad value: `"keybindings.{id}" is "{value}": {message}`
- conflict between effective keys: same format with `UsedBy`, naming the overridden id (if both are overridden, the later one in `ACTIONS` order).

`display_key` symbols: modifiers in the order `⌃ ⌥ ⇧ ⌘`, then `fn ` for fn; keys `backspace ⌫`, `delete ⌦`, `enter ↩`, `tab ⇥`, `escape ⎋`, `space Space`, `up ↑`, `down ↓`, `left ←`, `right →`, `pageup ⇞`, `pagedown ⇟`, `home ↖`, `end ↘`, `insert Ins`, `f5 F5`; letters uppercased; other chars as written.

`confirm_prompt`: `"Close?"` followed by ` {confirm} / {cancel}` in words, not symbols (user's choice): the stored key string with `escape` shown as `esc`, e.g. `enter`, `esc`, `y`, `cmd-y`. An unbound side is dropped together with the ` / `.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn table_has_every_action_once() {
    let ids: Vec<&str> = ACTIONS.iter().map(|a| a.id).collect();
    assert_eq!(ids, [
        "canvas.draw_terminal", "canvas.draw_browser", "canvas.cancel", "canvas.confirm",
        "panel.delete", "panel.settings", "canvas.zoom_in", "canvas.zoom_out",
        "canvas.zoom_reset", "canvas.center_origin", "canvas.fit_all", "view.fullscreen",
        "view.show_grid", "developer.toggle_stats", "developer.reset_fps", "app.settings",
        "app.open_config_folder", "app.reset_settings", "app.about", "app.quit",
    ]);
    assert_eq!(action("panel.delete").unwrap().label, "Delete focused panel");
    assert!(action("nope").is_none());
}

#[test]
fn defaults_are_todays_keys_except_fit_all() {
    let defaults: Vec<(&str, Option<&str>)> = ACTIONS.iter().map(|a| (a.id, a.default)).collect();
    // Spec table; fit_all moves from cmd-shift-0, which GPUI never reports.
    assert!(defaults.contains(&("canvas.fit_all", Some("cmd-)"))));
    assert!(defaults.contains(&("panel.settings", Some("cmd-<"))));
    assert!(defaults.contains(&("app.about", None)));
    assert_eq!(defaults.iter().filter(|(_, d)| d.is_some()).count(), 15);
}

#[test]
fn every_default_is_valid_and_conflict_free() {
    for a in ACTIONS { if let Some(key) = a.default { assert_eq!(check_key(key).map(|_| ()), Ok(()), "{}", a.id); } }
    assert_eq!(validate_overrides(&KeyOverrides::new()), Ok(()));
}

#[test]
fn grammar_accepts_what_gpui_reports() {
    for ok in ["cmd-=", "cmd--", "-", "cmd-0", "cmd-<", "cmd-)", "cmd-shift-backspace", "shift-tab",
               "cmd-shift-enter", "shift-f5", "alt-up", "f12", "t", "shift-t", "cmd-Ö", "cmd-ý", "fn-a", "ctrl-alt-space"] {
        assert!(parse_key(ok).is_ok(), "{ok}");
    }
    assert_eq!(parse_key("shift-cmd-backspace").unwrap().canonical(), "cmd-shift-backspace");
}

#[test]
fn grammar_rejects_keys_gpui_never_reports() {
    for (text, error) in [
        ("", KeyError::Syntax), ("cmd-", KeyError::Syntax), ("Cmd-d", KeyError::Syntax),
        ("cmd-cmd-x", KeyError::Syntax), ("hyper-x", KeyError::Syntax), ("cmd-xy", KeyError::Syntax),
        ("cmd-k cmd-s", KeyError::Syntax), ("cmd", KeyError::BareModifier),
        ("cmd-shift", KeyError::BareModifier), ("cmd-D", KeyError::UppercaseLetter),
        ("cmd-shift-,", KeyError::ShiftedSymbol), ("cmd-shift-0", KeyError::ShiftedSymbol),
        ("shift-1", KeyError::ShiftedSymbol), ("shift--", KeyError::ShiftedSymbol),
        ("fn-up", KeyError::FnNeverFires), ("fn-f1", KeyError::FnNeverFires),
    ] {
        assert_eq!(parse_key(text), Err(error), "{text:?}");
    }
}

#[test]
fn reserved_browser_keys_and_bare_space_are_refused() {
    assert_eq!(check_key("cmd-r"), Err(KeyError::Reserved("Reload")));
    assert_eq!(check_key("cmd-shift-z"), Err(KeyError::Reserved("Redo")));
    assert_eq!(BROWSER_KEYS.len(), 10);
    assert_eq!(check_key("space"), Err(KeyError::Space));
    assert_eq!(check_key("shift-space"), Err(KeyError::Space));
    assert!(check_key("cmd-space").is_ok());
    assert!(check_key("cmd-shift-r").is_ok());
}

#[test]
fn conflicts_use_effective_keys_not_defaults() {
    let delete = action("panel.delete").unwrap();
    let mut overrides = KeyOverrides::new();
    assert_eq!(check_rebind(&overrides, delete, "cmd-="), Err(KeyError::UsedBy("Zoom in")));
    // Canvas and global overlap.
    assert_eq!(check_rebind(&overrides, action("app.quit").unwrap(), "cmd-="), Err(KeyError::UsedBy("Zoom in")));
    overrides.insert("canvas.zoom_in".into(), Some("cmd-k".into()));
    overrides.insert("panel.delete".into(), Some("cmd-=".into()));
    assert_eq!(validate_overrides(&overrides), Ok(()));
    overrides.insert("canvas.zoom_in".into(), None);
    assert_eq!(validate_overrides(&overrides), Ok(()));
}

#[test]
fn modifier_order_does_not_hide_a_conflict() {
    let quit = action("app.quit").unwrap();
    assert_eq!(check_rebind(&KeyOverrides::new(), quit, "shift-cmd-h"), Err(KeyError::UsedBy("Center on origin")));
    let mut overrides = KeyOverrides::new();
    overrides.insert("app.quit".into(), Some("shift-cmd-h".into()));
    let error = validate_overrides(&overrides).unwrap_err();
    assert!(error.contains("keybindings.app.quit") && error.contains("Used by Center on origin"), "{error}");
}

#[test]
fn validate_overrides_names_the_field() {
    let mut overrides = KeyOverrides::new();
    overrides.insert("canvas.nope".into(), Some("cmd-k".into()));
    assert_eq!(validate_overrides(&overrides), Err("\"keybindings.canvas.nope\": unknown action".into()));
    let mut overrides = KeyOverrides::new();
    overrides.insert("panel.delete".into(), Some("cmd-shift-,".into()));
    let error = validate_overrides(&overrides).unwrap_err();
    assert!(error.starts_with("\"keybindings.panel.delete\" is \"cmd-shift-,\": shift with this key never fires"), "{error}");
}

#[test]
fn only_keys_without_cmd_or_ctrl_pass_to_the_terminal() {
    for key in ["t", "escape", "enter", "f5", "alt-x", "shift-tab"] { assert!(passes_to_terminal(&parse_key(key).unwrap()), "{key}"); }
    for key in ["cmd-x", "ctrl-x", "cmd-ctrl-f", "ctrl-alt-space"] { assert!(!passes_to_terminal(&parse_key(key).unwrap()), "{key}"); }
}

#[test]
fn keys_show_with_macos_symbols() {
    for (text, shown) in [("cmd-shift-backspace", "⇧⌘⌫"), ("cmd-<", "⌘<"), ("cmd-ctrl-f", "⌃⌘F"),
        ("enter", "↩"), ("escape", "⎋"), ("cmd-=", "⌘="), ("alt-up", "⌥↑"), ("f5", "F5"),
        ("cmd-)", "⌘)"), ("t", "T"), ("fn-a", "fn A")] {
        assert_eq!(display_key(text), shown, "{text}");
    }
}

#[test]
fn menu_cannot_show_fn_enter_or_tab() {
    for key in ["fn-a", "cmd-enter", "cmd-tab"] { assert!(!shows_in_menu(&parse_key(key).unwrap()), "{key}"); }
    assert!(shows_in_menu(&parse_key("cmd-shift-h").unwrap()));
}

#[test]
fn confirm_prompt_follows_the_bindings() {
    let mut overrides = KeyOverrides::new();
    assert_eq!(confirm_prompt(&overrides), "Close? enter / esc");
    overrides.insert("canvas.confirm".into(), Some("y".into()));
    assert_eq!(confirm_prompt(&overrides), "Close? y / esc");
    overrides.insert("canvas.cancel".into(), None);
    assert_eq!(confirm_prompt(&overrides), "Close? y");
    overrides.insert("canvas.confirm".into(), None);
    assert_eq!(confirm_prompt(&overrides), "Close?");
}
```

- [ ] **Step 2: Run, expect FAIL**

Run: `cargo test -p calm-core keymap`
Expected: compile error, `keymap` module and items not found.

- [ ] **Step 3: Implement the interfaces above in `crates/calm-core/src/keymap.rs`**

Module doc names the GPUI version the rules mirror (`gpui-pre-macos 0.3.4`). `ACTIONS` is a `const` slice of struct literals. Conflict and reserved checks compare `KeySpec` values, never strings.

- [ ] **Step 4: Run, expect PASS**

Run: `cargo test -p calm-core keymap`
Expected: all tests above pass.

- [ ] **Step 5: Mutation RED, one rule at a time**

Apply each mutation alone, run `cargo test -p calm-core keymap`, confirm the named test fails at its assertion, revert:
- fit_all default back to `cmd-shift-0`: `every_default_is_valid_and_conflict_free` and `defaults_are_todays_keys_except_fit_all` fail.
- drop the ShiftedSymbol rule: `grammar_rejects_keys_gpui_never_reports` fails on `cmd-shift-,`.
- drop the named-keys-keep-shift exemption (ShiftedSymbol for every key): `grammar_accepts_what_gpui_reports` fails on `cmd-shift-backspace`.
- drop the FnNeverFires rule: fails on `fn-up`.
- reject all uppercase, not only ASCII: fails on `cmd-Ö`.
- drop the reserved check, then the space check: `reserved_browser_keys_and_bare_space_are_refused` fails each time.
- `Scope::overlaps` returns `self == other`: `conflicts_use_effective_keys_not_defaults` fails on `app.quit`.
- compare conflicts by string: `modifier_order_does_not_hide_a_conflict` fails.
- conflicts against defaults instead of effective keys: `conflicts_use_effective_keys_not_defaults` fails at `validate_overrides`.
- `passes_to_terminal` ignores ctrl: `only_keys_without_cmd_or_ctrl_pass_to_the_terminal` fails on `ctrl-x`.

- [ ] **Step 6: Commit**

```bash
git add crates/calm-core/src/keymap.rs crates/calm-core/src/lib.rs
git commit -m "feat: add the key binding table and key rules to calm-core"
```

---

### Task 2: `keystroke_from_parts`, the GPUI macOS rule (calm-core)

**Files:**
- Modify: `crates/calm-core/src/keymap.rs`
- Test: same file

**Interfaces:**
- Consumes: `KeySpec::canonical` (Task 1).
- Produces:

```rust
/// What AppKit gives for one key-down. The `chars_*` fields are the characters of the
/// event's key code with no modifier, Shift, Cmd and Cmd+Shift (GPUI's
/// `chars_for_modified_key`, events.rs:512).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyParts {
    pub cmd: bool, pub ctrl: bool, pub alt: bool, pub shift: bool, pub function: bool,
    /// `charactersIgnoringModifiers`; picks the named key.
    pub chars_ignoring_modifiers: String,
    pub chars_plain: String,
    pub chars_shift: String,
    pub chars_cmd: String,
    pub chars_cmd_shift: String,
    /// GPUI's `always_use_command_layout()` (events.rs:499-505).
    pub always_use_cmd_layout: bool,
}
pub fn keystroke_from_parts(parts: &KeyParts) -> Option<String>; // canonical string, None for no key
```

- [ ] **Step 1: Write the failing tests**

Helper in tests: `fn parts(cmd: bool, shift: bool, chars: [&str; 4], cmd_layout: bool) -> KeyParts` filling `chars_plain, chars_shift, chars_cmd, chars_cmd_shift` and setting `chars_ignoring_modifiers` to `chars[0]`.

```rust
#[test]
fn us_layout_matches_gpui() {
    assert_eq!(keystroke_from_parts(&parts(true, true, ["0", ")", "0", ")"], false)).as_deref(), Some("cmd-)"));
    assert_eq!(keystroke_from_parts(&parts(true, true, [",", "<", ",", "<"], false)).as_deref(), Some("cmd-<"));
    assert_eq!(keystroke_from_parts(&parts(true, true, ["s", "S", "s", "S"], false)).as_deref(), Some("cmd-shift-s"));
    let mut backspace = parts(true, true, ["\u{7f}"; 4], false);
    backspace.chars_ignoring_modifiers = "\u{7f}".into();
    assert_eq!(keystroke_from_parts(&backspace).as_deref(), Some("cmd-shift-backspace"));
    assert_eq!(keystroke_from_parts(&parts(false, false, ["\u{1b}"; 4], false)).as_deref(), Some("escape"));
}

// The table in events.rs:422-430.
#[test]
fn gpui_comment_table_layouts() {
    // Armenian: non-ASCII layout, so the cmd layout is used.
    assert_eq!(keystroke_from_parts(&parts(true, false, ["ս", "Ս", "s", "S"], true)).as_deref(), Some("cmd-s"));
    assert_eq!(keystroke_from_parts(&parts(true, true, ["ս", "Ս", "s", "S"], true)).as_deref(), Some("cmd-shift-s"));
    // Dvorak+QWERTY: layout switches on cmd.
    assert_eq!(keystroke_from_parts(&parts(false, false, ["o", "O", "s", "S"], false)).as_deref(), Some("o"));
    assert_eq!(keystroke_from_parts(&parts(true, true, ["o", "O", "s", "S"], false)).as_deref(), Some("cmd-shift-s"));
    // Ukrainian+QWERTY: macOS reports cmd-s for cmd-shift-s.
    assert_eq!(keystroke_from_parts(&parts(true, true, ["с", "С", "s", "s"], false)).as_deref(), Some("cmd-shift-s"));
    // Czech: shifted numbers.
    assert_eq!(keystroke_from_parts(&parts(true, false, ["ý", "7", "ý", "7"], false)).as_deref(), Some("cmd-ý"));
    assert_eq!(keystroke_from_parts(&parts(true, true, ["ý", "7", "ý", "7"], false)).as_deref(), Some("cmd-7"));
    // Norwegian: cmd-shift-7 is cmd-/.
    assert_eq!(keystroke_from_parts(&parts(true, true, ["7", "/", "7", "7"], false)).as_deref(), Some("cmd-/"));
    // Russian: cmd layout wins.
    assert_eq!(keystroke_from_parts(&parts(true, true, ["7", ".", "7", "&"], true)).as_deref(), Some("cmd-&"));
    // German: the shift special case is a-z only.
    assert_eq!(keystroke_from_parts(&parts(true, true, ["ö", "Ö", "ö", "Ö"], false)).as_deref(), Some("cmd-Ö"));
}

#[test]
fn fn_is_dropped_on_function_keys_only() {
    let mut up = parts(false, false, ["\u{f700}"; 4], false);
    up.function = true;
    assert_eq!(keystroke_from_parts(&up).as_deref(), Some("up"));
    let mut a = parts(false, false, ["a", "A", "a", "A"], false);
    a.function = true;
    assert_eq!(keystroke_from_parts(&a).as_deref(), Some("fn-a"));
}

#[test]
fn every_recorded_key_parses() {
    // What the recorder produces must pass the grammar of Task 1.
    for p in [parts(true, true, ["0", ")", "0", ")"], false), parts(true, true, ["ö", "Ö", "ö", "Ö"], false)] {
        let key = keystroke_from_parts(&p).unwrap();
        assert!(parse_key(&key).is_ok(), "{key}");
    }
    assert_eq!(keystroke_from_parts(&KeyParts::default()), None);
}
```

- [ ] **Step 2: Run, expect FAIL**

Run: `cargo test -p calm-core keymap`
Expected: compile error, `KeyParts` and `keystroke_from_parts` not found.

- [ ] **Step 3: Implement `keystroke_from_parts(parts: &KeyParts) -> Option<String>`**

The named-key table mirrors `events.rs:359-420` (including `0x19` to `tab`, `0x03` to `enter`). The rest mirrors `events.rs:421-480`:

```rust
let first = parts.chars_ignoring_modifiers.chars().next()?;
let function = parts.function && !('\u{f700}'..='\u{f747}').contains(&first);
let mut shift = parts.shift;
let key = match named_key(first) {
    Some(name) => name.to_string(),
    None => {
        let mut plain = parts.chars_plain.clone();
        let mut shifted = parts.chars_shift.clone();
        if parts.cmd || parts.always_use_cmd_layout {
            if parts.chars_cmd_shift != parts.chars_cmd {
                shifted = parts.chars_cmd_shift.clone();
            } else if parts.chars_cmd.to_ascii_uppercase() != parts.chars_cmd {
                shifted = parts.chars_cmd.to_ascii_uppercase();
            }
            plain = parts.chars_cmd.clone();
        }
        if shift && plain.chars().all(|c| c.is_ascii_lowercase()) {
            plain
        } else if shift {
            shift = false;
            shifted
        } else {
            plain
        }
    }
};
// Empty key: None. Otherwise KeySpec { .. }.canonical().
```

- [ ] **Step 4: Run, expect PASS**

Run: `cargo test -p calm-core keymap`

- [ ] **Step 5: Mutation RED**

One at a time, revert after each: drop the `is_ascii_lowercase` shift branch (always keep shift): `us_layout_matches_gpui` fails on `cmd-)`. Drop the cmd-layout block: Dvorak line fails. Drop the Ukrainian `else if`: Ukrainian line fails. Drop the function-range check: `fn_is_dropped_on_function_keys_only` fails.

- [ ] **Step 6: Commit**

```bash
git add crates/calm-core/src/keymap.rs
git commit -m "feat: mirror GPUI's macOS keystroke rule in calm-core"
```

---

### Task 3: `keybindings` settings and the Keys form section (calm-core)

**Files:**
- Modify: `crates/calm-core/src/settings.rs:71-87` (field), `:219-224` (pattern for the field error), `Settings::validate` (`:452`)
- Modify: `crates/calm-core/src/settings_form.rs` (`Control` `:23`, `FieldValue` `:72`, `FormOutcome` `:103`, stats label `:354`, `settings_form` `:167`, `apply_field` `:516`, test helper `sample_value` `:1140`)
- Modify (compile only): `crates/calm-native/src/settings_window.rs` `build_control` (`:435`), `crates/calm-app/src/settings_menu.rs:200-247`, `crates/calm-app/src/panel_settings.rs:124-146`
- Test: `settings.rs` and `settings_form.rs` test modules

Lead's breakdown put the "Show stats" label in Task 4. It lives in `calm-core/src/settings_form.rs:354`, so it moves here.

**Interfaces:**
- Consumes: `KeyOverrides`, `ACTIONS`, `action`, `effective_key`, `check_rebind`, `validate_overrides`, `parse_key`, `shows_in_menu`, `display_key`, `BROWSER_KEYS`, `RESTART_KEY`, `KeyError` (Task 1).
- Produces:

```rust
// settings.rs
pub struct Settings { /* existing */ #[serde(deserialize_with = "keybindings_field")] pub keybindings: KeyOverrides }
fn keybindings_field<'de, D: serde::Deserializer<'de>>(d: D) -> Result<KeyOverrides, D::Error>; // prefixes "\"keybindings\": "

// settings_form.rs
pub enum Control { /* existing */ Key { value: Option<String>, default: Option<String> } }
pub enum FieldValue { /* existing */ Key(Option<String>) }
pub enum FormOutcome { /* existing */ KeyRefused { action: &'static str, reason: KeyError } } // stays Copy
pub const KEYS_SECTION: &str = "Keys";
```

Keys section (last section, so the last tab): for each `Group` in order a header row `note("keys.group.<lowercase title>", <title>, String::new())`, then one row per action of that group: id `keys.<action id>`, label `<label>` plus ` (not shown in the menu)` when `in_menu` and the effective key fails `shows_in_menu`, control `Control::Key { value: effective, default }`, state `FieldState::Set` when the id is in `keybindings`, else `Plain`. Then header `note("keys.group.browser", "Browser (fixed)", String::new())`, one `note("keys.browser.<index>", label, display_key(key))` per `BROWSER_KEYS` entry and `note("keys.terminal.restart", RESTART_KEY.1, display_key(RESTART_KEY.0))`.

`apply_field` for `keys.<id>`: `Key(Some(k))` runs `check_rebind`; an error returns `Ok(FormOutcome::KeyRefused { action: spec.id, reason })` and changes nothing. A key equal (as `KeySpec`) to the default removes the override, else stores `Some(k)`. `Key(None)` stores `None`, or removes the override when the default is `None`. `Reset` removes it. All three return `Changed { rebuild: true }`. Other values are `mismatch()`; an unknown action id is `unknown settings field`.

Stats label: `Show stats ({display_key(k)})` when `developer.toggle_stats` is bound, `Show stats` when not.

Compile-only arms (final behavior comes in Tasks 4 and 5): `build_control` draws `Control::Key` as a read-only label of `display_key(value)` and binds nothing; `settings_menu.rs` logs `calm: settings window: {}` with `reason.message()`; `panel_settings.rs` logs `calm: panel settings: unexpected key refusal`.

- [ ] **Step 1: Write the failing tests**

In `settings.rs` tests:

```rust
#[test]
fn keybindings_load_overrides_and_null_unbinds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, r#"{"version": 2, "keybindings": {"panel.delete": "cmd-d", "canvas.draw_browser": null}}"#).unwrap();
    let loaded = read_settings_from(&path).unwrap();
    assert_eq!(loaded.keybindings.get("panel.delete"), Some(&Some("cmd-d".to_string())));
    assert_eq!(loaded.keybindings.get("canvas.draw_browser"), Some(&None));
    assert_eq!(loaded.keybindings.len(), 2);
    let json = serde_json::to_value(&loaded).unwrap();
    assert_eq!(json["keybindings"], serde_json::json!({"canvas.draw_browser": null, "panel.delete": "cmd-d"}));
    std::fs::write(&path, r#"{"version": 2}"#).unwrap();
    assert!(read_settings_from(&path).unwrap().keybindings.is_empty());
}

#[test]
fn each_bad_keybinding_is_its_own_settings_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    for (entry, field, says) in [
        (r#""canvas.nope": "cmd-k""#, "keybindings.canvas.nope", "unknown action"),
        (r#""panel.delete": "Cmd-x""#, "keybindings.panel.delete", "not a key"),
        (r#""panel.delete": "cmd""#, "keybindings.panel.delete", "only modifiers"),
        (r#""panel.delete": "cmd-shift-,""#, "keybindings.panel.delete", "never fires"),
        (r#""panel.delete": "cmd-r""#, "keybindings.panel.delete", "browser key (Reload)"),
        (r#""canvas.draw_terminal": "space""#, "keybindings.canvas.draw_terminal", "hold-to-pan"),
        (r#""panel.delete": "cmd-=""#, "keybindings.panel.delete", "Used by Zoom in"),
    ] {
        std::fs::write(&path, format!(r#"{{"version": 2, "keybindings": {{{entry}}}}}"#)).unwrap();
        let error = read_settings_from(&path).unwrap_err();
        assert!(matches!(&error, SettingsError::Malformed { .. }), "{entry}: {error:?}");
        let text = error.to_string();
        assert!(text.contains(field) && text.contains(says), "{entry}: {text}");
        assert!(load_settings_from(&path).is_err(), "{entry}");
    }
}

#[test]
fn keybindings_with_wrong_types_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    for text in [
        r#"{"version": 2, "keybindings": 1}"#,
        r#"{"version": 2, "keybindings": null}"#,
        r#"{"version": 2, "keybindings": {"panel.delete": 1}}"#,
    ] {
        std::fs::write(&path, text).unwrap();
        let error = read_settings_from(&path).unwrap_err();
        assert!(error.to_string().contains("keybindings"), "{text}: {error}");
    }
}
```

In `settings_form.rs` tests:

```rust
fn key_field<'a>(form: &'a [Section], id: &str) -> &'a Field {
    form.iter().flat_map(|s| &s.fields).find(|f| f.id == id).unwrap()
}

#[test]
fn keys_tab_lists_every_action_with_its_key() {
    let form = settings_form(&Settings::default(), &FormContext::default());
    let keys = form.iter().find(|s| s.title == KEYS_SECTION).unwrap();
    assert_eq!(form.last().unwrap().title, KEYS_SECTION);
    for a in crate::keymap::ACTIONS {
        let field = key_field(&form, &format!("keys.{}", a.id));
        assert_eq!(field.control, Control::Key { value: a.default.map(String::from), default: a.default.map(String::from) });
        assert_eq!(field.state, FieldState::Plain);
    }
    let browser: Vec<&Field> = keys.fields.iter().filter(|f| f.id.starts_with("keys.browser.")).collect();
    assert_eq!(browser.len(), 10);
    assert!(browser.iter().all(|f| matches!(f.control, Control::Note(_))));
    assert!(keys.fields.iter().any(|f| f.label == "Browser (fixed)"));
    assert_eq!(key_field(&form, "keys.terminal.restart").control, Control::Note("↩".into()));
}

#[test]
fn recording_a_used_key_is_refused_and_changes_nothing() {
    let mut settings = Settings::default();
    let outcome = apply_field(&mut settings, "keys.panel.delete", FieldValue::Key(Some("cmd-=".into())));
    assert_eq!(outcome, Ok(FormOutcome::KeyRefused { action: "panel.delete", reason: KeyError::UsedBy("Zoom in") }));
    assert_eq!(KeyError::UsedBy("Zoom in").message(), "Used by Zoom in");
    assert_eq!(settings, Settings::default());
    let outcome = apply_field(&mut settings, "keys.panel.delete", FieldValue::Key(Some("cmd-r".into())));
    assert!(matches!(outcome, Ok(FormOutcome::KeyRefused { reason: KeyError::Reserved("Reload"), .. })));
}

#[test]
fn recording_the_same_or_default_key_is_not_a_conflict() {
    let mut settings = Settings::default();
    assert_eq!(apply_field(&mut settings, "keys.panel.delete", FieldValue::Key(Some("cmd-shift-backspace".into()))), Ok(FormOutcome::Changed { rebuild: true }));
    assert!(settings.keybindings.is_empty());
    apply_field(&mut settings, "keys.panel.delete", FieldValue::Key(Some("cmd-d".into()))).unwrap();
    assert_eq!(apply_field(&mut settings, "keys.panel.delete", FieldValue::Key(Some("cmd-d".into()))), Ok(FormOutcome::Changed { rebuild: true }));
    assert_eq!(settings.keybindings.get("panel.delete"), Some(&Some("cmd-d".into())));
}

#[test]
fn clear_and_reset_a_key() {
    let mut settings = Settings::default();
    apply_field(&mut settings, "keys.panel.delete", FieldValue::Key(None)).unwrap();
    assert_eq!(settings.keybindings.get("panel.delete"), Some(&None));
    let form = settings_form(&settings, &FormContext::default());
    assert_eq!(key_field(&form, "keys.panel.delete").state, FieldState::Set);
    apply_field(&mut settings, "keys.panel.delete", FieldValue::Reset).unwrap();
    assert!(settings.keybindings.is_empty());
    apply_field(&mut settings, "keys.app.about", FieldValue::Key(None)).unwrap();
    assert!(settings.keybindings.is_empty(), "clearing an unbound default stores nothing");
    assert!(apply_field(&mut settings, "keys.nope", FieldValue::Key(None)).is_err());
    assert!(apply_field(&mut settings, "keys.panel.delete", FieldValue::Bool(true)).is_err());
}

#[test]
fn show_stats_label_follows_the_binding() {
    let mut settings = Settings::default();
    let label = |s: &Settings| key_field(&settings_form(s, &FormContext::default()), "developer.show_stats").label.clone();
    assert_eq!(label(&settings), "Show stats (⇧⌘F)");
    settings.keybindings.insert("developer.toggle_stats".into(), Some("cmd-k".into()));
    assert_eq!(label(&settings), "Show stats (⌘K)");
    settings.keybindings.insert("developer.toggle_stats".into(), None);
    assert_eq!(label(&settings), "Show stats");
}

#[test]
fn rows_note_keys_the_menu_cannot_show() {
    let mut settings = Settings::default();
    settings.keybindings.insert("canvas.fit_all".into(), Some("cmd-enter".into()));
    settings.keybindings.insert("canvas.draw_terminal".into(), Some("tab".into()));
    let form = settings_form(&settings, &FormContext::default());
    assert_eq!(key_field(&form, "keys.canvas.fit_all").label, "Fit all panels (not shown in the menu)");
    assert_eq!(key_field(&form, "keys.canvas.draw_terminal").label, "Draw a terminal");
}
```

Extend `sample_value`: `Control::Key { .. } => Some(FieldValue::Key(None))`, so `every_control_in_every_form_applies_and_stays_valid` covers the new rows.

- [ ] **Step 2: Run, expect FAIL**

Run: `cargo test -p calm-core settings`
Expected: compile errors for `keybindings`, `Control::Key`, `FieldValue::Key`, `FormOutcome::KeyRefused`, `KEYS_SECTION`.

- [ ] **Step 3: Implement**

`Settings::validate` calls `crate::keymap::validate_overrides(&self.keybindings)?` after the range checks. `Settings::default()` has an empty map. Add the compile-only arms listed above.

- [ ] **Step 4: Run, expect PASS**

Run: `cargo test -p calm-core` then `cargo check -p calm-native -p calm-app --tests`
Expected: all pass; both crates compile.

- [ ] **Step 5: Mutation RED**

One at a time: remove the `validate_overrides` call: `each_bad_keybinding_is_its_own_settings_error` fails. Replace `keybindings_field` with plain derive: `keybindings_with_wrong_types_are_errors` fails on the type messages (no field name). Skip `check_rebind` in `apply_field`: `recording_a_used_key_is_refused_and_changes_nothing` fails. Always store the override: `recording_the_same_or_default_key_is_not_a_conflict` fails. Hard-code `"Show stats (cmd-shift-f)"`: `show_stats_label_follows_the_binding` fails. Drop the menu suffix: `rows_note_keys_the_menu_cannot_show` fails.

- [ ] **Step 6: Commit**

```bash
git add crates/calm-core/src/settings.rs crates/calm-core/src/settings_form.rs crates/calm-native/src/settings_window.rs crates/calm-app/src/settings_menu.rs crates/calm-app/src/panel_settings.rs
git commit -m "feat: store key binding overrides in settings with a Keys form section"
```

---

### Task 4: Build the keymap from settings and rebind live (calm-app)

**Files:**
- Create: `crates/calm-app/src/keymap.rs`; declare `mod keymap;` in `main.rs` after `mod grid_paint;`
- Modify: `crates/calm-app/src/canvas_view.rs:102-126` (remove `CONFIRM_TITLE` and `key_bindings`), `:2859`, `:2970` (use the view's confirm title), `:708-713` (`apply_settings_to_input`), tests `:3361-3417` (move to `keymap.rs`)
- Modify: `crates/calm-app/src/dev_menu.rs:14-20` (remove `key_bindings`)
- Modify: `crates/calm-app/src/settings_menu.rs:32-35` (remove `key_bindings`), `:67-70` (remove `install`), `:134` (`apply` instead of `set_menus`), test `:375-394`
- Modify: `crates/calm-app/src/main.rs:2254-2256` and `:2270`

**Interfaces:**
- Consumes: `keymap::{ACTIONS, Scope, bound_keys, parse_key, passes_to_terminal, confirm_prompt}` (Task 1), `Settings.keybindings` (Task 3).
- Produces:

```rust
pub fn gpui_action(id: &str) -> Option<Box<dyn gpui::Action>>;
pub fn key_bindings(settings: &Settings) -> Vec<gpui::KeyBinding>;
/// Clears the keymap, binds `key_bindings(settings)`, then sets the menus so their keys follow.
pub fn apply(cx: &mut gpui::App, settings: &Settings, show_stats: bool);
// canvas_view.rs
pub struct CanvasView { /* existing */ confirm_title: String }
```

Id to action: `canvas.zoom_in ZoomIn`, `canvas.zoom_out ZoomOut`, `canvas.zoom_reset ZoomReset`, `canvas.center_origin CenterOnOrigin`, `canvas.fit_all FitAll`, `canvas.draw_terminal DrawTerminal`, `canvas.draw_browser DrawBrowser`, `canvas.cancel Cancel`, `canvas.confirm ConfirmEnter`, `view.fullscreen ToggleFullscreen`, `panel.delete DeletePanel`, `panel.settings OpenPanelSettings`, `developer.toggle_stats ToggleFps`, `developer.reset_fps ResetFps`, `view.show_grid ToggleGrid`, `app.settings OpenSettings`, `app.open_config_folder OpenConfigFolder`, `app.reset_settings ResetSettings`, `app.about About`, `app.quit Quit`.

`key_bindings`: every `bound_keys` entry first, as `KeyBinding::load(key, action, predicate, false, None, &gpui::DummyKeyboardMapper)` with predicate `canvas_view::KEY_CONTEXT` for `Scope::Canvas` and `None` for `Scope::Global` (`binding.rs:48`; `KeyBinding::new` panics on a parse error and takes no boxed action). Then one `NoAction` binding in `terminal::KEY_CONTEXT` per bound key that `passes_to_terminal`. A key that fails to load is logged with its id and skipped (settings validation makes this unreachable).

`apply`: `cx.clear_key_bindings(); cx.bind_keys(key_bindings(settings)); cx.set_menus(settings_menu::menus(show_stats, settings));` (`app.rs:2242,2248`). Called in `main.rs` right after `startup_settings` (replacing the three `bind_keys` lines and `settings_menu::install`), and in `update_settings` as `cx.defer(move |cx| keymap::apply(cx, &settings, show_stats))` in place of the deferred `set_menus`. `set_show_stats` keeps its plain `set_menus` (keys unchanged). `confirm_title` is set in `apply_settings_to_input` from `confirm_prompt(&settings.keybindings)`.

- [ ] **Step 1: Write the fit_all proof test first, against the base code**

In `canvas_view.rs` tests, before any other change:

```rust
#[test]
fn fit_all_matches_the_press_gpui_reports_for_cmd_shift_0() {
    // events.rs:466-476: with Cmd held GPUI drops Shift for "0" and reports ")".
    let typed = gpui::Keystroke::parse("cmd-)").unwrap();
    let old = gpui::KeyBinding::new("cmd-shift-0", FitAll, Some(KEY_CONTEXT));
    assert_eq!(old.match_keystrokes(std::slice::from_ref(&typed)), None, "cmd-shift-0 never matches");
    assert!(key_bindings().iter().any(|b| b.match_keystrokes(std::slice::from_ref(&typed)) == Some(false) && b.action().partial_eq(&FitAll)));
}
```

Run: `cargo test -p calm-app fit_all_matches`
Expected: FAIL at the last assertion (the first assertion passes, which proves the bug). Record the output for `.ai/decisions.md`.

- [ ] **Step 2: Write the keymap tests in `keymap.rs`**

Helper: `fn matches(b: &KeyBinding, key: &str) -> bool { b.match_keystrokes(&[Keystroke::parse(key).unwrap()]) == Some(false) }` and `fn ctx(b: &KeyBinding) -> Option<String> { b.predicate().map(|p| p.to_string()) }`.

```rust
#[test]
fn every_registered_calm_action_has_one_table_row() {
    let ours: Vec<&str> = ACTIONS.iter().map(|a| gpui_action(a.id).expect(a.id).name()).collect();
    for data in gpui::generate_list_of_all_registered_actions().filter(|d| d.name.starts_with("calm::")) {
        assert_eq!(ours.iter().filter(|n| **n == data.name).count(), 1, "{}", data.name);
    }
    assert_eq!(ours.len(), 20);
}

#[test]
fn default_bindings_match_the_table() {
    let bindings = key_bindings(&Settings::default());
    for a in ACTIONS {
        let action = gpui_action(a.id).unwrap();
        let found: Vec<&KeyBinding> = bindings.iter().filter(|b| b.action().partial_eq(&*action)).collect();
        match a.default {
            Some(key) => {
                assert_eq!(found.len(), 1, "{}", a.id);
                assert!(matches(found[0], key), "{}", a.id);
                let want = (a.scope == Scope::Canvas).then(|| canvas_view::KEY_CONTEXT.to_string());
                assert_eq!(ctx(found[0]), want, "{}", a.id);
            }
            None => assert!(found.is_empty(), "{}", a.id),
        }
    }
}

#[test]
fn fit_all_matches_the_press_gpui_reports_for_cmd_shift_0() { /* the Step 1 test, now on key_bindings(&Settings::default()) */ }

#[test]
fn a_rebound_action_matches_the_new_key_and_not_the_old() {
    let mut settings = Settings::default();
    settings.keybindings.insert("panel.delete".into(), Some("cmd-d".into()));
    let bindings = key_bindings(&settings);
    assert!(bindings.iter().any(|b| matches(b, "cmd-d") && b.action().partial_eq(&DeletePanel)));
    assert!(!bindings.iter().any(|b| matches(b, "cmd-shift-backspace")));
}

#[test]
fn an_unbound_action_has_no_binding_and_frees_its_key_in_terminals() {
    let mut settings = Settings::default();
    settings.keybindings.insert("canvas.draw_browser".into(), None);
    let bindings = key_bindings(&settings);
    assert!(!bindings.iter().any(|b| b.action().partial_eq(&DrawBrowser)));
    assert!(!bindings.iter().any(|b| matches(b, "b")));
}

#[test]
fn terminal_noaction_exactly_for_keys_without_cmd_or_ctrl() {
    let mut settings = Settings::default();
    settings.keybindings.insert("canvas.zoom_in".into(), Some("f5".into()));
    let blocked: BTreeSet<String> = key_bindings(&settings).iter()
        .filter(|b| b.action().partial_eq(&NoAction))
        .inspect(|b| assert_eq!(ctx(b).as_deref(), Some(terminal::KEY_CONTEXT)))
        .map(|b| ["t", "b", "escape", "enter", "f5"].into_iter().find(|k| matches(b, k)).expect("unexpected NoAction").to_string())
        .collect();
    assert_eq!(blocked, ["b", "enter", "escape", "f5", "t"].map(String::from).into());
}
```

Move `panel_settings_shortcut_is_available_with_terminal_focus` and `delete_panel_shortcut_is_available_with_terminal_focus` from `canvas_view.rs:3361-3417` into `keymap.rs` unchanged except `key_bindings()` and `settings_menu::key_bindings()` become `key_bindings(&Settings::default())`. In `settings_menu.rs:388-393` replace `key_bindings()` with `crate::keymap::key_bindings(&Settings::default())`.

Add in `canvas_view.rs` tests:

```rust
#[test]
fn confirm_title_names_the_bound_keys() {
    let mut settings = Settings::default();
    assert_eq!(calm_core::keymap::confirm_prompt(&settings.keybindings), "Close? ↩ / ⎋");
    settings.keybindings.insert("canvas.confirm".into(), Some("y".into()));
    assert_eq!(calm_core::keymap::confirm_prompt(&settings.keybindings), "Close? Y / ⎋");
}
```

This pins the call the view makes; the paint sites read `self.confirm_title`, which `apply_settings_to_input` fills.

- [ ] **Step 3: Run, expect FAIL**

Run: `cargo test -p calm-app keymap`
Expected: compile error, `crate::keymap` items not found.

- [ ] **Step 4: Implement `crates/calm-app/src/keymap.rs` and the call site changes above**

Remove the three old `key_bindings()` functions and `settings_menu::install`; `rg -n "key_bindings\(\)|settings_menu::install|CONFIRM_TITLE" crates/calm-app` must return nothing afterwards.

- [ ] **Step 5: Run, expect PASS**

Run: `cargo test -p calm-app`
Expected: all pass, including the moved shortcut tests and `every_menu_entry_opens_settings_through_one_action`.

- [ ] **Step 6: Mutation RED**

One at a time: drop the `NoAction` loop: `terminal_noaction_exactly_for_keys_without_cmd_or_ctrl` and the moved panel tests' counterpart fail. Bind every action with `None` context: `default_bindings_match_the_table` fails on the context. Set the fit_all table default to `cmd-shift-0` (Task 1 file): `fit_all_matches_the_press_gpui_reports_for_cmd_shift_0` fails. Remove one arm of `gpui_action`: `every_registered_calm_action_has_one_table_row` fails. Use defaults instead of `bound_keys(&settings.keybindings)`: `a_rebound_action_matches_the_new_key_and_not_the_old` fails.

- [ ] **Step 7: Commit**

```bash
git add crates/calm-app/src/keymap.rs crates/calm-app/src/main.rs crates/calm-app/src/canvas_view.rs crates/calm-app/src/dev_menu.rs crates/calm-app/src/settings_menu.rs
git commit -m "feat: build key bindings from settings and rebind them live"
```

---

### Task 5: Recorder control in the Settings window (calm-native)

**Files:**
- Modify: `crates/calm-native/src/settings_window.rs` (`BindingKind` `:56`, `TargetIvars` `:75`, `FormTarget::read` `:136`, `build_control` `:435`, `set_form` `:276`, `hide` `:591`, `close` `:597`, new `Drop`)
- Modify: `crates/calm-native/Cargo.toml` (objc2-foundation features `NSNotification`, `NSOperation`)
- Modify: `crates/calm-native/src/browser.rs` tests (`:505-518`)
- Modify: `crates/calm-app/src/settings_menu.rs` (`KeyRefused` arm), `crates/calm-app/src/main.rs:2196-2201` (smoke binding count)

**Interfaces:**
- Consumes: `KeyParts`, `keystroke_from_parts` (Task 2); `display_key`, `BROWSER_KEYS`, `parse_key` (Task 1); `Control::Key`, `FieldValue::Key`, `FormOutcome::KeyRefused` (Task 3).
- Produces:

```rust
pub const RECORDING_TITLE: &str = "Press a shortcut…";
pub const CLEAR_TITLE: &str = "Clear";
pub const UNBOUND_TITLE: &str = "None";

#[derive(Debug, PartialEq, Eq)]
pub enum RecorderStep { Cancel, Ignore, Report(String) }
/// Escape with no modifier cancels; no key ignores; anything else reports.
pub fn recorder_step(parts: &KeyParts) -> RecorderStep;
/// Pure: the parts of one key-down, given its flags, `charactersIgnoringModifiers` and
/// a lookup of the key code's characters under given flags.
pub fn parts_from(flags: NSEventModifierFlags, chars_ignoring: &str, chars_with: impl Fn(NSEventModifierFlags) -> String, always_use_cmd_layout: bool) -> KeyParts;
pub fn recorder_title(value: Option<&str>) -> String; // display_key or UNBOUND_TITLE

impl SettingsWindow {
    pub fn start_recording(&self, id: &str) -> bool;       // also the recorder button's click
    pub fn recording_field(&self) -> Option<String>;
    pub fn show_field_message(&self, id: &str, text: &str);
    pub fn clear_field_message(&self, id: &str);           // back to recorder_title(current value)
}
```

Row layout for `Control::Key`: recorder `NSButton` at `CONTROL_X` with `CONTROL_WIDTH`, title `recorder_title(value)`, kind `BindingKind::KeyRecorder { value: Option<String> }`. Clear button at `VALUE_X` with `VALUE_WIDTH`, title `CLEAR_TITLE`, kind `BindingKind::KeyClear` reporting `FieldValue::Key(None)`, disabled when `value` is `None`. Reset is the existing `FieldState::Set` button. Replace the Task 3 compile-only arm.

Recording, in `FormTarget`:
- Click on a recorder stops any recording, sets the title to `RECORDING_TITLE`, installs a local `NSEventMask::KeyDown` monitor (pattern: `browser.rs:320-345`) and a `NSWindowDidResignKeyNotification` observer for this window (`NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock`).
- The monitor passes through events whose `window()` is not the settings window. For this window it swallows the event (returns null) and acts on `recorder_step(&parts_from(...))`: `Cancel` stops; `Ignore` keeps waiting; `Report(key)` stops and queues `(id, FieldValue::Key(Some(key)))` through the same pending queue `changed` uses.
- `chars_with(flags)` is `event.charactersByApplyingModifiers(flags)`. `always_use_cmd_layout` mirrors `events.rs:499-505` on a key-code-0 event built with `NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode` (never posted).
- One `stop_recording(&self)` removes the monitor (`NSEvent::removeMonitor`) and the observer (`removeObserver`), and restores the title. It runs on: Escape, a reported key, resign key, a second recorder click, `set_form`, `hide`, `close`, and `Drop for SettingsWindow`. Each `unsafe` block gets a SAFETY comment.

App side (`settings_menu.rs`): `Ok(FormOutcome::KeyRefused { action, reason })` calls `settings_window.show_field_message(&format!("keys.{action}"), &reason.message())`, then a GPUI timer of `KEY_MESSAGE_SECS: u64 = 3` calls `clear_field_message` for the same id.

Smoke count (`main.rs:2196-2201`): a `Control::Key` field adds 2 bindings (recorder and Clear).

- [ ] **Step 1: Write the failing tests**

In `settings_window.rs` tests:

```rust
#[test]
fn escape_alone_cancels_and_escape_with_cmd_records() {
    let escape = KeyParts { chars_ignoring_modifiers: "\u{1b}".into(), ..KeyParts::default() };
    assert_eq!(recorder_step(&escape), RecorderStep::Cancel);
    let cmd_escape = KeyParts { cmd: true, ..escape.clone() };
    assert_eq!(recorder_step(&cmd_escape), RecorderStep::Report("cmd-escape".into()));
    assert_eq!(recorder_step(&KeyParts::default()), RecorderStep::Ignore);
}

#[test]
fn parts_from_reads_each_flag_and_each_layer() {
    use objc2_app_kit::NSEventModifierFlags as F;
    let chars = |f: F| match (f.contains(F::Command), f.contains(F::Shift)) {
        (false, false) => "0", (false, true) => ")", (true, false) => "0", (true, true) => ")",
    }.to_string();
    let p = parts_from(F::Command | F::Shift, "0", chars, false);
    assert!(p.cmd && p.shift && !p.ctrl && !p.alt && !p.function);
    assert_eq!((p.chars_plain.as_str(), p.chars_shift.as_str(), p.chars_cmd.as_str(), p.chars_cmd_shift.as_str()), ("0", ")", "0", ")"));
    assert_eq!(keystroke_from_parts(&p).as_deref(), Some("cmd-)"));
    let p = parts_from(F::Control | F::Option | F::Function, "a", |_| "a".into(), false);
    assert!(p.ctrl && p.alt && p.function && !p.cmd);
}

#[test]
fn recorder_shows_symbols_or_none() {
    assert_eq!(recorder_title(Some("cmd-shift-backspace")), "⇧⌘⌫");
    assert_eq!(recorder_title(None), UNBOUND_TITLE);
}
```

In `browser.rs` tests:

```rust
#[test]
fn reserved_browser_keys_match_the_monitor() {
    for (key, _) in calm_core::keymap::BROWSER_KEYS {
        let spec = calm_core::keymap::parse_key(key).unwrap();
        assert!(spec.cmd && !spec.ctrl && !spec.alt, "{key}");
        assert!(browser_key(&spec.key, spec.shift).is_some(), "{key}");
    }
    assert_eq!(calm_core::keymap::BROWSER_KEYS.len(), 10);
}
```

- [ ] **Step 2: Run, expect FAIL**

Run: `cargo test -p calm-native`
Expected: compile error, `recorder_step`, `parts_from`, `recorder_title` not found.

- [ ] **Step 3: Implement the interfaces, the recorder, Clear, messages, the app arm and the smoke count above**

- [ ] **Step 4: Run, expect PASS**

Run: `cargo test -p calm-native` then `cargo test -p calm-app`

- [ ] **Step 5: Mutation RED**

One at a time: drop the Escape check in `recorder_step`: `escape_alone_cancels_and_escape_with_cmd_records` fails. Swap the Shift and Cmd layers in `parts_from`: `parts_from_reads_each_flag_and_each_layer` fails. Remove `("r", false)` from `browser_key`: `reserved_browser_keys_match_the_monitor` fails.

- [ ] **Step 6: Commit**

```bash
git add crates/calm-native/Cargo.toml crates/calm-native/src/settings_window.rs crates/calm-native/src/browser.rs crates/calm-app/src/settings_menu.rs crates/calm-app/src/main.rs
git commit -m "feat: record key bindings in the Settings window"
```

---

### Task 6: Smoke step, docs and the final gate

**Files:**
- Modify: `crates/calm-app/src/smoke_runner.rs:30-56` (add `step("keybindings", &[], &[])` right after `settings_dispatch`)
- Modify: `crates/calm-app/src/main.rs` (new `async fn smoke_keybindings`, called right after the `settings_dispatch` step, `:684-689`)
- Modify: `README.md:30,112-130`, `CLAUDE.md` ("In the app"), `docs/architecture.md:82`, `docs/dev-setup.md:35-38`, `.ai/decisions.md:714-720` plus new entries, `.ai/status.md:24` plus a landed entry

**Interfaces:**
- Consumes: `CanvasView::update_settings`, `keymap::key_bindings` path through `keymap::apply` (Task 4), `SettingsWindow::{start_recording, recording_field, hide}` (Task 5), `calm_core::settings::read_settings_from`.
- Produces: smoke line `smoke: keybindings override=<bool> old_gone=<bool> restored=<bool> recorder_stops=<bool>`.

`smoke_keybindings`: write `{"version": 2, "keybindings": {"panel.delete": "cmd-d"}}` to `keys.json` in the smoke temp dir, `read_settings_from` it, apply its `keybindings` through `update_settings`, wait one `cx.background_executor().timer(Duration::from_millis(100))` for the deferred `apply`, then read `cx.key_bindings().borrow().bindings_for_action(&DeletePanel)`:
- `override`: one binding, matching `Keystroke::parse("cmd-d")`.
- `old_gone`: none matches `cmd-shift-backspace`.
- then clear `keybindings` through `update_settings`, wait again; `restored`: the binding matches `cmd-shift-backspace` again.
- `recorder_stops`: open Settings, `start_recording("keys.panel.delete")` is true, `hide()`, then `recording_field()` is `None`.
Any false calls `smoke_fail` naming the field. No key events are sent.

- [ ] **Step 1: Register the step and write `smoke_keybindings` (failing first)**

Write the step with the `override` check before Task 4's live `apply` is reached in `update_settings` by temporarily reverting that one line to `set_menus`.

Run: `CALM_SMOKE_ONLY=keybindings cargo run -p calm-app -- --smoke; echo rc=$?`
Expected: `FAIL ... keybindings` on `override`, rc=1. Restore the line.

- [ ] **Step 2: Run the step, expect PASS**

Run: `CALM_SMOKE_ONLY=keybindings cargo run -p calm-app -- --smoke; echo rc=$?`
Expected: `smoke: keybindings override=true old_gone=true restored=true recorder_stops=true`, `smoke: step keybindings PASS`, rc=0.

- [ ] **Step 3: Docs**

- `README.md`: key table rows link to Settings > Keys ("every key below can be changed in Settings > Keys"); Fit all panels shows `cmd-shift-0` as pressed, stored as `cmd-)`; line 30 likewise.
- `CLAUDE.md` "In the app": `cmd-shift-0` text notes it is stored as `cmd-)`; one sentence that every key binding is a setting (`keybindings` in `settings.json`, Keys tab), browser keys and Enter-to-restart are fixed.
- `docs/architecture.md:82`: the `NoAction` overrides are now derived for every binding without Cmd or Ctrl.
- `docs/dev-setup.md:35-38`: add `keybindings` to the step list.
- `.ai/decisions.md`: rewrite the entry at `:714-720` so keys are settings, not hard-coded; add entries: keys are recorded, not typed (why); a conflict refuses, no swap; the derived `NoAction` rule; reserved browser keys and bare space; `fn` with function keys and Shift with symbols are rejected because they never fire; the `fit_all` fix with the Task 4 Step 1 proof output; symbols use Apple's modifier order `⌃⌥⇧⌘`.
- `.ai/status.md:24` and a landed entry naming the smoke line; list the by-eye checks for the user (record a key, use it, see the menu key change).

- [ ] **Step 4: Final gate (once)**

```bash
cargo clean -p calm-core -p calm-native -p calm-app
cargo check --workspace --tests --examples; echo rc=$?
cargo test --workspace; echo rc=$?
gmake lint; echo rc=$?
gmake plugins-check; echo rc=$?
cargo run -p calm-app -- --smoke; echo rc=$?
```

Expected: every rc=0; smoke ends `smoke: summary pass=N fail=0 skipped=0` with the `keybindings` line. A known flake is a failure; run a control on `origin/main` before blaming the branch. Check new text for dashes with a python count of U+2013 and U+2014 over the changed docs (a control file with one of each must count 2).

- [ ] **Step 5: Commit**

```bash
git add crates/calm-app/src/smoke_runner.rs crates/calm-app/src/main.rs README.md CLAUDE.md docs/architecture.md docs/dev-setup.md .ai/decisions.md .ai/status.md
git commit -m "docs: describe editable key bindings and add the keybindings smoke step"
```

After merge-ready: `repo-intel build; echo rc=$?` (new modules).

---

## Re-check after #63

PR #63 (`origin/feat/tui-extends-terminal`, merge base `2d3f2d9`, before #60) must land
before this plan starts. Read on its head, not merged. After it lands, refresh these
citations against the new `origin/main`:

- **Task 4, `canvas_view.rs`.** #63 rewrites the file (+538). On its head: `actions!` at
  `:78`, `CONFIRM_TITLE` at `:107`, `key_bindings` at `:109-128`, `apply_settings_to_input`
  at `:716`, deferred `set_menus` at `:729`, paint uses of `CONFIRM_TITLE` at `:3023` and
  `:3134`, tests at `:3512`. Its `actions!` and `key_bindings` lack `DeletePanel` and
  `OpenPanelSettings` because it branched before #60: confirm the merged file has both,
  and that `actions!` still has exactly the 12 canvas actions. #63 adds no GPUI action
  (Run Command is a context menu item, `calm-core/src/menu.rs`), so the 20-row table holds;
  `every_registered_calm_action_has_one_table_row` will catch it if that changes.
- **Task 4 and 5, `main.rs`.** On #63's head: `bind_keys` lines `:2411-2413`,
  `settings_menu::install` `:2427`, the smoke binding count `:2364`, `settings_dispatch`
  step `:689`.
- **Task 1 and 4, menu action list.** `settings_menu.rs` on #63 only drops
  `apply_terminal_config` and an import (`-8`); `actions!` (`:18`), `key_bindings` (`:31`),
  menu items (`:46-63`) and the deferred `set_menus` (`:133`) shift by one line.
  `dev_menu.rs` is unchanged. Re-run
  `rg -n "MenuItem::action|actions!" crates/calm-app/src` and confirm the five unbound rows
  are still the whole list.
- **Task 3, `settings_form.rs`.** #63 adds `terminal.command_restart` (a Choice in the
  Terminal section) and a test; the Developer section and the stats label line move. The
  Keys section must still be last; tabs map one per section (`settings_window.rs:276-321`,
  untouched by #63).
- **Task 3 and 4, `panel_settings.rs`.** #63 changes it (+/-44); re-find the
  `FormOutcome` match for the compile-only arm.
- **Task 1, restart key.** `terminal/adapter.rs` Enter-restart moves to `:315` on #63 and
  is still the literal `"enter"`. #63's `terminal.command_restart` setting ("On Enter")
  restarts typed commands on Enter too; check whether that path has its own key check and,
  if so, cite it next to `RESTART_KEY`.
- **Task 6, smoke.** #63 adds `step("run_command", ...)` after `tui` in `STEPS`; insert
  `keybindings` after `settings_dispatch` as planned. `docs/dev-setup.md` step list and
  `.ai/decisions.md`, `.ai/status.md`, `CLAUDE.md` all change in #63: re-find the lines
  Task 6 edits.
- **Task 5, `calm-native/Cargo.toml`.** #63 adds the `NSAlert` app-kit feature; add the
  foundation features next to it without reordering.
- **Space pan check.** Moves to `canvas_view.rs:2361` on #63; Task 1 cites it for the
  Space rule.
