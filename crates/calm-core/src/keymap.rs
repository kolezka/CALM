//! Key bindings as data: every action CALM binds, its default key and the key rules.
//!
//! The rules mirror how `gpui-pre-macos 0.3.4` turns a key press into a keystroke
//! (`events.rs`), so a key accepted here is one GPUI can actually report.

use std::collections::BTreeMap;

/// Changed bindings by action id. `None` means unbound.
pub type KeyOverrides = BTreeMap<String, Option<String>>;

/// Where a binding applies: the canvas key context, or the whole app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Canvas,
    Global,
}

impl Scope {
    /// Whether one key may not be used in both scopes. A new scope must decide here.
    pub fn overlaps(self, other: Scope) -> bool {
        match (self, other) {
            (Scope::Canvas, Scope::Canvas)
            | (Scope::Global, Scope::Global)
            | (Scope::Canvas, Scope::Global)
            | (Scope::Global, Scope::Canvas) => true,
        }
    }
}

/// Where a row sits in the Keys tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Canvas,
    Panels,
    View,
    Developer,
    App,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Canvas => "Canvas",
            Group::Panels => "Panels",
            Group::View => "View",
            Group::Developer => "Developer",
            Group::App => "App",
        }
    }
}

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

const fn row(
    id: &'static str,
    label: &'static str,
    group: Group,
    scope: Scope,
    default: Option<&'static str>,
    in_menu: bool,
) -> ActionSpec {
    ActionSpec {
        id,
        label,
        group,
        scope,
        default,
        in_menu,
    }
}

/// Every action, in the row order of the Keys tab.
pub const ACTIONS: &[ActionSpec] = &[
    row(
        "canvas.draw_terminal",
        "Draw a terminal",
        Group::Canvas,
        Scope::Canvas,
        Some("t"),
        false,
    ),
    row(
        "canvas.draw_browser",
        "Draw a browser",
        Group::Canvas,
        Scope::Canvas,
        Some("b"),
        false,
    ),
    row(
        "canvas.cancel",
        "Cancel / answer No",
        Group::Canvas,
        Scope::Canvas,
        Some("escape"),
        false,
    ),
    row(
        "canvas.confirm",
        "Confirm close",
        Group::Canvas,
        Scope::Canvas,
        Some("enter"),
        false,
    ),
    row(
        "panel.delete",
        "Delete focused panel",
        Group::Panels,
        Scope::Canvas,
        Some("cmd-shift-backspace"),
        false,
    ),
    row(
        "panel.settings",
        "Focused panel Settings",
        Group::Panels,
        Scope::Canvas,
        Some("cmd-<"),
        false,
    ),
    row(
        "panel.move_up",
        "Move panel up",
        Group::Panels,
        Scope::Canvas,
        Some("cmd-alt-up"),
        false,
    ),
    row(
        "panel.move_down",
        "Move panel down",
        Group::Panels,
        Scope::Canvas,
        Some("cmd-alt-down"),
        false,
    ),
    row(
        "panel.move_left",
        "Move panel left",
        Group::Panels,
        Scope::Canvas,
        Some("cmd-alt-left"),
        false,
    ),
    row(
        "panel.move_right",
        "Move panel right",
        Group::Panels,
        Scope::Canvas,
        Some("cmd-alt-right"),
        false,
    ),
    row(
        "panel.move_up_far",
        "Move panel up far",
        Group::Panels,
        Scope::Canvas,
        Some("cmd-alt-shift-up"),
        false,
    ),
    row(
        "panel.move_down_far",
        "Move panel down far",
        Group::Panels,
        Scope::Canvas,
        Some("cmd-alt-shift-down"),
        false,
    ),
    row(
        "panel.move_left_far",
        "Move panel left far",
        Group::Panels,
        Scope::Canvas,
        Some("cmd-alt-shift-left"),
        false,
    ),
    row(
        "panel.move_right_far",
        "Move panel right far",
        Group::Panels,
        Scope::Canvas,
        Some("cmd-alt-shift-right"),
        false,
    ),
    row(
        "canvas.zoom_in",
        "Zoom in",
        Group::View,
        Scope::Canvas,
        Some("cmd-="),
        false,
    ),
    row(
        "canvas.zoom_out",
        "Zoom out",
        Group::View,
        Scope::Canvas,
        Some("cmd--"),
        false,
    ),
    row(
        "canvas.zoom_reset",
        "Reset zoom",
        Group::View,
        Scope::Canvas,
        Some("cmd-0"),
        false,
    ),
    row(
        "canvas.center_origin",
        "Center on origin",
        Group::View,
        Scope::Canvas,
        Some("cmd-shift-h"),
        true,
    ),
    // GPUI reports Cmd+Shift+0 on a US layout as cmd-); cmd-shift-0 never matched.
    row(
        "canvas.fit_all",
        "Fit all panels",
        Group::View,
        Scope::Canvas,
        Some("cmd-)"),
        true,
    ),
    row(
        "view.fullscreen",
        "Toggle fullscreen",
        Group::View,
        Scope::Canvas,
        Some("cmd-ctrl-f"),
        false,
    ),
    row(
        "canvas.pan_up",
        "Pan up",
        Group::View,
        Scope::Canvas,
        Some("up"),
        false,
    ),
    row(
        "canvas.pan_down",
        "Pan down",
        Group::View,
        Scope::Canvas,
        Some("down"),
        false,
    ),
    row(
        "canvas.pan_left",
        "Pan left",
        Group::View,
        Scope::Canvas,
        Some("left"),
        false,
    ),
    row(
        "canvas.pan_right",
        "Pan right",
        Group::View,
        Scope::Canvas,
        Some("right"),
        false,
    ),
    row(
        "canvas.pan_up_far",
        "Pan up far",
        Group::View,
        Scope::Canvas,
        Some("shift-up"),
        false,
    ),
    row(
        "canvas.pan_down_far",
        "Pan down far",
        Group::View,
        Scope::Canvas,
        Some("shift-down"),
        false,
    ),
    row(
        "canvas.pan_left_far",
        "Pan left far",
        Group::View,
        Scope::Canvas,
        Some("shift-left"),
        false,
    ),
    row(
        "canvas.pan_right_far",
        "Pan right far",
        Group::View,
        Scope::Canvas,
        Some("shift-right"),
        false,
    ),
    row(
        "view.show_grid",
        "Show grid",
        Group::View,
        Scope::Global,
        None,
        true,
    ),
    row(
        "developer.toggle_stats",
        "Show stats",
        Group::Developer,
        Scope::Global,
        Some("cmd-shift-f"),
        true,
    ),
    row(
        "developer.reset_fps",
        "Reset FPS stats",
        Group::Developer,
        Scope::Global,
        None,
        true,
    ),
    row(
        "developer.show_welcome",
        "Show welcome screen",
        Group::Developer,
        Scope::Global,
        None,
        true,
    ),
    row(
        "app.save_preset",
        "Save preset",
        Group::App,
        Scope::Global,
        None,
        true,
    ),
    row(
        "app.load_preset",
        "Load preset",
        Group::App,
        Scope::Global,
        None,
        true,
    ),
    row(
        "app.settings",
        "Open Settings",
        Group::App,
        Scope::Global,
        Some("cmd-,"),
        true,
    ),
    row(
        "app.open_config_folder",
        "Open config folder",
        Group::App,
        Scope::Global,
        None,
        true,
    ),
    row(
        "app.reset_settings",
        "Reset settings to defaults",
        Group::App,
        Scope::Global,
        None,
        true,
    ),
    row(
        "app.about",
        "About CALM",
        Group::App,
        Scope::Global,
        None,
        true,
    ),
    row(
        "app.quit",
        "Quit",
        Group::App,
        Scope::Global,
        Some("cmd-q"),
        true,
    ),
];

pub fn action(id: &str) -> Option<&'static ActionSpec> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// A parsed key: which modifiers are held, then one key name or character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySpec {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub function: bool,
    pub key: String,
}

impl KeySpec {
    /// One spelling per key, so equal keys compare equal as text too.
    pub fn canonical(&self) -> String {
        let mut parts = Vec::new();
        for (on, name) in [
            (self.cmd, "cmd"),
            (self.ctrl, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
            (self.function, "fn"),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("-")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    Syntax,
    BareModifier,
    UppercaseLetter,
    ShiftedSymbol,
    FnNeverFires,
    Reserved(&'static str),
    Space,
    UsedBy(&'static str),
    NotInMenu,
}

impl KeyError {
    pub fn message(self) -> String {
        match self {
            KeyError::Syntax => "not a key; write modifiers (cmd, ctrl, alt, shift, fn) and one key joined by \"-\", like \"cmd-shift-h\"".into(),
            KeyError::BareModifier => "only modifiers; add a key, like \"cmd-k\"".into(),
            KeyError::UppercaseLetter => "write \"shift-\" and the lowercase letter, like \"cmd-shift-k\"".into(),
            KeyError::ShiftedSymbol => "shift with this key never fires, because macOS reports the shifted character; write that character, like \"cmd-<\" for Cmd+Shift+comma on a US layout".into(),
            KeyError::FnNeverFires => "fn with this key never fires; macOS sets fn on it already".into(),
            KeyError::Reserved(label) => format!("a fixed browser key ({label})"),
            KeyError::Space => "space alone is the hold-to-pan key".into(),
            KeyError::UsedBy(label) => format!("Used by {label}"),
            KeyError::NotInMenu => "a menu command cannot use fn, enter or tab; the menu would show a different key".into(),
        }
    }
}

/// CALM's own browser shortcuts (`calm-native/src/browser.rs`). Shown, never bindable.
pub const BROWSER_KEYS: &[(&str, &str)] = &[
    ("cmd-[", "Back"),
    ("cmd-]", "Forward"),
    ("cmd-r", "Reload"),
    ("cmd-l", "Address bar"),
    ("cmd-c", "Copy"),
    ("cmd-x", "Cut"),
    ("cmd-v", "Paste"),
    ("cmd-a", "Select all"),
    ("cmd-z", "Undo"),
    ("cmd-shift-z", "Redo"),
];

/// The key an exited terminal restarts on (`terminal/adapter.rs`). Shown, never bindable.
pub const RESTART_KEY: (&str, &str) = ("enter", "Restart an exited terminal");

const MODIFIERS: [&str; 5] = ["cmd", "ctrl", "alt", "shift", "fn"];
const NAMED_KEYS: [&str; 15] = [
    "space",
    "tab",
    "enter",
    "backspace",
    "escape",
    "up",
    "down",
    "left",
    "right",
    "pageup",
    "pagedown",
    "home",
    "end",
    "delete",
    "insert",
];
/// Keys macOS already marks with fn, so GPUI clears it (`events.rs:354-357`).
const FN_KEYS: [&str; 10] = [
    "up", "down", "left", "right", "pageup", "pagedown", "home", "end", "delete", "insert",
];

fn is_function_key(key: &str) -> bool {
    key.strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=35).contains(&n) && !key.starts_with("f0"))
}

fn is_named(key: &str) -> bool {
    NAMED_KEYS.contains(&key) || is_function_key(key)
}

/// The grammar and the never-fires rules.
pub fn parse_key(text: &str) -> Result<KeySpec, KeyError> {
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return Err(KeyError::Syntax);
    }
    let (mods, key): (Vec<&str>, &str) = if text == "-" {
        (Vec::new(), "-")
    } else if let Some(rest) = text.strip_suffix("--") {
        (rest.split('-').collect(), "-")
    } else {
        let mut parts: Vec<&str> = text.split('-').collect();
        let key = parts.pop().unwrap_or_default();
        (parts, key)
    };
    let mut spec = KeySpec {
        cmd: false,
        ctrl: false,
        alt: false,
        shift: false,
        function: false,
        key: key.to_string(),
    };
    for part in mods {
        let slot = match part {
            "cmd" => &mut spec.cmd,
            "ctrl" => &mut spec.ctrl,
            "alt" => &mut spec.alt,
            "shift" => &mut spec.shift,
            "fn" => &mut spec.function,
            _ => return Err(KeyError::Syntax),
        };
        if *slot {
            return Err(KeyError::Syntax);
        }
        *slot = true;
    }
    if MODIFIERS.contains(&key) {
        return Err(KeyError::BareModifier);
    }
    let mut chars = key.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    match single {
        _ if is_named(key) => {}
        Some(c) if !c.is_control() => {
            if c.is_ascii_uppercase() {
                return Err(KeyError::UppercaseLetter);
            }
            // GPUI keeps shift only for a-z; otherwise it reports the shifted character.
            if spec.shift && !c.is_ascii_lowercase() {
                return Err(KeyError::ShiftedSymbol);
            }
        }
        _ => return Err(KeyError::Syntax),
    }
    if spec.function && (FN_KEYS.contains(&key) || is_function_key(key)) {
        return Err(KeyError::FnNeverFires);
    }
    Ok(spec)
}

/// `parse_key` plus the keys CALM keeps for itself.
pub fn check_key(text: &str) -> Result<KeySpec, KeyError> {
    let spec = parse_key(text)?;
    for (key, label) in BROWSER_KEYS {
        if parse_key(key).as_ref() == Ok(&spec) {
            return Err(KeyError::Reserved(label));
        }
    }
    if spec.key == "space" && !spec.cmd && !spec.ctrl && !spec.alt {
        return Err(KeyError::Space);
    }
    Ok(spec)
}

pub fn effective_key(overrides: &KeyOverrides, action: &ActionSpec) -> Option<String> {
    match overrides.get(action.id) {
        Some(value) => value.clone(),
        None => action.default.map(str::to_string),
    }
}

/// Every bound action with its key, in `ACTIONS` order.
pub fn bound_keys(overrides: &KeyOverrides) -> Vec<(&'static ActionSpec, String)> {
    ACTIONS
        .iter()
        .filter_map(|a| effective_key(overrides, a).map(|key| (a, key)))
        .collect()
}

/// `KeyHelpRow` ids for `BROWSER_KEYS`, by position. Ids are `&'static str`, so they are listed.
const BROWSER_IDS: [&str; 10] = [
    "fixed.browser.0",
    "fixed.browser.1",
    "fixed.browser.2",
    "fixed.browser.3",
    "fixed.browser.4",
    "fixed.browser.5",
    "fixed.browser.6",
    "fixed.browser.7",
    "fixed.browser.8",
    "fixed.browser.9",
];
const _: () = assert!(BROWSER_IDS.len() == BROWSER_KEYS.len());

/// One row of the key help a plugin shows: an action or a fixed key, with its key as macOS writes it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct KeyHelpRow {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    /// `None` when the action is unbound.
    pub key: Option<String>,
}

/// Every action in `ACTIONS` order with its effective key, then the fixed browser keys and the
/// restart key under "Fixed".
pub fn key_help(overrides: &KeyOverrides) -> Vec<KeyHelpRow> {
    let actions = ACTIONS.iter().map(|action| KeyHelpRow {
        id: action.id,
        label: action.label,
        group: action.group.title(),
        key: effective_key(overrides, action).map(|key| display_key(&key)),
    });
    let fixed = BROWSER_IDS
        .iter()
        .zip(BROWSER_KEYS)
        .map(|(id, (key, label))| (*id, *key, *label))
        .chain([("fixed.restart", RESTART_KEY.0, RESTART_KEY.1)])
        .map(|(id, key, label)| KeyHelpRow {
            id,
            label,
            group: "Fixed",
            key: Some(display_key(key)),
        });
    actions.chain(fixed).collect()
}

/// Whether `action` may take `key`, given every other action's current key.
pub fn check_rebind(
    overrides: &KeyOverrides,
    action: &ActionSpec,
    key: &str,
) -> Result<KeySpec, KeyError> {
    let spec = check_key(key)?;
    check_menu(action, &spec)?;
    for (other, other_key) in bound_keys(overrides) {
        if other.id != action.id
            && other.scope.overlaps(action.scope)
            && parse_key(&other_key).as_ref() == Ok(&spec)
        {
            return Err(KeyError::UsedBy(other.label));
        }
    }
    Ok(spec)
}

/// GPUI's menu builder drops fn and passes enter and tab through unconverted, so a menu
/// item would get a different key than the binding. Menu commands refuse those keys.
fn check_menu(action: &ActionSpec, key: &KeySpec) -> Result<(), KeyError> {
    if action.in_menu && !shows_in_menu(key) {
        Err(KeyError::NotInMenu)
    } else {
        Ok(())
    }
}

/// The first problem in a `keybindings` section, naming the field.
pub fn validate_overrides(overrides: &KeyOverrides) -> Result<(), String> {
    let mut seen: Vec<(&ActionSpec, KeySpec)> = Vec::new();
    for action in ACTIONS {
        let overridden = overrides.contains_key(action.id);
        let Some(key) = effective_key(overrides, action) else {
            continue;
        };
        let spec = check_key(&key)
            .and_then(|spec| check_menu(action, &spec).map(|()| spec))
            .map_err(|e| {
                format!(
                    "\"keybindings.{}\" is \"{key}\": {}",
                    action.id,
                    e.message()
                )
            })?;
        if let Some((earlier, _)) = seen
            .iter()
            .find(|(o, s)| *s == spec && o.scope.overlaps(action.scope))
        {
            // Blame the overridden side; if both are, the later one.
            let (blamed, other) = if overridden || !overrides.contains_key(earlier.id) {
                (action, *earlier)
            } else {
                (*earlier, action)
            };
            let blamed_key = effective_key(overrides, blamed).unwrap_or_default();
            return Err(format!(
                "\"keybindings.{}\" is \"{blamed_key}\": {}",
                blamed.id,
                KeyError::UsedBy(other.label).message()
            ));
        }
        seen.push((action, spec));
    }
    match overrides.keys().find(|id| action(id).is_none()) {
        Some(id) => Err(format!("\"keybindings.{id}\": unknown action")),
        None => Ok(()),
    }
}

/// A key without Cmd or Ctrl is typing; a focused terminal keeps it.
pub fn passes_to_terminal(key: &KeySpec) -> bool {
    !key.cmd && !key.ctrl
}

/// GPUI's menu builder cannot show fn, and passes enter and tab through unconverted.
pub fn shows_in_menu(key: &KeySpec) -> bool {
    !key.function && key.key != "enter" && key.key != "tab"
}

/// A key as macOS writes it, like `⇧⌘⌫`. Text that does not parse is shown as written.
pub fn display_key(text: &str) -> String {
    let Ok(spec) = parse_key(text) else {
        return text.to_string();
    };
    let mut out = String::new();
    for (on, glyph) in [
        (spec.ctrl, "⌃"),
        (spec.alt, "⌥"),
        (spec.shift, "⇧"),
        (spec.cmd, "⌘"),
    ] {
        if on {
            out.push_str(glyph);
        }
    }
    if spec.function {
        out.push_str("fn ");
    }
    let key = match spec.key.as_str() {
        "backspace" => "⌫".to_string(),
        "delete" => "⌦".to_string(),
        "enter" => "↩".to_string(),
        "tab" => "⇥".to_string(),
        "escape" => "⎋".to_string(),
        "space" => "Space".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "pageup" => "⇞".to_string(),
        "pagedown" => "⇟".to_string(),
        "home" => "↖".to_string(),
        "end" => "↘".to_string(),
        "insert" => "Ins".to_string(),
        other => other.to_uppercase(),
    };
    out.push_str(&key);
    out
}

/// The close confirmation, naming the keys in words, like `Close? enter / esc`.
pub fn confirm_prompt(overrides: &KeyOverrides) -> String {
    let word = |id: &str| {
        let key = action(id).and_then(|a| effective_key(overrides, a))?;
        Some(match parse_key(&key) {
            Ok(mut spec) => {
                if spec.key == "escape" {
                    spec.key = "esc".into();
                }
                spec.canonical()
            }
            Err(_) => key,
        })
    };
    let keys: Vec<String> = [word("canvas.confirm"), word("canvas.cancel")]
        .into_iter()
        .flatten()
        .collect();
    if keys.is_empty() {
        "Close?".to_string()
    } else {
        format!("Close? {}", keys.join(" / "))
    }
}

/// What AppKit gives for one key-down. The `chars_*` fields are the characters of the
/// event's key code with no modifier, Shift, Cmd and Cmd+Shift (GPUI's
/// `chars_for_modified_key`, events.rs:512).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyParts {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub function: bool,
    /// `charactersIgnoringModifiers`; picks the named key.
    pub chars_ignoring_modifiers: String,
    pub chars_plain: String,
    pub chars_shift: String,
    pub chars_cmd: String,
    pub chars_cmd_shift: String,
    /// GPUI's `always_use_command_layout()` (events.rs:499-505).
    pub always_use_cmd_layout: bool,
}

/// GPUI's named keys by AppKit character (events.rs:359-420).
fn named_key(c: char) -> Option<String> {
    let name = match c as u32 {
        0x20 => "space",
        0x09 | 0x19 => "tab",
        0x0d | 0x03 => "enter",
        0x7f => "backspace",
        0x1b => "escape",
        0xF700 => "up",
        0xF701 => "down",
        0xF702 => "left",
        0xF703 => "right",
        0xF72C => "pageup",
        0xF72D => "pagedown",
        0xF729 => "home",
        0xF72B => "end",
        0xF728 => "delete",
        // GPUI maps Insert from NSHelpFunctionKey, as observed on hardware.
        0xF746 => "insert",
        n @ 0xF704..=0xF726 => return Some(format!("f{}", n - 0xF704 + 1)),
        _ => return None,
    };
    Some(name.to_string())
}

/// The keystroke GPUI reports for one AppKit key-down, as a canonical key string.
/// Mirrors `gpui-pre-macos 0.3.4` events.rs:350-480; keep the two in step on upgrade.
pub fn keystroke_from_parts(parts: &KeyParts) -> Option<String> {
    let first = parts.chars_ignoring_modifiers.chars().next()?;
    let function = parts.function && !('\u{f700}'..='\u{f747}').contains(&first);
    let mut shift = parts.shift;
    let key = match named_key(first) {
        Some(name) => name,
        None => {
            let mut plain = parts.chars_plain.clone();
            let mut shifted = parts.chars_shift.clone();
            if parts.cmd || parts.always_use_cmd_layout {
                if parts.chars_cmd_shift != parts.chars_cmd {
                    shifted = parts.chars_cmd_shift.clone();
                } else if parts.chars_cmd.to_ascii_uppercase() != parts.chars_cmd {
                    // macOS reports cmd-s for cmd-shift-s on some layouts (Ukrainian).
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
    if key.is_empty() {
        return None;
    }
    let spec = KeySpec {
        cmd: parts.cmd,
        ctrl: parts.ctrl,
        alt: parts.alt,
        shift,
        function,
        key,
    };
    Some(spec.canonical())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_every_action_once() {
        let ids: Vec<&str> = ACTIONS.iter().map(|a| a.id).collect();
        assert_eq!(
            ids,
            [
                "canvas.draw_terminal",
                "canvas.draw_browser",
                "canvas.cancel",
                "canvas.confirm",
                "panel.delete",
                "panel.settings",
                "panel.move_up",
                "panel.move_down",
                "panel.move_left",
                "panel.move_right",
                "panel.move_up_far",
                "panel.move_down_far",
                "panel.move_left_far",
                "panel.move_right_far",
                "canvas.zoom_in",
                "canvas.zoom_out",
                "canvas.zoom_reset",
                "canvas.center_origin",
                "canvas.fit_all",
                "view.fullscreen",
                "canvas.pan_up",
                "canvas.pan_down",
                "canvas.pan_left",
                "canvas.pan_right",
                "canvas.pan_up_far",
                "canvas.pan_down_far",
                "canvas.pan_left_far",
                "canvas.pan_right_far",
                "view.show_grid",
                "developer.toggle_stats",
                "developer.reset_fps",
                "developer.show_welcome",
                "app.save_preset",
                "app.load_preset",
                "app.settings",
                "app.open_config_folder",
                "app.reset_settings",
                "app.about",
                "app.quit",
            ]
        );
        assert_eq!(
            action("panel.delete").unwrap().label,
            "Delete focused panel"
        );
        assert!(action("nope").is_none());
    }

    #[test]
    fn defaults_are_todays_keys_except_fit_all() {
        let defaults: Vec<(&str, Option<&str>)> =
            ACTIONS.iter().map(|a| (a.id, a.default)).collect();
        // Spec table; fit_all moves from cmd-shift-0, which GPUI never reports.
        assert!(defaults.contains(&("canvas.fit_all", Some("cmd-)"))));
        assert!(defaults.contains(&("panel.settings", Some("cmd-<"))));
        assert!(defaults.contains(&("app.about", None)));
        assert_eq!(defaults.iter().filter(|(_, d)| d.is_some()).count(), 31);
    }

    #[test]
    fn every_default_is_valid_and_conflict_free() {
        for a in ACTIONS {
            if let Some(key) = a.default {
                assert_eq!(check_key(key).map(|_| ()), Ok(()), "{}", a.id);
            }
        }
        assert_eq!(validate_overrides(&KeyOverrides::new()), Ok(()));
    }

    #[test]
    fn grammar_accepts_what_gpui_reports() {
        for ok in [
            "cmd-=",
            "cmd--",
            "-",
            "cmd-0",
            "cmd-<",
            "cmd-)",
            "cmd-shift-backspace",
            "shift-tab",
            "cmd-shift-enter",
            "shift-f5",
            "alt-up",
            "f12",
            "t",
            "shift-t",
            "cmd-Ö",
            "cmd-ý",
            "fn-a",
            "ctrl-alt-space",
        ] {
            assert!(parse_key(ok).is_ok(), "{ok}");
        }
        assert_eq!(
            parse_key("shift-cmd-backspace").unwrap().canonical(),
            "cmd-shift-backspace"
        );
    }

    #[test]
    fn grammar_rejects_keys_gpui_never_reports() {
        for (text, error) in [
            ("", KeyError::Syntax),
            ("cmd-", KeyError::Syntax),
            ("Cmd-d", KeyError::Syntax),
            ("cmd-cmd-x", KeyError::Syntax),
            ("hyper-x", KeyError::Syntax),
            ("cmd-xy", KeyError::Syntax),
            ("cmd-k cmd-s", KeyError::Syntax),
            ("cmd", KeyError::BareModifier),
            ("cmd-shift", KeyError::BareModifier),
            ("cmd-D", KeyError::UppercaseLetter),
            ("cmd-shift-,", KeyError::ShiftedSymbol),
            ("cmd-shift-0", KeyError::ShiftedSymbol),
            ("shift-1", KeyError::ShiftedSymbol),
            ("shift--", KeyError::ShiftedSymbol),
            ("fn-up", KeyError::FnNeverFires),
            ("fn-f1", KeyError::FnNeverFires),
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
        assert_eq!(
            check_rebind(&overrides, delete, "cmd-="),
            Err(KeyError::UsedBy("Zoom in"))
        );
        // Canvas and global overlap.
        assert_eq!(
            check_rebind(&overrides, action("app.quit").unwrap(), "cmd-="),
            Err(KeyError::UsedBy("Zoom in"))
        );
        overrides.insert("panel.delete".into(), Some("cmd-=".into()));
        let error = validate_overrides(&overrides).unwrap_err();
        assert!(
            error.contains("keybindings.panel.delete") && error.contains("Used by Zoom in"),
            "{error}"
        );
        overrides.insert("canvas.zoom_in".into(), Some("cmd-k".into()));
        overrides.insert("panel.delete".into(), Some("cmd-=".into()));
        assert_eq!(validate_overrides(&overrides), Ok(()));
        overrides.insert("canvas.zoom_in".into(), None);
        assert_eq!(validate_overrides(&overrides), Ok(()));
    }

    #[test]
    fn modifier_order_does_not_hide_a_conflict() {
        let quit = action("app.quit").unwrap();
        assert_eq!(
            check_rebind(&KeyOverrides::new(), quit, "shift-cmd-h"),
            Err(KeyError::UsedBy("Center on origin"))
        );
        let mut overrides = KeyOverrides::new();
        overrides.insert("app.quit".into(), Some("shift-cmd-h".into()));
        let error = validate_overrides(&overrides).unwrap_err();
        assert!(
            error.contains("keybindings.app.quit") && error.contains("Used by Center on origin"),
            "{error}"
        );
    }

    #[test]
    fn validate_overrides_names_the_field() {
        let mut overrides = KeyOverrides::new();
        overrides.insert("canvas.nope".into(), Some("cmd-k".into()));
        assert_eq!(
            validate_overrides(&overrides),
            Err("\"keybindings.canvas.nope\": unknown action".into())
        );
        let mut overrides = KeyOverrides::new();
        overrides.insert("panel.delete".into(), Some("cmd-shift-,".into()));
        let error = validate_overrides(&overrides).unwrap_err();
        assert!(
            error.starts_with(
                "\"keybindings.panel.delete\" is \"cmd-shift-,\": shift with this key never fires"
            ),
            "{error}"
        );
    }

    #[test]
    fn only_keys_without_cmd_or_ctrl_pass_to_the_terminal() {
        for key in ["t", "escape", "enter", "f5", "alt-x", "shift-tab"] {
            assert!(passes_to_terminal(&parse_key(key).unwrap()), "{key}");
        }
        for key in ["cmd-x", "ctrl-x", "cmd-ctrl-f", "ctrl-alt-space"] {
            assert!(!passes_to_terminal(&parse_key(key).unwrap()), "{key}");
        }
    }

    #[test]
    fn keys_show_with_macos_symbols() {
        for (text, shown) in [
            ("cmd-shift-backspace", "⇧⌘⌫"),
            ("cmd-<", "⌘<"),
            ("cmd-ctrl-f", "⌃⌘F"),
            ("enter", "↩"),
            ("escape", "⎋"),
            ("cmd-=", "⌘="),
            ("alt-up", "⌥↑"),
            ("f5", "F5"),
            ("cmd-)", "⌘)"),
            ("t", "T"),
            ("fn-a", "fn A"),
        ] {
            assert_eq!(display_key(text), shown, "{text}");
        }
    }

    #[test]
    fn menu_cannot_show_fn_enter_or_tab() {
        for key in ["fn-a", "cmd-enter", "cmd-tab"] {
            assert!(!shows_in_menu(&parse_key(key).unwrap()), "{key}");
        }
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

    fn parts(cmd: bool, shift: bool, chars: [&str; 4], cmd_layout: bool) -> KeyParts {
        KeyParts {
            cmd,
            shift,
            chars_ignoring_modifiers: chars[0].into(),
            chars_plain: chars[0].into(),
            chars_shift: chars[1].into(),
            chars_cmd: chars[2].into(),
            chars_cmd_shift: chars[3].into(),
            always_use_cmd_layout: cmd_layout,
            ..KeyParts::default()
        }
    }

    #[test]
    fn us_layout_matches_gpui() {
        assert_eq!(
            keystroke_from_parts(&parts(true, true, ["0", ")", "0", ")"], false)).as_deref(),
            Some("cmd-)")
        );
        assert_eq!(
            keystroke_from_parts(&parts(true, true, [",", "<", ",", "<"], false)).as_deref(),
            Some("cmd-<")
        );
        assert_eq!(
            keystroke_from_parts(&parts(true, true, ["s", "S", "s", "S"], false)).as_deref(),
            Some("cmd-shift-s")
        );
        let mut backspace = parts(true, true, ["\u{7f}"; 4], false);
        backspace.chars_ignoring_modifiers = "\u{7f}".into();
        assert_eq!(
            keystroke_from_parts(&backspace).as_deref(),
            Some("cmd-shift-backspace")
        );
        assert_eq!(
            keystroke_from_parts(&parts(false, false, ["\u{1b}"; 4], false)).as_deref(),
            Some("escape")
        );
    }

    // The table in events.rs:422-430.
    #[test]
    fn gpui_comment_table_layouts() {
        // Armenian: non-ASCII layout, so the cmd layout is used.
        assert_eq!(
            keystroke_from_parts(&parts(true, false, ["ս", "Ս", "s", "S"], true)).as_deref(),
            Some("cmd-s")
        );
        assert_eq!(
            keystroke_from_parts(&parts(true, true, ["ս", "Ս", "s", "S"], true)).as_deref(),
            Some("cmd-shift-s")
        );
        // Dvorak+QWERTY: layout switches on cmd.
        assert_eq!(
            keystroke_from_parts(&parts(false, false, ["o", "O", "s", "S"], false)).as_deref(),
            Some("o")
        );
        assert_eq!(
            keystroke_from_parts(&parts(true, true, ["o", "O", "s", "S"], false)).as_deref(),
            Some("cmd-shift-s")
        );
        // Ukrainian+QWERTY: macOS reports cmd-s for cmd-shift-s.
        assert_eq!(
            keystroke_from_parts(&parts(true, true, ["с", "С", "s", "s"], false)).as_deref(),
            Some("cmd-shift-s")
        );
        // Czech: shifted numbers.
        assert_eq!(
            keystroke_from_parts(&parts(true, false, ["ý", "7", "ý", "7"], false)).as_deref(),
            Some("cmd-ý")
        );
        assert_eq!(
            keystroke_from_parts(&parts(true, true, ["ý", "7", "ý", "7"], false)).as_deref(),
            Some("cmd-7")
        );
        // Norwegian: cmd-shift-7 is cmd-/.
        assert_eq!(
            keystroke_from_parts(&parts(true, true, ["7", "/", "7", "7"], false)).as_deref(),
            Some("cmd-/")
        );
        // Russian: cmd layout wins.
        assert_eq!(
            keystroke_from_parts(&parts(true, true, ["7", ".", "7", "&"], true)).as_deref(),
            Some("cmd-&")
        );
        // German: the shift special case is a-z only.
        assert_eq!(
            keystroke_from_parts(&parts(true, true, ["ö", "Ö", "ö", "Ö"], false)).as_deref(),
            Some("cmd-Ö")
        );
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
        for p in [
            parts(true, true, ["0", ")", "0", ")"], false),
            parts(true, true, ["ö", "Ö", "ö", "Ö"], false),
        ] {
            let key = keystroke_from_parts(&p).unwrap();
            assert!(parse_key(&key).is_ok(), "{key}");
        }
        assert_eq!(keystroke_from_parts(&KeyParts::default()), None);
    }

    #[test]
    fn menu_commands_refuse_keys_the_menu_would_change() {
        let quit = action("app.quit").unwrap();
        for key in ["fn-cmd-k", "cmd-enter", "cmd-tab"] {
            assert_eq!(
                check_rebind(&KeyOverrides::new(), quit, key),
                Err(KeyError::NotInMenu),
                "{key}"
            );
            let mut overrides = KeyOverrides::new();
            overrides.insert("app.quit".into(), Some(key.into()));
            let error = validate_overrides(&overrides).unwrap_err();
            assert!(
                error.contains("keybindings.app.quit") && error.contains("menu"),
                "{error}"
            );
        }
        // A canvas action is not in a menu, so the same keys are fine there.
        assert!(
            check_rebind(
                &KeyOverrides::new(),
                action("canvas.zoom_in").unwrap(),
                "fn-cmd-k"
            )
            .is_ok()
        );
    }

    #[test]
    fn key_help_follows_overrides() {
        let row_for = |rows: &[KeyHelpRow], id: &str| {
            rows.iter()
                .find(|r| r.id == id)
                .unwrap_or_else(|| panic!("no row {id}"))
                .clone()
        };
        let mut overrides = KeyOverrides::new();
        let rows = key_help(&overrides);
        assert_eq!(rows.len(), ACTIONS.len() + BROWSER_KEYS.len() + 1);
        let ids: Vec<&str> = rows.iter().take(ACTIONS.len()).map(|r| r.id).collect();
        let table: Vec<&str> = ACTIONS.iter().map(|a| a.id).collect();
        assert_eq!(ids, table, "actions come first, in table order");

        let welcome = row_for(&rows, "developer.show_welcome");
        assert_eq!(welcome.key, None);
        assert_eq!(welcome.label, "Show welcome screen");
        assert_eq!(welcome.group, "Developer");
        let stats = row_for(&rows, "developer.toggle_stats");
        assert_eq!(stats.key, Some(display_key("cmd-shift-f")));
        assert_eq!(stats.group, Group::Developer.title());

        overrides.insert("developer.toggle_stats".into(), Some("cmd-shift-s".into()));
        let stats = row_for(&key_help(&overrides), "developer.toggle_stats");
        assert_eq!(stats.key, Some(display_key("cmd-shift-s")));
        overrides.insert("developer.toggle_stats".into(), None);
        assert_eq!(
            row_for(&key_help(&overrides), "developer.toggle_stats").key,
            None
        );

        let fixed = &rows[ACTIONS.len()..];
        for (n, ((key, label), row)) in BROWSER_KEYS.iter().zip(fixed).enumerate() {
            assert_eq!(row.id, format!("fixed.browser.{n}"));
            assert_eq!(row.label, *label);
            assert_eq!(row.group, "Fixed");
            assert_eq!(row.key, Some(display_key(key)));
        }
        let restart = rows.last().unwrap();
        assert_eq!(restart.id, "fixed.restart");
        assert_eq!(restart.label, RESTART_KEY.1);
        assert_eq!(restart.group, "Fixed");
        assert_eq!(restart.key, Some(display_key(RESTART_KEY.0)));
    }
}
