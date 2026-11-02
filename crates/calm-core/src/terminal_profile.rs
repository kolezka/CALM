//! Plain terminal options: the `terminal` section of the settings, its named profiles, and
//! how a profile, the defaults and the theme resolve into what one terminal uses.
//!
//! Every field is optional. A profile field wins over the same default field, and a field
//! set nowhere falls back to the theme (chrome) or to the built-in behavior (launch).

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chrome::{ChromeOverrides, lay_overrides, parse_hex_color};
use crate::scene::PanelOverrides;

pub const DEFAULT_TITLE_TEMPLATE: &str = "Terminal";
/// Becomes the terminal's number: plain terminals opened since CALM started, from 1.
pub const PLACEHOLDER_INDEX: &str = "{n}";
/// Becomes the directory the terminal started in, with the home folder shown as `~`.
pub const PLACEHOLDER_CWD: &str = "{cwd}";
/// Becomes the app's name, or the first word of the terminal's command.
pub const PLACEHOLDER_NAME: &str = "{name}";
pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_ICON_CHARS: usize = 4;
pub const MAX_PROFILE_ID_CHARS: usize = 64;
/// File types an image `icon` may have. Anything else is read as a text icon.
pub const ICON_IMAGE_EXTENSIONS: &[&str] = &["png", "svg"];
pub const CWD_HOME: &str = "home";
pub const CWD_INHERIT: &str = "inherit";

/// Ghostty colors for one surface, 0xRRGGBB. Ghostty takes all of them or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalColors {
    pub background: u32,
    pub foreground: u32,
    pub palette: [u32; 16],
}

/// A built-in palette the Settings window can load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PalettePreset {
    pub id: &'static str,
    pub label: &'static str,
    pub colors: TerminalColors,
}

pub const PALETTE_PRESETS: [PalettePreset; 3] = [
    // The pixel-art brand: navy ground, magenta and cyan neon.
    PalettePreset {
        id: "calm-neon",
        label: "CALM Neon",
        colors: TerminalColors {
            background: 0x090d1d,
            foreground: 0xe7f6ff,
            palette: [
                0x172042, 0xff4f7b, 0x48f5a0, 0xffd84f, 0x4f8bff, 0xff4fd8, 0x48f5e7, 0xc8d5e9,
                0x344a72, 0xff7b9e, 0x7bffc0, 0xffe57b, 0x7ab0ff, 0xff8be6, 0x8bfff3, 0xf2f7ff,
            ],
        },
    },
    PalettePreset {
        id: "solarized-dark",
        label: "Solarized Dark",
        colors: TerminalColors {
            background: 0x002b36,
            foreground: 0x839496,
            palette: [
                0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
            ],
        },
    },
    // Copied from the Ghostty source vendored by gpui-libghostty 0.3.1:
    // `terminal/color.zig` (palette) and `config/Config.zig` (background, foreground).
    PalettePreset {
        id: "ghostty-default",
        label: "Ghostty Default",
        colors: TerminalColors {
            background: 0x282c34,
            foreground: 0xffffff,
            palette: [
                0x1d1f21, 0xcc6666, 0xb5bd68, 0xf0c674, 0x81a2be, 0xb294bb, 0x8abeb7, 0xc5c8c6,
                0x666666, 0xd54e53, 0xb9ca4a, 0xe7c547, 0x7aa6da, 0xc397d8, 0x70c0b1, 0xeaeaea,
            ],
        },
    },
];

/// What a terminal's colors fall back to when nothing sets a palette.
pub fn ghostty_default_colors() -> TerminalColors {
    PALETTE_PRESETS
        .iter()
        .find(|preset| preset.id == "ghostty-default")
        .map(|preset| preset.colors)
        .expect("PALETTE_PRESETS has ghostty-default")
}

/// One terminal color: background, foreground or palette entry 0 to 15.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSlot {
    Background,
    Foreground,
    Palette(usize),
}

/// A panel's own terminal colors. Each color is set or inherited on its own. A full
/// palette, as older session files hold, reads as every color set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelTerminalColors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    /// Entry `i` is color `i`. `null` or a missing tail entry is inherited.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub palette: Vec<Option<String>>,
    /// Any other key. Kept only to reject it by name.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl PanelTerminalColors {
    /// Every color set.
    pub fn from_colors(colors: &TerminalColors) -> Self {
        PanelTerminalColors {
            background: Some(hex(colors.background)),
            foreground: Some(hex(colors.foreground)),
            palette: colors.palette.iter().map(|c| Some(hex(*c))).collect(),
            unknown: BTreeMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.background.is_none()
            && self.foreground.is_none()
            && self.palette.iter().all(Option::is_none)
            && self.unknown.is_empty()
    }

    /// The stored text for `slot`, valid or not.
    pub fn text(&self, slot: ColorSlot) -> Option<&str> {
        match slot {
            ColorSlot::Background => self.background.as_deref(),
            ColorSlot::Foreground => self.foreground.as_deref(),
            ColorSlot::Palette(index) => self.palette.get(index)?.as_deref(),
        }
    }

    /// Sets or clears one color. Other colors are untouched.
    pub fn set(&mut self, slot: ColorSlot, value: Option<u32>) {
        let text = value.map(hex);
        match slot {
            ColorSlot::Background => self.background = text,
            ColorSlot::Foreground => self.foreground = text,
            ColorSlot::Palette(index) => {
                if self.palette.len() <= index {
                    self.palette.resize(index + 1, None);
                }
                self.palette[index] = text;
                while self.palette.last().is_some_and(Option::is_none) {
                    self.palette.pop();
                }
            }
        }
    }

    /// `base` with every valid set color replaced. `None` when no color is valid.
    /// Loading does not validate, so a bad entry keeps the base color.
    pub fn over(&self, base: &TerminalColors) -> Option<TerminalColors> {
        let mut colors = *base;
        let mut any = false;
        let mut put = |text: Option<&str>, target: &mut u32| {
            if let Some(value) = text.and_then(opaque_color) {
                *target = value;
                any = true;
            }
        };
        put(self.background.as_deref(), &mut colors.background);
        put(self.foreground.as_deref(), &mut colors.foreground);
        for (index, target) in colors.palette.iter_mut().enumerate() {
            put(self.text(ColorSlot::Palette(index)), target);
        }
        any.then_some(colors)
    }

    /// The first bad color or key, named under `path`.
    pub fn validate(&self, path: &str) -> Result<(), String> {
        if let Some(key) = self.unknown.keys().next() {
            return Err(format!("unknown field \"{path}.{key}\""));
        }
        if self.palette.len() > 16 {
            return Err(format!(
                "\"{path}.palette\" has {} colors, expected at most 16",
                self.palette.len()
            ));
        }
        let check = |name: String, text: Option<&str>| match text {
            Some(text) if opaque_color(text).is_none() => {
                Err(format!("\"{path}.{name}\" is {text:?}, expected #rrggbb"))
            }
            _ => Ok(()),
        };
        check("background".into(), self.background.as_deref())?;
        check("foreground".into(), self.foreground.as_deref())?;
        for (index, text) in self.palette.iter().enumerate() {
            check(format!("palette[{index}]"), text.as_deref())?;
        }
        Ok(())
    }
}

/// `terminal_colors` as written in the file. Fields are optional here only so a missing
/// one gets an error that names it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalColorsSetting {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<Vec<String>>,
    /// Any other key. Kept only to reject it by name.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl TerminalColorsSetting {
    pub fn from_colors(colors: &TerminalColors) -> Self {
        TerminalColorsSetting {
            background: Some(hex(colors.background)),
            foreground: Some(hex(colors.foreground)),
            palette: Some(colors.palette.iter().map(|c| hex(*c)).collect()),
            unknown: BTreeMap::new(),
        }
    }

    /// The colors, or an error naming the first bad field under `path`.
    pub fn parse(&self, path: &str) -> Result<TerminalColors, String> {
        if let Some(key) = self.unknown.keys().next() {
            return Err(format!("unknown field \"{path}.{key}\""));
        }
        let required = |value: &Option<String>, key: &str| -> Result<u32, String> {
            let text = value
                .as_deref()
                .ok_or_else(|| format!("\"{path}.{key}\" is required"))?;
            opaque_color(text)
                .ok_or_else(|| format!("\"{path}.{key}\" is {text:?}, expected #rrggbb"))
        };
        let background = required(&self.background, "background")?;
        let foreground = required(&self.foreground, "foreground")?;
        let items = self
            .palette
            .as_ref()
            .ok_or_else(|| format!("\"{path}.palette\" is required"))?;
        if items.len() != 16 {
            return Err(format!(
                "\"{path}.palette\" has {} colors, expected 16",
                items.len()
            ));
        }
        let mut palette = [0; 16];
        for (index, text) in items.iter().enumerate() {
            palette[index] = opaque_color(text).ok_or_else(|| {
                format!("\"{path}.palette[{index}]\" is {text:?}, expected #rrggbb")
            })?;
        }
        Ok(TerminalColors {
            background,
            foreground,
            palette,
        })
    }
}

/// The fields shared by the defaults and every profile. All optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalFields {
    /// Title bar text. `{n}`, `{cwd}` and `{name}` are the only placeholders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_template: Option<String>,
    /// 1 to 4 characters, or a `.png` or `.svg` path relative to the settings folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// `#rrggbb` or `#rrggbbaa`: focus ring, focused join divider and text icon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_bar_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_colors: Option<TerminalColorsSetting>,
    /// Profiles only. The global switch is `panels.terminal_user_config`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_ghostty_user_config: Option<bool>,
    /// Program to run instead of the login shell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Only with `command`, in the same layer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// `"home"`, `"inherit"`, or a path starting with `/` or `~`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Added to the environment. Merged key by key, profile over defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalProfile {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub fields: TerminalFields,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// The `terminal` section. The defaults sit at its top level next to `profiles`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSettings {
    #[serde(flatten)]
    pub defaults: TerminalFields,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<TerminalProfile>,
    /// Used by the `t` tool and the plain Terminal menu item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    /// Exit handling for terminals running a typed command. Apps use their manifest.
    #[serde(
        skip_serializing_if = "CommandRestart::is_default",
        deserialize_with = "CommandRestart::deserialize_named"
    )]
    pub command_restart: CommandRestart,
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

/// What a terminal running a typed command does when the command exits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandRestart {
    Never,
    OnExit,
    #[default]
    Prompt,
}

impl CommandRestart {
    pub const ALL: [CommandRestart; 3] = [
        CommandRestart::Never,
        CommandRestart::OnExit,
        CommandRestart::Prompt,
    ];

    fn is_default(&self) -> bool {
        *self == CommandRestart::default()
    }

    /// Serde's own error would not name the field.
    fn deserialize_named<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        serde_json::from_value(value.clone()).map_err(|_| {
            serde::de::Error::custom(format!(
                "\"terminal.command_restart\" is {value}, expected \"never\", \"on_exit\" or \
                 \"prompt\""
            ))
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            CommandRestart::Never => "Never",
            CommandRestart::OnExit => "When it exits",
            CommandRestart::Prompt => "On Enter",
        }
    }
}

/// Where a terminal starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CwdMode {
    Home,
    /// The focused terminal's start directory, else home.
    Inherit,
    /// As written; `~` is expanded at launch.
    Fixed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalIcon {
    Text(String),
    /// Relative to the settings folder.
    Image(PathBuf),
    /// An app's image icon: an absolute path inside its plugin folder.
    AppImage(PathBuf),
}

/// Env every terminal starts with, under the defaults, profile and app layers.
/// gpui-libghostty 0.3.1 exports `TERM_PROGRAM=gpui-ghostty`, so apps that choose features
/// by terminal name (kitty graphics, for one) fall back. Ghostty proper exports `ghostty`.
pub const BUILTIN_ENV: &[(&str, &str)] = &[("TERM_PROGRAM", "ghostty")];

/// What a TUI app's manifest sets, laid between the profile and the panel overrides.
/// The app fills it from the manifest; calm-core never reads manifests.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppLayer {
    /// The manifest's title template. `{name}` is the app's name.
    pub title_template: Option<String>,
    pub icon: Option<TerminalIcon>,
    pub chrome: ChromeOverrides,
    pub colors: Option<TerminalColors>,
    /// Merged key by key over the defaults and the profile.
    pub env: BTreeMap<String, String>,
}

impl AppLayer {
    /// A typed command's layer: it sets only the title.
    pub fn typed_command() -> Self {
        AppLayer {
            title_template: Some(PLACEHOLDER_NAME.to_string()),
            ..AppLayer::default()
        }
    }
}

/// Everything one terminal is opened and drawn with.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTerminal {
    /// The profile used, if any.
    pub profile: Option<String>,
    pub title_template: String,
    /// True when the title came from a panel override: show it as is, with no `{n}` or
    /// `{cwd}` expansion.
    pub title_is_literal: bool,
    pub icon: Option<TerminalIcon>,
    pub overrides: ChromeOverrides,
    pub colors: Option<TerminalColors>,
    pub use_user_config: bool,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cwd: CwdMode,
    pub env: BTreeMap<String, String>,
}

pub fn hex(rgb: u32) -> String {
    format!("#{rgb:06x}")
}

/// `#rrggbb` only, to 0xRRGGBB. Ghostty colors have no alpha.
fn opaque_color(text: &str) -> Option<u32> {
    (text.len() == 7)
        .then(|| parse_hex_color(text))
        .flatten()
        .map(|rgba| rgba >> 8)
}

pub fn is_image_icon(icon: &str) -> bool {
    Path::new(icon)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ICON_IMAGE_EXTENSIONS
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        })
}

pub(crate) fn check_text(path: &str, text: &str, max: usize) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err(format!("\"{path}\" must not be empty"));
    }
    if text.chars().any(char::is_control) {
        return Err(format!("\"{path}\" must not contain control characters"));
    }
    let count = text.chars().count();
    if count > max {
        return Err(format!("\"{path}\" has {count} characters, at most {max}"));
    }
    Ok(())
}

fn check_title_template(path: &str, template: &str) -> Result<(), String> {
    check_text(path, template, MAX_TITLE_CHARS)?;
    let stripped = template
        .replace(PLACEHOLDER_INDEX, "")
        .replace(PLACEHOLDER_CWD, "")
        .replace(PLACEHOLDER_NAME, "");
    if stripped.contains(['{', '}']) {
        return Err(format!(
            "\"{path}\" is {template:?}: only {PLACEHOLDER_INDEX}, {PLACEHOLDER_CWD} and \
             {PLACEHOLDER_NAME} may use braces"
        ));
    }
    Ok(())
}

pub(crate) fn check_icon(path: &str, icon: &str) -> Result<(), String> {
    if !is_image_icon(icon) {
        return check_text(path, icon, MAX_ICON_CHARS);
    }
    let relative = Path::new(icon);
    let inside = relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    if !inside {
        return Err(format!(
            "\"{path}\" is {icon:?}: an image icon must be a path inside the settings folder, \
             without .. or a leading /"
        ));
    }
    Ok(())
}

pub(crate) fn check_color(path: &str, value: &Option<String>) -> Result<(), String> {
    match value {
        Some(text) if parse_hex_color(text).is_none() => Err(format!(
            "\"{path}\" is {text:?}, expected #rrggbb or #rrggbbaa"
        )),
        _ => Ok(()),
    }
}

/// `/usr/bin/env` runs the command, so a leading `-` or a `=` would be misread.
fn check_command(path: &str, command: &str) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err(format!("\"{path}\" must not be empty"));
    }
    if command.starts_with('-') || command.contains(['=', '\0']) {
        return Err(format!(
            "\"{path}\" is {command:?}: it must not start with - or contain = or a NUL byte"
        ));
    }
    Ok(())
}

fn check_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn parse_cwd(path: &str, cwd: &str) -> Result<CwdMode, String> {
    match cwd {
        CWD_HOME => Ok(CwdMode::Home),
        CWD_INHERIT => Ok(CwdMode::Inherit),
        fixed if (fixed.starts_with('/') || fixed.starts_with('~')) && !fixed.contains('\0') => {
            Ok(CwdMode::Fixed(fixed.to_string()))
        }
        other => Err(format!(
            "\"{path}\" is {other:?}, expected \"home\", \"inherit\" or a path starting with / \
             or ~"
        )),
    }
}

impl TerminalFields {
    /// Checks every set field. `prefix` is the field path, like `terminal.profiles[0]`.
    pub fn validate(&self, prefix: &str) -> Result<(), String> {
        let at = |name: &str| format!("{prefix}.{name}");
        if let Some(template) = &self.title_template {
            check_title_template(&at("title_template"), template)?;
        }
        if let Some(icon) = &self.icon {
            check_icon(&at("icon"), icon)?;
        }
        check_color(&at("accent_color"), &self.accent_color)?;
        check_color(&at("title_bar_color"), &self.title_bar_color)?;
        check_color(&at("title_color"), &self.title_color)?;
        if let Some(colors) = &self.terminal_colors {
            colors.parse(&at("terminal_colors"))?;
        }
        if let Some(command) = &self.command {
            check_command(&at("command"), command)?;
        }
        if let Some(args) = &self.args {
            if self.command.is_none() {
                return Err(format!(
                    "\"{}\" needs \"{}\" in the same place",
                    at("args"),
                    at("command")
                ));
            }
            if args.iter().any(|arg| arg.contains('\0')) {
                return Err(format!("\"{}\" must not contain a NUL byte", at("args")));
            }
        }
        if let Some(cwd) = &self.cwd {
            parse_cwd(&at("cwd"), cwd)?;
        }
        for (key, value) in self.env.iter().flatten() {
            if !check_env_key(key) {
                return Err(format!(
                    "\"{}\" has the key {key:?}, expected letters, digits and _, not starting \
                     with a digit",
                    at("env")
                ));
            }
            // The value is never echoed: it may be a secret.
            if value.contains('\0') {
                return Err(format!(
                    "\"{}.{key}\" must not contain a NUL byte",
                    at("env")
                ));
            }
        }
        Ok(())
    }
}

fn check_profile_id(path: &str, id: &str) -> Result<(), String> {
    let valid = !id.is_empty()
        && id.len() <= MAX_PROFILE_ID_CHARS
        && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "\"{path}\" is {id:?}, expected 1 to {MAX_PROFILE_ID_CHARS} of a-z, 0-9, - and _, \
             starting with a letter or digit"
        ))
    }
}

impl TerminalSettings {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(key) = self.unknown.keys().next() {
            return Err(format!("unknown field \"terminal.{key}\""));
        }
        if self.defaults.use_ghostty_user_config.is_some() {
            return Err(
                "\"terminal.use_ghostty_user_config\" belongs in a profile; the default is \
                 \"panels.terminal_user_config\""
                    .to_string(),
            );
        }
        self.defaults.validate("terminal")?;
        let mut seen = std::collections::BTreeSet::new();
        for (index, profile) in self.profiles.iter().enumerate() {
            let prefix = format!("terminal.profiles[{index}]");
            if let Some(key) = profile.unknown.keys().next() {
                return Err(format!("unknown field \"{prefix}.{key}\""));
            }
            check_profile_id(&format!("{prefix}.id"), &profile.id)?;
            if !seen.insert(profile.id.as_str()) {
                return Err(format!(
                    "\"{prefix}.id\" is {:?}, which an earlier profile already uses",
                    profile.id
                ));
            }
            check_text(&format!("{prefix}.name"), &profile.name, MAX_TITLE_CHARS)?;
            profile.fields.validate(&prefix)?;
        }
        if let Some(id) = &self.default_profile
            && self.profile(id).is_none()
        {
            return Err(format!(
                "\"terminal.default_profile\" is {id:?}, which no profile has"
            ));
        }
        Ok(())
    }

    pub fn profile(&self, id: &str) -> Option<&TerminalProfile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    /// (id, name) per profile, for the Add menu.
    pub fn profile_names(&self) -> Vec<(String, String)> {
        self.profiles
            .iter()
            .map(|profile| (profile.id.clone(), profile.name.clone()))
            .collect()
    }

    /// What a terminal opened with `profile` uses: the named profile, else the default
    /// profile, else the defaults alone. Each field comes from the profile, then the
    /// defaults, then the theme. An unknown id resolves like no profile; the caller logs it.
    /// `user_config` is `panels.terminal_user_config`. Call only on validated settings.
    pub fn resolve(&self, profile: Option<&str>, user_config: bool) -> ResolvedTerminal {
        let chosen = profile.and_then(|id| self.profile(id)).or_else(|| {
            self.default_profile
                .as_deref()
                .and_then(|id| self.profile(id))
        });
        let empty = TerminalFields::default();
        let top = chosen.map_or(&empty, |profile| &profile.fields);
        let base = &self.defaults;
        let pick = |get: fn(&TerminalFields) -> &Option<String>| {
            get(top).as_ref().or(get(base).as_ref()).cloned()
        };
        let color = |get: fn(&TerminalFields) -> &Option<String>| {
            pick(get).and_then(|text| parse_hex_color(&text))
        };
        // Command and args are one unit: the layer that sets the command gives the args.
        let launch = if top.command.is_some() { top } else { base };
        let mut env: BTreeMap<String, String> = BUILTIN_ENV
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        env.extend(base.env.clone().unwrap_or_default());
        env.extend(top.env.clone().unwrap_or_default());
        ResolvedTerminal {
            profile: chosen.map(|profile| profile.id.clone()),
            title_template: pick(|f| &f.title_template)
                .unwrap_or_else(|| DEFAULT_TITLE_TEMPLATE.to_string()),
            title_is_literal: false,
            icon: pick(|f| &f.icon).map(|icon| {
                if is_image_icon(&icon) {
                    TerminalIcon::Image(PathBuf::from(icon))
                } else {
                    TerminalIcon::Text(icon)
                }
            }),
            overrides: ChromeOverrides {
                accent: color(|f| &f.accent_color),
                title_bar: color(|f| &f.title_bar_color),
                title: color(|f| &f.title_color),
            },
            colors: top
                .terminal_colors
                .as_ref()
                .or(base.terminal_colors.as_ref())
                .and_then(|colors| colors.parse("terminal_colors").ok()),
            use_user_config: top.use_ghostty_user_config.unwrap_or(user_config),
            command: launch.command.clone(),
            args: launch.args.clone().unwrap_or_default(),
            cwd: pick(|f| &f.cwd)
                .and_then(|cwd| parse_cwd("cwd", &cwd).ok())
                .unwrap_or(CwdMode::Home),
            env,
        }
    }

    /// `resolve`, then an app's manifest layer, then each panel override that is set.
    /// Later layers win field by field; terminal colors per color.
    pub fn resolve_for_panel(
        &self,
        profile: Option<&str>,
        user_config: bool,
        app: Option<&AppLayer>,
        overrides: &PanelOverrides,
    ) -> ResolvedTerminal {
        let mut resolved = self.resolve(profile, user_config);
        if let Some(app) = app {
            if let Some(title) = &app.title_template {
                resolved.title_template = title.clone();
            }
            if let Some(icon) = &app.icon {
                resolved.icon = Some(icon.clone());
            }
            let chrome = &mut resolved.overrides;
            chrome.accent = app.chrome.accent.or(chrome.accent);
            chrome.title_bar = app.chrome.title_bar.or(chrome.title_bar);
            chrome.title = app.chrome.title.or(chrome.title);
            if app.colors.is_some() {
                resolved.colors = app.colors;
            }
            resolved
                .env
                .extend(app.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        if let Some(title) = &overrides.title {
            resolved.title_template = title.clone();
            resolved.title_is_literal = true;
        }
        if let Some(icon) = &overrides.icon {
            resolved.icon = Some(if is_image_icon(icon) {
                TerminalIcon::Image(PathBuf::from(icon))
            } else {
                TerminalIcon::Text(icon.clone())
            });
        }
        resolved.overrides = lay_overrides(resolved.overrides, overrides);
        // Each set color replaces its inherited one; with nothing inherited the rest
        // comes from Ghostty Default, since a live theme needs every color.
        if let Some(own) = &overrides.terminal_colors {
            let base = resolved.colors.unwrap_or_else(ghostty_default_colors);
            if let Some(colors) = own.over(&base) {
                resolved.colors = Some(colors);
            }
        }
        resolved
    }
}

/// The title for terminal number `index` that started in `cwd`, running `name`.
/// One pass over the template, so inserted values are never read as placeholders.
pub fn render_title(template: &str, index: u64, cwd: &str, name: &str) -> String {
    let index = index.to_string();
    let tokens = [
        (PLACEHOLDER_INDEX, index.as_str()),
        (PLACEHOLDER_CWD, cwd),
        (PLACEHOLDER_NAME, name),
    ];
    let mut title = String::with_capacity(template.len());
    let mut rest = template;
    'scan: while let Some(c) = rest.chars().next() {
        for (token, value) in tokens {
            if let Some(after) = rest.strip_prefix(token) {
                title.push_str(value);
                rest = after;
                continue 'scan;
            }
        }
        title.push(c);
        rest = &rest[c.len_utf8()..];
    }
    title
}

/// `path` with a leading `home` shown as `~`.
pub fn display_cwd(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// The argv for a terminal. With no command it is the login shell. With env it goes through
/// `/usr/bin/env`, and a bare shell then gets `-l` so it still reads the login profile:
/// Ghostty's `exec -l` marks only argv[0] as a login process, which is `env` there.
pub fn launch_argv(resolved: &ResolvedTerminal, login_shell: &str) -> Vec<String> {
    let mut argv = Vec::new();
    if !resolved.env.is_empty() {
        argv.push("/usr/bin/env".to_string());
        argv.extend(
            resolved
                .env
                .iter()
                .map(|(key, value)| format!("{key}={value}")),
        );
    }
    match &resolved.command {
        Some(command) => {
            argv.push(command.clone());
            argv.extend(resolved.args.iter().cloned());
        }
        None => {
            argv.push(login_shell.to_string());
            if !resolved.env.is_empty() {
                argv.push("-l".to_string());
            }
        }
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chrome::{ChromePalette, ChromeState, chrome_colors, divider_color, icon_tint};
    use crate::settings::Settings;

    fn parse(json: &str) -> Result<TerminalSettings, String> {
        let settings: TerminalSettings =
            serde_json::from_str(json).map_err(|error| error.to_string())?;
        settings.validate()?;
        Ok(settings)
    }

    fn error(json: &str) -> String {
        parse(json).expect_err(json)
    }

    const PALETTE16: &str = r##"["#000000","#111111","#222222","#333333","#444444","#555555",
        "#666666","#777777","#888888","#999999","#aaaaaa","#bbbbbb","#cccccc","#dddddd",
        "#eeeeee","#ffffff"]"##;

    fn full() -> String {
        format!(
            r##"{{
            "title_template": "{{n}} {{cwd}}",
            "icon": ">_",
            "accent_color": "#112233",
            "env": {{"EDITOR": "vim"}},
            "cwd": "home",
            "profiles": [
                {{"id": "neon", "name": "Neon", "accent_color": "#ff4fd8cc",
                  "title_bar_color": "#090d1d", "icon": "icons/neon.svg",
                  "terminal_colors": {{"background": "#090d1d", "foreground": "#e7f6ff",
                                       "palette": {PALETTE16}}},
                  "use_ghostty_user_config": false,
                  "command": "/bin/zsh", "args": ["-l"], "cwd": "~/src",
                  "env": {{"EDITOR": "nvim", "TERM_PROFILE": "neon"}}}},
                {{"id": "plain", "name": "Plain"}}
            ],
            "default_profile": "plain"
        }}"##
        )
    }

    #[test]
    fn panel_override_wins_over_profile_and_defaults() {
        let settings = parse(
            r##"{"title_bar_color": "#111111",
                 "profiles": [{"id": "p", "name": "P", "title_bar_color": "#222222"}]}"##,
        )
        .unwrap();
        let mut overrides = PanelOverrides {
            title_bar_color: Some("#333333".into()),
            ..PanelOverrides::default()
        };
        let resolved = settings.resolve_for_panel(Some("p"), false, None, &overrides);
        assert_eq!(resolved.overrides.title_bar, Some(0x333333ff));
        overrides.title_bar_color = None;
        let resolved = settings.resolve_for_panel(Some("p"), false, None, &overrides);
        assert_eq!(resolved.overrides.title_bar, Some(0x222222ff));
    }

    #[test]
    fn override_title_is_literal() {
        let settings = parse(r#"{"title_template": "{n} {cwd}"}"#).unwrap();
        assert!(!settings.resolve(None, false).title_is_literal);
        let overrides = PanelOverrides {
            title: Some("{n} logs".into()),
            ..PanelOverrides::default()
        };
        let resolved = settings.resolve_for_panel(None, false, None, &overrides);
        assert_eq!(resolved.title_template, "{n} logs");
        assert!(resolved.title_is_literal);
    }

    #[test]
    fn a_full_section_parses_and_round_trips() {
        let settings = parse(&full()).unwrap();
        assert_eq!(settings.profiles.len(), 2);
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(parse(&json).unwrap(), settings);
    }

    #[test]
    fn an_empty_section_is_the_default_and_writes_nothing_extra() {
        let settings = parse("{}").unwrap();
        assert_eq!(settings, TerminalSettings::default());
        assert_eq!(serde_json::to_string(&settings).unwrap(), "{}");
    }

    #[test]
    fn each_bad_field_gets_its_own_error() {
        let cases: &[(&str, &str)] = &[
            (
                r##"{"title_template": ""}"##,
                "\"terminal.title_template\" must not be empty",
            ),
            (
                r##"{"title_template": "{title}"}"##,
                "only {n}, {cwd} and {name}",
            ),
            (r##"{"title_template": "a\u0007"}"##, "control characters"),
            (
                r##"{"icon": "abcde"}"##,
                "\"terminal.icon\" has 5 characters",
            ),
            (
                r##"{"icon": "../x.png"}"##,
                "\"terminal.icon\" is \"../x.png\"",
            ),
            (r##"{"icon": "/tmp/x.svg"}"##, "inside the settings folder"),
            (
                r##"{"accent_color": "red"}"##,
                "\"terminal.accent_color\" is \"red\"",
            ),
            (
                r##"{"title_bar_color": "#12345"}"##,
                "\"terminal.title_bar_color\"",
            ),
            (
                r##"{"title_color": "#1234567"}"##,
                "\"terminal.title_color\"",
            ),
            (
                r##"{"terminal_colors": {"foreground": "#000000"}}"##,
                "\"terminal.terminal_colors.background\" is required",
            ),
            (
                r##"{"terminal_colors": {"background": "#000000", "foreground": "#ffffffff", "palette": []}}"##,
                "\"terminal.terminal_colors.foreground\" is \"#ffffffff\", expected #rrggbb",
            ),
            (
                r##"{"terminal_colors": {"background": "#000000", "foreground": "#000000", "palette": ["#000000"]}}"##,
                "has 1 colors, expected 16",
            ),
            (
                r##"{"terminal_colors": {"background": "#000000", "foreground": "#000000", "bold": "#000000"}}"##,
                "unknown field \"terminal.terminal_colors.bold\"",
            ),
            (
                r##"{"use_ghostty_user_config": true}"##,
                "belongs in a profile",
            ),
            (r##"{"command": "-x"}"##, "\"terminal.command\" is \"-x\""),
            (r##"{"command": "A=b"}"##, "\"terminal.command\""),
            (
                r##"{"args": ["-l"]}"##,
                "\"terminal.args\" needs \"terminal.command\"",
            ),
            (r##"{"cwd": "work"}"##, "\"terminal.cwd\" is \"work\""),
            (
                r##"{"env": {"1BAD": "x"}}"##,
                "\"terminal.env\" has the key \"1BAD\"",
            ),
            (
                r##"{"env": {"OK": "a\u0000b"}}"##,
                "\"terminal.env.OK\" must not contain a NUL",
            ),
            (
                r##"{"colour": "#000000"}"##,
                "unknown field \"terminal.colour\"",
            ),
            (
                r##"{"profiles": [{"id": "Neon", "name": "N"}]}"##,
                "\"terminal.profiles[0].id\" is \"Neon\"",
            ),
            (
                r##"{"profiles": [{"name": "N"}]}"##,
                "\"terminal.profiles[0].id\" is \"\"",
            ),
            (
                r##"{"profiles": [{"id": "a", "name": "A"}, {"id": "a", "name": "B"}]}"##,
                "\"terminal.profiles[1].id\" is \"a\", which an earlier profile",
            ),
            (
                r##"{"profiles": [{"id": "a", "name": " "}]}"##,
                "\"terminal.profiles[0].name\" must not be empty",
            ),
            (
                r##"{"profiles": [{"id": "a", "name": "A", "acent_color": "#000000"}]}"##,
                "unknown field \"terminal.profiles[0].acent_color\"",
            ),
            (
                r##"{"profiles": [{"id": "a", "name": "A", "accent_color": "nope"}]}"##,
                "\"terminal.profiles[0].accent_color\"",
            ),
            (
                r##"{"default_profile": "ghost"}"##,
                "\"terminal.default_profile\" is \"ghost\"",
            ),
        ];
        for (json, expected) in cases {
            let message = error(json);
            assert!(
                message.contains(expected),
                "{json}: got {message:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn env_errors_never_echo_the_value() {
        let message = error(r##"{"env": {"TOKEN": "s3cret\u0000"}}"##);
        assert!(!message.contains("s3cret"), "{message}");
    }

    #[test]
    fn resolution_goes_profile_then_defaults_then_theme() {
        let settings = parse(&full()).unwrap();
        let neon = settings.resolve(Some("neon"), true);
        assert_eq!(neon.profile.as_deref(), Some("neon"));
        // Profile wins.
        assert_eq!(neon.overrides.accent, Some(0xff4fd8cc));
        assert_eq!(
            neon.icon,
            Some(TerminalIcon::Image("icons/neon.svg".into()))
        );
        assert_eq!(neon.cwd, CwdMode::Fixed("~/src".into()));
        assert!(!neon.use_user_config);
        assert_eq!(neon.command.as_deref(), Some("/bin/zsh"));
        assert_eq!(neon.args, vec!["-l".to_string()]);
        assert_eq!(neon.colors.unwrap().background, 0x090d1d);
        // Defaults fill what the profile leaves out; env merges key by key.
        assert_eq!(neon.title_template, "{n} {cwd}");
        assert_eq!(neon.env.get("EDITOR").map(String::as_str), Some("nvim"));
        assert_eq!(
            neon.env.get("TERM_PROFILE").map(String::as_str),
            Some("neon")
        );
        // Theme: no title color anywhere.
        assert_eq!(neon.overrides.title, None);

        // No profile named: the default profile ("plain"), which sets nothing itself.
        let plain = settings.resolve(None, true);
        assert_eq!(plain.profile.as_deref(), Some("plain"));
        assert_eq!(plain.overrides.accent, Some(0x112233ff));
        assert_eq!(plain.icon, Some(TerminalIcon::Text(">_".into())));
        assert_eq!(plain.overrides.title_bar, None);
        assert!(plain.colors.is_none());
        assert!(
            plain.use_user_config,
            "falls back to panels.terminal_user_config"
        );
        assert_eq!(plain.command, None);
        assert_eq!(plain.cwd, CwdMode::Home);

        // Nothing configured at all: built-in title, theme colors, login shell.
        let bare = TerminalSettings::default().resolve(None, false);
        assert_eq!(bare.title_template, DEFAULT_TITLE_TEMPLATE);
        assert_eq!(bare.overrides, ChromeOverrides::default());
        assert!(!bare.use_user_config && bare.icon.is_none());
        assert_eq!(
            bare.env.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["TERM_PROGRAM"]
        );
    }

    #[test]
    fn an_unknown_profile_resolves_like_none() {
        let settings = parse(&full()).unwrap();
        assert_eq!(
            settings.resolve(Some("gone"), true).profile.as_deref(),
            Some("plain")
        );
    }

    #[test]
    fn title_values_stay_literal_even_when_they_look_like_placeholders() {
        assert_eq!(
            render_title("{cwd}", 1, "/tmp/{name}", "top"),
            "/tmp/{name}"
        );
        assert_eq!(
            render_title("{name} in {cwd}", 1, "~", "{cwd}"),
            "{cwd} in ~"
        );
        assert_eq!(render_title("{n} {name}", 4, "~", "{n}"), "4 {n}");
        // Unknown or unclosed braces are copied as they are.
        assert_eq!(render_title("{x} {n", 2, "~", "a"), "{x} {n");
    }

    #[test]
    fn titles_render_both_placeholders() {
        assert_eq!(render_title("Terminal", 3, "~", "zsh"), "Terminal");
        assert_eq!(
            render_title("{n}: {cwd} ({n})", 7, "~/src", "zsh"),
            "7: ~/src (7)"
        );
        let home = Path::new("/Users/me");
        assert_eq!(display_cwd(home, home), "~");
        assert_eq!(
            display_cwd(Path::new("/Users/me/src/calm"), home),
            "~/src/calm"
        );
        assert_eq!(display_cwd(Path::new("/tmp"), home), "/tmp");
    }

    #[test]
    fn every_terminal_identifies_as_ghostty_unless_a_layer_overrides_it() {
        // gpui-libghostty exports TERM_PROGRAM=gpui-ghostty, which apps that pick
        // features by terminal name (kitty graphics in slk) do not recognize.
        let program = |resolved: &ResolvedTerminal| resolved.env.get("TERM_PROGRAM").cloned();
        let ghostty = Some("ghostty".to_string());
        let bare = TerminalSettings::default();
        assert_eq!(program(&bare.resolve(None, false)), ghostty);
        let typed = AppLayer::typed_command();
        let panel = bare.resolve_for_panel(None, false, Some(&typed), &PanelOverrides::default());
        assert_eq!(program(&panel), ghostty);
        assert_eq!(
            launch_argv(&bare.resolve(None, false), "/bin/zsh"),
            vec!["/usr/bin/env", "TERM_PROGRAM=ghostty", "/bin/zsh", "-l"]
        );

        // Defaults, a profile and an app manifest can each replace it.
        let settings = parse(
            r#"{"env": {"TERM_PROGRAM": "defaults"},
                "profiles": [{"id": "p", "name": "P", "env": {"TERM_PROGRAM": "profile"}}]}"#,
        )
        .unwrap();
        assert_eq!(
            program(&settings.resolve(None, false)).as_deref(),
            Some("defaults")
        );
        assert_eq!(
            program(&settings.resolve(Some("p"), false)).as_deref(),
            Some("profile")
        );
        let app = AppLayer {
            env: [("TERM_PROGRAM".to_string(), "app".to_string())].into(),
            ..AppLayer::default()
        };
        let resolved =
            settings.resolve_for_panel(Some("p"), false, Some(&app), &PanelOverrides::default());
        assert_eq!(program(&resolved).as_deref(), Some("app"));
    }

    #[test]
    fn launch_argv_wraps_env_and_keeps_the_shell_a_login_shell() {
        let mut resolved = TerminalSettings::default().resolve(None, true);
        resolved.env.clear();
        assert_eq!(launch_argv(&resolved, "/bin/zsh"), vec!["/bin/zsh"]);
        resolved.env.insert("A".into(), "1".into());
        assert_eq!(
            launch_argv(&resolved, "/bin/zsh"),
            vec!["/usr/bin/env", "A=1", "/bin/zsh", "-l"]
        );
        resolved.command = Some("htop".into());
        resolved.args = vec!["-d".into(), "5".into()];
        assert_eq!(
            launch_argv(&resolved, "/bin/zsh"),
            vec!["/usr/bin/env", "A=1", "htop", "-d", "5"]
        );
    }

    #[test]
    fn presets_load_as_valid_settings() {
        for preset in PALETTE_PRESETS {
            let setting = TerminalColorsSetting::from_colors(&preset.colors);
            assert_eq!(setting.parse("x"), Ok(preset.colors), "{}", preset.id);
        }
    }

    /// The terminal chrome goes through the same pure functions as TUI panels.
    #[test]
    fn terminal_overrides_drive_the_shared_chrome_functions() {
        const THEME: ChromePalette = ChromePalette {
            bar: 0x01,
            bar_focused: 0x02,
            border: 0x03,
            accent: 0x04,
            title: 0x05,
            title_focused: 0x06,
            confirm_bar: 0x07,
            confirm_border: 0x08,
            confirm_title: 0x09,
        };
        let settings = parse(&full()).unwrap();
        let neon = settings.resolve(Some("neon"), true).overrides;
        let focused = chrome_colors(ChromeState::Focused, &neon, &THEME);
        assert_eq!(focused.border, 0xff4fd8cc);
        assert_eq!(focused.bar, 0x090d1dff);
        assert_eq!(focused.title, 0x06, "no title color set: the theme's");
        let idle = chrome_colors(ChromeState::Idle, &neon, &THEME);
        assert_eq!((idle.bar, idle.border), (0x090d1dff, 0x03));
        let confirming = chrome_colors(ChromeState::Confirming, &neon, &THEME);
        assert_eq!(confirming.bar, 0x07, "the warning always wins");
        assert_eq!(icon_tint(&neon, 0xaa), 0xff4fd8cc);
        let other = ChromeOverrides::default();
        assert_eq!(
            divider_color(
                [(ChromeState::Focused, &neon), (ChromeState::Idle, &other)],
                &THEME
            ),
            0xff4fd8cc
        );
    }

    /// Positive control: a version 2 file written before the `terminal` section loads the
    /// same as before, with every terminal option at its default.
    #[test]
    fn version_2_files_without_a_terminal_section_load_unchanged() {
        let dir = std::env::temp_dir().join(format!("calm-term-v2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r##"{"version": 2, "grid": {"show": true}, "panels": {"terminal_user_config": false}}"##,
        )
        .unwrap();
        let settings = crate::settings::read_settings_from(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        let mut expected = Settings::default();
        expected.grid.show = true;
        expected.panels.terminal_user_config = false;
        assert_eq!(settings, expected);
        assert_eq!(settings.terminal, TerminalSettings::default());
        assert!(
            !settings
                .terminal
                .resolve(None, settings.panels.terminal_user_config)
                .use_user_config
        );
    }

    #[test]
    fn a_bad_terminal_field_rejects_the_settings_file_by_name() {
        let dir = std::env::temp_dir().join(format!("calm-term-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r##"{"version": 2, "terminal": {"profiles": [{"id": "x", "name": "X", "cwd": "rel"}]}}"##,
        )
        .unwrap();
        let error = crate::settings::read_settings_from(&path)
            .unwrap_err()
            .to_string();
        std::fs::remove_dir_all(&dir).ok();
        assert!(
            error.contains("\"terminal.profiles[0].cwd\" is \"rel\""),
            "{error}"
        );
    }

    fn colors(base: u32) -> TerminalColors {
        TerminalColors {
            background: base,
            foreground: base + 1,
            palette: std::array::from_fn(|index| base + 2 + index as u32),
        }
    }

    fn colors_json(colors: &TerminalColors) -> String {
        serde_json::to_string(&TerminalColorsSetting::from_colors(colors)).unwrap()
    }

    /// Defaults, profile `p`, an app layer and panel overrides all set the accent, the
    /// title bar color and the terminal colors.
    fn four_layers() -> (TerminalSettings, AppLayer) {
        let settings = parse(&format!(
            r##"{{"accent_color": "#000001", "title_bar_color": "#000002",
                 "terminal_colors": {},
                 "profiles": [{{"id": "p", "name": "P", "accent_color": "#100001",
                   "title_bar_color": "#100002", "terminal_colors": {}}}]}}"##,
            colors_json(&colors(0x000100)),
            colors_json(&colors(0x100100)),
        ))
        .unwrap();
        let app = AppLayer {
            chrome: ChromeOverrides {
                accent: Some(0x200001ff),
                title_bar: Some(0x200002ff),
                title: None,
            },
            colors: Some(colors(0x200100)),
            ..AppLayer::default()
        };
        (settings, app)
    }

    #[test]
    fn the_app_layer_sits_between_the_profile_and_the_overrides() {
        let (settings, app) = four_layers();
        let none = PanelOverrides::default();
        // No app: the profile wins over the defaults.
        let plain = settings.resolve_for_panel(Some("p"), false, None, &none);
        assert_eq!(plain.overrides.accent, Some(0x100001ff));
        assert_eq!(plain.colors, Some(colors(0x100100)));
        // The manifest beats the profile.
        let app_only = settings.resolve_for_panel(Some("p"), false, Some(&app), &none);
        assert_eq!(app_only.overrides.accent, Some(0x200001ff));
        assert_eq!(app_only.overrides.title_bar, Some(0x200002ff));
        assert_eq!(app_only.colors, Some(colors(0x200100)));
        // A panel override beats the manifest.
        let overrides = PanelOverrides {
            accent_color: Some("#300001".into()),
            terminal_colors: Some(PanelTerminalColors {
                background: Some("#300100".into()),
                ..PanelTerminalColors::default()
            }),
            ..PanelOverrides::default()
        };
        let all = settings.resolve_for_panel(Some("p"), false, Some(&app), &overrides);
        assert_eq!(all.overrides.accent, Some(0x300001ff));
        assert_eq!(all.overrides.title_bar, Some(0x200002ff));
        let shown = all.colors.unwrap();
        assert_eq!(shown.background, 0x300100);
        // The colors the panel does not set come from the manifest, not the profile.
        assert_eq!(shown.foreground, colors(0x200100).foreground);
        assert_eq!(shown.palette, colors(0x200100).palette);
    }

    #[test]
    fn resetting_a_panel_color_falls_back_to_the_manifest() {
        let (settings, app) = four_layers();
        let mut own = PanelTerminalColors::default();
        own.set(ColorSlot::Palette(3), Some(0x303030));
        let mut overrides = PanelOverrides {
            terminal_colors: Some(own.clone()),
            ..PanelOverrides::default()
        };
        let set = settings.resolve_for_panel(Some("p"), false, Some(&app), &overrides);
        assert_eq!(set.colors.unwrap().palette[3], 0x303030);
        own.set(ColorSlot::Palette(3), None);
        overrides.terminal_colors = (!own.is_empty()).then_some(own);
        let reset = settings.resolve_for_panel(Some("p"), false, Some(&app), &overrides);
        assert_eq!(
            reset.colors.unwrap().palette[3],
            colors(0x200100).palette[3]
        );
    }

    #[test]
    fn app_env_merges_over_the_profile_and_the_defaults() {
        let settings = parse(
            r#"{"env": {"A": "defaults", "B": "defaults"},
                "profiles": [{"id": "p", "name": "P", "env": {"B": "profile", "C": "profile"}}]}"#,
        )
        .unwrap();
        let app = AppLayer {
            env: [("C", "manifest"), ("D", "manifest")]
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .into(),
            ..AppLayer::default()
        };
        let resolved =
            settings.resolve_for_panel(Some("p"), false, Some(&app), &PanelOverrides::default());
        let env: Vec<(&str, &str)> = resolved
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            env,
            [
                ("A", "defaults"),
                ("B", "profile"),
                ("C", "manifest"),
                ("D", "manifest"),
                ("TERM_PROGRAM", "ghostty")
            ]
        );
    }

    #[test]
    fn the_app_title_and_icon_beat_the_profile_but_not_the_overrides() {
        let settings = parse(
            r#"{"profiles": [{"id": "p", "name": "P", "title_template": "{n} shell",
                "icon": "P"}]}"#,
        )
        .unwrap();
        let app = AppLayer {
            title_template: Some("{name} app".into()),
            icon: Some(TerminalIcon::AppImage(PathBuf::from(
                "/plugins/top/icon.png",
            ))),
            ..AppLayer::default()
        };
        let none = PanelOverrides::default();
        let resolved = settings.resolve_for_panel(Some("p"), false, Some(&app), &none);
        assert_eq!(resolved.title_template, "{name} app");
        assert!(!resolved.title_is_literal);
        assert_eq!(resolved.icon, app.icon);
        let overrides = PanelOverrides {
            title: Some("Mine".into()),
            icon: Some("M".into()),
            ..PanelOverrides::default()
        };
        let resolved = settings.resolve_for_panel(Some("p"), false, Some(&app), &overrides);
        assert_eq!(resolved.title_template, "Mine");
        assert_eq!(resolved.icon, Some(TerminalIcon::Text("M".into())));
    }

    #[test]
    fn titles_render_the_name_placeholder() {
        assert_eq!(render_title("{name} #{n}", 2, "~", "htop"), "htop #2");
        assert!(parse(r#"{"title_template": "{name} in {cwd}"}"#).is_ok());
        assert!(error(r#"{"title_template": "{nam}"}"#).contains("braces"));
    }

    #[test]
    fn command_restart_takes_three_values_and_defaults_to_prompt() {
        assert_eq!(parse("{}").unwrap().command_restart, CommandRestart::Prompt);
        for (text, value) in [
            ("never", CommandRestart::Never),
            ("on_exit", CommandRestart::OnExit),
            ("prompt", CommandRestart::Prompt),
        ] {
            let json = format!(r#"{{"command_restart":"{text}"}}"#);
            let settings = parse(&json).unwrap();
            assert_eq!(settings.command_restart, value, "{text}");
            // The default is not written back.
            let written = if value == CommandRestart::Prompt {
                "{}"
            } else {
                &json
            };
            assert_eq!(serde_json::to_string(&settings).unwrap(), written);
        }
    }

    #[test]
    fn a_bad_command_restart_is_rejected() {
        for bad in [r#""sometimes""#, r#""on-exit""#, "true", "null"] {
            let json = format!(r#"{{"command_restart": {bad}}}"#);
            let error = error(&json);
            assert!(error.contains("command_restart"), "{json}: {error}");
        }
    }

    #[test]
    fn a_bad_command_restart_rejects_the_settings_file_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"version": 2, "terminal": {"command_restart": "sometimes"}}"#,
        )
        .unwrap();
        let error = crate::settings::read_settings_from(&path)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("\"terminal.command_restart\" is \"sometimes\""),
            "{error}"
        );
        assert!(crate::settings::load_settings_from(&path).is_err());
    }

    #[test]
    fn a_typed_command_is_titled_by_its_name_unless_the_panel_overrides_it() {
        let settings = parse(
            r#"{"title_template": "{n} shell",
                "profiles": [{"id": "p", "name": "P", "title_template": "P {n}"}]}"#,
        )
        .unwrap();
        let typed = AppLayer::typed_command();
        let none = PanelOverrides::default();
        for profile in [None, Some("p")] {
            let resolved = settings.resolve_for_panel(profile, false, Some(&typed), &none);
            assert_eq!(resolved.title_template, "{name}", "{profile:?}");
            assert_eq!(
                render_title(&resolved.title_template, 3, "~", "htop"),
                "htop"
            );
        }
        let overrides = PanelOverrides {
            title: Some("Mine".into()),
            ..PanelOverrides::default()
        };
        let resolved = settings.resolve_for_panel(Some("p"), false, Some(&typed), &overrides);
        assert_eq!(resolved.title_template, "Mine");
        assert!(resolved.title_is_literal);
        // Only the title: the look and env still come from the profile and defaults.
        assert_eq!(
            settings
                .resolve_for_panel(Some("p"), false, Some(&typed), &none)
                .env,
            settings.resolve(Some("p"), false).env
        );
    }
}
