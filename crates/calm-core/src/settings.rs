//! App settings persisted to `settings.json`: canvas background, grid and close behavior.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

pub const SETTINGS_VERSION: u32 = 1;
/// Makes temp file names unique per save call, not just per process.
static SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);
/// World units between grid dots.
pub const GRID_STEP_WORLD: f64 = 64.0;
/// Below this screen spacing the dots turn into noise, so the grid is skipped.
pub const MIN_GRID_SPACING_PX: f64 = 8.0;

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
    pub fn rgb(self) -> u32 {
        match self {
            BackgroundPreset::DarkGray => 0x151515,
            BackgroundPreset::Black => 0x000000,
            BackgroundPreset::Graphite => 0x2b2b2e,
            BackgroundPreset::MidnightBlue => 0x101827,
            BackgroundPreset::Forest => 0x0f1a14,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BackgroundPreset::DarkGray => "Dark Gray",
            BackgroundPreset::Black => "Black",
            BackgroundPreset::Graphite => "Graphite",
            BackgroundPreset::MidnightBlue => "Midnight Blue",
            BackgroundPreset::Forest => "Forest",
        }
    }

    /// Menu order.
    pub fn all() -> &'static [BackgroundPreset] {
        &[
            BackgroundPreset::DarkGray,
            BackgroundPreset::Black,
            BackgroundPreset::Graphite,
            BackgroundPreset::MidnightBlue,
            BackgroundPreset::Forest,
        ]
    }
}

/// Fields missing from the file take their default, so a hand-edited file may be partial.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub background: BackgroundPreset,
    pub show_grid: bool,
    pub confirm_close: bool,
    /// Terminals read the user's Ghostty config. Applies to terminals spawned afterwards.
    pub terminal_user_config: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: SETTINGS_VERSION,
            background: BackgroundPreset::DarkGray,
            show_grid: false,
            confirm_close: true,
            terminal_user_config: true,
        }
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
                "settings {} has version {found}, only version {SETTINGS_VERSION} is supported",
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

pub fn load_settings() -> Settings {
    load_settings_from(&settings_path())
}

/// Never fails: a missing file gives the defaults quietly, a bad one with a warning.
pub fn load_settings_from(path: &Path) -> Settings {
    match read_settings_from(path) {
        Ok(settings) => settings,
        Err(SettingsError::NotFound(_)) => Settings::default(),
        Err(error) => {
            eprintln!("calm: {error}; using default settings");
            Settings::default()
        }
    }
}

/// Strict reader behind `load_settings_from`, so callers and tests can see why a file was rejected.
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
    if found != u64::from(SETTINGS_VERSION) {
        return Err(SettingsError::UnsupportedVersion {
            path: path.to_path_buf(),
            found,
        });
    }
    serde_json::from_value(value).map_err(|error| malformed(error.to_string()))
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

/// Screen pixels between grid dots at `zoom`, or `None` when they would sit too close.
pub fn grid_screen_spacing(zoom: f64) -> Option<f64> {
    let spacing = GRID_STEP_WORLD * zoom;
    (spacing.is_finite() && spacing >= MIN_GRID_SPACING_PX).then_some(spacing)
}

/// Screen offsets in `[0, extent)` of the grid lines along one axis, where world 0 sits at
/// `screen_of_world_zero`. Empty when the grid is skipped at `zoom`.
/// Lines, not dots: one quad per line keeps the count near `extent / 8` at worst.
pub fn grid_line_offsets(screen_of_world_zero: f64, zoom: f64, extent: f64) -> Vec<f64> {
    let Some(spacing) = grid_screen_spacing(zoom) else {
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

/// `background` with each channel moved 12 % of the way to white.
pub fn grid_dot_rgb(background: u32) -> u32 {
    let lighten = |shift: u32| {
        let channel = f64::from((background >> shift) & 0xff);
        let lighter = (channel + (255.0 - channel) * 0.12).round() as u32;
        lighter.min(0xff) << shift
    };
    lighten(16) | lighten(8) | lighten(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_values() {
        let settings = Settings::default();
        assert_eq!(settings.version, 1);
        assert_eq!(settings.background, BackgroundPreset::DarkGray);
        assert!(!settings.show_grid);
        assert!(settings.confirm_close);
        assert!(settings.terminal_user_config);
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
    fn load_missing_file_gives_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert!(
            matches!(read_settings_from(&path), Err(SettingsError::NotFound(p)) if p == path),
            "missing file must read as NotFound"
        );
        assert_eq!(load_settings_from(&path), Settings::default());
        assert!(!path.exists(), "loading must not create the file");
    }

    #[test]
    fn load_malformed_file_gives_default_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        // Positive control: a valid non-default file loads as written.
        let custom = Settings {
            background: BackgroundPreset::Forest,
            show_grid: true,
            ..Settings::default()
        };
        std::fs::write(&path, serde_json::to_string(&custom).unwrap()).unwrap();
        assert_eq!(read_settings_from(&path).unwrap(), custom);
        assert_eq!(load_settings_from(&path), custom);

        let cases = [
            ("not json", "{not json"),
            ("wrong type", r#"{"version": 1, "show_grid": "yes"}"#),
            ("unknown preset", r#"{"version": 1, "background": "neon"}"#),
            ("no version", r#"{"background": "black"}"#),
            ("array", "[]"),
        ];
        for (name, text) in cases {
            std::fs::write(&path, text).unwrap();
            let error = read_settings_from(&path).unwrap_err();
            assert!(
                matches!(&error, SettingsError::Malformed { path: p, .. } if *p == path),
                "{name}: {error:?}"
            );
            assert!(error.to_string().contains(path.to_str().unwrap()), "{name}");
            assert_eq!(load_settings_from(&path), Settings::default(), "{name}");
        }

        std::fs::write(&path, r#"{"version": 2, "background": "black"}"#).unwrap();
        let error = read_settings_from(&path).unwrap_err();
        assert!(
            matches!(&error, SettingsError::UnsupportedVersion { found: 2, .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains(path.to_str().unwrap()));
        assert_eq!(load_settings_from(&path), Settings::default());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        // The directory does not exist yet; save creates it.
        let path = dir.path().join("nested/settings.json");
        let settings = Settings {
            version: SETTINGS_VERSION,
            background: BackgroundPreset::MidnightBlue,
            show_grid: true,
            confirm_close: false,
            terminal_user_config: false,
        };
        assert_eq!(save_settings_to(&path, &settings).unwrap(), path);
        assert_eq!(load_settings_from(&path), settings);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\n  \"background\": \"midnight_blue\""),
            "{text}"
        );
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
    fn preset_rgb_and_labels_cover_all() {
        let expected = [
            (BackgroundPreset::DarkGray, 0x151515, "Dark Gray"),
            (BackgroundPreset::Black, 0x000000, "Black"),
            (BackgroundPreset::Graphite, 0x2b2b2e, "Graphite"),
            (BackgroundPreset::MidnightBlue, 0x101827, "Midnight Blue"),
            (BackgroundPreset::Forest, 0x0f1a14, "Forest"),
        ];
        let all: Vec<BackgroundPreset> = expected.iter().map(|(preset, ..)| *preset).collect();
        assert_eq!(BackgroundPreset::all(), all.as_slice());
        for (preset, rgb, label) in expected {
            assert_eq!(preset.rgb(), rgb, "{preset:?}");
            assert_eq!(preset.label(), label, "{preset:?}");
        }
    }

    #[test]
    fn grid_spacing_skips_below_8px() {
        assert_eq!(grid_screen_spacing(1.0), Some(GRID_STEP_WORLD));
        assert_eq!(grid_screen_spacing(2.0), Some(128.0));
        assert_eq!(grid_screen_spacing(0.125), Some(8.0));
        assert_eq!(grid_screen_spacing(0.12), None);
        assert_eq!(grid_screen_spacing(0.0), None);
        assert_eq!(grid_screen_spacing(f64::NAN), None);
    }

    #[test]
    fn grid_lines_follow_zoom_and_pan() {
        assert_eq!(
            grid_line_offsets(0.0, 1.0, 256.0),
            vec![0.0, 64.0, 128.0, 192.0]
        );
        // World 0 left of the screen: the first line is the next multiple of the step.
        assert_eq!(
            grid_line_offsets(-10.0, 1.0, 256.0),
            vec![54.0, 118.0, 182.0, 246.0]
        );
        assert_eq!(grid_line_offsets(300.0, 1.0, 128.0), vec![44.0, 108.0]);
        // Zoom 0.4 over 1280 px: 25.6 px apart, 50 lines, all on screen.
        let lines = grid_line_offsets(0.0, 0.4, 1280.0);
        assert_eq!(lines.len(), 50);
        assert!(lines.iter().all(|x| (0.0..1280.0).contains(x)));
        assert!((lines[49] - 49.0 * 25.6).abs() < 1e-9);
        // At the 8 px limit the count stays at extent / 8; below it the grid is skipped.
        assert_eq!(grid_line_offsets(0.0, 0.125, 1280.0).len(), 160);
        assert!(grid_line_offsets(0.0, 0.1, 1280.0).is_empty());
        assert!(grid_line_offsets(f64::NAN, 1.0, 1280.0).is_empty());
        assert!(grid_line_offsets(0.0, 1.0, 0.0).is_empty());
    }

    #[test]
    fn grid_dot_is_12_percent_lighter() {
        // Each channel moves 12 % of the way to 255, rounded.
        assert_eq!(grid_dot_rgb(0x000000), 0x1f1f1f);
        assert_eq!(grid_dot_rgb(0x151515), 0x313131);
        assert_eq!(grid_dot_rgb(0x101827), 0x2d3441);
        assert_eq!(grid_dot_rgb(0xffffff), 0xffffff);
    }
}
