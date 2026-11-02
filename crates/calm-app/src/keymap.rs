//! Key bindings from settings: the action behind each table id, the GPUI binding list,
//! and the rebuild that runs after every settings change.

use std::rc::Rc;

use calm_core::keymap::{Scope, bound_keys, parse_key, passes_to_terminal};
use calm_core::settings::Settings;
use gpui::{Action, App, DummyKeyboardMapper, KeyBinding, KeyBindingContextPredicate, NoAction};

use crate::canvas_view::{
    self, Cancel, CenterOnOrigin, ConfirmEnter, DeletePanel, DrawBrowser, DrawTerminal, FitAll,
    MovePanelDown, MovePanelDownFar, MovePanelLeft, MovePanelLeftFar, MovePanelRight,
    MovePanelRightFar, MovePanelUp, MovePanelUpFar, OpenPanelSettings, PanDown, PanDownFar,
    PanLeft, PanLeftFar, PanRight, PanRightFar, PanUp, PanUpFar, ToggleFullscreen, TogglePinPanel,
    ZoomIn, ZoomOut, ZoomReset,
};
use crate::dev_menu::{About, Quit, ResetFps, ToggleFps};
use crate::preset_menu::{LoadPreset, SavePreset, ShowWelcome};
use crate::settings_menu::{self, OpenConfigFolder, OpenSettings, ResetSettings, ToggleGrid};
use crate::terminal;

/// The GPUI action behind a `calm_core::keymap::ACTIONS` id.
pub fn gpui_action(id: &str) -> Option<Box<dyn Action>> {
    Some(match id {
        "canvas.zoom_in" => Box::new(ZoomIn),
        "canvas.zoom_out" => Box::new(ZoomOut),
        "canvas.zoom_reset" => Box::new(ZoomReset),
        "canvas.center_origin" => Box::new(CenterOnOrigin),
        "canvas.fit_all" => Box::new(FitAll),
        "canvas.draw_terminal" => Box::new(DrawTerminal),
        "canvas.draw_browser" => Box::new(DrawBrowser),
        "canvas.cancel" => Box::new(Cancel),
        "canvas.confirm" => Box::new(ConfirmEnter),
        "view.fullscreen" => Box::new(ToggleFullscreen),
        "canvas.pan_up" => Box::new(PanUp),
        "canvas.pan_down" => Box::new(PanDown),
        "canvas.pan_left" => Box::new(PanLeft),
        "canvas.pan_right" => Box::new(PanRight),
        "canvas.pan_up_far" => Box::new(PanUpFar),
        "canvas.pan_down_far" => Box::new(PanDownFar),
        "canvas.pan_left_far" => Box::new(PanLeftFar),
        "canvas.pan_right_far" => Box::new(PanRightFar),
        "panel.move_up" => Box::new(MovePanelUp),
        "panel.move_down" => Box::new(MovePanelDown),
        "panel.move_left" => Box::new(MovePanelLeft),
        "panel.move_right" => Box::new(MovePanelRight),
        "panel.move_up_far" => Box::new(MovePanelUpFar),
        "panel.move_down_far" => Box::new(MovePanelDownFar),
        "panel.move_left_far" => Box::new(MovePanelLeftFar),
        "panel.move_right_far" => Box::new(MovePanelRightFar),
        "panel.delete" => Box::new(DeletePanel),
        "panel.settings" => Box::new(OpenPanelSettings),
        "panel.pin" => Box::new(TogglePinPanel),
        "developer.toggle_stats" => Box::new(ToggleFps),
        "developer.reset_fps" => Box::new(ResetFps),
        "developer.show_welcome" => Box::new(ShowWelcome),
        "app.save_preset" => Box::new(SavePreset),
        "app.load_preset" => Box::new(LoadPreset),
        "view.show_grid" => Box::new(ToggleGrid),
        "app.settings" => Box::new(OpenSettings),
        "app.open_config_folder" => Box::new(OpenConfigFolder),
        "app.reset_settings" => Box::new(ResetSettings),
        "app.about" => Box::new(About),
        "app.quit" => Box::new(Quit),
        _ => return None,
    })
}

fn load(key: &str, action: Box<dyn Action>, context: Option<&str>) -> Option<KeyBinding> {
    let predicate = context
        .map(|c| KeyBindingContextPredicate::parse(c).map(Rc::new))
        .transpose()
        .ok()?;
    KeyBinding::load(key, action, predicate, false, None, &DummyKeyboardMapper).ok()
}

/// Every binding the settings ask for, then a `NoAction` per typing key so a focused
/// terminal keeps it.
pub fn key_bindings(settings: &Settings) -> Vec<KeyBinding> {
    let bound = bound_keys(&settings.keybindings);
    let mut bindings = Vec::new();
    for (spec, key) in &bound {
        let context = match spec.scope {
            Scope::Canvas => Some(canvas_view::KEY_CONTEXT),
            Scope::Global => None,
        };
        // Validation makes these unreachable; a bad key must not take the app down.
        match gpui_action(spec.id).and_then(|action| load(key, action, context)) {
            Some(binding) => bindings.push(binding),
            None => eprintln!("calm: key binding {}: cannot bind {key:?}", spec.id),
        }
    }
    for (spec, key) in &bound {
        if parse_key(key).is_ok_and(|k| passes_to_terminal(&k)) {
            match load(key, Box::new(NoAction), Some(terminal::KEY_CONTEXT)) {
                Some(binding) => bindings.push(binding),
                None => eprintln!(
                    "calm: key binding {}: cannot free {key:?} in terminals",
                    spec.id
                ),
            }
        }
    }
    bindings
}

/// Clears the keymap, binds `key_bindings(settings)`, then sets the menus so their keys follow.
pub fn apply(cx: &mut App, settings: &Settings, show_stats: bool) {
    cx.clear_key_bindings();
    cx.bind_keys(key_bindings(settings));
    cx.set_menus(settings_menu::menus(show_stats, settings));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use calm_core::keymap::{ACTIONS, Scope};
    use calm_core::settings::Settings;
    use gpui::{KeyBinding, Keystroke, NoAction};

    use super::*;
    use crate::canvas_view::{
        self, DeletePanel, DrawBrowser, FitAll, MovePanelDown, MovePanelDownFar, MovePanelLeft,
        MovePanelLeftFar, MovePanelRight, MovePanelRightFar, MovePanelUp, MovePanelUpFar,
        OpenPanelSettings, PanDown, PanDownFar, PanLeft, PanLeftFar, PanRight, PanRightFar, PanUp,
        PanUpFar,
    };
    use crate::settings_menu::OpenSettings;
    use crate::terminal;

    fn matches(b: &KeyBinding, key: &str) -> bool {
        b.match_keystrokes(&[Keystroke::parse(key).unwrap()]) == Some(false)
    }

    fn ctx(b: &KeyBinding) -> Option<String> {
        b.predicate().map(|p| p.to_string())
    }

    #[test]
    fn every_registered_calm_action_has_one_table_row() {
        let ours: Vec<&str> = ACTIONS
            .iter()
            .map(|a| gpui_action(a.id).expect(a.id).name())
            .collect();
        for data in
            gpui::generate_list_of_all_registered_actions().filter(|d| d.name.starts_with("calm::"))
        {
            assert_eq!(
                ours.iter().filter(|n| **n == data.name).count(),
                1,
                "{}",
                data.name
            );
        }
        assert_eq!(ours.len(), 40);
    }

    #[test]
    fn default_bindings_match_the_table() {
        let bindings = key_bindings(&Settings::default());
        for a in ACTIONS {
            let action = gpui_action(a.id).unwrap();
            let found: Vec<&KeyBinding> = bindings
                .iter()
                .filter(|b| b.action().partial_eq(&*action))
                .collect();
            match a.default {
                Some(key) => {
                    assert_eq!(found.len(), 1, "{}", a.id);
                    assert!(matches(found[0], key), "{}", a.id);
                    let want =
                        (a.scope == Scope::Canvas).then(|| canvas_view::KEY_CONTEXT.to_string());
                    assert_eq!(ctx(found[0]), want, "{}", a.id);
                }
                None => assert!(found.is_empty(), "{}", a.id),
            }
        }
    }

    #[test]
    fn a_rebound_action_matches_the_new_key_and_not_the_old() {
        let mut settings = Settings::default();
        settings
            .keybindings
            .insert("panel.delete".into(), Some("cmd-d".into()));
        let bindings = key_bindings(&settings);
        assert!(
            bindings
                .iter()
                .any(|b| matches(b, "cmd-d") && b.action().partial_eq(&DeletePanel))
        );
        assert!(!bindings.iter().any(|b| matches(b, "cmd-shift-backspace")));
    }

    #[test]
    fn an_unbound_action_has_no_binding_and_frees_its_key_in_terminals() {
        let mut settings = Settings::default();
        settings
            .keybindings
            .insert("canvas.draw_browser".into(), None);
        let bindings = key_bindings(&settings);
        assert!(!bindings.iter().any(|b| b.action().partial_eq(&DrawBrowser)));
        assert!(!bindings.iter().any(|b| matches(b, "b")));
    }

    #[test]
    fn terminal_noaction_exactly_for_keys_without_cmd_or_ctrl() {
        let mut settings = Settings::default();
        settings
            .keybindings
            .insert("canvas.zoom_in".into(), Some("f5".into()));
        let blocked: BTreeSet<String> = key_bindings(&settings)
            .iter()
            .filter(|b| b.action().partial_eq(&NoAction))
            .inspect(|b| assert_eq!(ctx(b).as_deref(), Some(terminal::KEY_CONTEXT)))
            .map(|b| {
                [
                    "t",
                    "b",
                    "escape",
                    "enter",
                    "f5",
                    "up",
                    "down",
                    "left",
                    "right",
                    "shift-up",
                    "shift-down",
                    "shift-left",
                    "shift-right",
                ]
                .into_iter()
                .find(|k| matches(b, k))
                .expect("unexpected NoAction")
                .to_string()
            })
            .collect();
        let want: BTreeSet<String> = [
            "b",
            "enter",
            "escape",
            "f5",
            "t",
            "up",
            "down",
            "left",
            "right",
            "shift-up",
            "shift-down",
            "shift-left",
            "shift-right",
        ]
        .map(String::from)
        .into();
        assert_eq!(blocked, want);
    }

    #[test]
    fn fit_all_matches_the_press_gpui_reports_for_cmd_shift_0() {
        // events.rs:466-476: with Cmd held GPUI drops Shift for "0" and reports ")".
        let typed = gpui::Keystroke::parse("cmd-)").unwrap();
        let old = gpui::KeyBinding::new("cmd-shift-0", FitAll, Some(canvas_view::KEY_CONTEXT));
        assert_eq!(
            old.match_keystrokes(std::slice::from_ref(&typed)),
            None,
            "cmd-shift-0 never matches"
        );
        assert!(key_bindings(&Settings::default()).iter().any(|b| {
            b.match_keystrokes(std::slice::from_ref(&typed)) == Some(false)
                && b.action().partial_eq(&FitAll)
        }));
    }

    #[test]
    fn panel_settings_shortcut_is_available_with_terminal_focus() {
        // macOS GPUI represents US cmd-shift-comma as cmd-<.
        let keystroke = gpui::Keystroke::parse("cmd-<").unwrap();
        let bindings = key_bindings(&Settings::default());
        assert!(
            !bindings.iter().any(|binding| {
                binding.match_keystrokes(std::slice::from_ref(&keystroke)) == Some(false)
                    && binding.action().partial_eq(&NoAction)
                    && binding
                        .predicate()
                        .is_some_and(|p| p.to_string() == terminal::KEY_CONTEXT)
            }),
            "terminal context must not block the panel settings shortcut"
        );
        assert!(
            bindings.iter().any(|binding| {
                binding.match_keystrokes(std::slice::from_ref(&keystroke)) == Some(false)
                    && binding.action().partial_eq(&OpenPanelSettings)
                    && binding
                        .predicate()
                        .is_some_and(|p| p.to_string() == canvas_view::KEY_CONTEXT)
            }),
            "canvas must bind cmd-< to OpenPanelSettings"
        );
        assert!(
            !key_bindings(&Settings::default()).iter().any(|binding| {
                binding.match_keystrokes(std::slice::from_ref(&keystroke)) == Some(false)
                    && binding.action().partial_eq(&OpenSettings)
            }),
            "panel settings shortcut must not also open global settings"
        );
    }

    #[test]
    fn delete_panel_shortcut_is_available_with_terminal_focus() {
        let keystroke = gpui::Keystroke::parse("cmd-shift-backspace").unwrap();
        let bindings = key_bindings(&Settings::default());
        assert!(
            !bindings.iter().any(|binding| {
                binding.match_keystrokes(std::slice::from_ref(&keystroke)) == Some(false)
                    && binding.action().partial_eq(&NoAction)
                    && binding
                        .predicate()
                        .is_some_and(|p| p.to_string() == terminal::KEY_CONTEXT)
            }),
            "terminal context must not block the delete shortcut"
        );
        assert!(
            bindings.iter().any(|binding| {
                binding.match_keystrokes(std::slice::from_ref(&keystroke)) == Some(false)
                    && binding.action().partial_eq(&DeletePanel)
                    && binding
                        .predicate()
                        .is_some_and(|p| p.to_string() == canvas_view::KEY_CONTEXT)
            }),
            "canvas must bind cmd-shift-backspace to DeletePanel"
        );
    }

    fn binds(keys: &str, action: &dyn gpui::Action, context: &str) -> bool {
        let keystroke = gpui::Keystroke::parse(keys).unwrap();
        key_bindings(&Settings::default()).iter().any(|binding| {
            binding.match_keystrokes(std::slice::from_ref(&keystroke)) == Some(false)
                && binding.action().partial_eq(action)
                && binding
                    .predicate()
                    .is_some_and(|p| p.to_string() == context)
        })
    }

    #[test]
    fn plain_arrows_pan_the_canvas_but_reach_a_focused_terminal() {
        let cases: [(&str, &dyn gpui::Action); 8] = [
            ("up", &PanUp),
            ("down", &PanDown),
            ("left", &PanLeft),
            ("right", &PanRight),
            ("shift-up", &PanUpFar),
            ("shift-down", &PanDownFar),
            ("shift-left", &PanLeftFar),
            ("shift-right", &PanRightFar),
        ];
        for (keys, action) in cases {
            assert!(
                binds(keys, action, canvas_view::KEY_CONTEXT),
                "canvas must bind {keys}"
            );
            assert!(
                binds(keys, &NoAction, terminal::KEY_CONTEXT),
                "terminal context must keep {keys}"
            );
        }
    }

    #[test]
    fn panel_move_arrows_are_available_with_terminal_focus() {
        let cases: [(&str, &dyn gpui::Action); 8] = [
            ("cmd-alt-up", &MovePanelUp),
            ("cmd-alt-down", &MovePanelDown),
            ("cmd-alt-left", &MovePanelLeft),
            ("cmd-alt-right", &MovePanelRight),
            ("cmd-alt-shift-up", &MovePanelUpFar),
            ("cmd-alt-shift-down", &MovePanelDownFar),
            ("cmd-alt-shift-left", &MovePanelLeftFar),
            ("cmd-alt-shift-right", &MovePanelRightFar),
        ];
        for (keys, action) in cases {
            assert!(
                binds(keys, action, canvas_view::KEY_CONTEXT),
                "canvas must bind {keys}"
            );
            assert!(
                !binds(keys, &NoAction, terminal::KEY_CONTEXT),
                "terminal context must not block {keys}"
            );
        }
    }
}
