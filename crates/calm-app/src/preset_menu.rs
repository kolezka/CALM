//! Save Preset, Load Preset and Show Welcome Screen. Each flow asks through blocking
//! native dialogs in a task outside any GPUI update, and decides everything before a
//! panel closes: a cancel at any dialog leaves the canvas as it was.

use std::path::{Path, PathBuf};

use calm_core::layout::{LayoutFile, layouts_dir, save_layout_in};
use calm_core::preset::{
    ClosingPanel, PresetOrigin, decide_load, find_preset, list_presets, load_confirmation,
    save_name,
};
use gpui::{Action, App, Context, Menu, MenuItem, Window, WindowHandle, actions};

use crate::canvas_view::CanvasView;

actions!(calm, [SavePreset, LoadPreset, ShowWelcome]);

const SAVE_TITLE: &str = "Save Preset";
const LOAD_TITLE: &str = "Load Preset";
const WELCOME_TITLE: &str = "Show Welcome Screen";
/// Opened by Show Welcome Screen when `session.first_launch_preset` is empty.
const WELCOME_FALLBACK: &str = "welcome";

/// The blocking dialogs a flow asks through. Native in the app, scripted in tests.
pub(crate) trait Dialogs {
    fn choose(&mut self, title: &str, options: &[String]) -> Result<Option<usize>, String>;
    fn confirm(&mut self, title: &str, message: &str, ok: &str) -> Result<bool, String>;
    fn alert(&mut self, title: &str, message: &str);
}

struct NativeDialogs;

impl Dialogs for NativeDialogs {
    fn choose(&mut self, title: &str, options: &[String]) -> Result<Option<usize>, String> {
        calm_native::choose(title, "", options).map_err(|err| err.to_string())
    }
    fn confirm(&mut self, title: &str, message: &str, ok: &str) -> Result<bool, String> {
        calm_native::confirm(title, message, ok).map_err(|err| err.to_string())
    }
    fn alert(&mut self, title: &str, message: &str) {
        if let Err(err) = calm_native::alert(title, message) {
            eprintln!("calm: {title}: {message} (alert unavailable: {err})");
        }
    }
}

/// Which preset a load opens.
pub(crate) enum Pick {
    /// Ask with a list of every preset.
    Choose,
    Named(String),
}

/// A load the user has agreed to. `agreed` is the confirmation they answered, if any, so
/// the apply step can refuse when the canvas changed while a dialog was open.
pub(crate) struct LoadDecision {
    pub name: String,
    pub layout: LayoutFile,
    pub agreed: Option<String>,
}

/// Runs the load dialogs. `closing` reports what would close and whether
/// `panels.confirm_close` is on, or `None` when the canvas is gone. Returns `None` on any
/// cancel or error, after telling the user about an error; nothing is closed here.
pub(crate) fn decide_preset_load(
    title: &str,
    pick: Pick,
    user_dir: &Path,
    builtin_dirs: &[PathBuf],
    dialogs: &mut impl Dialogs,
    closing: impl FnOnce() -> Option<(Vec<ClosingPanel>, bool)>,
) -> Option<LoadDecision> {
    let name = match pick {
        Pick::Named(name) => name,
        Pick::Choose => {
            let entries = list_presets(user_dir, builtin_dirs);
            if entries.is_empty() {
                dialogs.alert(
                    title,
                    &format!(
                        "No presets found. Save one, or put it in {}.",
                        user_dir.display()
                    ),
                );
                return None;
            }
            let labels: Vec<String> = entries
                .iter()
                .map(|entry| match entry.origin {
                    PresetOrigin::User => entry.name.clone(),
                    PresetOrigin::Builtin => format!("{} (built-in)", entry.name),
                })
                .collect();
            match dialogs.choose(title, &labels) {
                Ok(Some(index)) => entries.get(index)?.name.clone(),
                Ok(None) => return None,
                Err(err) => {
                    dialogs.alert(title, &err);
                    return None;
                }
            }
        }
    };
    let layout = match find_preset(&name, user_dir, builtin_dirs) {
        Ok((layout, _)) => layout,
        Err(err) => {
            dialogs.alert(title, &format!("Preset {name} could not be loaded: {err}"));
            return None;
        }
    };
    let (panels, confirm_close) = closing()?;
    let confirmation = load_confirmation(&name, &panels, confirm_close);
    let said_ok = match &confirmation {
        Some(message) => match dialogs.confirm(title, message, "Load") {
            Ok(ok) => ok,
            Err(err) => {
                dialogs.alert(title, &err);
                return None;
            }
        },
        None => false,
    };
    if !decide_load(confirmation.as_deref(), said_ok) {
        return None;
    }
    Some(LoadDecision {
        name,
        layout,
        agreed: confirmation,
    })
}

/// Turns the typed name into the one to save under, asking before it replaces a user
/// preset or hides a built-in one. `None` on cancel or a bad name (after an alert).
pub(crate) fn decide_preset_save(
    input: &str,
    user_dir: &Path,
    builtin_dirs: &[PathBuf],
    dialogs: &mut impl Dialogs,
) -> Option<String> {
    let name = match save_name(input) {
        Ok(name) => name,
        Err(message) => {
            dialogs.alert(SAVE_TITLE, &message);
            return None;
        }
    };
    let file = format!("{name}.json");
    let message = if user_dir.join(&file).is_file() {
        Some(format!("Your saved preset {name} is overwritten."))
    } else if builtin_dirs.iter().any(|dir| dir.join(&file).is_file()) {
        Some(format!(
            "Your preset is used in place of the built-in preset {name}."
        ))
    } else {
        None
    };
    if let Some(message) = message {
        match dialogs.confirm(&format!("Replace preset {name}?"), &message, "Replace") {
            Ok(true) => {}
            Ok(false) => return None,
            Err(err) => {
                dialogs.alert(SAVE_TITLE, &err);
                return None;
            }
        }
    }
    Some(name)
}

/// Global handlers, so the menu items work whatever holds the keyboard. GPUI runs them
/// while it holds the canvas window, so each defers its update until that is released.
pub fn install(cx: &mut App, canvas: WindowHandle<CanvasView>) {
    fn on(
        canvas: WindowHandle<CanvasView>,
        cx: &mut App,
        run: fn(&mut CanvasView, &mut Window, &mut Context<CanvasView>),
    ) {
        cx.defer(move |cx| {
            if let Err(err) = canvas.update(cx, run) {
                eprintln!("calm: preset menu: {err:#}");
            }
        });
    }
    cx.on_action(move |_: &SavePreset, cx| on(canvas, cx, CanvasView::save_preset));
    cx.on_action(move |_: &LoadPreset, cx| {
        on(canvas, cx, |view, window, cx| {
            view.load_preset(Pick::Choose, LOAD_TITLE, window, cx)
        })
    });
    cx.on_action(move |_: &ShowWelcome, cx| {
        on(canvas, cx, |view, window, cx| {
            let configured = &view.settings.settings.session.first_launch_preset;
            let name = if configured.is_empty() {
                WELCOME_FALLBACK.to_string()
            } else {
                configured.clone()
            };
            view.load_preset(Pick::Named(name), WELCOME_TITLE, window, cx)
        })
    });
}

/// What is missing from the menu bar for the preset items, or `None` when File has Save
/// and Load Preset and Developer has Show Welcome Screen after a separator.
pub(crate) fn missing_menu_items(menus: &[Menu]) -> Option<String> {
    fn has(menus: &[Menu], menu: &str, label: &str, action: &dyn Action) -> bool {
        menus.iter().filter(|m| m.name == menu).any(|m| {
            m.items.iter().any(|item| {
                matches!(item, MenuItem::Action { name, action: a, .. }
                    if name == label && a.partial_eq(action))
            })
        })
    }
    let mut missing = Vec::new();
    if menus.get(1).is_none_or(|menu| menu.name != "File") {
        missing.push("File menu after the app menu".to_string());
    }
    for (menu, label, action) in [
        ("File", "Save Preset…", &SavePreset as &dyn Action),
        ("File", "Load Preset…", &LoadPreset),
        ("Developer", "Show Welcome Screen", &ShowWelcome),
    ] {
        if !has(menus, menu, label, action) {
            missing.push(format!("{menu} > {label}"));
        }
    }
    let after_separator = menus
        .iter()
        .find(|menu| menu.name == "Developer")
        .and_then(|menu| {
            let at = menu.items.iter().position(|item| {
                matches!(item, MenuItem::Action { name, .. } if name == "Show Welcome Screen")
            })?;
            Some(at > 0 && matches!(menu.items[at - 1], MenuItem::Separator))
        });
    if after_separator == Some(false) {
        missing.push("separator before Show Welcome Screen".to_string());
    }
    (!missing.is_empty()).then(|| missing.join(", "))
}

impl CanvasView {
    fn save_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let initial = self.layout_name.clone().unwrap_or_default();
        cx.spawn_in(window, async move |this, cx| {
            let answer = calm_native::prompt_text(&calm_native::TextPrompt {
                title: SAVE_TITLE,
                message: "",
                note: "Letters, digits, - and _.",
                placeholder: "my-layout",
                initial: &initial,
                ok: "Save",
            });
            let input = match answer {
                Ok(Some(input)) => input,
                Ok(None) => return,
                Err(err) => {
                    eprintln!("calm: save preset prompt unavailable: {err}");
                    return;
                }
            };
            let dir = layouts_dir();
            let builtin = calm_plugins::builtin_preset_dirs();
            let mut dialogs = NativeDialogs;
            let Some(name) = decide_preset_save(&input, &dir, &builtin, &mut dialogs) else {
                return;
            };
            let Ok((layout, browser_query)) = this.update(cx, |view, _| {
                (
                    view.scene.to_layout(),
                    view.settings.settings.session.browser_query,
                )
            }) else {
                return;
            };
            if let Err(err) = save_layout_in(&dir, &name, &layout, browser_query) {
                dialogs.alert(SAVE_TITLE, &format!("Preset {name} was not saved: {err}"));
            }
        })
        .detach();
    }

    fn load_preset(
        &mut self,
        pick: Pick,
        title: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let dir = layouts_dir();
            let builtin = calm_plugins::builtin_preset_dirs();
            let mut dialogs = NativeDialogs;
            let decision = decide_preset_load(title, pick, &dir, &builtin, &mut dialogs, || {
                this.update(cx, |view, cx| {
                    (
                        view.closing_panels(cx),
                        view.settings.settings.panels.confirm_close,
                    )
                })
                .ok()
            });
            let Some(decision) = decision else {
                return;
            };
            let refused = this
                .update_in(cx, |view, window, cx| {
                    view.apply_decided_load(decision, window, cx)
                })
                .ok()
                .flatten();
            if let Some(message) = refused {
                dialogs.alert(title, &message);
            }
        })
        .detach();
    }

    /// Applies an agreed load, unless what it would close changed since the user agreed
    /// (a dialog does not stop every task). Returns the message for a refusal.
    fn apply_decided_load(
        &mut self,
        decision: LoadDecision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let now = load_confirmation(
            &decision.name,
            &self.closing_panels(cx),
            self.settings.settings.panels.confirm_close,
        );
        if now != decision.agreed {
            return Some(format!(
                "The canvas changed while the dialog was open, so preset {} was not loaded.",
                decision.name
            ));
        }
        self.apply_preset(decision.layout, window, cx);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calm_core::geometry::Rect;
    use calm_core::preset::ClosingKind;
    use calm_core::scene::{PanelKind, Scene};
    use std::cell::Cell;
    use std::collections::VecDeque;

    /// Scripted answers, and a log of what was asked.
    #[derive(Default)]
    struct Script {
        choices: VecDeque<Result<Option<usize>, String>>,
        confirms: VecDeque<Result<bool, String>>,
        asked: Vec<String>,
    }

    impl Dialogs for Script {
        fn choose(&mut self, _: &str, options: &[String]) -> Result<Option<usize>, String> {
            self.asked.push(format!("choose {options:?}"));
            self.choices.pop_front().expect("unexpected choose")
        }
        fn confirm(&mut self, title: &str, message: &str, ok: &str) -> Result<bool, String> {
            self.asked
                .push(format!("confirm {title} | {message} | {ok}"));
            self.confirms.pop_front().expect("unexpected confirm")
        }
        fn alert(&mut self, _: &str, message: &str) {
            self.asked.push(format!("alert {message}"));
        }
    }

    fn write_preset(dir: &Path, name: &str) {
        let mut scene = Scene::default();
        scene.add(
            PanelKind::Browser {
                url: "about:blank".to_string(),
                page_zoom: None,
            },
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_string(&scene.to_layout()).unwrap(),
        )
        .unwrap();
    }

    fn live_terminals(n: usize) -> Vec<ClosingPanel> {
        vec![
            ClosingPanel {
                kind: ClosingKind::Terminal,
                confirms: true,
            };
            n
        ]
    }

    #[test]
    fn load_preset_cancel_changes_nothing() {
        let user = tempfile::tempdir().unwrap();
        let builtin = tempfile::tempdir().unwrap();
        write_preset(user.path(), "mine");
        write_preset(builtin.path(), "welcome");
        let dirs = [builtin.path().to_path_buf()];

        // Declining the confirmation: no decision, so the caller closes nothing.
        let mut script = Script::default();
        script.choices.push_back(Ok(Some(1)));
        script.confirms.push_back(Ok(false));
        let decision = decide_preset_load(
            LOAD_TITLE,
            Pick::Choose,
            user.path(),
            &dirs,
            &mut script,
            || Some((live_terminals(2), true)),
        );
        assert!(decision.is_none());
        assert_eq!(
            script.asked,
            [
                r#"choose ["mine", "welcome (built-in)"]"#.to_string(),
                "confirm Load Preset | Load preset welcome? This closes 2 terminals. | Load"
                    .to_string(),
            ]
        );

        // Cancelling the list: the canvas is never even inspected.
        let inspected = Cell::new(false);
        let mut script = Script::default();
        script.choices.push_back(Ok(None));
        let decision = decide_preset_load(
            LOAD_TITLE,
            Pick::Choose,
            user.path(),
            &dirs,
            &mut script,
            || {
                inspected.set(true);
                Some((live_terminals(1), true))
            },
        );
        assert!(decision.is_none());
        assert!(!inspected.get());

        // A dialog error is shown and nothing loads.
        let mut script = Script::default();
        script.confirms.push_back(Err("no main thread".to_string()));
        let decision = decide_preset_load(
            LOAD_TITLE,
            Pick::Named("mine".to_string()),
            user.path(),
            &dirs,
            &mut script,
            || Some((live_terminals(1), true)),
        );
        assert!(decision.is_none());
        assert_eq!(script.asked.last().unwrap(), "alert no main thread");
    }

    #[test]
    fn load_preset_agreed_or_unneeded_confirmation_loads() {
        let user = tempfile::tempdir().unwrap();
        write_preset(user.path(), "mine");
        let mut script = Script::default();
        script.confirms.push_back(Ok(true));
        let decision = decide_preset_load(
            LOAD_TITLE,
            Pick::Named("mine".to_string()),
            user.path(),
            &[],
            &mut script,
            || Some((live_terminals(1), true)),
        )
        .expect("agreed");
        assert_eq!(decision.name, "mine");
        assert_eq!(decision.layout.panels.len(), 1);
        assert_eq!(
            decision.agreed.as_deref(),
            Some("Load preset mine? This closes 1 terminal.")
        );

        // confirm_close off: no question at all.
        let mut script = Script::default();
        let decision = decide_preset_load(
            LOAD_TITLE,
            Pick::Named("mine".to_string()),
            user.path(),
            &[],
            &mut script,
            || Some((live_terminals(3), false)),
        )
        .expect("no question needed");
        assert_eq!(decision.agreed, None);
        assert!(script.asked.is_empty());
    }

    #[test]
    fn load_preset_missing_shows_the_error_and_changes_nothing() {
        let user = tempfile::tempdir().unwrap();
        let inspected = Cell::new(false);
        let mut script = Script::default();
        let decision = decide_preset_load(
            WELCOME_TITLE,
            Pick::Named("absent".to_string()),
            user.path(),
            &[],
            &mut script,
            || {
                inspected.set(true);
                Some((Vec::new(), true))
            },
        );
        assert!(decision.is_none());
        assert!(!inspected.get());
        assert_eq!(script.asked.len(), 1);
        assert!(script.asked[0].starts_with("alert Preset absent could not be loaded: "));

        // An empty list tells the user instead of showing an empty chooser.
        let mut script = Script::default();
        assert!(
            decide_preset_load(
                LOAD_TITLE,
                Pick::Choose,
                user.path(),
                &[],
                &mut script,
                || None
            )
            .is_none()
        );
        assert!(script.asked[0].starts_with("alert No presets found."));
    }

    #[test]
    fn menu_bar_has_the_preset_items() {
        let menus = crate::settings_menu::menus(false, &calm_core::settings::Settings::default());
        assert_eq!(missing_menu_items(&menus), None);
        assert!(missing_menu_items(&[]).is_some());
    }

    #[test]
    fn save_asks_before_replacing_or_shadowing() {
        let user = tempfile::tempdir().unwrap();
        let builtin = tempfile::tempdir().unwrap();
        write_preset(user.path(), "mine");
        write_preset(builtin.path(), "welcome");
        let dirs = [builtin.path().to_path_buf()];

        let mut script = Script::default();
        assert_eq!(
            decide_preset_save(" fresh ", user.path(), &dirs, &mut script),
            Some("fresh".to_string())
        );
        assert!(script.asked.is_empty());

        let mut script = Script::default();
        script.confirms.push_back(Ok(false));
        assert_eq!(
            decide_preset_save("mine", user.path(), &dirs, &mut script),
            None
        );
        assert_eq!(
            script.asked,
            ["confirm Replace preset mine? | Your saved preset mine is overwritten. | Replace"]
        );

        let mut script = Script::default();
        script.confirms.push_back(Ok(true));
        assert_eq!(
            decide_preset_save("welcome", user.path(), &dirs, &mut script),
            Some("welcome".to_string())
        );
        assert!(script.asked[0].contains("in place of the built-in preset welcome"));

        let mut script = Script::default();
        assert_eq!(
            decide_preset_save("my preset", user.path(), &dirs, &mut script),
            None
        );
        assert_eq!(
            script.asked,
            ["alert Preset names use letters, digits, - and _."]
        );
    }
}
