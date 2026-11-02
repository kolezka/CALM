//! Context menu items contributed by plugins: the filter model, the merged menu section
//! that follows the built-in items, and the `command` payload a chosen item sends.

use serde_json::{Value, json};

use crate::geometry::Point;
use crate::menu::MenuItem;
use crate::scene::{Panel, PanelId, PanelKind, Scene};
use crate::settings::PluginMenuSettings;

/// Prefix of a contributed item's menu id: `contrib.<plugin-id>/<item-id>`.
pub const CONTRIB_PREFIX: &str = "contrib.";

/// Where an item shows: the right click on empty canvas or on a panel's title bar or chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuLocation {
    Canvas,
    Panel,
}

impl MenuLocation {
    pub fn as_str(self) -> &'static str {
        match self {
            MenuLocation::Canvas => "canvas",
            MenuLocation::Panel => "panel",
        }
    }
}

/// Panel kinds a `when` filter can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindName {
    Terminal,
    Browser,
    Plugin,
    Tui,
}

impl KindName {
    pub const ALL: [(&'static str, KindName); 4] = [
        ("terminal", KindName::Terminal),
        ("browser", KindName::Browser),
        ("plugin", KindName::Plugin),
        ("tui", KindName::Tui),
    ];

    /// `None` for an unsupported kind, which no `when.kind` filter can name.
    pub fn of(kind: &PanelKind) -> Option<KindName> {
        match kind {
            // An app panel is a terminal underneath but keeps its own name in filters.
            PanelKind::Terminal { app: Some(_), .. } => Some(KindName::Tui),
            PanelKind::Terminal { .. } => Some(KindName::Terminal),
            PanelKind::Browser { .. } => Some(KindName::Browser),
            PanelKind::Plugin { .. } => Some(KindName::Plugin),
            PanelKind::Unsupported { .. } => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        KindName::ALL
            .iter()
            .find(|(_, kind)| *kind == self)
            .map_or("", |(name, _)| name)
    }
}

/// Which panels a panel item shows for. Each field left empty or `None` matches any panel;
/// the fields that are set must all match.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuWhen {
    pub kinds: Vec<KindName>,
    /// Plugin ids. A panel matches when it is a plugin or TUI panel of one of them.
    pub plugins: Vec<String>,
    pub minimized: Option<bool>,
}

impl MenuWhen {
    pub fn matches(&self, panel: &Panel) -> bool {
        let kind_ok = self.kinds.is_empty()
            || KindName::of(&panel.kind).is_some_and(|kind| self.kinds.contains(&kind));
        let plugin_ok = self.plugins.is_empty()
            || plugin_of(&panel.kind).is_some_and(|id| self.plugins.iter().any(|p| p == id));
        let minimized_ok = self.minimized.is_none_or(|m| m == panel.minimized);
        kind_ok && plugin_ok && minimized_ok
    }
}

/// Whether choosing an item reuses an open panel of the plugin or always opens a new one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpenMode {
    #[default]
    Reuse,
    New,
}

/// One validated `contributes.menus` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuContribution {
    pub id: String,
    pub title: String,
    pub location: MenuLocation,
    pub when: MenuWhen,
    /// Items with the same group sit together; groups are split by separators.
    pub group: String,
    pub order: i64,
    /// A short glyph drawn before the title.
    pub icon: Option<String>,
    pub open: OpenMode,
    /// TUI plugins only: arguments appended to the manifest's `args` for the new panel.
    pub run: Option<Vec<String>>,
}

impl MenuContribution {
    fn label(&self) -> String {
        match &self.icon {
            Some(icon) => format!("{icon}  {}", self.title),
            None => self.title.clone(),
        }
    }
}

/// The items one plugin contributes, in manifest order.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginMenus {
    pub plugin_id: String,
    pub plugin_name: String,
    pub items: Vec<MenuContribution>,
}

pub fn plugin_of(kind: &PanelKind) -> Option<&str> {
    match kind {
        PanelKind::Plugin { plugin_id, .. } => Some(plugin_id),
        PanelKind::Terminal { app: Some(app), .. } => Some(&app.plugin_id),
        _ => None,
    }
}

pub fn item_menu_id(plugin_id: &str, item_id: &str) -> String {
    format!("{CONTRIB_PREFIX}{plugin_id}/{item_id}")
}

/// (plugin id, item id) of a contributed menu id, `None` for every built-in id.
pub fn parse_menu_id(id: &str) -> Option<(&str, &str)> {
    id.strip_prefix(CONTRIB_PREFIX)?.split_once('/')
}

fn is_hidden(settings: &PluginMenuSettings, plugin_id: &str, item_id: &str) -> bool {
    let full = format!("{plugin_id}/{item_id}");
    settings
        .hidden
        .iter()
        .any(|entry| entry == plugin_id || *entry == full)
}

/// The plugin section of a menu at `location`. `panel` is the right-clicked panel for
/// panel menus. Plugins keep their order and are split by separators. A plugin with more
/// than `settings.submenu_above` visible items gets a submenu named after it.
pub fn plugin_section(
    plugins: &[PluginMenus],
    location: MenuLocation,
    panel: Option<&Panel>,
    settings: &PluginMenuSettings,
) -> Vec<MenuItem> {
    let mut section: Vec<MenuItem> = Vec::new();
    for plugin in plugins {
        let mut items: Vec<(usize, &MenuContribution)> = plugin
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.location == location)
            .filter(|(_, item)| match (location, panel) {
                (MenuLocation::Panel, Some(panel)) => item.when.matches(panel),
                (MenuLocation::Panel, None) => false,
                (MenuLocation::Canvas, _) => true,
            })
            .filter(|(_, item)| !is_hidden(settings, &plugin.plugin_id, &item.id))
            .collect();
        if items.is_empty() {
            continue;
        }
        items.sort_by(|(ia, a), (ib, b)| {
            (a.group.as_str(), a.order, ia).cmp(&(b.group.as_str(), b.order, ib))
        });
        let mut entries = Vec::new();
        let mut group = None;
        for (_, item) in &items {
            if group.is_some_and(|g| g != item.group.as_str()) {
                entries.push(MenuItem::Separator);
            }
            group = Some(item.group.as_str());
            entries.push(MenuItem::Action {
                id: item_menu_id(&plugin.plugin_id, &item.id),
                label: item.label(),
            });
        }
        if !section.is_empty() {
            section.push(MenuItem::Separator);
        }
        if items.len() > settings.submenu_above {
            section.push(MenuItem::Submenu {
                label: plugin.plugin_name.clone(),
                items: entries,
            });
        } else {
            section.extend(entries);
        }
    }
    section
}

/// Built-in items first, then a separator and the plugin section if it has any items.
/// Plugins can only add: the built-in items come through unchanged.
pub fn merge(mut builtin: Vec<MenuItem>, section: Vec<MenuItem>) -> Vec<MenuItem> {
    if !section.is_empty() {
        builtin.push(MenuItem::Separator);
        builtin.extend(section);
    }
    builtin
}

/// What the plugin page's `command` handler receives.
pub fn command_payload(
    item_id: &str,
    location: MenuLocation,
    panel: Option<&Panel>,
    world: Option<Point>,
) -> Value {
    json!({
        "id": item_id,
        "location": location.as_str(),
        "panel": panel.map(panel_info),
        "world": world.map(|p| json!({ "x": p.x, "y": p.y })),
    })
}

pub fn panel_info(panel: &Panel) -> Value {
    json!({
        "id": panel.id.0,
        "kind": KindName::of(&panel.kind).map_or("unsupported", KindName::as_str),
        "pluginId": plugin_of(&panel.kind),
        "title": panel.title,
        "rect": {
            "x": panel.rect.origin.x,
            "y": panel.rect.origin.y,
            "width": panel.rect.size.width,
            "height": panel.rect.size.height,
        },
        "minimized": panel.minimized,
    })
}

/// The open panel of `plugin_id` that gets a reused command: the right-clicked panel if it
/// is the plugin's, else the focused one if it is, else the plugin's frontmost panel.
pub fn reuse_target(
    scene: &Scene,
    plugin_id: &str,
    clicked: Option<PanelId>,
    focused: Option<PanelId>,
) -> Option<PanelId> {
    let owned = |id: PanelId| {
        scene
            .get(id)
            .is_some_and(|panel| plugin_of(&panel.kind) == Some(plugin_id))
    };
    clicked
        .filter(|id| owned(*id))
        .or(focused.filter(|id| owned(*id)))
        .or_else(|| {
            scene
                .panels
                .iter()
                .rev()
                .find(|panel| plugin_of(&panel.kind) == Some(plugin_id))
                .map(|panel| panel.id)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::menu::{canvas_menu, panel_menu};

    fn item(id: &str, location: MenuLocation) -> MenuContribution {
        MenuContribution {
            id: id.to_string(),
            title: id.to_uppercase(),
            location,
            when: MenuWhen::default(),
            group: String::new(),
            order: 0,
            icon: None,
            open: OpenMode::Reuse,
            run: None,
        }
    }

    fn plugin(id: &str, items: Vec<MenuContribution>) -> PluginMenus {
        PluginMenus {
            plugin_id: id.to_string(),
            plugin_name: format!("{id} name"),
            items,
        }
    }

    fn action(plugin: &str, item: &str) -> MenuItem {
        MenuItem::Action {
            id: item_menu_id(plugin, item),
            label: item.to_uppercase(),
        }
    }

    fn settings() -> PluginMenuSettings {
        PluginMenuSettings::default()
    }

    fn panel(kind: PanelKind, minimized: bool) -> Panel {
        Panel {
            id: PanelId(7),
            kind,
            rect: Rect::new(10.0, 20.0, 400.0, 300.0),
            title: "T".to_string(),
            minimized,
            resize: crate::resize::ResizePolicy::FREE,
            overrides: crate::scene::PanelOverrides::default(),
        }
    }

    fn plugin_kind(id: &str) -> PanelKind {
        PanelKind::Plugin {
            plugin_id: id.to_string(),
            plugin_version: None,
            settings: json!({}),
        }
    }

    fn terminal() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: None,
            app: None,
        }
    }

    #[test]
    fn builtins_come_first_then_plugins_split_by_separators() {
        let plugins = [
            plugin("a", vec![item("one", MenuLocation::Canvas)]),
            plugin("b", vec![item("two", MenuLocation::Canvas)]),
        ];
        let builtin = canvas_menu(&[], &[], &[]);
        let section = plugin_section(&plugins, MenuLocation::Canvas, None, &settings());
        let merged = merge(builtin.clone(), section);
        assert_eq!(merged[..builtin.len()], builtin[..]);
        assert_eq!(
            merged[builtin.len()..],
            [
                MenuItem::Separator,
                action("a", "one"),
                MenuItem::Separator,
                action("b", "two"),
            ]
        );
        // No plugin items: the built-in menu is unchanged, no trailing separator.
        assert_eq!(merge(builtin.clone(), Vec::new()), builtin);
    }

    #[test]
    fn panel_menu_keeps_every_builtin_item_and_appends_plugin_items() {
        let p = panel(terminal(), false);
        let builtin = panel_menu(&p, false);
        let plugins = [plugin("a", vec![item("x", MenuLocation::Panel)])];
        let merged = merge(
            builtin.clone(),
            plugin_section(&plugins, MenuLocation::Panel, Some(&p), &settings()),
        );
        assert_eq!(merged[..builtin.len()], builtin[..]);
        assert_eq!(merged.last(), Some(&action("a", "x")));
    }

    #[test]
    fn location_separates_canvas_and_panel_items() {
        let plugins = [plugin(
            "a",
            vec![
                item("c", MenuLocation::Canvas),
                item("p", MenuLocation::Panel),
            ],
        )];
        let p = panel(terminal(), false);
        assert_eq!(
            plugin_section(&plugins, MenuLocation::Canvas, None, &settings()),
            [action("a", "c")]
        );
        assert_eq!(
            plugin_section(&plugins, MenuLocation::Panel, Some(&p), &settings()),
            [action("a", "p")]
        );
        // A panel menu with no panel shows no panel items.
        assert!(plugin_section(&plugins, MenuLocation::Panel, None, &settings()).is_empty());
    }

    #[test]
    fn when_filters_by_kind_plugin_and_minimized() {
        let mut by_kind = item("kind", MenuLocation::Panel);
        by_kind.when.kinds = vec![KindName::Terminal, KindName::Tui];
        let mut own = item("own", MenuLocation::Panel);
        own.when.plugins = vec!["a".to_string()];
        let mut restored = item("open", MenuLocation::Panel);
        restored.when.minimized = Some(false);
        let plugins = [plugin("a", vec![by_kind, own, restored])];
        let ids = |p: &Panel| -> Vec<MenuItem> {
            plugin_section(&plugins, MenuLocation::Panel, Some(p), &settings())
        };
        assert_eq!(
            ids(&panel(terminal(), false)),
            [action("a", "kind"), action("a", "open")]
        );
        assert_eq!(ids(&panel(terminal(), true)), [action("a", "kind")]);
        assert_eq!(
            ids(&panel(plugin_kind("a"), false)),
            [action("a", "own"), action("a", "open")]
        );
        // Another plugin's panel does not match `plugins: ["a"]`.
        assert_eq!(ids(&panel(plugin_kind("b"), false)), [action("a", "open")]);
        let tui = PanelKind::app_terminal("a", None);
        assert_eq!(
            ids(&panel(tui, true)),
            [action("a", "kind"), action("a", "own")]
        );
        let browser = PanelKind::Browser {
            url: "about:blank".to_string(),
            page_zoom: None,
        };
        assert_eq!(ids(&panel(browser, true)), Vec::<MenuItem>::new());
    }

    #[test]
    fn hidden_setting_hides_a_plugin_or_one_item() {
        let plugins = [
            plugin(
                "a",
                vec![
                    item("one", MenuLocation::Canvas),
                    item("two", MenuLocation::Canvas),
                ],
            ),
            plugin("b", vec![item("three", MenuLocation::Canvas)]),
        ];
        let mut hide = settings();
        hide.hidden = vec!["a/one".to_string()];
        assert_eq!(
            plugin_section(&plugins, MenuLocation::Canvas, None, &hide),
            [
                action("a", "two"),
                MenuItem::Separator,
                action("b", "three")
            ]
        );
        hide.hidden = vec!["a".to_string()];
        assert_eq!(
            plugin_section(&plugins, MenuLocation::Canvas, None, &hide),
            [action("b", "three")]
        );
        // An id prefix is not a match: "a/on" hides nothing.
        hide.hidden = vec!["a/on".to_string()];
        assert_eq!(
            plugin_section(&plugins, MenuLocation::Canvas, None, &hide).len(),
            4
        );
    }

    #[test]
    fn more_than_submenu_above_items_go_into_a_submenu() {
        let four: Vec<_> = ["w", "x", "y", "z"]
            .iter()
            .map(|id| item(id, MenuLocation::Canvas))
            .collect();
        let plugins = [plugin("a", four[..3].to_vec()), plugin("b", four.clone())];
        let section = plugin_section(&plugins, MenuLocation::Canvas, None, &settings());
        assert_eq!(settings().submenu_above, 3);
        assert_eq!(
            section,
            [
                action("a", "w"),
                action("a", "x"),
                action("a", "y"),
                MenuItem::Separator,
                MenuItem::Submenu {
                    label: "b name".to_string(),
                    items: ["w", "x", "y", "z"]
                        .iter()
                        .map(|i| action("b", i))
                        .collect(),
                },
            ]
        );
        // Hidden items do not count toward the threshold.
        let mut hide = settings();
        hide.hidden = vec!["b/z".to_string()];
        let section = plugin_section(&plugins[1..], MenuLocation::Canvas, None, &hide);
        assert_eq!(section.len(), 3);
        // 0 puts every plugin in its own submenu.
        let mut always = settings();
        always.submenu_above = 0;
        let section = plugin_section(&plugins[..1], MenuLocation::Canvas, None, &always);
        assert!(matches!(&section[..], [MenuItem::Submenu { .. }]));
    }

    #[test]
    fn groups_and_order_sort_items_and_split_groups() {
        let mut late = item("late", MenuLocation::Canvas);
        late.order = 5;
        let mut early = item("early", MenuLocation::Canvas);
        early.order = -1;
        let mut other = item("other", MenuLocation::Canvas);
        other.group = "zz".to_string();
        let mut iconed = item("same", MenuLocation::Canvas);
        iconed.icon = Some("*".to_string());
        let plugins = [plugin("a", vec![other, late, iconed, early])];
        let mut wide = settings();
        wide.submenu_above = 10;
        assert_eq!(
            plugin_section(&plugins, MenuLocation::Canvas, None, &wide),
            [
                action("a", "early"),
                MenuItem::Action {
                    id: item_menu_id("a", "same"),
                    label: "*  SAME".to_string(),
                },
                action("a", "late"),
                MenuItem::Separator,
                action("a", "other"),
            ]
        );
    }

    #[test]
    fn menu_ids_round_trip_and_builtins_are_not_contributions() {
        let id = item_menu_id("github-prs", "open-repo");
        assert_eq!(id, "contrib.github-prs/open-repo");
        assert_eq!(parse_menu_id(&id), Some(("github-prs", "open-repo")));
        for builtin in ["add.terminal", "add.plugin.github-prs", "panel.delete"] {
            assert_eq!(parse_menu_id(builtin), None, "{builtin}");
        }
    }

    #[test]
    fn command_payload_carries_item_location_panel_and_world() {
        let p = panel(plugin_kind("a"), false);
        assert_eq!(
            command_payload("x", MenuLocation::Panel, Some(&p), None),
            json!({
                "id": "x",
                "location": "panel",
                "panel": {
                    "id": 7, "kind": "plugin", "pluginId": "a", "title": "T",
                    "rect": { "x": 10.0, "y": 20.0, "width": 400.0, "height": 300.0 },
                    "minimized": false,
                },
                "world": null,
            })
        );
        assert_eq!(
            command_payload("y", MenuLocation::Canvas, None, Some(Point::new(1.5, -2.0))),
            json!({ "id": "y", "location": "canvas", "panel": null,
                    "world": { "x": 1.5, "y": -2.0 } })
        );
    }

    #[test]
    fn reuse_target_prefers_clicked_then_focused_then_frontmost() {
        let mut scene = Scene::default();
        let rect = Rect::new(0.0, 0.0, 300.0, 200.0);
        let first = scene.add(plugin_kind("a"), rect);
        let term = scene.add(terminal(), rect);
        let second = scene.add(plugin_kind("a"), rect);
        let other = scene.add(plugin_kind("b"), rect);
        assert_eq!(
            reuse_target(&scene, "a", Some(first), Some(second)),
            Some(first)
        );
        assert_eq!(
            reuse_target(&scene, "a", Some(term), Some(first)),
            Some(first)
        );
        assert_eq!(reuse_target(&scene, "a", Some(other), None), Some(second));
        assert_eq!(reuse_target(&scene, "c", None, Some(first)), None);
    }
}
