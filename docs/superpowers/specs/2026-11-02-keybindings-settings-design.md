# Editable key bindings: design

Date: 2026-11-02. Status: draft for the user's review. Builder-checked (route-gpt-sol); the
user answered its two open questions (browser keys, unbound menu commands).

## Intent

The user asked for a Settings tab where every key binding can be changed. Today all 15
bindings are hard-coded `KeyBinding::new` calls in three places (`canvas_view.rs`,
`dev_menu.rs`, `settings_menu.rs`), which breaks the repo rule "Customization first:
keys are settings". The user skipped the question round, so everything below that is not
"every binding, in a Settings tab" is the lead's call and marked as such.

## Goals

- A **Keys** tab in the global Settings window lists every key binding CALM has, one row
  per action: label, the current key, Reset (back to the default), Clear (unbound).
- Every menu command gets a row, including the ones with no shortcut today (Show Grid,
  Reset FPS Stats, Reset Settings to Defaults and any other registered menu action). They
  start unbound. (User's answer.)
- CALM's own browser shortcuts are listed read-only in a "Browser (fixed)" group and
  cannot be changed (user's answer): Cmd+[ back, Cmd+] forward, Cmd+R reload, Cmd+L
  address bar, and the Cmd+C/X/V/A/Z and Cmd+Shift+Z edit keys
  (`calm-native/src/browser.rs:227-238`). They are reserved: no canvas or global binding
  may use them.
- A row's key is changed by **recording**: click the key button, press the new shortcut.
  (Lead's call. Typed text would let the user enter `cmd-shift-,`, which GPUI never matches
  because it reports that press as `cmd-<`. Recording stores what GPUI will report.)
- Changes apply at once, with no restart, and persist in `settings.json`.
- Menu items show the current key (GPUI derives menu key equivalents from the keymap, so
  rebinding plus `set_menus` refreshes them). GPUI's menu builder cannot show `fn`
  combinations, and passes `enter` and `tab` through unconverted
  (`gpui-pre-macos-0.3.4/src/platform.rs:364-402`); such a binding still works but its menu
  item shows no shortcut. The row notes "not shown in the menu" (lead's call).
- Any UI text that names a key follows the binding: the close confirmation
  "Close? enter / esc" and the "Show stats (cmd-shift-f)" checkbox label.

## Non-goals

- Chords (`cmd-k cmd-s`), several keys per action, per-panel bindings.
- Mouse gestures and drag modifiers. `canvas.move_modifier` stays in its tab; the
  snapping modifier (Cmd+Shift after PR #60) stays fixed.
- Plugin key bindings. Those stay with plugin API v2 `keybindings` (spec
  `2026-10-31-plugin-api-v2-design.md`); this design must not block them (see
  Conflicts).
- Keys inside terminals and web pages. CALM does not remap what Ghostty or WKWebView do.
- Rebinding CALM's browser shortcuts or the Enter-to-restart key of an exited terminal
  (`terminal/adapter.rs:304-309`). The latter is listed read-only with the browser group.

## Anti-goals

- A binding that silently never fires. Every stored key must be one GPUI can produce.
- Losing the ability to type in a terminal because a user bound a bare letter.
- A bad `settings.json` entry quietly falling back to defaults. Errors are explicit, as
  for every other settings field (`malformed_file_is_an_explicit_error_not_defaults`).

## Depends on

PR #60 (`feat/delete-panel-shortcut`), which adds the `DeletePanel` and
`OpenPanelSettings` actions and the snapping change. This work branches from it after it
merges, or stacks on it and is retargeted to `main` before merge.

## The action table (calm-core)

New module `calm-core/src/keymap.rs`, pure, no GPUI. One entry per action:

| id | label | default | scope |
|---|---|---|---|
| `canvas.zoom_in` | Zoom in | `cmd-=` | canvas |
| `canvas.zoom_out` | Zoom out | `cmd--` | canvas |
| `canvas.zoom_reset` | Reset zoom | `cmd-0` | canvas |
| `canvas.center_origin` | Center on origin | `cmd-shift-h` | canvas |
| `canvas.fit_all` | Fit all panels | `cmd-)` (see below) | canvas |
| `canvas.draw_terminal` | Draw a terminal | `t` | canvas, typing |
| `canvas.draw_browser` | Draw a browser | `b` | canvas, typing |
| `canvas.cancel` | Cancel / answer No | `escape` | canvas, typing |
| `canvas.confirm` | Confirm close | `enter` | canvas, typing |
| `view.fullscreen` | Toggle fullscreen | `cmd-ctrl-f` | canvas |
| `panel.delete` | Delete focused panel | `cmd-shift-backspace` | canvas |
| `panel.settings` | Focused panel Settings | `cmd-<` | canvas |
| `developer.toggle_stats` | Show stats | `cmd-shift-f` | global |
| `app.settings` | Open Settings | `cmd-,` | global |
| `app.quit` | Quit | `cmd-q` | global |

- "canvas" bindings use the `Canvas` key context, "global" ones no context, exactly as
  today. The scope is fixed per action, not user-editable.
- "typing" marks actions whose default has no Cmd or Ctrl. Rule (lead's call): **any**
  binding without Cmd or Ctrl, whatever its action, also gets a `NoAction` binding in the
  `Terminal` key context, so a terminal keeps that key. This generalizes today's four
  hand-written overrides (`canvas_view.rs:117-120`). Cmd and Ctrl bindings reach the canvas
  through a focused terminal, as zoom does today.
- `canvas.fit_all`: today it is bound `cmd-shift-0`, which by the GPUI rule in PR #60
  (shift dropped for non-letters when Cmd is held) can never match a key press. The
  builder verifies this with the same test style PR #60 uses, and if confirmed the default
  becomes what GPUI reports for Cmd+Shift+0 on a US layout (`cmd-)`), so the shortcut works
  out of the box. Recorded in decisions.md as a fix found by this work.
- Labels in the UI show keys with macOS symbols in Apple's modifier order (`⇧⌘⌫`), produced from the stored string
  by one pure function in calm-core. Shifted punctuation shows as pressed: `cmd-<` shows as
  `⌘<`.

Rows for menu commands without a default (`view.show_grid`, `developer.reset_fps`,
`app.reset_settings`, and every other action the menus register) are added with
`default: None`; the builder takes the exact list from `dev_menu.rs` and
`settings_menu.rs`.

The app maps each id to its GPUI action in one place (`calm-app/src/keymap.rs`), so the
three `key_bindings()` functions go away.

## Settings file

```json
"keybindings": {
  "panel.delete": "cmd-d",
  "canvas.draw_browser": null
}
```

- The section is optional; a missing id keeps its default. `null` means unbound.
- Strict validation in `settings.rs`, each with its own error naming the field:
  unknown id; a value that `gpui`'s keystroke syntax would not parse (the core checks the
  same grammar: modifiers `cmd ctrl alt shift fn` joined by `-`, then one key); a bare
  modifier; `shift` together with a printable non-letter key (it can never fire, because
  GPUI reports the shifted character instead; say so and name the form to use). Named keys
  (backspace, enter, tab, arrows, function keys) keep Shift and are valid with it, so
  `cmd-shift-backspace` passes (`gpui-pre-macos-0.3.4/src/events.rs:352-378`). Also
  rejected: two actions with the same key in overlapping scopes (canvas and global
  overlap); a reserved browser key; bare `space`, which stays the hold-to-pan key
  (`canvas_view.rs:2195`; lead's call).
- Only overrides are written back. Reset removes the id from the section.

## Recording (calm-native plus a pure rule in calm-core)

The Settings window is AppKit; GPUI never sees its key events, and GPUI's conversion
(`platform_input_from_native`) is `pub(crate)`. So:

- calm-core gets `keystroke_from_parts(parts) -> Option<String>`: the pure part of GPUI's
  rule (`gpui-pre-macos-0.3.4/src/events.rs:425-480`: named keys table, the cmd layout
  choice, shift kept only for a-z). Its inputs are what AppKit gives for the event:
  modifier flags, key code, `charactersIgnoringModifiers`, the characters with shift, and
  with cmd and cmd+shift. Unit tests cover the cases in GPUI's own comment table (US,
  Dvorak+QWERTY, Czech, Norwegian, German) and the two we already hit (`cmd-<`,
  `cmd-shift-backspace`). A comment names the GPUI version it mirrors.
- calm-native adds a recorder control: a button showing the key. Clicking it shows
  "Press a shortcut…" and installs a local key-down monitor on the Settings window only.
  The next key-down that is not a bare modifier is turned into parts, sent to the core
  rule and reported as `FieldValue::Key(String)`. Escape alone cancels recording (so a
  user cannot bind bare Escape by recording; `canvas.cancel` gets it back via Reset).
  Losing focus cancels too. The monitor is removed on every exit path.
- Form model: new `Control::Key { value: Option<String>, default: Option<String> }` and
  `FieldValue::Key(Option<String>)`, ids `keys.<action id>`. Reset uses the existing
  `FieldValue::Reset`.

## Conflicts

- Recording a key another action already uses (overlapping scope) is refused; the row
  shows "Used by Zoom in" for a few seconds and keeps its old key. (Lead's call; the
  alternative, swapping, surprises more.)
- Reserved: the browser keys and bare `space` (see Settings file). `cmd-q` and `cmd-,` are
  rebindable; if a user unbinds Settings they still reach it from the app menu.
- When plugin v2 keybindings arrive they must lose to user bindings and say so; this
  design leaves room by keeping the action table the single source of truth.

## Applying a change

`cx.clear_key_bindings()` then `cx.bind_keys(all)` from the current settings, then
`set_menus` so menu shortcuts refresh. Both exist in `gpui-pre-0.3.4/src/app.rs:2242,2248`.
One function builds the full list, used at startup and after every change.

## Testing

- calm-core: table completeness (every GPUI action the app registers in a keymap or a
  menu has exactly one row; the app asserts this too), defaults equal today's keys except the documented fit_all
  fix, validation errors one by one, NoAction derivation for unmodified keys, symbol
  rendering, `keystroke_from_parts` cases.
- calm-app: the built binding list matches the table for defaults and for overrides; a
  rebound `panel.delete` matches the new key and not the old one; an unbound action has no
  binding; Terminal-context NoAction appears exactly for unmodified keys.
- Smoke: a new step loads a settings file with one override and checks the registered
  keymap has it (no synthetic key presses: agents never drive the GUI).
- By-eye list for the user: record a key in the tab, use it, see the menu label change.

## Left to the builder

Row order inside the tab (group by Canvas, Panels, View, App is suggested), symbol choice
for keys without a standard glyph, the exact error strings, and how the "Used by" message
is timed.
