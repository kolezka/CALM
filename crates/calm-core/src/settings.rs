//! App settings persisted to `settings.json`: background, grid, snapping and panel options.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::background::{
    BackgroundKind, BackgroundSettings, MAX_BLUR, MAX_FILTER_FACTOR, MAX_GRADIENT_STOPS,
    MIN_GRADIENT_STOPS, Rgb,
};
use crate::geometry::Size;
use crate::menu::DEFAULT_TERMINAL_SIZE;
use crate::preview::PREVIEW_ZOOM_THRESHOLD;
use crate::snap::{DEFAULT_SNAP_EDGE, DEFAULT_SNAP_GRID, SnapConfig};

/// Version 2 replaced the background preset with solid, gradient and image backgrounds and
/// added grid, snapping and panel options. Version 1 files are migrated on read.
pub const SETTINGS_VERSION: u32 = 2;
/// Makes temp file names unique per save call, not just per process.
static SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);
/// Below this screen spacing the grid lines turn into noise, so the grid is skipped.
pub const MIN_GRID_SPACING_PX: f64 = 8.0;

pub const GRID_SPACING_RANGE: RangeInclusive<f64> = 16.0..=256.0;
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
}

/// Plugin discovery options. Read once at startup.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    /// Also load the plugins shipped with CALM (the repo's `plugins/` in a dev build,
    /// `CALM.app/Contents/Resources/plugins` in a bundle).
    pub builtin: bool,
}

impl Default for PluginSettings {
    fn default() -> Self {
        PluginSettings { builtin: true }
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
}

impl Default for GridSettings {
    fn default() -> Self {
        // White at 12 % matches the version 1 grid: the background 12 % lighter.
        GridSettings {
            show: false,
            spacing: 64.0,
            color: Rgb(0xffffff),
            opacity: 0.12,
        }
    }
}

impl GridSettings {
    /// `0xRRGGBBAA`, alpha from `opacity`, so lines blend over images and gradients too.
    pub fn line_rgba(&self) -> u32 {
        let alpha = (self.opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
        (self.color.0 & 0xff_ffff) << 8 | alpha
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
        )
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
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".to_string());
    for attempt in 0..1000 {
        let target = path.with_file_name(format!("{name}.invalid-{attempt}"));
        if target.exists() {
            continue;
        }
        return fs::rename(path, &target)
            .map(|()| target)
            .map_err(|source| SettingsError::Io {
                path: path.to_path_buf(),
                source,
            });
    }
    Err(SettingsError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::AlreadyExists, "too many invalid copies"),
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

/// Pretty JSON through a temp file and a rename, like layouts, so a crash never
/// leaves a half-written file.
pub fn save_settings_to(path: &Path, settings: &Settings) -> Result<PathBuf, SettingsError> {
    let io_error = |source| SettingsError::Io {
        path: path.to_path_buf(),
        source,
    };
    let json =
        serde_json::to_string_pretty(settings).map_err(|error| SettingsError::Malformed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(io_error)?;
    let temp_path = dir.join(format!(
        ".settings.json.tmp-{}-{}",
        std::process::id(),
        SAVE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(source) = fs::write(&temp_path, json).and_then(|()| fs::rename(&temp_path, path)) {
        // Best effort cleanup; the write or rename error is the one worth reporting.
        let _ = fs::remove_file(&temp_path);
        return Err(io_error(source));
    }
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

    fn custom_settings() -> Settings {
        let mut settings = Settings::default();
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
        settings.snap.grid = 16.0;
        settings.panels.confirm_close = false;
        settings.panels.default_terminal_width = 800.0;
        settings.panels.preview_below_zoom = 0.3;
        settings
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
        assert_eq!(GridSettings::default().line_rgba(), 0xffffff1f);
        let grid = GridSettings {
            color: Rgb(0x336699),
            opacity: 1.0,
            ..GridSettings::default()
        };
        assert_eq!(grid.line_rgba(), 0x336699ff);
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
