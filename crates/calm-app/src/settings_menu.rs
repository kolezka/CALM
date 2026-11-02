//! The Settings menu bar menu and the Settings window glue.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use calm_core::settings::{Settings, save_settings_to};
use calm_core::settings_form::{
    FieldValue, FormCommand, FormContext, FormOutcome, WINDOW_CLOSED_FIELD, apply_field,
    settings_form,
};
use calm_core::settings_save::{SaveDebounce, keymap_affected};
use calm_native::{SettingsWindow, WindowLayout, choose_image_file};
use gpui::{
    App, AppContext as _, Context, InteractiveElement, Menu, MenuItem, Window, WindowHandle,
    actions,
};

use crate::canvas_view::CanvasView;
use crate::dev_menu;

actions!(
    calm,
    [OpenSettings, ToggleGrid, OpenConfigFolder, ResetSettings]
);

/// How long a refused key's reason stays on its row.
const KEY_MESSAGE_SECS: u64 = 3;

/// Settings plus the file they persist to. The smoke points `path` at a temp dir.
pub struct SettingsState {
    pub settings: Settings,
    pub path: PathBuf,
    /// Shown at the top of the Settings window, such as a rejected file that was set aside.
    pub notice: Option<String>,
    /// A Settings window change not yet written, held while a slider or well is dragged.
    pub save: SaveDebounce,
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
        self.apply_settings_change(change, false, window, cx);
    }

    /// With `debounced`, the write waits for `session.settings_save_ms` of quiet, so a
    /// drag writes once. The key bindings and menus rebuild only when they read a change.
    fn apply_settings_change(
        &mut self,
        change: impl FnOnce(&mut Settings),
        debounced: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let old = self.settings.settings.clone();
        change(&mut self.settings.settings);
        if debounced {
            self.schedule_settings_save(cx);
        } else {
            self.settings.save.take_pending();
            self.write_settings();
        }
        self.apply_settings_to_input();
        self.reconcile_autosave(&old.session, cx);
        self.terminal_settings_changed(cx);
        self.sync_stats_timer(cx);
        self.refresh_all_panel_settings();
        if keymap_affected(&old, &self.settings.settings) {
            let show_stats = self.show_stats;
            let settings = self.settings.settings.clone();
            cx.defer(move |cx| crate::keymap::apply(cx, &settings, show_stats));
        }
        window.refresh();
        cx.notify();
    }

    fn write_settings(&self) {
        if let Err(err) = save_settings_to(&self.settings.path, &self.settings.settings) {
            eprintln!("calm: {err}");
        }
    }

    fn settings_save_delay(&self) -> Duration {
        Duration::from_millis(self.settings.settings.session.settings_save_ms)
    }

    /// Each change starts a timer; only the one that finds the drag quiet writes.
    fn schedule_settings_save(&mut self, cx: &mut Context<Self>) {
        let delay = self.settings_save_delay();
        self.settings.save.note_change(Instant::now());
        if delay.is_zero() {
            self.save_settings_if_due();
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |view, _| view.save_settings_if_due()).ok();
        })
        .detach();
    }

    fn save_settings_if_due(&mut self) {
        let delay = self.settings_save_delay();
        if self.settings.save.take_due(Instant::now(), delay) {
            self.write_settings();
        }
    }

    /// Writes a held change now: the Settings window closed or the app quits.
    pub fn flush_settings_save(&mut self) {
        if self.settings.save.take_pending() {
            self.write_settings();
        }
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
            SettingsWindow::new("CALM Settings", WindowLayout::Tabs, move |id, value| {
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
            settings_window.show_above(window);
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

    pub(crate) fn settings_field_changed(
        &mut self,
        id: &str,
        value: FieldValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if id == WINDOW_CLOSED_FIELD {
            self.flush_settings_save();
            return;
        }
        let mut next = self.settings.settings.clone();
        match apply_field(&mut next, id, value) {
            // A value change, as a slider or color well sends on every tick of a drag.
            Ok(FormOutcome::Changed { rebuild: false }) => {
                self.apply_settings_change(|s| *s = next, true, window, cx);
            }
            Ok(FormOutcome::Changed { rebuild: true }) => {
                self.update_settings(|s| *s = next, window, cx);
                self.refresh_settings_form();
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
                    let chosen = match choose_image_file("Choose a Background Image") {
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
            // Only the per-panel form sends this.
            Ok(FormOutcome::Command(command @ FormCommand::ChoosePanelIcon(_))) => {
                eprintln!("calm: settings window: unexpected {command:?}");
            }
            // The row says why for a moment, then shows its key again.
            Ok(FormOutcome::KeyRefused { action, reason }) => {
                let field = format!("keys.{action}");
                if let Some(settings_window) = &self.settings_window {
                    settings_window.show_field_message(&field, &reason.message());
                }
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_secs(KEY_MESSAGE_SECS))
                        .await;
                    let _ = this.update(cx, |view, _| {
                        if let Some(settings_window) = &view.settings_window {
                            settings_window.clear_field_message(&field);
                        }
                    });
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
            crate::keymap::key_bindings(&Settings::default())
                .iter()
                .any(|binding| binding.action().partial_eq(&OpenSettings)),
            "cmd-, must dispatch the same action"
        );
    }
}
