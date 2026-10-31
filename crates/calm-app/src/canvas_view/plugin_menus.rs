//! Plugin context menu items: building the merged menus and running a chosen item.

use calm_core::plugin_menu::{
    self, MenuLocation, OpenMode, PluginMenus, command_payload, reuse_target,
};
use calm_plugins::BridgeEvent;

use super::*;

/// How long a command waits for a plugin page to finish loading before it is reported.
const COMMAND_LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const COMMAND_POLL: Duration = Duration::from_millis(50);
/// A panel opened for a panel menu item goes this far right of the clicked panel.
const NEW_PANEL_GAP: f64 = 24.0;

impl CanvasView {
    fn plugin_menus(&self) -> Vec<PluginMenus> {
        self.installed_plugins
            .iter()
            .filter(|plugin| !plugin.manifest.menus.is_empty())
            .map(|plugin| PluginMenus {
                plugin_id: plugin.manifest.id.clone(),
                plugin_name: plugin.manifest.name.clone(),
                items: plugin.manifest.menus.clone(),
            })
            .collect()
    }

    /// The canvas menu as the user would see it now: Add, then plugin items.
    pub fn canvas_menu(&self) -> Vec<MenuItem> {
        let builtin = menu::with_terminal_profiles(
            menu::canvas_menu(&self.plugins, &self.apps, &self.plugin_search_dirs),
            &self.settings.settings.terminal.profile_names(),
        );
        let section = plugin_menu::plugin_section(
            &self.plugin_menus(),
            MenuLocation::Canvas,
            None,
            &self.settings.settings.plugins.menus,
        );
        plugin_menu::merge(builtin, section)
    }

    /// The panel menu for `id` as the user would see it now: built-ins, then plugin items.
    pub fn panel_menu(&self, id: PanelId) -> Option<Vec<MenuItem>> {
        let panel = self.scene.get(id)?;
        let can_restart = self
            .tui_run(id)
            .is_some_and(|run| run.supervisor.can_restart());
        let section = plugin_menu::plugin_section(
            &self.plugin_menus(),
            MenuLocation::Panel,
            Some(panel),
            &self.settings.settings.plugins.menus,
        );
        Some(plugin_menu::merge(
            menu::panel_menu(panel, can_restart),
            section,
        ))
    }

    /// Runs a contributed item. Web plugins get a `command` event in an open panel, or in
    /// a new one opened for it; TUI plugins open a new panel with the item's `run` args.
    /// Returns false when `id` is not a contributed item.
    pub(super) fn apply_plugin_choice(
        &mut self,
        id: &str,
        target: MenuTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((plugin_id, item_id)) = plugin_menu::parse_menu_id(id) else {
            return false;
        };
        let Some(plugin) = self
            .installed_plugins
            .iter()
            .find(|plugin| plugin.manifest.id == plugin_id)
        else {
            eprintln!("calm: menu item {id}: plugin {plugin_id} is not installed");
            return true;
        };
        let Some(item) = plugin.manifest.menus.iter().find(|item| item.id == item_id) else {
            eprintln!("calm: menu item {id}: plugin {plugin_id} has no such item");
            return true;
        };
        let item = item.clone();
        let size = plugin.manifest.default_size;
        let size = Size::new(size.width, size.height);
        let is_tui = plugin.manifest.tui().is_some();
        let plugin_id = plugin_id.to_string();

        let (location, clicked, world) = match target {
            MenuTarget::Canvas { world } => (MenuLocation::Canvas, None, Some(world)),
            MenuTarget::Panel(panel) => (MenuLocation::Panel, Some(panel), None),
        };
        let clicked_panel = clicked.and_then(|panel| self.scene.get(panel)).cloned();
        let origin = world.unwrap_or_else(|| {
            clicked_panel
                .as_ref()
                .map_or(Point::new(0.0, 0.0), |panel| {
                    Point::new(
                        panel.rect.origin.x + panel.rect.size.width + NEW_PANEL_GAP,
                        panel.rect.origin.y,
                    )
                })
        });

        if is_tui {
            self.pending_tui_args = Some(item.run.clone().unwrap_or_default());
            let kind = PanelKind::Tui {
                plugin_id,
                plugin_version: None,
            };
            self.open_panel_for_command(kind, Rect { origin, size }, window, cx);
            self.pending_tui_args = None;
            return true;
        }

        let payload = command_payload(&item.id, location, clicked_panel.as_ref(), world);
        let reuse = match item.open {
            OpenMode::Reuse => reuse_target(&self.scene, &plugin_id, clicked, self.focused),
            OpenMode::New => None,
        };
        let panel = match reuse {
            Some(panel) => panel,
            None => {
                let kind = PanelKind::Plugin {
                    plugin_id,
                    plugin_version: None,
                    settings: serde_json::Value::Object(serde_json::Map::new()),
                };
                self.open_panel_for_command(kind, Rect { origin, size }, window, cx)
            }
        };
        self.deliver_command(
            panel,
            BridgeEvent {
                event: "command".to_string(),
                payload,
            },
            cx,
        );
        true
    }

    /// Adds a panel through the same effects as the Add menu and returns its id.
    fn open_panel_for_command(
        &mut self,
        kind: PanelKind,
        rect: Rect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PanelId {
        let id = self.scene.add(kind.clone(), rect);
        let rect = self.scene.get(id).map_or(rect, |panel| panel.rect);
        self.apply_effects(
            vec![Effect::CreatePanel { kind, rect }, Effect::FocusPanel(id)],
            window,
            cx,
        );
        id
    }

    /// Sends `event` to the plugin page of `panel` once it has loaded. The prelude holds
    /// commands until the page registers a handler. A command that cannot be delivered is
    /// logged, never dropped silently.
    fn deliver_command(&mut self, panel: PanelId, event: BridgeEvent, cx: &mut Context<Self>) {
        if self.try_deliver(panel, &event) == Delivery::Waiting {
            let started = Instant::now();
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(COMMAND_POLL).await;
                    let state = this.update(cx, |view, _| view.try_deliver(panel, &event));
                    match state {
                        Ok(Delivery::Waiting) if started.elapsed() < COMMAND_LOAD_TIMEOUT => {}
                        Ok(Delivery::Waiting) => {
                            eprintln!(
                                "calm: plugin panel {}: command {} not delivered, the page \
                                 did not load within {} s",
                                panel.0,
                                event.payload["id"],
                                COMMAND_LOAD_TIMEOUT.as_secs()
                            );
                            break;
                        }
                        Ok(_) | Err(_) => break,
                    }
                }
            })
            .detach();
        }
    }

    fn try_deliver(&mut self, panel: PanelId, event: &BridgeEvent) -> Delivery {
        let Some((_, page)) = self.plugin_panels.iter_mut().find(|(id, _)| *id == panel) else {
            eprintln!(
                "calm: plugin panel {}: command {} not delivered, the panel is gone",
                panel.0, event.payload["id"]
            );
            return Delivery::Gone;
        };
        if page.is_loading() {
            return Delivery::Waiting;
        }
        if let Some(err) = page.load_error() {
            eprintln!(
                "calm: plugin panel {}: command {} sent to a page that failed to load: {err}",
                panel.0, event.payload["id"]
            );
        }
        page.emit(event);
        Delivery::Sent
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Delivery {
    Sent,
    Waiting,
    Gone,
}
