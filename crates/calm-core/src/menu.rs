//! Context menu contents and what each item does. The app shows them as a native menu.

use crate::geometry::{Point, Rect, Size};
use crate::scene::PanelKind;

pub const DEFAULT_TERMINAL_SIZE: Size = Size::new(640.0, 400.0);
pub const DEFAULT_BROWSER_SIZE: Size = Size::new(800.0, 600.0);

pub const ADD_TERMINAL: &str = "add.terminal";
pub const ADD_BROWSER: &str = "add.browser";
pub const ADD_PLUGIN_PREFIX: &str = "add.plugin.";
pub const PANEL_MINIMIZE: &str = "panel.minimize";
pub const PANEL_CLOSE: &str = "panel.close";

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
    Close,
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

/// Right click on a panel's chrome strip.
pub fn panel_menu(minimized: bool) -> Vec<MenuItem> {
    let minimize = if minimized { "Restore" } else { "Minimize" };
    vec![
        action(PANEL_MINIMIZE, minimize),
        MenuItem::Separator,
        action(PANEL_CLOSE, "Close"),
    ]
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
        PANEL_CLOSE => Some(MenuCommand::Close),
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

    #[test]
    fn panel_menu_labels_restore_when_minimized() {
        assert_eq!(
            panel_menu(false),
            vec![
                action("panel.minimize", "Minimize"),
                MenuItem::Separator,
                action("panel.close", "Close"),
            ]
        );
        assert_eq!(panel_menu(true)[0], action("panel.minimize", "Restore"));
        let world = Point::new(0.0, 0.0);
        assert_eq!(
            menu_action("panel.minimize", world, &[]),
            Some(MenuCommand::Minimize)
        );
        assert_eq!(
            menu_action("panel.close", world, &[]),
            Some(MenuCommand::Close)
        );
    }
}
