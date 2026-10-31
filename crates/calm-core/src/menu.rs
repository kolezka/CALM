//! Context menu contents and what each item does. The app shows them as a native menu.

use crate::geometry::{Point, Rect, Size};
use crate::scene::{Panel, PanelKind};

pub const DEFAULT_TERMINAL_SIZE: Size = Size::new(640.0, 400.0);
pub const DEFAULT_BROWSER_SIZE: Size = Size::new(800.0, 600.0);

pub const ADD_TERMINAL: &str = "add.terminal";
pub const ADD_BROWSER: &str = "add.browser";
pub const ADD_PLUGIN_PREFIX: &str = "add.plugin.";
pub const PANEL_MINIMIZE: &str = "panel.minimize";
pub const PANEL_ZOOM_TO_FIT: &str = "panel.zoom_to_fit";
pub const PANEL_BRING_TO_FRONT: &str = "panel.bring_to_front";
pub const PANEL_SEND_TO_BACK: &str = "panel.send_to_back";
pub const PANEL_DUPLICATE: &str = "panel.duplicate";
/// Goes through the same confirmed close as every other close.
pub const PANEL_DELETE: &str = "panel.delete";

#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    Action { id: String, label: String },
    Submenu { label: String, items: Vec<MenuItem> },
    Separator,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuCommand {
    AddPanel { kind: PanelKind, rect: Rect },
    Minimize,
    ZoomToFit,
    BringToFront,
    SendToBack,
    Duplicate,
    Delete,
}

fn action(id: &str, label: &str) -> MenuItem {
    MenuItem::Action {
        id: id.to_string(),
        label: label.to_string(),
    }
}

/// Right click on empty canvas. `plugins` is (id, name) per installed plugin.
pub fn canvas_menu(plugins: &[(String, String)]) -> Vec<MenuItem> {
    let mut items = vec![
        action(ADD_TERMINAL, "Terminal"),
        action(ADD_BROWSER, "Browser"),
    ];
    if !plugins.is_empty() {
        items.push(MenuItem::Separator);
    }
    for (plugin_id, name) in plugins {
        items.push(action(&format!("{ADD_PLUGIN_PREFIX}{plugin_id}"), name));
    }
    vec![MenuItem::Submenu {
        label: "Add".to_string(),
        items,
    }]
}

/// Right click on a panel's chrome strip or minimized chip.
/// Zoom to Fit needs the full panel, so a minimized panel omits it. Duplicate is offered
/// for terminals only: browser and plugin panels have no native view in the app yet.
pub fn panel_menu(panel: &Panel) -> Vec<MenuItem> {
    let minimize = if panel.minimized {
        "Restore"
    } else {
        "Minimize"
    };
    let mut items = vec![action(PANEL_MINIMIZE, minimize)];
    if !panel.minimized {
        items.push(action(PANEL_ZOOM_TO_FIT, "Zoom to Fit"));
    }
    items.extend([
        MenuItem::Separator,
        action(PANEL_BRING_TO_FRONT, "Bring to Front"),
        action(PANEL_SEND_TO_BACK, "Send to Back"),
    ]);
    if matches!(panel.kind, PanelKind::Terminal { .. }) {
        items.extend([MenuItem::Separator, action(PANEL_DUPLICATE, "Duplicate")]);
    }
    items.extend([MenuItem::Separator, action(PANEL_DELETE, "Delete")]);
    items
}

/// Maps a chosen id to a command. A new panel's origin is the right-click point `world`.
/// `plugin_sizes` is (id, default size) per installed plugin; unknown plugins give `None`.
pub fn menu_action(id: &str, world: Point, plugin_sizes: &[(String, Size)]) -> Option<MenuCommand> {
    let add = |kind: PanelKind, size: Size| {
        Some(MenuCommand::AddPanel {
            kind,
            rect: Rect {
                origin: world,
                size,
            },
        })
    };
    match id {
        ADD_TERMINAL => add(
            PanelKind::Terminal {
                command: None,
                cwd: None,
            },
            DEFAULT_TERMINAL_SIZE,
        ),
        ADD_BROWSER => add(
            PanelKind::Browser {
                url: "about:blank".to_string(),
            },
            DEFAULT_BROWSER_SIZE,
        ),
        PANEL_MINIMIZE => Some(MenuCommand::Minimize),
        PANEL_ZOOM_TO_FIT => Some(MenuCommand::ZoomToFit),
        PANEL_BRING_TO_FRONT => Some(MenuCommand::BringToFront),
        PANEL_SEND_TO_BACK => Some(MenuCommand::SendToBack),
        PANEL_DUPLICATE => Some(MenuCommand::Duplicate),
        PANEL_DELETE => Some(MenuCommand::Delete),
        _ => {
            let plugin_id = id.strip_prefix(ADD_PLUGIN_PREFIX)?;
            let (_, size) = plugin_sizes.iter().find(|(known, _)| known == plugin_id)?;
            add(
                PanelKind::Plugin {
                    plugin_id: plugin_id.to_string(),
                    settings: serde_json::Value::Object(serde_json::Map::new()),
                },
                *size,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugins() -> Vec<(String, String)> {
        vec![
            ("hello-world".to_string(), "Hello World".to_string()),
            ("github-prs".to_string(), "GitHub PRs".to_string()),
        ]
    }

    #[test]
    fn canvas_menu_lists_add_submenu_with_terminal_browser_and_plugins() {
        assert_eq!(
            canvas_menu(&plugins()),
            vec![MenuItem::Submenu {
                label: "Add".to_string(),
                items: vec![
                    action("add.terminal", "Terminal"),
                    action("add.browser", "Browser"),
                    MenuItem::Separator,
                    action("add.plugin.hello-world", "Hello World"),
                    action("add.plugin.github-prs", "GitHub PRs"),
                ],
            }]
        );
    }

    #[test]
    fn canvas_menu_without_plugins_has_no_trailing_separator() {
        assert_eq!(
            canvas_menu(&[]),
            vec![MenuItem::Submenu {
                label: "Add".to_string(),
                items: vec![
                    action("add.terminal", "Terminal"),
                    action("add.browser", "Browser"),
                ],
            }]
        );
    }

    #[test]
    fn menu_action_add_terminal_places_default_rect_at_click() {
        let world = Point::new(120.0, -40.0);
        assert_eq!(
            menu_action("add.terminal", world, &[]),
            Some(MenuCommand::AddPanel {
                kind: PanelKind::Terminal {
                    command: None,
                    cwd: None
                },
                rect: Rect::new(120.0, -40.0, 640.0, 400.0),
            })
        );
        assert_eq!(
            menu_action("add.browser", world, &[]),
            Some(MenuCommand::AddPanel {
                kind: PanelKind::Browser {
                    url: "about:blank".to_string()
                },
                rect: Rect::new(120.0, -40.0, 800.0, 600.0),
            })
        );
        assert_eq!(menu_action("add.nothing", world, &[]), None);
    }

    #[test]
    fn menu_action_add_plugin_uses_plugin_default_size() {
        let sizes = vec![("hello-world".to_string(), Size::new(360.0, 240.0))];
        let world = Point::new(10.0, 20.0);
        let Some(MenuCommand::AddPanel { kind, rect }) =
            menu_action("add.plugin.hello-world", world, &sizes)
        else {
            panic!("expected AddPanel");
        };
        assert!(matches!(kind, PanelKind::Plugin { plugin_id, .. } if plugin_id == "hello-world"));
        assert_eq!(rect, Rect::new(10.0, 20.0, 360.0, 240.0));
        assert_eq!(menu_action("add.plugin.missing", world, &sizes), None);
    }

    fn panel(kind: PanelKind, minimized: bool) -> Panel {
        Panel {
            id: crate::scene::PanelId(1),
            kind,
            rect: Rect::new(0.0, 0.0, 400.0, 300.0),
            title: "Terminal".to_string(),
            minimized,
        }
    }

    fn terminal_kind() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
        }
    }

    #[test]
    fn panel_menu_for_terminal_lists_all_actions_with_delete_last() {
        assert_eq!(
            panel_menu(&panel(terminal_kind(), false)),
            vec![
                action("panel.minimize", "Minimize"),
                action("panel.zoom_to_fit", "Zoom to Fit"),
                MenuItem::Separator,
                action("panel.bring_to_front", "Bring to Front"),
                action("panel.send_to_back", "Send to Back"),
                MenuItem::Separator,
                action("panel.duplicate", "Duplicate"),
                MenuItem::Separator,
                action("panel.delete", "Delete"),
            ]
        );
    }

    #[test]
    fn panel_menu_minimized_says_restore_and_drops_zoom_to_fit() {
        let items = panel_menu(&panel(terminal_kind(), true));
        assert_eq!(items[0], action("panel.minimize", "Restore"));
        assert!(!items.contains(&action("panel.zoom_to_fit", "Zoom to Fit")));
        assert_eq!(items.last(), Some(&action("panel.delete", "Delete")));
    }

    #[test]
    fn panel_menu_offers_duplicate_only_for_terminals() {
        let browser = PanelKind::Browser {
            url: "about:blank".to_string(),
        };
        let items = panel_menu(&panel(browser, false));
        assert!(!items.contains(&action("panel.duplicate", "Duplicate")));
        assert_eq!(items.last(), Some(&action("panel.delete", "Delete")));
        // No two separators in a row once Duplicate is gone.
        assert!(
            !items
                .windows(2)
                .any(|pair| pair == [MenuItem::Separator, MenuItem::Separator])
        );
    }

    #[test]
    fn every_panel_menu_id_maps_to_its_command() {
        let world = Point::new(0.0, 0.0);
        let cases = [
            ("panel.minimize", MenuCommand::Minimize),
            ("panel.zoom_to_fit", MenuCommand::ZoomToFit),
            ("panel.bring_to_front", MenuCommand::BringToFront),
            ("panel.send_to_back", MenuCommand::SendToBack),
            ("panel.duplicate", MenuCommand::Duplicate),
            ("panel.delete", MenuCommand::Delete),
        ];
        for (id, command) in cases {
            assert_eq!(menu_action(id, world, &[]), Some(command), "{id}");
        }
        // Every id the menu can show has a command, so no item is a dead click.
        for item in panel_menu(&panel(terminal_kind(), false)) {
            if let MenuItem::Action { id, .. } = item {
                assert!(menu_action(&id, world, &[]).is_some(), "{id}");
            }
        }
        assert_eq!(menu_action("panel.close", world, &[]), None);
    }
}
