//! One Settings window per panel, opened from the panel's context menu.

use calm_core::panel_form::{apply_panel_field, panel_effective, panel_form};
use calm_core::panel_icons::import_icon;
use calm_core::scene::PanelId;
use calm_core::settings_form::{FieldValue, FormCommand, FormOutcome};
use calm_native::{SettingsWindow, WindowLayout, choose_image_file};
use gpui::{AppContext as _, Context, Window};

use crate::canvas_view::CanvasView;

/// The window title for a panel whose effective title is `title`.
pub fn window_title(title: &str) -> String {
    format!("{title} Settings")
}

impl CanvasView {
    /// Opens panel `id`'s Settings window, or brings the open one to the front.
    pub fn open_panel_settings(
        &mut self,
        id: PanelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.scene.get(id) else {
            eprintln!("calm: panel settings: no panel {}", id.0);
            return;
        };
        if !self.panel_settings_windows.contains_key(&id) {
            let title = window_title(&panel_effective(panel, &self.settings.settings).title);
            let this = cx.entity().downgrade();
            let handle = window.window_handle();
            let async_cx = cx.to_async();
            let executor = cx.foreground_executor().clone();
            // As in the global window: AppKit calls this inside the control's action, so
            // the change waits for GPUI's executor instead of updating the view mid-update.
            let created = SettingsWindow::new(&title, WindowLayout::Single, move |field, value| {
                let this = this.clone();
                let mut cx = async_cx.clone();
                executor
                    .spawn(async move {
                        cx.update_window(handle, |_, window, cx| {
                            this.update(cx, |view, cx| {
                                view.panel_settings_changed(id, &field, value, window, cx);
                            })
                        })
                        .ok();
                    })
                    .detach();
            });
            match created {
                Ok(settings_window) => {
                    self.panel_settings_windows.insert(id, settings_window);
                }
                Err(err) => {
                    eprintln!("calm: panel settings window unavailable: {err}");
                    return;
                }
            }
        }
        self.refresh_panel_settings(id);
        if let Some(settings_window) = self.panel_settings_windows.get(&id) {
            settings_window.show();
        }
    }

    /// Rebuilds panel `id`'s window from the current state, title included.
    pub fn refresh_panel_settings(&self, id: PanelId) {
        let (Some(settings_window), Some(panel)) =
            (self.panel_settings_windows.get(&id), self.scene.get(id))
        else {
            return;
        };
        let effective = panel_effective(panel, &self.settings.settings);
        settings_window.set_title(&window_title(&effective.title));
        settings_window.set_form(&panel_form(panel, &effective));
    }

    /// Closes panel `id`'s window, if it has one. The panel is gone or going.
    pub fn close_panel_settings(&mut self, id: PanelId) {
        if let Some(settings_window) = self.panel_settings_windows.remove(&id) {
            settings_window.close();
        }
    }

    pub(crate) fn panel_settings_changed(
        &mut self,
        id: PanelId,
        field: &str,
        value: FieldValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.scene.get(id) else {
            // A change queued just before the panel closed. Nothing to apply it to.
            if self.gone_panel_changes.insert(id) {
                eprintln!(
                    "calm: panel settings: panel {} is gone, ignored {field:?}",
                    id.0
                );
            }
            return;
        };
        let inherited = panel_effective(panel, &self.settings.settings).colors;
        match apply_panel_field(&mut self.scene, id, field, value, inherited.as_ref()) {
            Ok(FormOutcome::Changed { rebuild }) => {
                self.apply_panel_overrides(id, cx);
                if rebuild {
                    self.refresh_panel_settings(id);
                }
            }
            Ok(FormOutcome::Command(FormCommand::ChoosePanelIcon(target))) => {
                self.choose_panel_icon(target, window, cx);
            }
            Ok(FormOutcome::Command(command)) => {
                eprintln!("calm: panel settings: unexpected {command:?}");
            }
            // Bad input changes nothing; the window keeps the last good state.
            Err(err) => eprintln!("calm: panel settings: {err}"),
        }
    }

    /// Runs the image chooser, copies the pick into the settings folder and makes it the
    /// panel's icon.
    fn choose_panel_icon(&mut self, id: PanelId, window: &mut Window, cx: &mut Context<Self>) {
        // The open panel runs its own modal loop, so it runs outside this update.
        cx.spawn_in(window, async move |this, cx| {
            let chosen = match choose_image_file() {
                Ok(chosen) => chosen,
                Err(err) => {
                    eprintln!("calm: image chooser unavailable: {err}");
                    return;
                }
            };
            let Some(source) = chosen else { return };
            this.update_in(cx, |view, window, cx| {
                let relative = match import_icon(&view.settings_dir(), &source) {
                    Ok(relative) => relative,
                    Err(err) => {
                        eprintln!("calm: {err}");
                        return;
                    }
                };
                view.panel_settings_changed(
                    id,
                    "panel.icon",
                    FieldValue::Text(relative),
                    window,
                    cx,
                );
                // An image replacing an image keeps the same kind, so force the rebuild.
                view.refresh_panel_settings(id);
            })
            .ok();
        })
        .detach();
    }
}
