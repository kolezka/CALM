//! Context menu contents and what each item does. The app shows them as a native menu.

use crate::geometry::{Point, Rect, Size};
use crate::scene::{Panel, PanelKind};

pub const DEFAULT_TERMINAL_SIZE: Size = Size::new(640.0, 400.0);
pub const DEFAULT_BROWSER_SIZE: Size = Size::new(800.0, 600.0);

pub const ADD_TERMINAL: &str = "add.terminal";
pub const ADD_BROWSER: &str = "add.browser";
pub const ADD_PLUGIN_PREFIX: &str = "add.plugin.";
pub const ADD_TUI_PREFIX: &str = "add.tui.";
/// A plain terminal opened with the named profile from `terminal.profiles`.
pub const ADD_TERMINAL_PROFILE_PREFIX: &str = "add.terminal_profile.";
pub const TERMINAL_PROFILES_LABEL: &str = "Terminal Profiles";
/// Section header above Terminal and Browser in the Add menu.
pub const NATIVE_SECTION_LABEL: &str = "Native";
/// Section header above the plugin entries in the Add menu.
pub const PLUGINS_SECTION_LABEL: &str = "Plugins";
/// Opens the global Settings window, as `cmd-,` does.
pub const OPEN_SETTINGS: &str = "app.settings";
/// Starts an app or typed command again after it exited.
pub const PANEL_RESTART: &str = "panel.restart";
pub const PANEL_MINIMIZE: &str = "panel.minimize";
pub const PANEL_ZOOM_TO_FIT: &str = "panel.zoom_to_fit";
pub const PANEL_BRING_TO_FRONT: &str = "panel.bring_to_front";
pub const PANEL_SEND_TO_BACK: &str = "panel.send_to_back";
pub const PANEL_DUPLICATE: &str = "panel.duplicate";
/// Goes through the same confirmed close as every other close.
pub const PANEL_DELETE: &str = "panel.delete";
/// Opens this panel's own Settings window.
pub const PANEL_SETTINGS: &str = "panel.settings";

#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    Action {
        id: String,
        label: String,
    },
    Submenu {
        label: String,
        items: Vec<MenuItem>,
    },
    /// Shown greyed out, picks nothing. Used to explain an empty list.
    Disabled {
        label: String,
    },
    /// A non-interactive title that names the group of items below it.
    Header {
        label: String,
    },
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
    Restart,
    OpenSettings,
    PanelSettings,
}

fn action(id: &str, label: &str) -> MenuItem {
    MenuItem::Action {
        id: id.to_string(),
        label: label.to_string(),
    }
}

fn disabled(label: &str) -> MenuItem {
    MenuItem::Disabled {
        label: label.to_string(),
    }
}

/// Right click on empty canvas. `plugins` is (id, name) per installed web plugin and
/// `apps` the same per TUI plugin. Terminal and Browser sit under a "Native" header; apps
/// and plugins share one section under a "Plugins" header, apps first. `searched_dirs` is shown when no plugin of either kind was found, so the user knows
/// where to put one.
pub fn canvas_menu(
    plugins: &[(String, String)],
    apps: &[(String, String)],
    searched_dirs: &[String],
) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::Header {
            label: NATIVE_SECTION_LABEL.to_string(),
        },
        action(ADD_TERMINAL, "Terminal"),
        action(ADD_BROWSER, "Browser"),
        MenuItem::Separator,
        MenuItem::Header {
            label: PLUGINS_SECTION_LABEL.to_string(),
        },
    ];
    for (plugin_id, name) in apps {
        items.push(action(&format!("{ADD_TUI_PREFIX}{plugin_id}"), name));
    }
    if plugins.is_empty() && apps.is_empty() {
        items.push(disabled("No plugins found"));
        let searched: Vec<MenuItem> = if searched_dirs.is_empty() {
            vec![disabled("(no folders)")]
        } else {
            searched_dirs.iter().map(|dir| disabled(dir)).collect()
        };
        items.push(MenuItem::Submenu {
            label: "Searched Folders".to_string(),
            items: searched,
        });
    }
    for (plugin_id, name) in plugins {
        items.push(action(&format!("{ADD_PLUGIN_PREFIX}{plugin_id}"), name));
    }
    vec![MenuItem::Submenu {
        label: "Add".to_string(),
        items,
    }]
}

/// Adds a "Terminal Profiles" submenu right after Terminal in the Add menu, one item per
/// (id, name) profile. No profiles leaves the menu as it is.
pub fn with_terminal_profiles(
    mut items: Vec<MenuItem>,
    profiles: &[(String, String)],
) -> Vec<MenuItem> {
    if profiles.is_empty() {
        return items;
    }
    let submenu = MenuItem::Submenu {
        label: TERMINAL_PROFILES_LABEL.to_string(),
        items: profiles
            .iter()
            .map(|(id, name)| action(&format!("{ADD_TERMINAL_PROFILE_PREFIX}{id}"), name))
            .collect(),
    };
    for item in &mut items {
        if let MenuItem::Submenu { items: add, .. } = item {
            let at = add.iter().position(
                |entry| matches!(entry, MenuItem::Action { id, .. } if id == ADD_TERMINAL),
            );
            if let Some(at) = at {
                add.insert(at + 1, submenu);
                break;
            }
        }
    }
    items
}

/// Adds "Settings…" below the Add menu, split off by a separator.
pub fn with_settings(mut items: Vec<MenuItem>) -> Vec<MenuItem> {
    items.extend([MenuItem::Separator, action(OPEN_SETTINGS, "Settings…")]);
    items
}

/// The profile id a menu choice asks for, if it is a profile item.
pub fn terminal_profile_choice(id: &str) -> Option<&str> {
    id.strip_prefix(ADD_TERMINAL_PROFILE_PREFIX)
}

/// Right click on a panel's chrome strip or minimized chip.
/// Zoom to Fit needs the full panel, so a minimized panel omits it. Duplicate is offered
/// for terminals only: browser and plugin panels have no native view in the app yet.
/// `can_restart` adds Restart on top, for an app or typed command whose process exited.
pub fn panel_menu(panel: &Panel, can_restart: bool) -> Vec<MenuItem> {
    let minimize = if panel.minimized {
        "Restore"
    } else {
        "Minimize"
    };
    let mut items = Vec::new();
    if can_restart {
        items.extend([action(PANEL_RESTART, "Restart"), MenuItem::Separator]);
    }
    items.push(action(PANEL_MINIMIZE, minimize));
    if !panel.minimized {
        items.push(action(PANEL_ZOOM_TO_FIT, "Zoom to Fit"));
    }
    items.extend([
        MenuItem::Separator,
        action(PANEL_BRING_TO_FRONT, "Bring to Front"),
        action(PANEL_SEND_TO_BACK, "Send to Back"),
    ]);
    items.extend([MenuItem::Separator, action(PANEL_SETTINGS, "Settings…")]);
    if panel.kind.is_terminal() {
        items.extend([MenuItem::Separator, action(PANEL_DUPLICATE, "Duplicate")]);
    }
    items.extend([MenuItem::Separator, action(PANEL_DELETE, "Delete")]);
    items
}

/// A restored panel that cannot run (missing plugin, unknown kind) offers Delete only.
pub fn placeholder_menu() -> Vec<MenuItem> {
    vec![action(PANEL_DELETE, "Delete")]
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
        _ if terminal_profile_choice(id).is_some() => {
            menu_action(ADD_TERMINAL, world, plugin_sizes)
        }
        ADD_TERMINAL => add(PanelKind::terminal(), DEFAULT_TERMINAL_SIZE),
        ADD_BROWSER => add(
            PanelKind::Browser {
                url: "about:blank".to_string(),
                page_zoom: None,
            },
            DEFAULT_BROWSER_SIZE,
        ),
        PANEL_MINIMIZE => Some(MenuCommand::Minimize),
        PANEL_ZOOM_TO_FIT => Some(MenuCommand::ZoomToFit),
        PANEL_BRING_TO_FRONT => Some(MenuCommand::BringToFront),
        PANEL_SEND_TO_BACK => Some(MenuCommand::SendToBack),
        PANEL_DUPLICATE => Some(MenuCommand::Duplicate),
        PANEL_DELETE => Some(MenuCommand::Delete),
        PANEL_SETTINGS => Some(MenuCommand::PanelSettings),
        PANEL_RESTART => Some(MenuCommand::Restart),
        OPEN_SETTINGS => Some(MenuCommand::OpenSettings),
        _ if id.starts_with(ADD_TUI_PREFIX) => {
            let plugin_id = &id[ADD_TUI_PREFIX.len()..];
            let (_, size) = plugin_sizes.iter().find(|(known, _)| known == plugin_id)?;
            add(PanelKind::app_terminal(plugin_id, None), *size)
        }
        _ => {
            let plugin_id = id.strip_prefix(ADD_PLUGIN_PREFIX)?;
            let (_, size) = plugin_sizes.iter().find(|(known, _)| known == plugin_id)?;
            add(
                PanelKind::Plugin {
                    plugin_id: plugin_id.to_string(),
                    plugin_version: None,
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

    #[test]
    fn terminal_profiles_get_a_submenu_after_terminal() {
        let base = canvas_menu(&[], &[], &[]);
        assert_eq!(with_terminal_profiles(base.clone(), &[]), base);
        let profiles = [("neon".to_string(), "Neon".to_string())];
        let [MenuItem::Submenu { items, .. }] = &with_terminal_profiles(base, &profiles)[..] else {
            panic!("expected the Add submenu");
        };
        assert!(matches!(&items[1], MenuItem::Action { id, .. } if id == ADD_TERMINAL));
        let MenuItem::Submenu {
            label,
            items: entries,
        } = &items[2]
        else {
            panic!(
                "expected the profiles submenu after Terminal, got {:?}",
                items[2]
            );
        };
        assert_eq!(label, TERMINAL_PROFILES_LABEL);
        assert_eq!(entries, &vec![action("add.terminal_profile.neon", "Neon")]);
        assert_eq!(
            terminal_profile_choice("add.terminal_profile.neon"),
            Some("neon")
        );
        assert_eq!(terminal_profile_choice(ADD_TERMINAL), None);
        assert_eq!(
            menu_action("add.terminal_profile.neon", Point::new(0.0, 0.0), &[]),
            menu_action(ADD_TERMINAL, Point::new(0.0, 0.0), &[])
        );
    }

    #[test]
    fn settings_sits_below_add_and_opens_settings() {
        let base = canvas_menu(&plugins(), &[], &[]);
        let items = with_settings(base.clone());
        assert_eq!(&items[..base.len()], &base[..]);
        assert_eq!(
            &items[base.len()..],
            &[MenuItem::Separator, action("app.settings", "Settings…")]
        );
        assert_eq!(
            menu_action("app.settings", Point::new(3.0, 4.0), &[]),
            Some(MenuCommand::OpenSettings)
        );
    }

    fn plugins() -> Vec<(String, String)> {
        vec![
            ("hello-world".to_string(), "Hello World".to_string()),
            ("github-prs".to_string(), "GitHub PRs".to_string()),
        ]
    }

    #[test]
    fn canvas_menu_lists_add_submenu_with_terminal_browser_and_plugins() {
        assert_eq!(
            canvas_menu(&plugins(), &[], &["/unused".to_string()]),
            vec![MenuItem::Submenu {
                label: "Add".to_string(),
                items: vec![
                    MenuItem::Header {
                        label: "Native".to_string(),
                    },
                    action("add.terminal", "Terminal"),
                    action("add.browser", "Browser"),
                    MenuItem::Separator,
                    MenuItem::Header {
                        label: "Plugins".to_string(),
                    },
                    action("add.plugin.hello-world", "Hello World"),
                    action("add.plugin.github-prs", "GitHub PRs"),
                ],
            }]
        );
    }

    #[test]
    fn canvas_menu_without_plugins_says_so_and_lists_searched_dirs() {
        let dirs = ["/u/plugins".to_string(), "/repo/plugins".to_string()];
        assert_eq!(
            canvas_menu(&[], &[], &dirs),
            vec![MenuItem::Submenu {
                label: "Add".to_string(),
                items: vec![
                    MenuItem::Header {
                        label: "Native".to_string(),
                    },
                    action("add.terminal", "Terminal"),
                    action("add.browser", "Browser"),
                    MenuItem::Separator,
                    MenuItem::Header {
                        label: "Plugins".to_string(),
                    },
                    disabled("No plugins found"),
                    MenuItem::Submenu {
                        label: "Searched Folders".to_string(),
                        items: vec![disabled("/u/plugins"), disabled("/repo/plugins")],
                    },
                ],
            }]
        );
        let Some(MenuItem::Submenu { items, .. }) = canvas_menu(&[], &[], &[]).pop() else {
            panic!("no Add submenu");
        };
        assert_eq!(
            items.last(),
            Some(&MenuItem::Submenu {
                label: "Searched Folders".to_string(),
                items: vec![disabled("(no folders)")],
            })
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
                    cwd: None,
                    profile: None,
                    app: None,
                },
                rect: Rect::new(120.0, -40.0, 640.0, 400.0),
            })
        );
        assert_eq!(
            menu_action("add.browser", world, &[]),
            Some(MenuCommand::AddPanel {
                kind: PanelKind::Browser {
                    url: "about:blank".to_string(),
                    page_zoom: None,
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
            overrides: crate::scene::PanelOverrides::default(),
            resize: crate::resize::ResizePolicy::FREE,
        }
    }

    fn terminal_kind() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: None,
            app: None,
        }
    }

    #[test]
    fn panel_menu_for_terminal_lists_all_actions_with_delete_last() {
        assert_eq!(
            panel_menu(&panel(terminal_kind(), false), false),
            vec![
                action("panel.minimize", "Minimize"),
                action("panel.zoom_to_fit", "Zoom to Fit"),
                MenuItem::Separator,
                action("panel.bring_to_front", "Bring to Front"),
                action("panel.send_to_back", "Send to Back"),
                MenuItem::Separator,
                action("panel.settings", "Settings…"),
                MenuItem::Separator,
                action("panel.duplicate", "Duplicate"),
                MenuItem::Separator,
                action("panel.delete", "Delete"),
            ]
        );
    }

    #[test]
    fn panel_menu_minimized_says_restore_and_drops_zoom_to_fit() {
        let items = panel_menu(&panel(terminal_kind(), true), false);
        assert_eq!(items[0], action("panel.minimize", "Restore"));
        assert!(!items.contains(&action("panel.zoom_to_fit", "Zoom to Fit")));
        assert_eq!(items.last(), Some(&action("panel.delete", "Delete")));
    }

    #[test]
    fn placeholder_menu_is_delete_only() {
        let items = placeholder_menu();
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0], MenuItem::Action { id, .. } if id == PANEL_DELETE));
    }

    #[test]
    fn panel_menu_offers_duplicate_only_for_terminals() {
        let browser = PanelKind::Browser {
            url: "about:blank".to_string(),
            page_zoom: None,
        };
        let items = panel_menu(&panel(browser, false), false);
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
    fn every_kind_offers_settings() {
        let settings = action("panel.settings", "Settings…");
        let kinds = [
            terminal_kind(),
            PanelKind::Browser {
                url: "about:blank".to_string(),
                page_zoom: None,
            },
            PanelKind::Plugin {
                plugin_id: "hello".to_string(),
                plugin_version: None,
                settings: Default::default(),
            },
            PanelKind::app_terminal("top", None),
        ];
        for kind in kinds {
            for minimized in [false, true] {
                let items = panel_menu(&panel(kind.clone(), minimized), false);
                assert!(items.contains(&settings), "{kind:?} minimized={minimized}");
            }
        }
        assert!(!placeholder_menu().contains(&settings));
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
            ("panel.settings", MenuCommand::PanelSettings),
        ];
        for (id, command) in cases {
            assert_eq!(menu_action(id, world, &[]), Some(command), "{id}");
        }
        // Every id the menu can show has a command, so no item is a dead click.
        for item in panel_menu(&panel(terminal_kind(), false), false) {
            if let MenuItem::Action { id, .. } = item {
                assert!(menu_action(&id, world, &[]).is_some(), "{id}");
            }
        }
        assert_eq!(menu_action("panel.close", world, &[]), None);
    }

    #[test]
    fn tui_apps_and_web_plugins_share_the_plugins_section() {
        let apps = vec![("slack".to_string(), "Slack".to_string())];
        let [MenuItem::Submenu { items, .. }] = &canvas_menu(&plugins(), &apps, &[])[..] else {
            panic!("expected one Add submenu");
        };
        // One "Plugins" section: TUI apps, then web plugins, no divider between them.
        assert_eq!(
            &items[3..],
            &[
                MenuItem::Separator,
                MenuItem::Header {
                    label: "Plugins".to_string()
                },
                action("add.tui.slack", "Slack"),
                action("add.plugin.hello-world", "Hello World"),
                action("add.plugin.github-prs", "GitHub PRs"),
            ]
        );
        let sizes = vec![("slack".to_string(), Size::new(900.0, 600.0))];
        let world = Point::new(5.0, 6.0);
        assert_eq!(
            menu_action("add.tui.slack", world, &sizes),
            Some(MenuCommand::AddPanel {
                kind: PanelKind::app_terminal("slack", None),
                rect: Rect {
                    origin: world,
                    size: Size::new(900.0, 600.0)
                },
            })
        );
        assert_eq!(menu_action("add.tui.missing", world, &sizes), None);
    }

    #[test]
    fn exited_tui_panel_offers_restart_first_and_duplicate() {
        let tui = PanelKind::app_terminal("slack", None);
        let items = panel_menu(&panel(tui.clone(), false), true);
        assert_eq!(items[0], action("panel.restart", "Restart"));
        assert_eq!(items[1], MenuItem::Separator);
        assert!(items.contains(&action("panel.duplicate", "Duplicate")));
        assert_eq!(
            menu_action("panel.restart", Point::new(0.0, 0.0), &[]),
            Some(MenuCommand::Restart)
        );
        let running = panel_menu(&panel(tui, false), false);
        assert!(!running.contains(&action("panel.restart", "Restart")));
    }
}
