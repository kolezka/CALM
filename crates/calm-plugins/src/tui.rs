//! `"kind": "tui"` manifests: a terminal panel that runs one command.
//!
//! Parsed by hand from a JSON object so every bad field gets its own message. A wrong
//! value is an error, never a silent default.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use crate::manifest::Size;

/// Longest `title`, in characters.
pub const MAX_TITLE_CHARS: usize = 120;

/// The one placeholder `title` may use. It becomes the manifest `name`.
pub const TITLE_NAME_PLACEHOLDER: &str = "{name}";

/// File types an image `icon` may have. Anything else is read as a text icon.
pub const ICON_IMAGE_EXTENSIONS: &[&str] = &["png", "svg"];

/// Ghostty appearance fields CALM cannot pass per terminal yet. gpui-libghostty 0.3.1 only
/// takes colors per surface; fonts and padding need the planned shim patch.
const PLANNED_FIELDS: &[&str] = &["fontSize", "fontFamily", "padding"];

/// What the title bar shows in the icon slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TuiIcon {
    /// 1 to 4 characters, drawn as text.
    Text(String),
    /// A PNG or SVG inside the plugin folder, relative to it.
    Image(PathBuf),
}

/// Terminal colors for this plugin's surface. Colors are 0xRRGGBB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TuiTerminalColors {
    pub background: u32,
    pub foreground: u32,
    pub palette: [u32; 16],
}

/// Size of a new TUI panel when the manifest has no `defaultSize`.
pub const DEFAULT_TUI_SIZE: Size = Size {
    width: 640.0,
    height: 400.0,
};

/// Longest `icon`, in characters. It is drawn as text in the title bar.
pub const MAX_ICON_CHARS: usize = 4;

const FIELDS: &[&str] = &[
    "kind",
    "id",
    "name",
    "version",
    "command",
    "args",
    "cwd",
    "env",
    "defaultSize",
    "icon",
    "description",
    "restart",
    "shell",
    "title",
    "accentColor",
    "titleBarColor",
    "titleColor",
    "terminalColors",
    "contributes",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TuiRestart {
    Never,
    OnExit,
    #[default]
    Prompt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TuiSpec {
    pub command: String,
    pub args: Vec<String>,
    /// As written; `~` is expanded at launch.
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub icon: Option<TuiIcon>,
    pub description: Option<String>,
    pub restart: TuiRestart,
    /// Run through the user's login shell, so its `PATH` applies.
    pub shell: bool,
    /// Chrome title template. `None` shows the manifest `name`.
    pub title: Option<String>,
    /// 0xRRGGBBAA. Focus ring, join divider and icon tint.
    pub accent_color: Option<u32>,
    /// 0xRRGGBBAA.
    pub title_bar_color: Option<u32>,
    /// 0xRRGGBBAA.
    pub title_color: Option<u32>,
    /// Boxed: 72 bytes of palette would bloat every `PluginContent`.
    pub terminal_colors: Option<Box<TuiTerminalColors>>,
}

impl TuiSpec {
    /// The chrome title for a plugin named `name`: the `title` template, else `name`.
    pub fn chrome_title(&self, name: &str) -> String {
        match &self.title {
            Some(template) => template.replace(TITLE_NAME_PLACEHOLDER, name),
            None => name.to_string(),
        }
    }

    /// The image icon's path, if the icon is an image.
    pub fn icon_image(&self) -> Option<&Path> {
        match &self.icon {
            Some(TuiIcon::Image(path)) => Some(path),
            _ => None,
        }
    }
}

/// The parts of a TUI manifest shared with web manifests.
pub(crate) struct TuiManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub default_size: Size,
    pub spec: TuiSpec,
}

pub(crate) fn parse(object: &Map<String, Value>) -> Result<TuiManifest, String> {
    if let Some(key) = object.keys().find(|key| !FIELDS.contains(&key.as_str())) {
        return Err(match key.as_str() {
            "entry" => "a tui plugin has no entry: it runs `command`".to_string(),
            planned if PLANNED_FIELDS.contains(&planned) => format!(
                "{planned} is planned but not supported yet: CALM cannot set Ghostty fonts or \
                 padding per terminal until the libghostty shim patch lands"
            ),
            other => format!("unknown field {other:?} for kind \"tui\""),
        });
    }
    let id = required_string(object, "id")?;
    let name = required_string(object, "name")?;
    if name.trim().is_empty() {
        return Err("name must not be empty".to_string());
    }
    let command = required_string(object, "command")?;
    check_command(&command)?;
    let version = optional_string(object, "version")?.unwrap_or_default();
    let args = match object.get("args") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, item)| match item {
                Value::String(arg) if !arg.contains('\0') => Ok(arg.clone()),
                Value::String(_) => Err(format!("args[{index}] contains a NUL byte")),
                other => Err(format!(
                    "args[{index}] must be a string, got {}",
                    type_name(other)
                )),
            })
            .collect::<Result<_, _>>()?,
        Some(other) => {
            return Err(format!(
                "args must be an array of strings, got {}",
                type_name(other)
            ));
        }
    };
    let cwd = optional_string(object, "cwd")?;
    if let Some(cwd) = &cwd
        && !(cwd.starts_with('/') || cwd == "~" || cwd.starts_with("~/"))
    {
        return Err(format!("cwd {cwd:?} must be absolute or start with ~/"));
    }
    let env = match object.get("env") {
        None => BTreeMap::new(),
        Some(Value::Object(vars)) => vars
            .iter()
            .map(|(key, value)| {
                if !is_env_name(key) {
                    return Err(format!("env key {key:?} must match [A-Za-z_][A-Za-z0-9_]*"));
                }
                match value {
                    Value::String(text) if !text.contains('\0') => Ok((key.clone(), text.clone())),
                    Value::String(_) => Err(format!("env.{key} contains a NUL byte")),
                    other => Err(format!(
                        "env.{key} must be a string, got {}",
                        type_name(other)
                    )),
                }
            })
            .collect::<Result<_, _>>()?,
        Some(other) => {
            return Err(format!(
                "env must be an object of strings, got {}",
                type_name(other)
            ));
        }
    };
    let default_size = match object.get("defaultSize") {
        None => DEFAULT_TUI_SIZE,
        Some(value) => parse_size(value)?,
    };
    let icon = optional_string(object, "icon")?
        .map(|icon| parse_icon(&icon))
        .transpose()?;
    let title = optional_string(object, "title")?;
    if let Some(title) = &title {
        check_title(title)?;
    }
    let accent_color = optional_color(object, "accentColor")?;
    let title_bar_color = optional_color(object, "titleBarColor")?;
    let title_color = optional_color(object, "titleColor")?;
    let terminal_colors = object
        .get("terminalColors")
        .map(parse_terminal_colors)
        .transpose()?
        .map(Box::new);
    let description = optional_string(object, "description")?;
    let restart = match object.get("restart") {
        None => TuiRestart::default(),
        Some(Value::String(value)) => match value.as_str() {
            "never" => TuiRestart::Never,
            "on-exit" => TuiRestart::OnExit,
            "prompt" => TuiRestart::Prompt,
            other => {
                return Err(format!(
                    "restart {other:?} must be \"never\", \"on-exit\" or \"prompt\""
                ));
            }
        },
        Some(other) => {
            return Err(format!(
                "restart must be a string, got {}",
                type_name(other)
            ));
        }
    };
    let shell = match object.get("shell") {
        None => true,
        Some(Value::Bool(shell)) => *shell,
        Some(other) => {
            return Err(format!(
                "shell must be true or false, got {}",
                type_name(other)
            ));
        }
    };
    Ok(TuiManifest {
        id,
        name,
        version,
        default_size,
        spec: TuiSpec {
            command,
            args,
            cwd,
            env,
            icon,
            description,
            restart,
            shell,
            title,
            accent_color,
            title_bar_color,
            title_color,
            terminal_colors,
        },
    })
}

fn parse_icon(icon: &str) -> Result<TuiIcon, String> {
    let path = Path::new(icon);
    let is_image = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ICON_IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
    if is_image {
        let inside = path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if !inside || icon.contains('\0') {
            return Err(format!(
                "icon {icon:?} must be a relative path inside the plugin folder"
            ));
        }
        return Ok(TuiIcon::Image(path.to_path_buf()));
    }
    let chars = icon.chars().count();
    if chars == 0
        || chars > MAX_ICON_CHARS
        || icon.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(format!(
            "icon {icon:?} must be 1 to {MAX_ICON_CHARS} characters without spaces, \
             or a .png or .svg path"
        ));
    }
    Ok(TuiIcon::Text(icon.to_string()))
}

fn check_title(title: &str) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("title must not be empty".to_string());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!(
            "title must be at most {MAX_TITLE_CHARS} characters"
        ));
    }
    if title.chars().any(char::is_control) {
        return Err(format!(
            "title {title:?} must not contain control characters"
        ));
    }
    if title
        .replace(TITLE_NAME_PLACEHOLDER, "")
        .contains(['{', '}'])
    {
        return Err(format!(
            "title {title:?} may only use the {TITLE_NAME_PLACEHOLDER} placeholder"
        ));
    }
    Ok(())
}

/// `#rrggbb` or `#rrggbbaa` to 0xRRGGBBAA.
pub fn parse_hex_color(text: &str) -> Option<u32> {
    let hex = text.strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(if hex.len() == 6 {
        (value << 8) | 0xff
    } else {
        value
    })
}

fn optional_color(object: &Map<String, Value>, key: &str) -> Result<Option<u32>, String> {
    optional_string(object, key)?
        .map(|text| {
            parse_hex_color(&text)
                .ok_or_else(|| format!("{key} {text:?} must be #rrggbb or #rrggbbaa"))
        })
        .transpose()
}

/// Ghostty surface colors have no alpha, so only `#rrggbb` is accepted. Returns 0xRRGGBB.
fn opaque_color(value: &Value, key: &str) -> Result<u32, String> {
    match value {
        Value::String(text) if text.len() == 7 => parse_hex_color(text)
            .map(|rgba| rgba >> 8)
            .ok_or_else(|| format!("{key} {text:?} must be #rrggbb")),
        Value::String(text) => Err(format!("{key} {text:?} must be #rrggbb")),
        other => Err(format!(
            "{key} must be a #rrggbb string, got {}",
            type_name(other)
        )),
    }
}

fn parse_terminal_colors(value: &Value) -> Result<TuiTerminalColors, String> {
    let Value::Object(object) = value else {
        return Err(format!(
            "terminalColors must be an object, got {}",
            type_name(value)
        ));
    };
    if let Some(key) = object
        .keys()
        .find(|key| !["background", "foreground", "palette"].contains(&key.as_str()))
    {
        return Err(format!("unknown field terminalColors.{key}"));
    }
    let required = |key: &str| {
        object
            .get(key)
            .ok_or_else(|| format!("terminalColors.{key} is required"))
    };
    let background = opaque_color(required("background")?, "terminalColors.background")?;
    let foreground = opaque_color(required("foreground")?, "terminalColors.foreground")?;
    let Value::Array(items) = required("palette")? else {
        return Err("terminalColors.palette must be an array of 16 #rrggbb colors".to_string());
    };
    if items.len() != 16 {
        return Err(format!(
            "terminalColors.palette must have 16 colors, got {}",
            items.len()
        ));
    }
    let mut palette = [0; 16];
    for (index, item) in items.iter().enumerate() {
        palette[index] = opaque_color(item, &format!("terminalColors.palette[{index}]"))?;
    }
    Ok(TuiTerminalColors {
        background,
        foreground,
        palette,
    })
}

// `/usr/bin/env` would read a leading `-` as an option and a `=` as a variable.
fn check_command(command: &str) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err("command must not be empty".to_string());
    }
    if command.starts_with('-') {
        return Err(format!("command {command:?} must not start with -"));
    }
    if command.contains('=') || command.contains('\0') {
        return Err(format!(
            "command {command:?} must not contain = or a NUL byte"
        ));
    }
    Ok(())
}

fn is_env_name(key: &str) -> bool {
    let mut bytes = key.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn parse_size(value: &Value) -> Result<Size, String> {
    let dimension = |key: &str| match value.get(key) {
        Some(Value::Number(n)) => match n.as_f64() {
            Some(v) if v.is_finite() && v > 0.0 => Ok(v),
            _ => Err(format!("defaultSize.{key} must be a positive number")),
        },
        _ => Err(format!("defaultSize.{key} must be a positive number")),
    };
    if !value.is_object() {
        return Err(format!(
            "defaultSize must be {{ \"width\", \"height\" }}, got {}",
            type_name(value)
        ));
    }
    Ok(Size {
        width: dimension("width")?,
        height: dimension("height")?,
    })
}

fn required_string(object: &Map<String, Value>, key: &str) -> Result<String, String> {
    match object.get(key) {
        None => Err(format!("missing field {key:?}")),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(other) => Err(format!("{key} must be a string, got {}", type_name(other))),
    }
}

fn optional_string(object: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match object.get(key) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(other) => Err(format!("{key} must be a string, got {}", type_name(other))),
    }
}

fn type_name(value: &Value) -> &'static str {
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
    use crate::manifest::{PluginContent, PluginManifest, discover};
    use std::path::PathBuf;

    const SLACK: &str = r##"{
        "kind": "tui", "id": "slack", "name": "Slack", "command": "slk",
        "args": ["--workspace", "my team"], "cwd": "~/work", "env": { "SLK_THEME": "dark" },
        "defaultSize": { "width": 900, "height": 600 }, "icon": "#",
        "description": "Slack in the terminal", "restart": "on-exit", "shell": false
    }"##;

    fn error_for(json: &str) -> String {
        PluginManifest::from_json(json).expect_err("manifest was accepted")
    }

    fn with_field(field: &str) -> String {
        format!(r#"{{ "kind": "tui", "id": "t", "name": "T", "command": "top", {field} }}"#)
    }

    #[test]
    fn parses_a_full_tui_manifest() {
        let manifest = PluginManifest::from_json(SLACK).unwrap();
        assert_eq!(manifest.id, "slack");
        assert_eq!(manifest.name, "Slack");
        assert_eq!(manifest.version, "");
        assert_eq!(
            manifest.default_size,
            Size {
                width: 900.0,
                height: 600.0
            }
        );
        assert_eq!(manifest.entry(), None);
        assert_eq!(
            manifest.content,
            PluginContent::Tui(TuiSpec {
                command: "slk".into(),
                args: vec!["--workspace".into(), "my team".into()],
                cwd: Some("~/work".into()),
                env: BTreeMap::from([("SLK_THEME".into(), "dark".into())]),
                icon: Some(TuiIcon::Text("#".into())),
                description: Some("Slack in the terminal".into()),
                restart: TuiRestart::OnExit,
                shell: false,
                title: None,
                accent_color: None,
                title_bar_color: None,
                title_color: None,
                terminal_colors: None,
            })
        );
    }

    const PALETTE: &str = r##"["#000000","#cd3131","#0dbc79","#e5e510","#2472c8","#bc3fbc",
        "#11a8cd","#e5e5e5","#666666","#f14c4c","#23d18b","#f5f543","#3b8eea","#d670d6",
        "#29b8db","#ffffff"]"##;

    #[test]
    fn parses_every_appearance_field() {
        let json = format!(
            r##"{{ "kind": "tui", "id": "slack", "name": "Slack", "command": "slk",
            "title": "{{name}} (work)", "icon": "icons/slack.svg",
            "accentColor": "#4A154B", "titleBarColor": "#1a1d21cc", "titleColor": "#ffffff",
            "terminalColors": {{ "background": "#1a1d21", "foreground": "#d1d2d3",
                "palette": {PALETTE} }} }}"##
        );
        let manifest = PluginManifest::from_json(&json).unwrap();
        let spec = manifest.tui().unwrap();
        assert_eq!(spec.chrome_title(&manifest.name), "Slack (work)");
        assert_eq!(spec.icon, Some(TuiIcon::Image("icons/slack.svg".into())));
        assert_eq!(spec.icon_image(), Some(Path::new("icons/slack.svg")));
        assert_eq!(spec.accent_color, Some(0x4a154bff));
        assert_eq!(spec.title_bar_color, Some(0x1a1d21cc));
        assert_eq!(spec.title_color, Some(0xffffffff));
        let colors = spec.terminal_colors.as_ref().unwrap();
        assert_eq!(colors.background, 0x1a1d21);
        assert_eq!(colors.foreground, 0xd1d2d3);
        assert_eq!(colors.palette[1], 0xcd3131);
        assert_eq!(colors.palette[15], 0xffffff);
    }

    #[test]
    fn title_defaults_to_name_and_text_icon_stays_text() {
        let manifest = PluginManifest::from_json(&with_field(r#""icon": "▲""#)).unwrap();
        let spec = manifest.tui().unwrap();
        assert_eq!(spec.chrome_title("T"), "T");
        assert_eq!(spec.icon, Some(TuiIcon::Text("▲".into())));
        assert_eq!(spec.icon_image(), None);
        let plain = PluginManifest::from_json(&with_field(r#""title": "Chat""#)).unwrap();
        assert_eq!(plain.tui().unwrap().chrome_title("T"), "Chat");
    }

    #[test]
    fn documented_slack_example_parses_and_uses_every_field() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let docs = std::fs::read_to_string(repo.join("docs/plugins.md")).unwrap();
        let section = docs
            .split_once("### Customizing TUI apps")
            .expect("docs have the customizing section")
            .1;
        let json = section
            .split_once("```json\n")
            .and_then(|(_, rest)| rest.split_once("```"))
            .expect("section has a json example")
            .0;
        let value: Value = serde_json::from_str(json).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        for field in FIELDS {
            assert!(keys.contains(field), "docs example lacks {field}");
        }
        let manifest = PluginManifest::from_json(json).unwrap();
        let spec = manifest.tui().unwrap();
        assert_eq!(spec.chrome_title(&manifest.name), "Slack (work)");
        assert!(spec.terminal_colors.is_some() && spec.icon_image().is_some());
    }

    #[test]
    fn hex_colors_parse_strictly() {
        assert_eq!(parse_hex_color("#5b8def"), Some(0x5b8defff));
        assert_eq!(parse_hex_color("#5B8DEF80"), Some(0x5b8def80));
        for bad in [
            "5b8def",
            "#5b8de",
            "#5b8deff",
            "#gggggg",
            "#5b8def801",
            "red",
            "",
            "#",
        ] {
            assert_eq!(parse_hex_color(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn each_bad_appearance_field_names_itself() {
        let palette_of = |n: usize| {
            format!(
                r##""terminalColors": {{ "background": "#000000", "foreground": "#ffffff", "palette": [{}] }}"##,
                vec![r##""#101010""##; n].join(",")
            )
        };
        let cases = [
            (
                with_field(r##""title": """##),
                "title must not be empty".to_string(),
            ),
            (
                with_field(r##""title": "   ""##),
                "title must not be empty".into(),
            ),
            (
                with_field(r##""title": 3"##),
                "title must be a string, got a number".into(),
            ),
            (
                with_field(r##""title": "{id} chat""##),
                "may only use the {name} placeholder".into(),
            ),
            (
                with_field(r##""title": "a\nb""##),
                "must not contain control characters".into(),
            ),
            (
                with_field(&format!(
                    r##""title": "{}""##,
                    "x".repeat(MAX_TITLE_CHARS + 1)
                )),
                format!("title must be at most {MAX_TITLE_CHARS} characters"),
            ),
            (
                with_field(r##""accentColor": "red""##),
                "accentColor \"red\" must be #rrggbb or #rrggbbaa".into(),
            ),
            (
                with_field(r##""accentColor": "#12345""##),
                "accentColor \"#12345\" must be".into(),
            ),
            (
                with_field(r##""accentColor": 255"##),
                "accentColor must be a string, got a number".into(),
            ),
            (
                with_field(r##""titleBarColor": "#zzzzzz""##),
                "titleBarColor \"#zzzzzz\" must be".into(),
            ),
            (
                with_field(r##""titleColor": "fff""##),
                "titleColor \"fff\" must be".into(),
            ),
            (
                with_field(r##""icon": "../up.png""##),
                "icon \"../up.png\" must be a relative path inside the plugin folder".into(),
            ),
            (
                with_field(r##""icon": "/tmp/x.svg""##),
                "icon \"/tmp/x.svg\" must be a relative path inside".into(),
            ),
            (
                with_field(r##""icon": "icon.gif""##),
                "or a .png or .svg path".into(),
            ),
            (
                with_field(r##""terminalColors": "dark""##),
                "terminalColors must be an object, got a string".into(),
            ),
            (
                with_field(r##""terminalColors": { "foreground": "#ffffff", "palette": [] }"##),
                "terminalColors.background is required".into(),
            ),
            (
                with_field(&palette_of(15)),
                "terminalColors.palette must have 16 colors, got 15".into(),
            ),
            (
                with_field(
                    r##""terminalColors": { "background": "#00000080", "foreground": "#ffffff", "palette": [] }"##,
                ),
                "terminalColors.background \"#00000080\" must be #rrggbb".into(),
            ),
            (
                with_field(
                    r##""terminalColors": { "background": "#000000", "foreground": "#ffffff", "palette": [], "cursor": "#ffffff" }"##,
                ),
                "unknown field terminalColors.cursor".into(),
            ),
            (
                with_field(r##""fontSize": 13"##),
                "fontSize is planned but not supported yet".into(),
            ),
            (
                with_field(r##""fontFamily": "Iosevka""##),
                "fontFamily is planned but not supported yet".into(),
            ),
            (
                with_field(r##""padding": 4"##),
                "padding is planned but not supported yet".into(),
            ),
        ];
        for (json, expected) in cases {
            let error = error_for(&json);
            assert!(
                error.contains(&expected),
                "{json}\n  expected {expected:?}\n  got {error:?}"
            );
        }
        // Positive control: the same fixture shape with 16 colors is accepted.
        PluginManifest::from_json(&with_field(&palette_of(16))).unwrap();
    }

    #[test]
    fn discover_checks_icon_file_on_disk() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("evil.png"), b"png").unwrap();
        let manifest = |id: &str, icon: &str| {
            format!(
                r#"{{ "kind": "tui", "id": "{id}", "name": "N", "command": "top", "icon": "{icon}" }}"#
            )
        };
        for (dir, icon) in [
            ("good", "icon.png"),
            ("missing", "nope.svg"),
            ("escape", "link.png"),
            ("folder", "dir.png"),
        ] {
            let plugin = root.path().join(dir);
            std::fs::create_dir_all(&plugin).unwrap();
            std::fs::write(
                plugin.join(crate::manifest::MANIFEST_FILE),
                manifest(dir, icon),
            )
            .unwrap();
        }
        std::fs::write(root.path().join("good/icon.png"), b"png").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("evil.png"),
            root.path().join("escape/link.png"),
        )
        .unwrap();
        std::fs::create_dir(root.path().join("folder/dir.png")).unwrap();
        let found = discover(&[root.path().to_path_buf()]);
        let ids: Vec<&str> = found
            .plugins
            .iter()
            .map(|p| p.manifest.id.as_str())
            .collect();
        assert_eq!(ids, vec!["good"]);
        let errors: Vec<String> = found.errors.iter().map(ToString::to_string).collect();
        assert_eq!(errors.len(), 3, "{errors:?}");
        let find = |needle: &str| errors.iter().any(|e| e.contains(needle));
        assert!(find("cannot resolve icon nope.svg"), "{errors:?}");
        assert!(find("icon link.png resolves to"), "{errors:?}");
        assert!(find("icon dir.png is not a file"), "{errors:?}");
    }

    #[test]
    fn minimal_tui_manifest_uses_documented_defaults() {
        let manifest = PluginManifest::from_json(
            r#"{ "kind": "tui", "id": "top", "name": "Top", "command": "top" }"#,
        )
        .unwrap();
        let spec = manifest.tui().unwrap();
        assert_eq!(spec.restart, TuiRestart::Prompt);
        assert!(spec.shell);
        assert!(spec.args.is_empty() && spec.env.is_empty() && spec.cwd.is_none());
        assert_eq!(manifest.default_size, DEFAULT_TUI_SIZE);
    }

    #[test]
    fn each_bad_field_names_itself() {
        let cases = [
            (
                r#"{ "kind": "tui", "id": "t", "name": "T" }"#.to_string(),
                "missing field \"command\"",
            ),
            (
                with_field(r#""args": "-d""#),
                "args must be an array of strings, got a string",
            ),
            (
                with_field(r#""args": ["ok", 3]"#),
                "args[1] must be a string, got a number",
            ),
            (
                with_field(r#""env": { "A": 1 }"#),
                "env.A must be a string, got a number",
            ),
            (
                with_field(r#""env": { "A": null }"#),
                "env.A must be a string, got null",
            ),
            (
                with_field(r#""env": ["A=1"]"#),
                "env must be an object of strings, got an array",
            ),
            (
                with_field(r#""env": { "1A": "x" }"#),
                "env key \"1A\" must match",
            ),
            (
                with_field(r#""restart": "always""#),
                "restart \"always\" must be",
            ),
            (
                with_field(r#""restart": true"#),
                "restart must be a string, got a boolean",
            ),
            (
                with_field(r#""shell": "yes""#),
                "shell must be true or false, got a string",
            ),
            (
                with_field(r#""cwd": "work""#),
                "cwd \"work\" must be absolute or start with ~/",
            ),
            (
                with_field(r#""icon": "too long""#),
                "icon \"too long\" must be 1 to 4",
            ),
            (
                with_field(r#""defaultSize": { "width": 0, "height": 1 }"#),
                "defaultSize.width must be a positive number",
            ),
            (
                with_field(r#""entry": "index.html""#),
                "a tui plugin has no entry",
            ),
            (
                with_field(r#""arg": ["x"]"#),
                "unknown field \"arg\" for kind \"tui\"",
            ),
            (
                r#"{ "kind": "tui", "id": "t", "name": "T", "command": "" }"#.to_string(),
                "command must not be empty",
            ),
            (
                r#"{ "kind": "tui", "id": "t", "name": "T", "command": "-x" }"#.to_string(),
                "must not start with -",
            ),
            (
                r#"{ "kind": "tui", "id": "t", "name": "T", "command": "A=1" }"#.to_string(),
                "must not contain =",
            ),
            (
                r#"{ "kind": "tui", "id": "t", "name": "", "command": "top" }"#.to_string(),
                "name must not be empty",
            ),
            (
                r#"{ "kind": "tui", "id": "Bad", "name": "T", "command": "top" }"#.to_string(),
                "id \"Bad\" must match",
            ),
            (
                r#"{ "kind": "native", "id": "t", "name": "T", "command": "top" }"#.to_string(),
                "unknown kind \"native\"",
            ),
            (
                r#"{ "kind": 1, "id": "t", "name": "T", "command": "top" }"#.to_string(),
                "kind must be a string",
            ),
        ];
        for (json, expected) in cases {
            let error = error_for(&json);
            assert!(
                error.contains(expected),
                "{json}\n  expected {expected:?}\n  got {error:?}"
            );
        }
    }

    #[test]
    fn web_manifest_without_kind_is_unchanged() {
        let json = r#"{ "id": "hello-world", "name": "Hello World", "version": "0.1.0",
            "entry": "index.html", "defaultSize": { "width": 320, "height": 200 } }"#;
        let implicit = PluginManifest::from_json(json).unwrap();
        let explicit =
            PluginManifest::from_json(&json.replacen('{', r#"{ "kind": "web","#, 1)).unwrap();
        assert_eq!(implicit, explicit);
        assert_eq!(implicit.entry(), Some(std::path::Path::new("index.html")));
        assert_eq!(implicit.tui(), None);
        // A web manifest still needs its entry.
        assert!(error_for(r#"{ "id": "w", "name": "W", "version": "1", "defaultSize": { "width": 1, "height": 1 } }"#)
            .contains("entry"));
    }

    #[test]
    fn discover_lists_tui_plugin_without_entry_and_reports_bad_one() {
        let root = tempfile::tempdir().unwrap();
        for (dir, json) in [
            (
                "top",
                r#"{ "kind": "tui", "id": "top", "name": "Top", "command": "top" }"#,
            ),
            (
                "bad",
                r#"{ "kind": "tui", "id": "bad", "name": "Bad", "command": "x", "restart": "sometimes" }"#,
            ),
        ] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
            std::fs::write(
                root.path().join(dir).join(crate::manifest::MANIFEST_FILE),
                json,
            )
            .unwrap();
        }
        let found = discover(&[root.path().to_path_buf()]);
        let ids: Vec<&str> = found
            .plugins
            .iter()
            .map(|p| p.manifest.id.as_str())
            .collect();
        assert_eq!(ids, vec!["top"]);
        let errors: Vec<String> = found.errors.iter().map(ToString::to_string).collect();
        assert_eq!(errors.len(), 1, "{errors:?}");
        let bad: PathBuf = root.path().join("bad").join(crate::manifest::MANIFEST_FILE);
        assert!(
            errors[0].starts_with(&bad.display().to_string()),
            "{errors:?}"
        );
        assert!(errors[0].contains("restart \"sometimes\""), "{errors:?}");
    }

    #[test]
    fn shipped_tui_example_and_template_parse() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (dir, command) in [
            ("plugins/top-example", "top"),
            ("templates/plugin-tui", "my-app"),
        ] {
            let path = repo.join(dir).join(crate::manifest::MANIFEST_FILE);
            let text = std::fs::read_to_string(&path).unwrap();
            let manifest = PluginManifest::from_json(&text)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            assert_eq!(
                manifest.tui().map(|spec| spec.command.as_str()),
                Some(command)
            );
        }
    }
}
