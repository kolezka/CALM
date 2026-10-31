//! `"kind": "tui"` manifests: a terminal panel that runs one command.
//!
//! Parsed by hand from a JSON object so every bad field gets its own message. A wrong
//! value is an error, never a silent default.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::manifest::Size;

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
    pub icon: Option<String>,
    pub description: Option<String>,
    pub restart: TuiRestart,
    /// Run through the user's login shell, so its `PATH` applies.
    pub shell: bool,
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
    let icon = optional_string(object, "icon")?;
    if let Some(icon) = &icon {
        let chars = icon.chars().count();
        if chars == 0
            || chars > MAX_ICON_CHARS
            || icon.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(format!(
                "icon {icon:?} must be 1 to {MAX_ICON_CHARS} characters without spaces"
            ));
        }
    }
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
        },
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
                icon: Some("#".into()),
                description: Some("Slack in the terminal".into()),
                restart: TuiRestart::OnExit,
                shell: false,
            })
        );
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
