//! App settings persisted to `settings.json`: background, grid, snapping and panel options.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::background::{
    BackgroundKind, BackgroundSettings, MAX_BLUR, MAX_FILTER_FACTOR, MAX_GRADIENT_STOPS,
    MIN_GRADIENT_STOPS, Rgb,
};
use crate::browser_url::BrowserQueryMode;
use crate::geometry::Size;
use crate::grid::GridKind;
use crate::interaction::MoveModifier;
use crate::layout::{move_aside, write_durable};
use crate::menu::DEFAULT_TERMINAL_SIZE;
use crate::preview::PREVIEW_ZOOM_THRESHOLD;
use crate::process_stats::{DEFAULT_INTERVAL_MS, INTERVAL_MS_RANGE, MemoryUnit};
use crate::snap::{DEFAULT_SNAP_EDGE, DEFAULT_SNAP_GRID, SnapConfig};
use crate::terminal_profile::TerminalSettings;

/// Version 2 replaced the background preset with solid, gradient and image backgrounds and
/// added grid, snapping and panel options. Version 1 files are migrated on read. Grid kind,
/// mark size and the major grid were added later with defaults, so they need no new version.
/// The same holds for the `terminal` section: a file without it gets the defaults.
pub const SETTINGS_VERSION: u32 = 2;
/// Below this screen spacing the grid lines turn into noise, so the grid is skipped.
pub const MIN_GRID_SPACING_PX: f64 = 8.0;

pub const GRID_SPACING_RANGE: RangeInclusive<f64> = 16.0..=256.0;
/// Dot diameter, or cross arm thickness doubled, in screen points.
pub const GRID_MARK_SIZE_RANGE: RangeInclusive<f64> = 1.0..=4.0;
/// Cells between major lines or marks. 0 turns the major grid off; 1 would make every
/// line major, so it is not allowed.
pub const GRID_MAJOR_EVERY_MAX: u32 = 32;
pub const SNAP_GRID_STEPS: [f64; 4] = [4.0, 8.0, 16.0, 32.0];
pub const SNAP_EDGE_RANGE: RangeInclusive<f64> = 0.0..=32.0;
pub const TERMINAL_WIDTH_RANGE: RangeInclusive<f64> = 240.0..=2000.0;
pub const TERMINAL_HEIGHT_RANGE: RangeInclusive<f64> = 120.0..=1600.0;
pub const PREVIEW_ZOOM_RANGE: RangeInclusive<f64> = 0.1..=1.0;

/// The version 1 background choice. Kept only to migrate old files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundPreset {
    #[default]
    DarkGray,
    Black,
    Graphite,
    MidnightBlue,
    Forest,
}

impl BackgroundPreset {
    pub fn rgb(self) -> Rgb {
        Rgb(match self {
            BackgroundPreset::DarkGray => 0x151515,
            BackgroundPreset::Black => 0x000000,
            BackgroundPreset::Graphite => 0x2b2b2e,
            BackgroundPreset::MidnightBlue => 0x101827,
            BackgroundPreset::Forest => 0x0f1a14,
        })
    }
}

/// Fields missing from the file take their default, so a hand-edited file may be partial.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub background: BackgroundSettings,
    pub grid: GridSettings,
    pub snap: SnapSettings,
    pub panels: PanelSettings,
    pub plugins: PluginSettings,
    /// Plain terminal defaults and profiles.
    pub terminal: TerminalSettings,
    pub canvas: CanvasSettings,
    pub developer: DeveloperSettings,
    /// Session restore and autosave. A file without the section gets the defaults.
    pub session: SessionSettings,
}

pub const SESSION_DEBOUNCE_MS_RANGE: RangeInclusive<u64> = 250..=10_000;
pub const SESSION_BACKUP_COUNT_RANGE: RangeInclusive<u32> = 0..=20;

/// Whether the last session comes back at launch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreOnLaunch {
    #[default]
    On,
    Off,
    Ask,
}

impl RestoreOnLaunch {
    pub const ALL: [RestoreOnLaunch; 3] = [
        RestoreOnLaunch::On,
        RestoreOnLaunch::Off,
        RestoreOnLaunch::Ask,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RestoreOnLaunch::On => "Always",
            RestoreOnLaunch::Off => "Never",
            RestoreOnLaunch::Ask => "Ask",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionSettings {
    pub restore_on_launch: RestoreOnLaunch,
    pub autosave: bool,
    /// Quiet time after the last change before an autosave.
    pub debounce_ms: u64,
    /// `session.json.1` to `.n`, rotated once per launch. 0 keeps none.
    pub backup_count: u32,
    pub browser_query: BrowserQueryMode,
}

impl Default for SessionSettings {
    fn default() -> Self {
        SessionSettings {
            restore_on_launch: RestoreOnLaunch::On,
            autosave: true,
            debounce_ms: 1000,
            backup_count: 3,
            browser_query: BrowserQueryMode::StripAll,
        }
    }
}

impl SessionSettings {
    fn validate(&self) -> Result<(), String> {
        if !SESSION_DEBOUNCE_MS_RANGE.contains(&self.debounce_ms) {
            return Err(format!(
                "\"session.debounce_ms\" is {}, expected {} to {}",
                self.debounce_ms,
                SESSION_DEBOUNCE_MS_RANGE.start(),
                SESSION_DEBOUNCE_MS_RANGE.end()
            ));
        }
        if !SESSION_BACKUP_COUNT_RANGE.contains(&self.backup_count) {
            return Err(format!(
                "\"session.backup_count\" is {}, expected {} to {}",
                self.backup_count,
                SESSION_BACKUP_COUNT_RANGE.start(),
                SESSION_BACKUP_COUNT_RANGE.end()
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeveloperSettings {
    pub stats: StatsSettings,
}

/// Which items the stats readout (cmd-shift-f) shows, and how often CPU and memory are
/// sampled. The readout itself starts hidden each launch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StatsSettings {
    pub show_fps: bool,
    pub show_cpu: bool,
    pub show_memory: bool,
    /// Also count child processes (terminal shells, plugin backends) in CPU and memory.
    pub include_children: bool,
    pub interval_ms: u64,
    pub memory_unit: MemoryUnit,
}

impl Default for StatsSettings {
    fn default() -> Self {
        StatsSettings {
            show_fps: true,
            show_cpu: true,
            show_memory: true,
            include_children: false,
            interval_ms: DEFAULT_INTERVAL_MS,
            memory_unit: MemoryUnit::Auto,
        }
    }
}

/// How the empty canvas reacts to input.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CanvasSettings {
    /// A click on empty canvas, or Escape while no native view has the keyboard, drops
    /// panel focus. Pans and zooms never do.
    pub click_empty_clears_focus: bool,
    /// Held while dragging a panel's content to move the panel.
    #[serde(deserialize_with = "move_modifier_field")]
    pub move_modifier: MoveModifier,
}

impl Default for CanvasSettings {
    fn default() -> Self {
        CanvasSettings {
            click_empty_clears_focus: true,
            move_modifier: MoveModifier::Cmd,
        }
    }
}

/// Strict like the derive, but the error names the field.
fn move_modifier_field<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<MoveModifier, D::Error> {
    MoveModifier::deserialize(deserializer)
        .map_err(|error| serde::de::Error::custom(format!("\"canvas.move_modifier\": {error}")))
}

/// Plugin discovery options. Read once at startup.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    /// Also load the plugins shipped with CALM (the repo's `plugins/` in a dev build,
    /// `CALM.app/Contents/Resources/plugins` in a bundle).
    pub builtin: bool,
    pub menus: PluginMenuSettings,
}

impl Default for PluginSettings {
    fn default() -> Self {
        PluginSettings {
            builtin: true,
            menus: PluginMenuSettings::default(),
        }
    }
}

/// A plugin with more context menu items than this gets a submenu named after it.
pub const DEFAULT_SUBMENU_ABOVE: usize = 3;
pub const SUBMENU_ABOVE_MAX: usize = 50;

/// How plugin items show in the context menus. Read each time a menu opens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginMenuSettings {
    /// A plugin id hides all its items; `<plugin-id>/<item-id>` hides one item.
    pub hidden: Vec<String>,
    /// 0 puts every plugin's items in a submenu.
    pub submenu_above: usize,
}

impl Default for PluginMenuSettings {
    fn default() -> Self {
        PluginMenuSettings {
            hidden: Vec::new(),
            submenu_above: DEFAULT_SUBMENU_ABOVE,
        }
    }
}

/// `[a-z0-9-]+`, the shape of plugin and menu item ids.
pub fn is_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl PluginMenuSettings {
    fn validate(&self) -> Result<(), String> {
        if self.submenu_above > SUBMENU_ABOVE_MAX {
            return Err(format!(
                "\"plugins.menus.submenu_above\" is {}, expected 0 to {SUBMENU_ABOVE_MAX}",
                self.submenu_above
            ));
        }
        for (index, entry) in self.hidden.iter().enumerate() {
            let ok = match entry.split_once('/') {
                Some((plugin, item)) => is_plugin_id(plugin) && is_plugin_id(item),
                None => is_plugin_id(entry),
            };
            if !ok {
                return Err(format!(
                    "\"plugins.menus.hidden[{index}]\" is {entry:?}, expected a plugin id or \
                     <plugin-id>/<item-id>"
                ));
            }
        }
        Ok(())
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            background: BackgroundSettings::default(),
            grid: GridSettings::default(),
            snap: SnapSettings::default(),
            panels: PanelSettings::default(),
            plugins: PluginSettings::default(),
            terminal: TerminalSettings::default(),
            canvas: CanvasSettings::default(),
            developer: DeveloperSettings::default(),
            session: SessionSettings::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GridSettings {
    pub show: bool,
    /// World units between lines.
    pub spacing: f64,
    pub color: Rgb,
    pub opacity: f64,
    pub kind: GridKind,
    /// Dot or cross size in screen points, the same at every zoom.
    pub mark_size: f64,
    /// Every this many cells a major line or mark is drawn at `major_opacity`. 0 is off.
    pub major_every: u32,
    pub major_opacity: f64,
}

impl Default for GridSettings {
    fn default() -> Self {
        // White at 12 % matches the version 1 grid: the background 12 % lighter.
        GridSettings {
            show: false,
            spacing: 64.0,
            color: Rgb(0xffffff),
            opacity: 0.12,
            kind: GridKind::Lines,
            mark_size: 2.0,
            // Off, so files written before the major grid existed look the same.
            major_every: 0,
            major_opacity: 0.3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapSettings {
    pub enabled: bool,
    /// World units, one of `SNAP_GRID_STEPS`.
    pub grid: f64,
    /// Neighbor edge snap distance in world units. 0 turns edge snapping off.
    pub edge_distance: f64,
}

impl Default for SnapSettings {
    fn default() -> Self {
        SnapSettings {
            enabled: true,
            grid: DEFAULT_SNAP_GRID,
            edge_distance: DEFAULT_SNAP_EDGE,
        }
    }
}

impl SnapSettings {
    /// The interaction config. Disabled snapping turns off both grid and edge snapping.
    pub fn config(&self) -> SnapConfig {
        if self.enabled {
            SnapConfig {
                grid: Some(self.grid),
                edge_distance: self.edge_distance,
            }
        } else {
            SnapConfig {
                grid: None,
                edge_distance: 0.0,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelSettings {
    pub confirm_close: bool,
    /// Terminals read the user's Ghostty config. Applies to terminals spawned afterwards.
    pub terminal_user_config: bool,
    /// Size of a terminal added from the context menu, in world units.
    pub default_terminal_width: f64,
    pub default_terminal_height: f64,
    /// Below this zoom panels show a snapshot instead of the live view.
    pub preview_below_zoom: f64,
}

impl Default for PanelSettings {
    fn default() -> Self {
        PanelSettings {
            confirm_close: true,
            terminal_user_config: true,
            default_terminal_width: DEFAULT_TERMINAL_SIZE.width,
            default_terminal_height: DEFAULT_TERMINAL_SIZE.height,
            preview_below_zoom: PREVIEW_ZOOM_THRESHOLD,
        }
    }
}

impl PanelSettings {
    pub fn default_terminal_size(&self) -> Size {
        Size::new(self.default_terminal_width, self.default_terminal_height)
    }
}

/// The version 1 file, read only to migrate it.
#[derive(Deserialize)]
#[serde(default)]
struct SettingsV1 {
    background: BackgroundPreset,
    show_grid: bool,
    confirm_close: bool,
    terminal_user_config: bool,
}

impl Default for SettingsV1 {
    fn default() -> Self {
        SettingsV1 {
            background: BackgroundPreset::DarkGray,
            show_grid: false,
            confirm_close: true,
            terminal_user_config: true,
        }
    }
}

impl From<SettingsV1> for Settings {
    fn from(old: SettingsV1) -> Settings {
        let mut settings = Settings::default();
        settings.background.kind = BackgroundKind::Solid;
        settings.background.solid = old.background.rgb();
        settings.grid.show = old.show_grid;
        settings.panels.confirm_close = old.confirm_close;
        settings.panels.terminal_user_config = old.terminal_user_config;
        settings
    }
}

impl Settings {
    /// Checks every value against its allowed range. Names the field on failure.
    pub fn validate(&self) -> Result<(), String> {
        fn check(field: &str, value: f64, range: RangeInclusive<f64>) -> Result<(), String> {
            if value.is_finite() && range.contains(&value) {
                Ok(())
            } else {
                Err(format!(
                    "\"{field}\" is {value}, expected {} to {}",
                    range.start(),
                    range.end()
                ))
            }
        }
        let gradient = &self.background.gradient;
        let count = gradient.stops.len();
        if !(MIN_GRADIENT_STOPS..=MAX_GRADIENT_STOPS).contains(&count) {
            return Err(format!(
                "\"background.gradient.stops\" has {count} stops, expected \
                 {MIN_GRADIENT_STOPS} to {MAX_GRADIENT_STOPS}"
            ));
        }
        check(
            "background.gradient.angle_deg",
            gradient.angle_deg,
            0.0..=360.0,
        )?;
        for (index, stop) in gradient.stops.iter().enumerate() {
            check(
                &format!("background.gradient.stops[{index}].position"),
                stop.position,
                0.0..=1.0,
            )?;
        }
        let filters = &self.background.image.filters;
        check(
            "background.image.filters.blur",
            filters.blur,
            0.0..=MAX_BLUR,
        )?;
        for (field, value) in [
            ("brightness", filters.brightness),
            ("contrast", filters.contrast),
            ("saturation", filters.saturation),
        ] {
            check(
                &format!("background.image.filters.{field}"),
                value,
                0.0..=MAX_FILTER_FACTOR,
            )?;
        }
        check("background.image.filters.dim", filters.dim, 0.0..=1.0)?;
        check("grid.spacing", self.grid.spacing, GRID_SPACING_RANGE)?;
        check("grid.opacity", self.grid.opacity, 0.0..=1.0)?;
        check("grid.mark_size", self.grid.mark_size, GRID_MARK_SIZE_RANGE)?;
        let major_every = self.grid.major_every;
        if major_every == 1 || major_every > GRID_MAJOR_EVERY_MAX {
            return Err(format!(
                "\"grid.major_every\" is {major_every}, expected 0 (off) or 2 to \
                 {GRID_MAJOR_EVERY_MAX}"
            ));
        }
        check("grid.major_opacity", self.grid.major_opacity, 0.0..=1.0)?;
        if !SNAP_GRID_STEPS.contains(&self.snap.grid) {
            return Err(format!(
                "\"snap.grid\" is {}, expected one of {SNAP_GRID_STEPS:?}",
                self.snap.grid
            ));
        }
        check(
            "snap.edge_distance",
            self.snap.edge_distance,
            SNAP_EDGE_RANGE,
        )?;
        let panels = &self.panels;
        check(
            "panels.default_terminal_width",
            panels.default_terminal_width,
            TERMINAL_WIDTH_RANGE,
        )?;
        check(
            "panels.default_terminal_height",
            panels.default_terminal_height,
            TERMINAL_HEIGHT_RANGE,
        )?;
        check(
            "panels.preview_below_zoom",
            panels.preview_below_zoom,
            PREVIEW_ZOOM_RANGE,
        )?;
        self.terminal.validate()?;
        let interval = self.developer.stats.interval_ms;
        if !INTERVAL_MS_RANGE.contains(&interval) {
            return Err(format!(
                "\"developer.stats.interval_ms\" is {interval}, expected {} to {}",
                INTERVAL_MS_RANGE.start(),
                INTERVAL_MS_RANGE.end()
            ));
        }
        self.session.validate()?;
        self.plugins.menus.validate()
    }
}

#[derive(Debug)]
pub enum SettingsError {
    NotFound(PathBuf),
    UnsupportedVersion { path: PathBuf, found: u64 },
    Malformed { path: PathBuf, message: String },
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::NotFound(path) => write!(f, "settings not found: {}", path.display()),
            SettingsError::UnsupportedVersion { path, found } => write!(
                f,
                "settings {} has version {found}, only versions 1 to {SETTINGS_VERSION} are supported",
                path.display()
            ),
            SettingsError::Malformed { path, message } => {
                write!(f, "settings {} are malformed: {message}", path.display())
            }
            SettingsError::Io { path, source } => {
                write!(
                    f,
                    "settings {} could not be accessed: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SettingsError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// `$CALM_HOME/settings.json` when `CALM_HOME` is set, else `$HOME/.config/calm/settings.json`.
pub fn settings_path() -> PathBuf {
    settings_path_from(std::env::var_os("CALM_HOME"), std::env::var_os("HOME"))
}

/// Pure form of `settings_path`. An empty variable counts as unset, as in `layouts_dir_from`.
pub fn settings_path_from(calm_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    match calm_home.filter(|value| !value.is_empty()) {
        Some(calm_home) => PathBuf::from(calm_home).join("settings.json"),
        None => home
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".config/calm/settings.json"),
    }
}

/// `load_settings_from` at `settings_path()`.
pub fn load_settings() -> Result<Settings, SettingsError> {
    load_settings_from(&settings_path())
}

/// A missing file gives the defaults. Any other problem is an error the caller must handle:
/// silently starting from defaults would let the next save overwrite the user's file.
pub fn load_settings_from(path: &Path) -> Result<Settings, SettingsError> {
    match read_settings_from(path) {
        Err(SettingsError::NotFound(_)) => Ok(Settings::default()),
        other => other,
    }
}

/// Moves a rejected settings file aside to `settings.json.invalid-<n>` so it survives the
/// next save. Returns the new path.
pub fn set_aside_invalid(path: &Path) -> Result<PathBuf, SettingsError> {
    move_aside(path).map_err(|source| SettingsError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Strict reader: missing, unreadable, malformed, out of range and unknown versions are all
/// distinct errors. Version 1 files are migrated.
pub fn read_settings_from(path: &Path) -> Result<Settings, SettingsError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(SettingsError::NotFound(path.to_path_buf()));
        }
        Err(source) => {
            return Err(SettingsError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let malformed = |message: String| SettingsError::Malformed {
        path: path.to_path_buf(),
        message,
    };
    // Version first, so a newer file reports its version instead of a field error.
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| malformed(error.to_string()))?;
    let found = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| malformed("missing or non-integer \"version\"".to_string()))?;
    let settings: Settings = match found {
        1 => serde_json::from_value::<SettingsV1>(value)
            .map_err(|error| malformed(error.to_string()))?
            .into(),
        2 => serde_json::from_value(value).map_err(|error| malformed(error.to_string()))?,
        _ => {
            return Err(SettingsError::UnsupportedVersion {
                path: path.to_path_buf(),
                found,
            });
        }
    };
    settings.validate().map_err(malformed)?;
    Ok(settings)
}

pub fn save_settings(settings: &Settings) -> Result<PathBuf, SettingsError> {
    save_settings_to(&settings_path(), settings)
}

/// Pretty JSON through `write_durable`, like layouts: a crash never leaves a half-written
/// file, and the file is 0600.
pub fn save_settings_to(path: &Path, settings: &Settings) -> Result<PathBuf, SettingsError> {
    let json =
        serde_json::to_string_pretty(settings).map_err(|error| SettingsError::Malformed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    write_durable(path, json.as_bytes()).map_err(|source| SettingsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(path.to_path_buf())
}

/// Screen pixels between grid lines `step` world units apart at `zoom`, or `None` when they
/// would sit too close.
pub fn grid_screen_spacing(step: f64, zoom: f64) -> Option<f64> {
    let spacing = step * zoom;
    (spacing.is_finite() && spacing >= MIN_GRID_SPACING_PX).then_some(spacing)
}

/// Screen offsets in `[0, extent)` of the grid lines along one axis, where world 0 sits at
/// `screen_of_world_zero`. Empty when the grid is skipped at `zoom`.
/// Lines, not dots: one quad per line keeps the count near `extent / 8` at worst.
pub fn grid_line_offsets(screen_of_world_zero: f64, step: f64, zoom: f64, extent: f64) -> Vec<f64> {
    let Some(spacing) = grid_screen_spacing(step, zoom) else {
        return Vec::new();
    };
    if !screen_of_world_zero.is_finite() || !extent.is_finite() || extent <= 0.0 {
        return Vec::new();
    }
    let first = screen_of_world_zero.rem_euclid(spacing);
    let count = ((extent - first) / spacing).ceil().max(0.0) as usize;
    // Multiply instead of adding up, so rounding does not drift across the screen.
    (0..count)
        .map(|index| first + index as f64 * spacing)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::background::{GradientKind, GradientStop, ImageFit};
    use crate::grid::rgba_with_alpha;

    fn custom_settings() -> Settings {
        let mut settings = Settings::default();
        settings.canvas.click_empty_clears_focus = false;
        settings.background.kind = BackgroundKind::Gradient;
        settings.background.gradient.kind = GradientKind::Radial;
        settings.background.gradient.stops.push(GradientStop {
            color: Rgb(0xff0000),
            position: 0.5,
        });
        settings.background.image.path = Some(PathBuf::from("/tmp/wall.png"));
        settings.background.image.fit = ImageFit::Tile;
        settings.background.image.filters.blur = 12.0;
        settings.grid.show = true;
        settings.grid.spacing = 32.0;
        settings.grid.kind = GridKind::Crosses;
        settings.grid.mark_size = 3.0;
        settings.grid.major_every = 4;
        settings.grid.major_opacity = 0.5;
        settings.snap.grid = 16.0;
        settings.panels.confirm_close = false;
        settings.panels.default_terminal_width = 800.0;
        settings.panels.preview_below_zoom = 0.3;
        settings
    }

    #[test]
    fn session_settings_missing_file_and_missing_section_give_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("settings.json");
        assert!(matches!(
            read_settings_from(&missing),
            Err(SettingsError::NotFound(_))
        ));
        assert_eq!(
            load_settings_from(&missing).unwrap().session,
            SessionSettings::default()
        );

        // An existing v2 file written before the section existed.
        std::fs::write(&missing, r#"{ "version": 2, "grid": { "show": true } }"#).unwrap();
        let old = read_settings_from(&missing).unwrap();
        assert!(old.grid.show);
        let session = old.session;
        assert_eq!(session.restore_on_launch, RestoreOnLaunch::On);
        assert!(session.autosave);
        assert_eq!(session.debounce_ms, 1000);
        assert_eq!(session.backup_count, 3);
        assert_eq!(session.browser_query, BrowserQueryMode::StripAll);
    }

    #[test]
    fn session_settings_reject_malformed_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let file = |session: &str| format!(r#"{{ "version": 2, "session": {session} }}"#);

        // Positive control: every field set to a valid non-default value, edges included.
        std::fs::write(
            &path,
            file(
                r#"{ "restore_on_launch": "ask", "autosave": false, "debounce_ms": 10000,
                "backup_count": 0, "browser_query": "keep" }"#,
            ),
        )
        .unwrap();
        let valid = read_settings_from(&path).unwrap().session;
        assert_eq!(
            valid,
            SessionSettings {
                restore_on_launch: RestoreOnLaunch::Ask,
                autosave: false,
                debounce_ms: 10_000,
                backup_count: 0,
                browser_query: BrowserQueryMode::Keep,
            }
        );

        for (session, field) in [
            (r#"{ "debounce_ms": 249 }"#, "session.debounce_ms"),
            (r#"{ "debounce_ms": 10001 }"#, "session.debounce_ms"),
            (r#"{ "backup_count": 21 }"#, "session.backup_count"),
            (r#"{ "backup_count": -1 }"#, "integer `-1`"),
            (r#"{ "restore_on_launch": "maybe" }"#, "maybe"),
            (
                r#"{ "browser_query": "strip_sensitive" }"#,
                "strip_sensitive",
            ),
            (r#"{ "autosave": "yes" }"#, "invalid type"),
            (r#"{ "debounce": 500 }"#, "debounce"),
            (r#""on""#, "invalid type"),
        ] {
            std::fs::write(&path, file(session)).unwrap();
            match read_settings_from(&path) {
                Err(SettingsError::Malformed { message, .. }) => {
                    assert!(message.contains(field), "{session}: {message}");
                }
                other => panic!("{session}: expected Malformed, got {other:?}"),
            }
            // The tolerant loader must not turn a malformed file into defaults.
            assert!(load_settings_from(&path).is_err(), "{session}");
        }
    }

    #[test]
    fn saved_settings_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new/settings.json");
        save_settings_to(&path, &Settings::default()).unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&dir.path().join("new")), 0o700);
        assert_eq!(read_settings_from(&path).unwrap(), Settings::default());
    }

    #[test]
    fn stats_settings_default_and_reject_bad_values() {
        let absent: Settings = serde_json::from_str(r#"{ "version": 2 }"#).unwrap();
        assert_eq!(absent.developer.stats, StatsSettings::default());
        assert_eq!(absent.developer.stats.interval_ms, 1000);
        let set: Settings = serde_json::from_str(
            r#"{ "version": 2, "developer": { "stats": { "show_fps": false,
                "interval_ms": 250, "memory_unit": "gb", "include_children": true } } }"#,
        )
        .unwrap();
        let stats = &set.developer.stats;
        assert!(!stats.show_fps && stats.show_cpu && stats.include_children);
        assert_eq!(stats.memory_unit, MemoryUnit::Gb);
        assert!(set.validate().is_ok());
        for interval in [0, 249, 10_001] {
            let mut bad = Settings::default();
            bad.developer.stats.interval_ms = interval;
            let error = bad.validate().unwrap_err();
            assert!(error.contains("developer.stats.interval_ms"), "{error}");
        }
        for json in [
            r#"{ "version": 2, "developer": { "stats": { "memory_unit": "kb" } } }"#,
            r#"{ "version": 2, "developer": { "stats": { "show_gpu": true } } }"#,
            r#"{ "version": 2, "developer": { "stats": { "interval_ms": -5 } } }"#,
            r#"{ "version": 2, "developer": { "stats": { "show_cpu": "yes" } } }"#,
        ] {
            assert!(serde_json::from_str::<Settings>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn plugin_menu_settings_default_and_reject_bad_values() {
        let absent: Settings = serde_json::from_str(r#"{ "version": 2 }"#).unwrap();
        assert_eq!(absent.plugins.menus, PluginMenuSettings::default());
        assert_eq!(absent.plugins.menus.submenu_above, 3);
        let set: Settings = serde_json::from_str(
            r#"{ "version": 2, "plugins": { "menus": {
                "hidden": ["github-prs", "hello-world/rename"], "submenu_above": 0 } } }"#,
        )
        .unwrap();
        assert_eq!(set.plugins.menus.hidden.len(), 2);
        assert!(set.validate().is_ok());
        for (field, value) in [
            ("hidden", r#"["Bad Id"]"#),
            ("hidden", r#"["a/"]"#),
            ("hidden", r#"["a/b/c"]"#),
            ("submenu_above", "51"),
        ] {
            let json = format!(
                r#"{{ "version": 2, "plugins": {{ "menus": {{ "{field}": {value} }} }} }}"#
            );
            let settings: Settings = serde_json::from_str(&json).unwrap();
            let err = settings.validate().unwrap_err();
            assert!(err.contains(&format!("plugins.menus.{field}")), "{err}");
        }
        // A misspelled key is an error, not silently ignored.
        assert!(
            serde_json::from_str::<Settings>(
                r#"{ "version": 2, "plugins": { "menus": { "hide": [] } } }"#
            )
            .is_err()
        );
    }

    #[test]
    fn plugins_builtin_reads_from_file_and_defaults_on() {
        let off: Settings =
            serde_json::from_str(r#"{ "version": 2, "plugins": { "builtin": false } }"#).unwrap();
        assert!(!off.plugins.builtin);
        let absent: Settings = serde_json::from_str(r#"{ "version": 2 }"#).unwrap();
        assert!(absent.plugins.builtin);
    }

    #[test]
    fn default_settings_values() {
        let settings = Settings::default();
        assert_eq!(settings.version, 2);
        assert_eq!(settings.background.kind, BackgroundKind::Solid);
        assert_eq!(settings.background.solid, Rgb(0x151515));
        assert!(!settings.grid.show);
        assert!(settings.snap.enabled);
        assert_eq!(settings.snap.config(), SnapConfig::default());
        assert!(settings.panels.confirm_close);
        assert!(settings.panels.terminal_user_config);
        assert_eq!(
            settings.panels.default_terminal_size(),
            DEFAULT_TERMINAL_SIZE
        );
        assert_eq!(settings.panels.preview_below_zoom, PREVIEW_ZOOM_THRESHOLD);
        assert!(settings.plugins.builtin);
        assert_eq!(settings.validate(), Ok(()));
    }

    #[test]
    fn settings_path_prefers_calm_home() {
        assert_eq!(
            settings_path_from(Some("/calm".into()), Some("/Users/me".into())),
            PathBuf::from("/calm/settings.json")
        );
        assert_eq!(
            settings_path_from(None, Some("/Users/me".into())),
            PathBuf::from("/Users/me/.config/calm/settings.json")
        );
        assert_eq!(
            settings_path_from(Some("".into()), Some("/Users/me".into())),
            PathBuf::from("/Users/me/.config/calm/settings.json")
        );
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(
            matches!(read_settings_from(&path), Err(SettingsError::NotFound(p)) if p == path),
            "missing file must read as NotFound"
        );
        assert_eq!(load_settings_from(&path).unwrap(), Settings::default());
        assert!(!path.exists(), "loading must not create the file");
    }

    #[test]
    fn version_1_file_migrates_without_loss() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, include_str!("../testdata/settings_v1.json")).unwrap();
        let settings = load_settings_from(&path).unwrap();
        let mut expected = Settings::default();
        expected.background.solid = Rgb(0x101827);
        expected.grid.show = true;
        expected.panels.confirm_close = false;
        expected.panels.terminal_user_config = false;
        assert_eq!(settings, expected);
        // Every version 1 preset keeps its exact color.
        for (preset, rgb) in [
            ("dark_gray", 0x151515),
            ("black", 0x000000),
            ("graphite", 0x2b2b2e),
            ("midnight_blue", 0x101827),
            ("forest", 0x0f1a14),
        ] {
            std::fs::write(
                &path,
                format!(r#"{{"version": 1, "background": "{preset}"}}"#),
            )
            .unwrap();
            let settings = read_settings_from(&path).unwrap();
            assert_eq!(settings.background.solid, Rgb(rgb), "{preset}");
            assert_eq!(settings.background.kind, BackgroundKind::Solid, "{preset}");
        }
        // A migrated file saves as version 2 and reads back unchanged.
        save_settings_to(&path, &settings).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("\"version\": 2")
        );
        assert_eq!(read_settings_from(&path).unwrap(), settings);
    }

    #[test]
    fn unknown_move_modifier_is_a_settings_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // Positive control: every known value loads, and a file without it gets cmd.
        for (name, value) in [
            ("cmd", MoveModifier::Cmd),
            ("ctrl", MoveModifier::Ctrl),
            ("alt", MoveModifier::Alt),
            ("cmd+ctrl", MoveModifier::CmdCtrl),
            ("cmd+alt", MoveModifier::CmdAlt),
            ("ctrl+alt", MoveModifier::CtrlAlt),
        ] {
            let text = format!(r#"{{"version": 2, "canvas": {{"move_modifier": "{name}"}}}}"#);
            std::fs::write(&path, text).unwrap();
            assert_eq!(
                read_settings_from(&path).unwrap().canvas.move_modifier,
                value
            );
            let mut settings = Settings::default();
            settings.canvas.move_modifier = value;
            let json = serde_json::to_value(&settings).unwrap();
            assert_eq!(json["canvas"]["move_modifier"], name);
        }
        std::fs::write(&path, r#"{"version": 2}"#).unwrap();
        assert_eq!(
            read_settings_from(&path).unwrap().canvas.move_modifier,
            MoveModifier::Cmd
        );

        for text in [
            r#"{"version": 2, "canvas": {"move_modifier": "off"}}"#,
            r#"{"version": 2, "canvas": {"move_modifier": "Cmd"}}"#,
            r#"{"version": 2, "canvas": {"move_modifier": ""}}"#,
            r#"{"version": 2, "canvas": {"move_modifier": 1}}"#,
            r#"{"version": 2, "canvas": {"move_modifier": null}}"#,
        ] {
            std::fs::write(&path, text).unwrap();
            let error = read_settings_from(&path).unwrap_err();
            assert!(
                matches!(&error, SettingsError::Malformed { .. }),
                "{text}: {error:?}"
            );
            assert!(
                error.to_string().contains("canvas.move_modifier"),
                "{text}: {error}"
            );
            assert!(load_settings_from(&path).is_err(), "{text}");
        }
    }

    #[test]
    fn malformed_file_is_an_explicit_error_not_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // Positive control: a valid non-default file loads as written.
        let custom = custom_settings();
        std::fs::write(&path, serde_json::to_string(&custom).unwrap()).unwrap();
        assert_eq!(read_settings_from(&path).unwrap(), custom);
        assert_eq!(load_settings_from(&path).unwrap(), custom);

        let cases = [
            ("not json", "{not json"),
            ("array", "[]"),
            ("no version", r#"{"grid": {"show": true}}"#),
            ("v1 wrong type", r#"{"version": 1, "show_grid": "yes"}"#),
            (
                "v1 unknown preset",
                r#"{"version": 1, "background": "neon"}"#,
            ),
            (
                "v2 wrong type",
                r#"{"version": 2, "grid": {"show": "yes"}}"#,
            ),
            (
                "click clears focus type",
                r#"{"version": 2, "canvas": {"click_empty_clears_focus": "yes"}}"#,
            ),
            (
                "click clears focus number",
                r#"{"version": 2, "canvas": {"click_empty_clears_focus": 1}}"#,
            ),
            (
                "bad color",
                r##"{"version": 2, "background": {"solid": "#12"}}"##,
            ),
            (
                "unknown kind",
                r#"{"version": 2, "background": {"kind": "video"}}"#,
            ),
            (
                "opacity range",
                r#"{"version": 2, "grid": {"opacity": 1.5}}"#,
            ),
            ("snap step", r#"{"version": 2, "snap": {"grid": 5}}"#),
            (
                "unknown grid kind",
                r#"{"version": 2, "grid": {"kind": "hexagons"}}"#,
            ),
            ("grid kind type", r#"{"version": 2, "grid": {"kind": 1}}"#),
            (
                "mark size range",
                r#"{"version": 2, "grid": {"mark_size": 9}}"#,
            ),
            (
                "major every one",
                r#"{"version": 2, "grid": {"major_every": 1}}"#,
            ),
            (
                "major every range",
                r#"{"version": 2, "grid": {"major_every": 33}}"#,
            ),
            (
                "major every negative",
                r#"{"version": 2, "grid": {"major_every": -8}}"#,
            ),
            (
                "major opacity range",
                r#"{"version": 2, "grid": {"major_opacity": -0.1}}"#,
            ),
            (
                "blur range",
                r#"{"version": 2, "background": {"image": {"filters": {"blur": -1}}}}"#,
            ),
            (
                "one stop",
                r##"{"version": 2, "background": {"gradient": {"stops": [{"color": "#000000", "position": 0}]}}}"##,
            ),
            (
                "terminal size",
                r#"{"version": 2, "panels": {"default_terminal_width": 1}}"#,
            ),
        ];
        for (name, text) in cases {
            std::fs::write(&path, text).unwrap();
            let error = read_settings_from(&path).unwrap_err();
            assert!(
                matches!(&error, SettingsError::Malformed { path: p, .. } if *p == path),
                "{name}: {error:?}"
            );
            assert!(error.to_string().contains(path.to_str().unwrap()), "{name}");
            assert!(
                matches!(
                    load_settings_from(&path),
                    Err(SettingsError::Malformed { .. })
                ),
                "{name}: load must not fall back to defaults"
            );
        }

        std::fs::write(&path, r#"{"version": 3}"#).unwrap();
        let error = load_settings_from(&path).unwrap_err();
        assert!(
            matches!(&error, SettingsError::UnsupportedVersion { found: 3, .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains(path.to_str().unwrap()));
    }

    #[test]
    fn version_2_file_without_grid_styles_loads_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let fixture = include_str!("../testdata/settings_v2_before_grid_styles.json");
        assert!(!fixture.contains("kind\": \"lines") && !fixture.contains("major"));
        std::fs::write(&path, fixture).unwrap();
        let settings = read_settings_from(&path).unwrap();
        assert!(settings.grid.show);
        assert_eq!(settings.grid.spacing, 48.0);
        assert_eq!(settings.grid.color, Rgb(0x88ccff));
        assert_eq!(settings.grid.opacity, 0.2);
        // New fields take defaults that draw exactly the old grid: lines, no major grid.
        assert_eq!(settings.grid.kind, GridKind::Lines);
        assert_eq!(settings.grid.mark_size, 2.0);
        assert_eq!(settings.grid.major_every, 0);
        assert_eq!(settings.version, 2);

        // Positive control: every new grid field written by hand reads back as written.
        std::fs::write(
            &path,
            r#"{"version": 2, "grid": {"kind": "dots", "mark_size": 4, "major_every": 8, "major_opacity": 0.6}}"#,
        )
        .unwrap();
        let grid = read_settings_from(&path).unwrap().grid;
        assert_eq!(grid.kind, GridKind::Dots);
        assert_eq!(grid.mark_size, 4.0);
        assert_eq!(grid.major_every, 8);
        assert_eq!(grid.major_opacity, 0.6);
        for kind in ["lines", "crosses", "none"] {
            std::fs::write(
                &path,
                format!(r#"{{"version": 2, "grid": {{"kind": "{kind}"}}}}"#),
            )
            .unwrap();
            assert!(read_settings_from(&path).is_ok(), "{kind}");
        }
    }

    #[test]
    fn invalid_file_is_set_aside_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{broken").unwrap();
        let first = set_aside_invalid(&path).unwrap();
        assert_eq!(first, dir.path().join("settings.json.invalid-0"));
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "{broken");
        std::fs::write(&path, "{again").unwrap();
        let second = set_aside_invalid(&path).unwrap();
        assert_eq!(second, dir.path().join("settings.json.invalid-1"));
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "{broken");
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        // The directory does not exist yet; save creates it.
        let path = dir.path().join("nested/settings.json");
        let settings = custom_settings();
        assert_eq!(save_settings_to(&path, &settings).unwrap(), path);
        assert_eq!(load_settings_from(&path).unwrap(), settings);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"kind\": \"gradient\""), "{text}");
        assert!(text.contains("\"color\": \"#ff0000\""), "{text}");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            vec![OsString::from("settings.json")],
            "temp file left"
        );
    }

    #[test]
    fn snap_and_grid_derive_from_settings() {
        let mut snap = SnapSettings {
            grid: 16.0,
            edge_distance: 4.0,
            ..SnapSettings::default()
        };
        assert_eq!(
            snap.config(),
            SnapConfig {
                grid: Some(16.0),
                edge_distance: 4.0
            }
        );
        snap.enabled = false;
        assert_eq!(
            snap.config(),
            SnapConfig {
                grid: None,
                edge_distance: 0.0
            }
        );
        let grid = GridSettings::default();
        assert_eq!(rgba_with_alpha(grid.color.0, grid.opacity), 0xffffff1f);
        assert_eq!(rgba_with_alpha(0x336699, 1.0), 0x336699ff);
    }

    #[test]
    fn grid_spacing_skips_below_8px() {
        assert_eq!(grid_screen_spacing(64.0, 1.0), Some(64.0));
        assert_eq!(grid_screen_spacing(64.0, 2.0), Some(128.0));
        assert_eq!(grid_screen_spacing(64.0, 0.125), Some(8.0));
        assert_eq!(grid_screen_spacing(64.0, 0.12), None);
        assert_eq!(grid_screen_spacing(16.0, 0.5), Some(8.0));
        assert_eq!(grid_screen_spacing(64.0, 0.0), None);
        assert_eq!(grid_screen_spacing(64.0, f64::NAN), None);
    }

    #[test]
    fn grid_lines_follow_zoom_pan_and_step() {
        assert_eq!(
            grid_line_offsets(0.0, 64.0, 1.0, 256.0),
            vec![0.0, 64.0, 128.0, 192.0]
        );
        // World 0 left of the screen: the first line is the next multiple of the step.
        assert_eq!(
            grid_line_offsets(-10.0, 64.0, 1.0, 256.0),
            vec![54.0, 118.0, 182.0, 246.0]
        );
        assert_eq!(
            grid_line_offsets(300.0, 64.0, 1.0, 128.0),
            vec![44.0, 108.0]
        );
        assert_eq!(
            grid_line_offsets(0.0, 32.0, 1.0, 128.0),
            vec![0.0, 32.0, 64.0, 96.0]
        );
        // Zoom 0.4 over 1280 px: 25.6 px apart, 50 lines, all on screen.
        let lines = grid_line_offsets(0.0, 64.0, 0.4, 1280.0);
        assert_eq!(lines.len(), 50);
        assert!(lines.iter().all(|x| (0.0..1280.0).contains(x)));
        assert!((lines[49] - 49.0 * 25.6).abs() < 1e-9);
        // At the 8 px limit the count stays at extent / 8; below it the grid is skipped.
        assert_eq!(grid_line_offsets(0.0, 64.0, 0.125, 1280.0).len(), 160);
        assert!(grid_line_offsets(0.0, 64.0, 0.1, 1280.0).is_empty());
        assert!(grid_line_offsets(f64::NAN, 64.0, 1.0, 1280.0).is_empty());
        assert!(grid_line_offsets(0.0, 64.0, 1.0, 0.0).is_empty());
    }
}
