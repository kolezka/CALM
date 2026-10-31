//! The Settings menu bar menu and the Settings window glue.

use std::path::{Path, PathBuf};

use calm_core::settings::{Settings, save_settings_to};
use calm_core::settings_form::{
    FieldValue, FormCommand, FormContext, FormOutcome, apply_field, settings_form,
};
use calm_native::{SettingsWindow, choose_image_file};
use gpui::{
    App, AppContext as _, Context, InteractiveElement, KeyBinding, Menu, MenuItem, Window,
    WindowHandle, actions,
};
use gpui_libghostty::{TerminalConfiguration, TerminalOptions};

use crate::canvas_view::CanvasView;
use crate::dev_menu;

actions!(
    calm,
    [OpenSettings, ToggleGrid, OpenConfigFolder, ResetSettings]
);

/// Settings plus the file they persist to. The smoke points `path` at a temp dir.
pub struct SettingsState {
    pub settings: Settings,
    pub path: PathBuf,
    /// Shown at the top of the Settings window, such as a rejected file that was set aside.
    pub notice: Option<String>,
}

pub fn key_bindings() -> Vec<KeyBinding> {
    // No context, so it also works while a terminal has focus.
    vec![KeyBinding::new("cmd-,", OpenSettings, None)]
}

/// Every menu bar menu, so the Developer and Settings menus are always set together.
/// The app menu gets the standard "Settings…" item after About, as macOS apps have it.
pub fn menus(show_stats: bool, settings: &Settings) -> Vec<Menu> {
    let mut menus = dev_menu::menus(show_stats);
    if let Some(app_menu) = menus.first_mut() {
        // After "About CALM" and its separator.
        let at = app_menu.items.len().min(2);
        app_menu.items.splice(
            at..at,
            [
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
            ],
        );
    }
    menus.push(settings_menu(settings));
    menus
}

/// GPUI turns every top-level `Menu` into a menu bar item with a submenu, so "Settings"
/// cannot be a direct opener. Its first item opens the window instead.
fn settings_menu(settings: &Settings) -> Menu {
    Menu::new("Settings").items([
        MenuItem::action("Open Settings…", OpenSettings),
        MenuItem::separator(),
        MenuItem::action("Show Grid", ToggleGrid).checked(settings.grid.show),
        MenuItem::action("Open Config Folder", OpenConfigFolder),
        MenuItem::action("Reset to Defaults", ResetSettings),
    ])
}

pub fn install(cx: &mut App, settings: &Settings) {
    cx.set_menus(menus(false, settings));
}

/// One global handler for `OpenSettings`, used by `cmd-,` and both menu items. Global, so
/// it stays available while the Settings window, which is not a GPUI window, is key.
pub fn install_open_handler(cx: &mut App, canvas: WindowHandle<CanvasView>) {
    cx.on_action(move |_: &OpenSettings, cx| {
        // GPUI runs this while it holds the canvas window for the dispatch, so updating
        // that window here fails with "window not found". Defer until it is released.
        cx.defer(move |cx| {
            if let Err(err) = canvas.update(cx, |view, window, cx| view.open_settings(window, cx)) {
                eprintln!("calm: cannot open Settings: {err:#}");
            }
        });
    });
}

/// The window in `slot`, created by `create` only when there is none yet. Keeps every
/// entry point on one Settings window.
fn get_or_create<T, E>(
    slot: &mut Option<T>,
    create: impl FnOnce() -> Result<T, E>,
) -> Result<&T, E> {
    if slot.is_none() {
        *slot = Some(create()?);
    }
    Ok(slot.as_ref().expect("filled above"))
}

/// Attaches the Settings action handlers to the canvas root element.
pub fn bind<E: InteractiveElement>(element: E, cx: &mut Context<CanvasView>) -> E {
    element
        .on_action(cx.listener(|view, _: &ToggleGrid, window, cx| {
            view.update_settings(|s| s.grid.show = !s.grid.show, window, cx);
            view.refresh_settings_form();
        }))
        .on_action(cx.listener(|view, _: &ResetSettings, window, cx| {
            view.update_settings(|s| *s = Settings::default(), window, cx);
            view.refresh_settings_form();
        }))
        .on_action(cx.listener(|view, _: &OpenConfigFolder, _, _| {
            open_config_folder(&view.settings.path);
        }))
}

impl CanvasView {
    /// Applies `change`, saves at once and rebuilds the menus so check marks follow.
    pub fn update_settings(
        &mut self,
        change: impl FnOnce(&mut Settings),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let old_session = self.settings.settings.session.clone();
        change(&mut self.settings.settings);
        if let Err(err) = save_settings_to(&self.settings.path, &self.settings.settings) {
            eprintln!("calm: {err}");
        }
        self.apply_settings_to_input();
        self.reconcile_autosave(&old_session, cx);
        self.terminal_settings_changed(cx);
        self.sync_stats_timer(cx);
        let show_stats = self.show_stats;
        let settings = self.settings.settings.clone();
        cx.defer(move |cx| cx.set_menus(menus(show_stats, &settings)));
        window.refresh();
        cx.notify();
    }

    pub fn form_context(&self) -> FormContext {
        FormContext {
            show_stats: self.show_stats,
            settings_path: self.settings.path.display().to_string(),
            notice: self.settings.notice.clone(),
            image_error: self.background.error.clone(),
        }
    }

    /// Opens the Settings window, creating it on first use. When it is already open it is
    /// brought to the front; a second window is never made.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let created = get_or_create(&mut self.settings_window, || {
            let this = cx.entity().downgrade();
            let handle = window.window_handle();
            let async_cx = cx.to_async();
            let executor = cx.foreground_executor().clone();
            // AppKit calls this inside the control's action. Hop onto GPUI's executor so
            // the view is never updated while GPUI itself is mid-update.
            SettingsWindow::new(move |id, value| {
                let this = this.clone();
                let mut cx = async_cx.clone();
                executor
                    .spawn(async move {
                        cx.update_window(handle, |_, window, cx| {
                            this.update(cx, |view, cx| {
                                view.settings_field_changed(&id, value, window, cx);
                            })
                        })
                        .ok();
                    })
                    .detach();
            })
        });
        if let Err(err) = created {
            eprintln!("calm: settings window unavailable: {err}");
            return;
        }
        self.refresh_settings_form();
        if let Some(settings_window) = &self.settings_window {
            settings_window.show();
        }
    }

    /// Rebuilds the controls from the current state. A no-op until the window exists.
    pub fn refresh_settings_form(&self) {
        if let Some(settings_window) = &self.settings_window {
            settings_window.set_form(&settings_form(
                &self.settings.settings,
                &self.form_context(),
            ));
        }
    }

    fn settings_field_changed(
        &mut self,
        id: &str,
        value: FieldValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut next = self.settings.settings.clone();
        match apply_field(&mut next, id, value) {
            Ok(FormOutcome::Changed { rebuild }) => {
                self.update_settings(|s| *s = next, window, cx);
                if rebuild {
                    self.refresh_settings_form();
                }
            }
            Ok(FormOutcome::Command(FormCommand::SetShowStats(show))) => {
                self.set_show_stats(show, window, cx);
            }
            Ok(FormOutcome::Command(FormCommand::OpenConfigFolder)) => {
                open_config_folder(&self.settings.path);
            }
            Ok(FormOutcome::Command(FormCommand::ChooseImage)) => {
                // The open panel runs its own modal loop, so it runs outside this update.
                cx.spawn_in(window, async move |this, cx| {
                    let chosen = match choose_image_file() {
                        Ok(chosen) => chosen,
                        Err(err) => {
                            eprintln!("calm: image chooser unavailable: {err}");
                            return;
                        }
                    };
                    let Some(path) = chosen else { return };
                    this.update_in(cx, |view, window, cx| {
                        view.background.error = None;
                        view.update_settings(
                            |s| {
                                s.background.image.path = Some(path);
                                s.background.kind = calm_core::background::BackgroundKind::Image;
                            },
                            window,
                            cx,
                        );
                        view.refresh_settings_form();
                    })
                    .ok();
                })
                .detach();
            }
            // The form and `apply_field` disagree: a bug, so say so instead of guessing.
            Err(err) => eprintln!("calm: settings window: {err}"),
        }
    }
}

fn open_config_folder(settings_path: &Path) {
    let Some(dir) = settings_path.parent().map(PathBuf::from) else {
        eprintln!(
            "calm: settings path {} has no folder",
            settings_path.display()
        );
        return;
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("calm: cannot create {}: {err}", dir.display());
        return;
    }
    // `open` returns quickly, but waiting on it still belongs off the UI thread.
    std::thread::spawn(move || {
        match std::process::Command::new("/usr/bin/open")
            .arg(&dir)
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("calm: open {} exited with {status}", dir.display()),
            Err(err) => eprintln!("calm: open {} failed: {err}", dir.display()),
        }
    });
}

/// Without the user config Ghostty uses its built-in defaults.
pub fn apply_terminal_config(options: &mut TerminalOptions, settings: &Settings) {
    if !settings.panels.terminal_user_config {
        options.configuration = TerminalConfiguration::Default;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_or_create_makes_one_window_and_reuses_it() {
        let mut slot: Option<u32> = None;
        let mut created = 0;
        let mut open = |slot: &mut Option<u32>| {
            get_or_create(slot, || {
                created += 1;
                Ok::<u32, String>(created)
            })
            .copied()
        };
        assert_eq!(open(&mut slot), Ok(1));
        assert_eq!(open(&mut slot), Ok(1));
        assert_eq!(open(&mut slot), Ok(1));
        assert_eq!(created, 1);
    }

    #[test]
    fn get_or_create_retries_after_a_failed_create() {
        let mut slot: Option<u32> = None;
        assert_eq!(
            get_or_create(&mut slot, || Err("no main thread")),
            Err("no main thread")
        );
        assert!(slot.is_none());
        assert_eq!(
            get_or_create(&mut slot, || Ok::<u32, &str>(7)).copied(),
            Ok(7)
        );
    }

    #[test]
    fn every_menu_entry_opens_settings_through_one_action() {
        let menus = menus(false, &Settings::default());
        let openers: Vec<(String, String)> = menus
            .iter()
            .flat_map(|menu| {
                menu.items.iter().filter_map(move |item| match item {
                    MenuItem::Action { name, action, .. } if action.partial_eq(&OpenSettings) => {
                        Some((menu.name.to_string(), name.to_string()))
                    }
                    _ => None,
                })
            })
            .collect();
        assert_eq!(
            openers,
            vec![
                (menus[0].name.to_string(), "Settings…".to_string()),
                ("Settings".to_string(), "Open Settings…".to_string()),
            ]
        );
        // About first, then Settings, as on every macOS app.
        assert!(
            matches!(&menus[0].items[0], MenuItem::Action { name, .. } if name == "About CALM")
        );
        assert!(matches!(&menus[0].items[1], MenuItem::Separator));
        assert!(matches!(&menus[0].items[2], MenuItem::Action { name, .. } if name == "Settings…"));
        assert!(
            matches!(&menus.last().unwrap().items[0], MenuItem::Action { name, .. } if name == "Open Settings…")
        );
        assert!(
            key_bindings()
                .iter()
                .any(|binding| binding.action().partial_eq(&OpenSettings)),
            "cmd-, must dispatch the same action"
        );
    }
}
