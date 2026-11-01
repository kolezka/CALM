//! `contributes` in `calm-plugin.json`: context menu items, for web and TUI plugins.
//!
//! Parsed by hand so every bad field names its path. A wrong value is a manifest error,
//! never a default, and one bad item skips the whole plugin.

use std::collections::HashSet;

use calm_core::plugin_menu::{KindName, MenuContribution, MenuLocation, MenuWhen, OpenMode};
use calm_core::settings::is_plugin_id;
use serde_json::{Map, Value};

use crate::tui::MAX_ICON_CHARS;

const CONTRIBUTES_FIELDS: &[&str] = &["menus"];
const ITEM_FIELDS: &[&str] = &[
    "id", "title", "location", "when", "group", "order", "icon", "open", "run",
];
const WHEN_FIELDS: &[&str] = &["kind", "plugin", "minimized"];
pub const MAX_MENU_ITEMS: usize = 32;
pub const MAX_TITLE_CHARS: usize = 80;

/// `tui` says whether the manifest is a TUI plugin: only those take `run`.
pub fn parse(value: Option<&Value>, tui: bool) -> Result<Vec<MenuContribution>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let object = value
        .as_object()
        .ok_or_else(|| format!("contributes must be an object, got {}", kind_of(value)))?;
    if let Some(key) = object
        .keys()
        .find(|key| !CONTRIBUTES_FIELDS.contains(&key.as_str()))
    {
        return Err(format!(
            "contributes.{key}: unknown field, expected \"menus\""
        ));
    }
    let Some(menus) = object.get("menus") else {
        return Ok(Vec::new());
    };
    let menus = menus
        .as_array()
        .ok_or_else(|| format!("contributes.menus must be an array, got {}", kind_of(menus)))?;
    if menus.len() > MAX_MENU_ITEMS {
        return Err(format!(
            "contributes.menus has {} items, at most {MAX_MENU_ITEMS}",
            menus.len()
        ));
    }
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for (index, entry) in menus.iter().enumerate() {
        let path = format!("contributes.menus[{index}]");
        let item = parse_item(entry, tui).map_err(|err| format!("{path}{err}"))?;
        if !seen.insert(item.id.clone()) {
            return Err(format!("{path}.id: duplicate id {:?}", item.id));
        }
        items.push(item);
    }
    Ok(items)
}

/// Errors start with `.field: `, so the caller prefixes the item path.
fn parse_item(value: &Value, tui: bool) -> Result<MenuContribution, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!(": must be an object, got {}", kind_of(value)))?;
    if let Some(key) = object
        .keys()
        .find(|key| !ITEM_FIELDS.contains(&key.as_str()))
    {
        return Err(format!(".{key}: unknown field"));
    }
    let id = required_string(object, "id")?;
    if !is_plugin_id(&id) {
        return Err(format!(".id: {id:?} must match [a-z0-9-]+"));
    }
    let title = required_string(object, "title")?;
    if title.trim().is_empty() {
        return Err(".title: must not be empty".to_string());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!(".title: longer than {MAX_TITLE_CHARS} characters"));
    }
    let location = match required_string(object, "location")?.as_str() {
        "canvas" => MenuLocation::Canvas,
        "panel" => MenuLocation::Panel,
        other => {
            return Err(format!(
                ".location: {other:?}, expected \"canvas\" or \"panel\""
            ));
        }
    };
    let when = match object.get("when") {
        None => MenuWhen::default(),
        Some(_) if location == MenuLocation::Canvas => {
            return Err(".when: applies to \"panel\" items only".to_string());
        }
        Some(value) => parse_when(value)?,
    };
    let group = match optional_string(object, "group")? {
        Some(group) if group.trim().is_empty() => {
            return Err(".group: must not be empty".to_string());
        }
        group => group.unwrap_or_default(),
    };
    let order = match object.get("order") {
        None => 0,
        Some(value) => value
            .as_i64()
            .ok_or_else(|| format!(".order: must be an integer, got {value}"))?,
    };
    let icon = optional_string(object, "icon")?;
    if let Some(icon) = &icon {
        let chars = icon.chars().count();
        if chars == 0 || chars > MAX_ICON_CHARS {
            return Err(format!(".icon: must be 1 to {MAX_ICON_CHARS} characters"));
        }
    }
    let open = match optional_string(object, "open")?.as_deref() {
        None | Some("reuse") => OpenMode::Reuse,
        Some("new") => OpenMode::New,
        Some(other) => {
            return Err(format!(".open: {other:?}, expected \"reuse\" or \"new\""));
        }
    };
    if tui && object.contains_key("open") {
        return Err(".open: a tui plugin's item always opens a new panel".to_string());
    }
    let run = match object.get("run") {
        None => None,
        Some(_) if !tui => return Err(".run: applies to tui plugins only".to_string()),
        Some(value) => Some(string_array(value).map_err(|err| format!(".run: {err}"))?),
    };
    Ok(MenuContribution {
        id,
        title,
        location,
        when,
        group,
        order,
        icon,
        open,
        run,
    })
}

fn parse_when(value: &Value) -> Result<MenuWhen, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!(".when: must be an object, got {}", kind_of(value)))?;
    if let Some(key) = object
        .keys()
        .find(|key| !WHEN_FIELDS.contains(&key.as_str()))
    {
        return Err(format!(
            ".when.{key}: unknown field, expected \"kind\", \"plugin\" or \"minimized\""
        ));
    }
    let mut when = MenuWhen::default();
    if let Some(kinds) = object.get("kind") {
        for name in non_empty(kinds).map_err(|err| format!(".when.kind: {err}"))? {
            let kind = KindName::ALL
                .iter()
                .find(|(known, _)| *known == name)
                .map(|(_, kind)| *kind)
                .ok_or_else(|| {
                    format!(".when.kind: {name:?}, expected terminal, browser, plugin or tui")
                })?;
            when.kinds.push(kind);
        }
    }
    if let Some(plugins) = object.get("plugin") {
        for id in non_empty(plugins).map_err(|err| format!(".when.plugin: {err}"))? {
            if !is_plugin_id(&id) {
                return Err(format!(".when.plugin: {id:?} must match [a-z0-9-]+"));
            }
            when.plugins.push(id);
        }
    }
    if let Some(minimized) = object.get("minimized") {
        when.minimized = Some(
            minimized
                .as_bool()
                .ok_or_else(|| format!(".when.minimized: must be a boolean, got {minimized}"))?,
        );
    }
    Ok(when)
}

fn non_empty(value: &Value) -> Result<Vec<String>, String> {
    let list = string_array(value)?;
    if list.is_empty() {
        return Err("must not be empty".to_string());
    }
    Ok(list)
}

fn string_array(value: &Value) -> Result<Vec<String>, String> {
    let list = value
        .as_array()
        .ok_or_else(|| format!("must be an array of strings, got {}", kind_of(value)))?;
    list.iter()
        .map(|entry| {
            entry
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("must be an array of strings, found {entry}"))
        })
        .collect()
}

fn required_string(object: &Map<String, Value>, key: &str) -> Result<String, String> {
    optional_string(object, key)?.ok_or_else(|| format!(".{key}: missing"))
}

fn optional_string(object: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match object.get(key) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(other) => Err(format!(".{key}: must be a string, got {}", kind_of(other))),
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PluginManifest;

    fn web(contributes: &str) -> Result<PluginManifest, String> {
        PluginManifest::from_json(&format!(
            r#"{{"id":"p","name":"P","version":"0.1.0","entry":"index.html",
                "defaultSize":{{"width":300,"height":200}},"contributes":{contributes}}}"#
        ))
    }

    fn tui(contributes: &str) -> Result<PluginManifest, String> {
        PluginManifest::from_json(&format!(
            r#"{{"kind":"tui","id":"t","name":"T","version":"0.1.0","command":"top",
                "contributes":{contributes}}}"#
        ))
    }

    #[test]
    fn parses_canvas_and_panel_items_with_every_field() {
        let manifest = web(r#"{"menus":[
                {"id":"here","title":"New here","location":"canvas","open":"new"},
                {"id":"rename","title":"Rename","location":"panel","group":"edit",
                 "order":-2,"icon":"R",
                 "when":{"kind":["plugin"],"plugin":["p"],"minimized":false}}
            ]}"#)
        .unwrap();
        assert_eq!(manifest.menus.len(), 2);
        let here = &manifest.menus[0];
        assert_eq!(here.location, MenuLocation::Canvas);
        assert_eq!(here.open, OpenMode::New);
        assert_eq!(here.when, MenuWhen::default());
        let rename = &manifest.menus[1];
        assert_eq!(rename.location, MenuLocation::Panel);
        assert_eq!((rename.group.as_str(), rename.order), ("edit", -2));
        assert_eq!(rename.icon.as_deref(), Some("R"));
        assert_eq!(rename.open, OpenMode::Reuse);
        assert_eq!(
            rename.when,
            MenuWhen {
                kinds: vec![KindName::Plugin],
                plugins: vec!["p".to_string()],
                minimized: Some(false),
            }
        );
    }

    #[test]
    fn no_contributes_or_no_menus_means_no_items() {
        let plain = PluginManifest::from_json(
            r#"{"id":"p","name":"P","version":"0.1.0","entry":"index.html",
                "defaultSize":{"width":300,"height":200}}"#,
        )
        .unwrap();
        assert!(plain.menus.is_empty());
        assert!(web("{}").unwrap().menus.is_empty());
    }

    #[test]
    fn tui_items_take_run_and_web_items_do_not() {
        let manifest =
            tui(r#"{"menus":[{"id":"logs","title":"Logs","location":"canvas","run":["-l","5"]}]}"#)
                .unwrap();
        assert_eq!(
            manifest.menus[0].run,
            Some(vec!["-l".to_string(), "5".to_string()])
        );
        let err =
            web(r#"{"menus":[{"id":"x","title":"X","location":"canvas","run":[]}]}"#).unwrap_err();
        assert_eq!(err, "contributes.menus[0].run: applies to tui plugins only");
        let err = tui(r#"{"menus":[{"id":"x","title":"X","location":"canvas","open":"new"}]}"#)
            .unwrap_err();
        assert!(err.starts_with("contributes.menus[0].open:"), "{err}");
    }

    #[test]
    fn each_bad_field_is_a_manifest_error_naming_its_path() {
        let item = |fields: &str| format!(r#"{{"menus":[{fields}]}}"#);
        let cases = [
            (
                item(r#"{"id":"x","title":"X","location":"window"}"#),
                "contributes.menus[0].location: \"window\", expected \"canvas\" or \"panel\"",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"panel","when":{"focused":true}}"#),
                "contributes.menus[0].when.focused: unknown field, expected \"kind\", \
                 \"plugin\" or \"minimized\"",
            ),
            (
                item(
                    r#"{"id":"x","title":"A","location":"canvas"},
                       {"id":"x","title":"B","location":"canvas"}"#,
                ),
                "contributes.menus[1].id: duplicate id \"x\"",
            ),
            (
                item(r#"{"id":"x","location":"canvas"}"#),
                "contributes.menus[0].title: missing",
            ),
            (
                item(r#"{"id":"x","title":"  ","location":"canvas"}"#),
                "contributes.menus[0].title: must not be empty",
            ),
            (
                item(r#"{"id":"Bad","title":"X","location":"canvas"}"#),
                "contributes.menus[0].id: \"Bad\" must match [a-z0-9-]+",
            ),
            (
                item(r#"{"title":"X","location":"canvas"}"#),
                "contributes.menus[0].id: missing",
            ),
            (
                item(r#"{"id":"x","title":"X"}"#),
                "contributes.menus[0].location: missing",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"canvas","shortcut":"cmd-k"}"#),
                "contributes.menus[0].shortcut: unknown field",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"canvas","when":{"kind":["terminal"]}}"#),
                "contributes.menus[0].when: applies to \"panel\" items only",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"panel","when":{"kind":["shell"]}}"#),
                "contributes.menus[0].when.kind: \"shell\", expected terminal, browser, \
                 plugin or tui",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"panel","when":{"kind":"terminal"}}"#),
                "contributes.menus[0].when.kind: must be an array of strings, got a string",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"panel","when":{"plugin":[]}}"#),
                "contributes.menus[0].when.plugin: must not be empty",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"panel","when":{"minimized":1}}"#),
                "contributes.menus[0].when.minimized: must be a boolean, got 1",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"canvas","order":1.5}"#),
                "contributes.menus[0].order: must be an integer, got 1.5",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"canvas","icon":"TOOLONG"}"#),
                "contributes.menus[0].icon: must be 1 to 4 characters",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"canvas","open":"always"}"#),
                "contributes.menus[0].open: \"always\", expected \"reuse\" or \"new\"",
            ),
            (
                item(r#"{"id":"x","title":"X","location":"canvas","group":""}"#),
                "contributes.menus[0].group: must not be empty",
            ),
            (
                item(r#""x""#),
                "contributes.menus[0]: must be an object, got a string",
            ),
            (
                r#"{"commands":[]}"#.to_string(),
                "contributes.commands: unknown field, expected \"menus\"",
            ),
            (
                r#"{"menus":{}}"#.to_string(),
                "contributes.menus must be an array, got an object",
            ),
            (
                "[]".to_string(),
                "contributes must be an object, got an array",
            ),
        ];
        for (contributes, expected) in cases {
            assert_eq!(web(&contributes).unwrap_err(), expected, "{contributes}");
        }
        // TUI manifests go through the same validation.
        assert_eq!(
            tui(r#"{"menus":[{"id":"x","title":"X","location":"dock"}]}"#).unwrap_err(),
            "contributes.menus[0].location: \"dock\", expected \"canvas\" or \"panel\""
        );
    }

    #[test]
    fn shipped_plugins_and_template_contribute_their_items() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (dir, ids) in [
            ("plugins/hello-world", &["wave"][..]),
            ("plugins/github-prs", &["open-repo"][..]),
            ("plugins/top-example", &[][..]),
            ("templates/plugin-tsx", &["say-hi"][..]),
        ] {
            let path = repo.join(dir).join(crate::manifest::MANIFEST_FILE);
            let text = std::fs::read_to_string(&path).unwrap();
            let manifest = PluginManifest::from_json(&text)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            let found: Vec<&str> = manifest.menus.iter().map(|item| item.id.as_str()).collect();
            assert_eq!(found, ids, "{dir}");
        }
    }

    #[test]
    fn too_many_items_is_an_error() {
        let items: Vec<String> = (0..=MAX_MENU_ITEMS)
            .map(|i| format!(r#"{{"id":"i{i}","title":"I","location":"canvas"}}"#))
            .collect();
        let err = web(&format!(r#"{{"menus":[{}]}}"#, items.join(","))).unwrap_err();
        assert_eq!(err, "contributes.menus has 33 items, at most 32");
    }
}
