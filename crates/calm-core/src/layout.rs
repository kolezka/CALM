use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::browser_url::{BrowserQueryMode, scrub_for_save, scrub_text_for_save};
use crate::camera::{Camera, MAX_ZOOM, MIN_ZOOM};
use crate::geometry::Rect;
use crate::scene::{PanelKind, PanelOverrides, Scene};

pub const LAYOUT_VERSION: u32 = 2;
/// The oldest version `load_layout_from` migrates.
pub const LAYOUT_VERSION_V1: u32 = 1;

/// Makes temp file names unique per save call, not just per process.
static SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutFile {
    pub version: u32,
    /// When the file was written. `None` for a migrated v1 file or an unstamped layout.
    pub saved_at_unix: Option<u64>,
    pub camera: Camera,
    /// Back to front, same order as `Scene::panels`.
    pub panels: Vec<LayoutPanel>,
}

/// Configuration only. Session state (scrollback, history, cookies) never goes here.
/// The resize policy is not saved: it comes from the kind and the current manifest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutPanel {
    pub kind: PanelKind,
    pub rect: Rect,
    pub title: String,
    pub minimized: bool,
    /// A missing key means no overrides; no overrides writes no key.
    #[serde(default, skip_serializing_if = "PanelOverrides::is_empty")]
    pub overrides: PanelOverrides,
}

/// Version 1 shape, read only.
#[derive(Deserialize)]
struct LayoutFileV1 {
    camera: Camera,
    panels: Vec<LayoutPanelV1>,
}

#[derive(Deserialize)]
struct LayoutPanelV1 {
    kind: PanelKind,
    rect: Rect,
    title: String,
    minimized: bool,
}

/// A v1 layout as v2: no timestamp, no overrides. The kinds already default their new
/// fields to `None`. Nothing else changes.
fn migrate_v1(v1: LayoutFileV1) -> LayoutFile {
    LayoutFile {
        version: LAYOUT_VERSION,
        saved_at_unix: None,
        camera: v1.camera,
        panels: v1
            .panels
            .into_iter()
            .map(|panel| LayoutPanel {
                kind: panel.kind,
                rect: panel.rect,
                title: panel.title,
                minimized: panel.minimized,
                overrides: PanelOverrides::default(),
            })
            .collect(),
    }
}

#[derive(Deserialize)]
struct VersionProbe {
    version: Option<serde_json::Value>,
}

#[derive(Debug)]
pub enum LayoutError {
    NotFound(PathBuf),
    UnsupportedVersion { path: PathBuf, found: u64 },
    Malformed { path: PathBuf, message: String },
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::NotFound(path) => write!(f, "layout not found: {}", path.display()),
            LayoutError::UnsupportedVersion { path, found } => write!(
                f,
                "layout {} has version {found}, only versions {LAYOUT_VERSION_V1} to {LAYOUT_VERSION} are supported",
                path.display()
            ),
            LayoutError::Malformed { path, message } => {
                write!(f, "layout {} is malformed: {message}", path.display())
            }
            LayoutError::Io { path, source } => {
                write!(
                    f,
                    "layout {} could not be accessed: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for LayoutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LayoutError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Scene {
    pub fn to_layout(&self) -> LayoutFile {
        LayoutFile {
            version: LAYOUT_VERSION,
            saved_at_unix: None,
            camera: self.camera,
            panels: self
                .panels
                .iter()
                .map(|panel| LayoutPanel {
                    kind: panel.kind.clone(),
                    rect: panel.rect,
                    title: panel.title.clone(),
                    minimized: panel.minimized,
                    overrides: panel.overrides.clone(),
                })
                .collect(),
        }
    }

    /// Panels get fresh ids and keep their order. Rects below `MIN_PANEL_SIZE`
    /// are enlarged, the same rule `Scene::add` applies.
    pub fn from_layout(layout: LayoutFile) -> Scene {
        let mut scene = Scene::default();
        scene.camera = layout.camera;
        for layout_panel in layout.panels {
            let id = scene.add(layout_panel.kind, layout_panel.rect);
            if let Some(panel) = scene.get_mut(id) {
                panel.title = layout_panel.title;
                panel.minimized = layout_panel.minimized;
                panel.overrides = layout_panel.overrides;
            }
        }
        scene
    }
}

/// `$CALM_HOME/layouts` when `CALM_HOME` is set, else `$HOME/.config/calm/layouts`.
pub fn layouts_dir() -> PathBuf {
    layouts_dir_from(std::env::var_os("CALM_HOME"), std::env::var_os("HOME"))
}

/// Pure form of `layouts_dir`, so tests need not mutate the process environment.
/// An empty variable counts as unset. Without `HOME` the path is relative to the cwd.
pub fn layouts_dir_from(calm_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    match calm_home.filter(|value| !value.is_empty()) {
        Some(calm_home) => PathBuf::from(calm_home).join("layouts"),
        None => home
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".config/calm/layouts"),
    }
}

/// `save_layout_in` at `layouts_dir()`.
pub fn save_layout(
    name: &str,
    layout: &LayoutFile,
    browser_query: BrowserQueryMode,
) -> Result<PathBuf, LayoutError> {
    save_layout_in(&layouts_dir(), name, layout, browser_query)
}

pub fn load_layout(name: &str) -> Result<LayoutFile, LayoutError> {
    load_layout_from(&layouts_dir(), name)
}

/// Writes `<dir>/<name>.json` with `write_layout_file`.
pub fn save_layout_in(
    dir: &Path,
    name: &str,
    layout: &LayoutFile,
    browser_query: BrowserQueryMode,
) -> Result<PathBuf, LayoutError> {
    let path = layout_path(dir, name)?;
    write_layout_file(&path, layout, browser_query)?;
    Ok(path)
}

/// The one write path for named layouts and the session. Stamps `saved_at_unix` with
/// the current time, scrubs browser URLs per `browser_query`, then writes durably.
pub fn write_layout_file(
    path: &Path,
    layout: &LayoutFile,
    browser_query: BrowserQueryMode,
) -> Result<(), LayoutError> {
    let prepared = prepare_for_save(layout, browser_query, now_unix());
    let json = serde_json::to_string_pretty(&prepared).map_err(|error| LayoutError::Malformed {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    write_durable(path, json.as_bytes()).map_err(|source| LayoutError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn prepare_for_save(layout: &LayoutFile, browser_query: BrowserQueryMode, now: u64) -> LayoutFile {
    let mut prepared = layout.clone();
    prepared.saved_at_unix = Some(now);
    for panel in &mut prepared.panels {
        if let PanelKind::Browser { url, .. } = &mut panel.kind {
            let scrubbed = scrub_for_save(url, browser_query);
            // A new browser panel is titled with its url; it follows the url, size guard
            // included. Any other title is page text that can quote urls, so scrub those.
            if panel.title == *url {
                panel.title.clone_from(&scrubbed);
            } else {
                panel.title = scrub_text_for_save(&panel.title, browser_query);
            }
            *url = scrubbed;
        }
    }
    prepared
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Writes `bytes` to `path` so a crash leaves either the old file or the new one: a new
/// same-directory temp file (mode 0600, created exclusively), write, fsync, rename over
/// `path`, fsync the directory. Missing directories are created with mode 0700. The temp
/// file is removed on failure.
pub fn write_durable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_durable_with(path, bytes, |from, to| fs::rename(from, to))
}

/// `write_durable` with the rename step injectable, so tests can fail it.
fn write_durable_with(
    path: &Path,
    bytes: &[u8],
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let dir = parent_dir(path);
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy();
    create_private_dir(dir)?;
    let temp_path = dir.join(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        SAVE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp_path)?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            rename(&temp_path, path)
        });
    if let Err(error) = written {
        // The write or rename error is the one worth reporting; the temp may already be gone.
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    sync_dir(dir)
}

/// The directory holding `path`, `.` for a bare file name.
fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    }
}

/// Creates `dir` and its missing ancestors with mode 0700, top down. After each one the
/// parent is fsynced, so the new entry survives a power loss.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    create_private_dir_with(dir, sync_dir)
}

/// `create_private_dir` with the parent fsync injectable, so tests can record it.
fn create_private_dir_with(
    dir: &Path,
    mut sync_parent: impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut current = dir;
    while !current.exists() {
        missing.push(current);
        match current.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => current = parent,
            _ => break,
        }
    }
    for new_dir in missing.into_iter().rev() {
        match fs::DirBuilder::new().mode(0o700).create(new_dir) {
            Ok(()) => sync_parent(parent_dir(new_dir))?,
            // Another save created it first; that save syncs the parent.
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && new_dir.is_dir() => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

/// Renames `path` to `<name>.invalid-<n>` with the first free `n`, so a file CALM refused
/// to load survives the next save. A rename keeps the file's mode. Returns the new path.
pub fn move_aside(path: &Path) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy()
        .into_owned();
    for attempt in 0..1000 {
        let target = path.with_file_name(format!("{name}.invalid-{attempt}"));
        if target.symlink_metadata().is_ok() {
            continue;
        }
        fs::rename(path, &target)?;
        return Ok(target);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many invalid copies",
    ))
}

pub const SESSION_FILE_NAME: &str = "session.json";

/// `session_path_from` with the process environment.
pub fn session_path() -> PathBuf {
    session_path_from(std::env::var_os("CALM_HOME"), std::env::var_os("HOME"))
}

/// `$CALM_HOME/session.json` when `CALM_HOME` is set, else
/// `$HOME/.config/calm/session.json`. Empty variables count as unset.
pub fn session_path_from(calm_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    layouts_dir_from(calm_home, home).with_file_name(SESSION_FILE_NAME)
}

/// `<path>.<n>`, the n-th backup of `path`. `.1` is the newest.
pub fn backup_path(path: &Path, n: u32) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{n}"));
    path.with_file_name(name)
}

/// Shifts `.1..` up by one, dropping the oldest past `count`, and copies `path` to `.1`.
/// `path` stays in place, so a write that fails after rotation still leaves the session
/// to restore. Every backup ends up 0600. Does nothing when `path` is missing, so a file
/// already moved aside never enters the backups, or when `count` is 0.
pub fn rotate_backups(path: &Path, count: u32) -> io::Result<()> {
    if count == 0 || path.symlink_metadata().is_err() {
        return Ok(());
    }
    for n in (1..count).rev() {
        let from = backup_path(path, n);
        if from.symlink_metadata().is_ok() {
            let to = backup_path(path, n + 1);
            fs::rename(&from, &to)?;
            fs::set_permissions(&to, fs::Permissions::from_mode(0o600))?;
        }
    }
    // A durable write creates the copy 0600 and fsyncs the directory.
    write_durable(&backup_path(path, 1), &fs::read(path)?)
}

/// Writes the session file for one launch. Backups rotate once, before the first save,
/// never on later autosaves: rotating on every save would push out all backups within
/// seconds of a bad session.
#[derive(Debug)]
pub struct SessionWriter {
    path: PathBuf,
    backup_count: u32,
    browser_query: BrowserQueryMode,
    rotated: bool,
}

impl SessionWriter {
    pub fn new(path: PathBuf, backup_count: u32, browser_query: BrowserQueryMode) -> Self {
        SessionWriter {
            path,
            backup_count,
            browser_query,
            rotated: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Rotates on the first call of this writer, then writes `layout`.
    pub fn save(&mut self, layout: &LayoutFile) -> Result<(), LayoutError> {
        self.save_with(layout, write_layout_file)
    }

    /// `save` with the write step injectable, so tests can fail it after rotation.
    fn save_with(
        &mut self,
        layout: &LayoutFile,
        write: impl FnOnce(&Path, &LayoutFile, BrowserQueryMode) -> Result<(), LayoutError>,
    ) -> Result<(), LayoutError> {
        if !self.rotated {
            rotate_backups(&self.path, self.backup_count).map_err(|source| LayoutError::Io {
                path: self.path.clone(),
                source,
            })?;
            self.rotated = true;
        }
        write(&self.path, layout, self.browser_query)
    }
}

pub fn load_layout_from(dir: &Path, name: &str) -> Result<LayoutFile, LayoutError> {
    let path = layout_path(dir, name)?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(LayoutError::NotFound(path));
        }
        Err(source) => return Err(LayoutError::Io { path, source }),
    };
    let malformed = |message: String| LayoutError::Malformed {
        path: path.clone(),
        message,
    };
    // Check the version before the shape, so a newer file reports its version
    // instead of an unrelated field error. Shapes parse from the text, not from a
    // `Value`, so an unsupported kind keeps its exact bytes. The file is never rewritten.
    let probe: VersionProbe =
        serde_json::from_str(&text).map_err(|error| malformed(error.to_string()))?;
    let found = probe
        .version
        .as_ref()
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| malformed("missing or non-integer \"version\"".to_string()))?;
    let parse_error = |error: serde_json::Error| malformed(error.to_string());
    let layout = match found {
        1 => migrate_v1(serde_json::from_str::<LayoutFileV1>(&text).map_err(parse_error)?),
        2 => serde_json::from_str::<LayoutFile>(&text).map_err(parse_error)?,
        _ => return Err(LayoutError::UnsupportedVersion { path, found }),
    };
    validate_layout(&layout).map_err(malformed)?;
    Ok(layout)
}

/// Rejects numbers that would break camera or hit-test math. JSON cannot carry
/// NaN or infinity (serde_json refuses them), so the non-finite checks guard
/// in-memory values; the zoom range check is what file input actually hits.
fn validate_layout(layout: &LayoutFile) -> Result<(), String> {
    let zoom = layout.camera.zoom;
    if !zoom.is_finite() || !(MIN_ZOOM..=MAX_ZOOM).contains(&zoom) {
        return Err(format!(
            "camera.zoom is {zoom}, expected a value in [{MIN_ZOOM}, {MAX_ZOOM}]"
        ));
    }
    let offset = layout.camera.offset;
    if !offset.x.is_finite() || !offset.y.is_finite() {
        return Err(format!(
            "camera.offset is ({}, {}), expected finite numbers",
            offset.x, offset.y
        ));
    }
    for (index, panel) in layout.panels.iter().enumerate() {
        let rect = panel.rect;
        let numbers = [
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        ];
        if !numbers.iter().all(|number| number.is_finite()) {
            return Err(format!(
                "panels[{index}].rect is {rect:?}, expected finite numbers"
            ));
        }
    }
    Ok(())
}

fn layout_path(dir: &Path, name: &str) -> Result<PathBuf, LayoutError> {
    let path = dir.join(format!("{name}.json"));
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if valid {
        Ok(path)
    } else {
        Err(LayoutError::Malformed {
            path,
            message: format!("layout name {name:?} must match ^[A-Za-z0-9_-]+$"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;
    use crate::geometry::{Point, Rect};
    use crate::scene::{PanelKind, Scene};
    use std::path::PathBuf;

    fn sample_scene() -> Scene {
        let mut scene = Scene::default();
        scene.camera = Camera {
            offset: Point::new(-40.0, 12.5),
            zoom: 0.75,
        };
        scene.add(
            PanelKind::Terminal {
                command: Some("htop".into()),
                cwd: Some(PathBuf::from("/tmp")),
            },
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        let browser = scene.add(
            PanelKind::Browser {
                url: "https://example.com".into(),
                page_zoom: None,
            },
            Rect::new(500.0, 20.0, 640.0, 480.0),
        );
        scene.add(
            PanelKind::Plugin {
                plugin_id: "github-prs".into(),
                plugin_version: None,
                settings: serde_json::json!({"repos": ["a/b"]}),
            },
            Rect::new(-300.0, 400.0, 300.0, 200.0),
        );
        if let Some(panel) = scene.get_mut(browser) {
            panel.minimized = true;
            panel.title = "Docs".into();
        }
        scene.bring_to_front(browser);
        scene
    }

    #[test]
    fn roundtrip_preserves_panels_camera_and_order() {
        let dir = tempfile::tempdir().unwrap();
        let scene = sample_scene();
        let path = save_layout_in(
            dir.path(),
            "work",
            &scene.to_layout(),
            BrowserQueryMode::Keep,
        )
        .unwrap();
        assert_eq!(path, dir.path().join("work.json"));
        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(entries.len(), 1, "temp file must not be left behind");

        let loaded = load_layout_from(dir.path(), "work").unwrap();
        assert_eq!(loaded.version, 2);
        let restored = Scene::from_layout(loaded);
        assert_eq!(restored.camera, scene.camera);
        assert_eq!(restored.panels.len(), scene.panels.len());
        for (restored_panel, original) in restored.panels.iter().zip(&scene.panels) {
            assert_eq!(restored_panel.kind, original.kind);
            assert_eq!(restored_panel.rect, original.rect);
            assert_eq!(restored_panel.title, original.title);
            assert_eq!(restored_panel.minimized, original.minimized);
        }
    }

    #[test]
    fn terminal_panel_has_no_session_state_fields() {
        let mut scene = Scene::default();
        scene.add(
            PanelKind::Terminal {
                command: Some("zsh".into()),
                cwd: None,
            },
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        let json = serde_json::to_value(scene.to_layout()).unwrap();
        let panel = json["panels"][0].as_object().unwrap();
        let mut keys: Vec<&str> = panel.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["kind", "minimized", "rect", "title"]);
    }

    #[test]
    fn load_rejects_unknown_version_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.json");
        std::fs::write(
            &path,
            r#"{"version": 3, "camera": {"offset": {"x": 0, "y": 0}, "zoom": 1}, "panels": []}"#,
        )
        .unwrap();
        let error = load_layout_from(dir.path(), "future").unwrap_err();
        assert!(
            matches!(&error, LayoutError::UnsupportedVersion { path: p, found: 3 } if *p == path),
            "{error:?}"
        );
        assert!(error.to_string().contains(path.to_str().unwrap()));
    }

    #[test]
    fn load_reports_malformed_json_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.json");
        std::fs::write(&path, "{not json").unwrap();
        let error = load_layout_from(dir.path(), "broken").unwrap_err();
        assert!(
            matches!(&error, LayoutError::Malformed { path: p, .. } if *p == path),
            "{error:?}"
        );
        assert!(error.to_string().contains(path.to_str().unwrap()));

        // Valid JSON with the right version but the wrong shape is malformed too.
        std::fs::write(&path, r#"{"version": 1, "panels": "nope"}"#).unwrap();
        let error = load_layout_from(dir.path(), "broken").unwrap_err();
        assert!(
            matches!(&error, LayoutError::Malformed { path: p, .. } if *p == path),
            "{error:?}"
        );
    }

    #[test]
    fn load_missing_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let error = load_layout_from(dir.path(), "absent").unwrap_err();
        assert!(
            matches!(&error, LayoutError::NotFound(p) if *p == dir.path().join("absent.json")),
            "{error:?}"
        );
    }

    #[test]
    fn plugin_settings_roundtrip_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let settings = serde_json::json!({
            "repos": ["octo/one", "octo/two"],
            "refresh": 30,
            "ratio": 0.25,
            "nested": {"empty": [], "flag": false, "none": null, "text": "zażółć"}
        });
        let mut scene = Scene::default();
        scene.add(
            PanelKind::Plugin {
                plugin_id: "github-prs".into(),
                plugin_version: None,
                settings: settings.clone(),
            },
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        save_layout_in(
            dir.path(),
            "plugins",
            &scene.to_layout(),
            BrowserQueryMode::Keep,
        )
        .unwrap();
        let loaded = load_layout_from(dir.path(), "plugins").unwrap();
        match &loaded.panels[0].kind {
            PanelKind::Plugin {
                settings: restored, ..
            } => assert_eq!(restored, &settings),
            other => panic!("expected plugin panel, got {other:?}"),
        }
    }

    #[test]
    fn save_rejects_unsafe_names() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Scene::default().to_layout();
        for name in ["../escape", "a/b", "", "with space", "dot.json"] {
            let error =
                save_layout_in(dir.path(), name, &layout, BrowserQueryMode::Keep).unwrap_err();
            assert!(
                matches!(error, LayoutError::Malformed { .. }),
                "{name}: {error:?}"
            );
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(save_layout_in(dir.path(), "My_layout-2", &layout, BrowserQueryMode::Keep).is_ok());
    }

    #[test]
    fn layouts_dir_prefers_calm_home() {
        assert_eq!(
            layouts_dir_from(Some("/calm".into()), Some("/Users/me".into())),
            PathBuf::from("/calm/layouts")
        );
        assert_eq!(
            layouts_dir_from(None, Some("/Users/me".into())),
            PathBuf::from("/Users/me/.config/calm/layouts")
        );
        assert_eq!(
            layouts_dir_from(Some("".into()), Some("/Users/me".into())),
            PathBuf::from("/Users/me/.config/calm/layouts")
        );
    }

    fn layout_json(zoom: &str, offset_x: &str, rect_width: &str) -> String {
        format!(
            r#"{{"version": 1,
                "camera": {{"offset": {{"x": {offset_x}, "y": 0}}, "zoom": {zoom}}},
                "panels": [{{"kind": {{"type": "browser", "url": "https://example.com"}},
                             "rect": {{"origin": {{"x": 0, "y": 0}}, "size": {{"width": {rect_width}, "height": 300}}}},
                             "title": "Docs", "minimized": false}}]}}"#
        )
    }

    fn assert_malformed_naming(dir: &Path, name: &str, field: &str) {
        let path = dir.join(format!("{name}.json"));
        let error = load_layout_from(dir, name).unwrap_err();
        match &error {
            LayoutError::Malformed { path: p, message } => {
                assert_eq!(*p, path);
                assert!(
                    message.contains(field),
                    "message {message:?} must name {field}"
                );
            }
            other => panic!("expected Malformed naming {field}, got {other:?}"),
        }
        assert!(error.to_string().contains(path.to_str().unwrap()));
    }

    #[test]
    fn load_rejects_zero_zoom_with_path() {
        let dir = tempfile::tempdir().unwrap();
        // Positive control: the same file with a valid zoom loads.
        std::fs::write(dir.path().join("ok.json"), layout_json("1.0", "0", "400")).unwrap();
        assert!(load_layout_from(dir.path(), "ok").is_ok());

        for (name, zoom) in [
            ("zero", "0"),
            ("negative", "-1"),
            ("tiny", "0.01"),
            ("huge", "40"),
        ] {
            std::fs::write(
                dir.path().join(format!("{name}.json")),
                layout_json(zoom, "0", "400"),
            )
            .unwrap();
            assert_malformed_naming(dir.path(), name, "camera.zoom");
        }
    }

    #[test]
    fn load_rejects_non_finite_offset() {
        let dir = tempfile::tempdir().unwrap();
        // 1e999 overflows f64. serde_json refuses it, so it never becomes an offset.
        std::fs::write(
            dir.path().join("inf.json"),
            layout_json("1.0", "1e999", "400"),
        )
        .unwrap();
        assert_malformed_naming(dir.path(), "inf", "number out of range");

        // In-memory values can still be non-finite; the validator names the field.
        let mut layout = Scene::default().to_layout();
        assert_eq!(validate_layout(&layout), Ok(()));
        for offset in [Point::new(f64::INFINITY, 0.0), Point::new(0.0, f64::NAN)] {
            layout.camera.offset = offset;
            let message = validate_layout(&layout).unwrap_err();
            assert!(message.contains("camera.offset"), "{message}");
        }
        layout.camera = Camera {
            offset: Point::new(0.0, 0.0),
            zoom: f64::NAN,
        };
        assert!(
            validate_layout(&layout)
                .unwrap_err()
                .contains("camera.zoom")
        );
    }

    #[test]
    fn load_rejects_non_finite_rect() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("inf.json"),
            layout_json("1.0", "0", "1e999"),
        )
        .unwrap();
        assert_malformed_naming(dir.path(), "inf", "number out of range");

        let mut scene = Scene::default();
        scene.add(
            PanelKind::Browser {
                url: "https://example.com".into(),
                page_zoom: None,
            },
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        let mut layout = scene.to_layout();
        assert_eq!(validate_layout(&layout), Ok(()));
        layout.panels[0].rect.size.width = f64::INFINITY;
        let message = validate_layout(&layout).unwrap_err();
        assert!(message.contains("panels[0].rect"), "{message}");
    }

    #[test]
    fn from_layout_clamps_subminimum_rects() {
        let layout = LayoutFile {
            version: LAYOUT_VERSION,
            saved_at_unix: None,
            camera: Camera::default(),
            panels: vec![
                LayoutPanel {
                    kind: PanelKind::Browser {
                        url: "https://example.com".into(),
                        page_zoom: None,
                    },
                    rect: Rect::new(5.0, 6.0, 10.0, 10.0),
                    title: "Tiny".into(),
                    minimized: false,
                    overrides: PanelOverrides::default(),
                },
                LayoutPanel {
                    kind: PanelKind::Browser {
                        url: "https://example.com".into(),
                        page_zoom: None,
                    },
                    rect: Rect::new(0.0, 0.0, 640.0, 40.0),
                    title: "Short".into(),
                    minimized: true,
                    overrides: PanelOverrides::default(),
                },
            ],
        };
        let scene = Scene::from_layout(layout);
        assert_eq!(scene.panels[0].rect, Rect::new(5.0, 6.0, 120.0, 80.0));
        assert_eq!(scene.panels[1].rect, Rect::new(0.0, 0.0, 640.0, 80.0));
        assert_eq!(scene.panels[0].title, "Tiny");
        assert!(scene.panels[1].minimized);
    }

    #[test]
    fn concurrent_saves_do_not_share_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let big = {
            let mut scene = Scene::default();
            for index in 0..200 {
                scene.add(
                    PanelKind::Browser {
                        url: format!("https://example.com/{index}"),
                        page_zoom: None,
                    },
                    Rect::new(index as f64, 0.0, 400.0, 300.0),
                );
            }
            scene.to_layout()
        };
        let small = Scene::default().to_layout();
        let barrier = std::sync::Barrier::new(2);

        std::thread::scope(|scope| {
            for layout in [&big, &small] {
                let barrier = &barrier;
                let dir = dir.path();
                scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..200 {
                        let saved = save_layout_in(dir, "shared", layout, BrowserQueryMode::Keep);
                        assert!(saved.is_ok(), "save failed: {saved:?}");
                    }
                });
            }
        });

        let loaded = load_layout_from(dir.path(), "shared").unwrap();
        // The stamp differs per save, so compare what the writers chose.
        assert!(
            loaded.panels == big.panels || loaded.panels == small.panels,
            "final file mixes writers"
        );
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            vec![std::ffi::OsString::from("shared.json")],
            "temp files left behind"
        );
    }

    // Durable writes, session file and backups.

    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn dir_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn browser_layout(url: &str) -> LayoutFile {
        let mut scene = Scene::default();
        scene.add(
            PanelKind::Browser {
                url: url.to_string(),
                page_zoom: None,
            },
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        scene.to_layout()
    }

    fn saved_url(path: &Path) -> String {
        let text = std::fs::read_to_string(path).unwrap();
        let layout: LayoutFile = serde_json::from_str(&text).unwrap();
        match &layout.panels[0].kind {
            PanelKind::Browser { url, .. } => url.clone(),
            other => panic!("expected a browser, got {other:?}"),
        }
    }

    #[test]
    fn durable_write_is_private_and_tightens_a_wide_file() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a/b/session.json");
        write_durable(&path, b"one").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        assert_eq!(mode_of(&path), 0o600);
        assert_eq!(mode_of(&root.path().join("a")), 0o700);
        assert_eq!(mode_of(&root.path().join("a/b")), 0o700);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_durable(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(mode_of(&path), 0o600);
        assert_eq!(dir_names(&root.path().join("a/b")), ["session.json"]);
    }

    #[test]
    fn durable_write_failure_removes_the_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        // A non-empty directory where the file should go makes the real rename fail.
        let target = dir.path().join("session.json");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), b"x").unwrap();
        assert!(write_durable(&target, b"new").is_err());
        assert_eq!(dir_names(dir.path()), ["session.json"]);
        assert_eq!(dir_names(&target), ["keep"]);
    }

    #[test]
    fn rename_failure_keeps_the_prior_valid_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("work.json");
        write_layout_file(
            &path,
            &browser_layout("https://example.com/old"),
            BrowserQueryMode::Keep,
        )
        .unwrap();
        let before = std::fs::read(&path).unwrap();

        let error = write_durable_with(&path, b"{ broken", |_, _| {
            Err(io::Error::other("injected rename failure"))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "injected rename failure");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(dir_names(dir.path()), ["work.json"]);
        assert!(load_layout_from(dir.path(), "work").is_ok());
    }

    #[test]
    fn session_path_follows_calm_home() {
        assert_eq!(
            session_path_from(Some("/tmp/calm".into()), Some("/home/u".into())),
            PathBuf::from("/tmp/calm/session.json")
        );
        assert_eq!(
            session_path_from(Some("".into()), Some("/home/u".into())),
            PathBuf::from("/home/u/.config/calm/session.json")
        );
        assert_eq!(
            backup_path(Path::new("/x/session.json"), 2),
            PathBuf::from("/x/session.json.2")
        );
    }

    /// One simulated launch: a fresh writer that autosaves three times.
    fn launch(path: &Path, count: u32, number: u32) {
        let mut writer = SessionWriter::new(path.to_path_buf(), count, BrowserQueryMode::Keep);
        for save in ["a", "b", "c"] {
            let url = format!("https://example.com/launch-{number}-{save}");
            writer.save(&browser_layout(&url)).unwrap();
        }
    }

    #[test]
    fn backups_rotate_once_per_launch() {
        let url = |number: u32| format!("https://example.com/launch-{number}-c");
        for (count, expected_backups) in [(0, vec![]), (1, vec![4]), (3, vec![4, 3, 2])] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(SESSION_FILE_NAME);
            for number in 1..=5 {
                launch(&path, count, number);
            }
            assert_eq!(saved_url(&path), url(5), "count {count}");
            let mut expected_names = vec![SESSION_FILE_NAME.to_string()];
            for (index, number) in expected_backups.iter().enumerate() {
                let backup = backup_path(&path, index as u32 + 1);
                assert_eq!(saved_url(&backup), url(*number), "count {count}");
                assert_eq!(mode_of(&backup), 0o600);
                expected_names.push(format!("{SESSION_FILE_NAME}.{}", index + 1));
            }
            assert_eq!(dir_names(dir.path()), expected_names, "count {count}");
        }
    }

    #[test]
    fn a_quarantined_failed_restore_never_enters_the_backups() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SESSION_FILE_NAME);
        launch(&path, 3, 1);
        launch(&path, 3, 2);
        std::fs::write(&path, b"{ not a layout").unwrap();

        // The next launch fails to restore and moves the file aside before any save.
        let failed = load_layout_from(dir.path(), "session").unwrap_err();
        assert!(matches!(failed, LayoutError::Malformed { .. }), "{failed}");
        let aside = move_aside(&path).unwrap();
        assert_eq!(aside, dir.path().join("session.json.invalid-0"));
        launch(&path, 3, 3);

        assert_eq!(
            dir_names(dir.path()),
            ["session.json", "session.json.1", "session.json.invalid-0"]
        );
        assert_eq!(std::fs::read(&aside).unwrap(), b"{ not a layout");
        assert_eq!(
            saved_url(&backup_path(&path, 1)),
            "https://example.com/launch-1-c"
        );
    }

    #[test]
    fn move_aside_picks_the_first_free_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"1").unwrap();
        assert_eq!(
            move_aside(&path).unwrap(),
            dir.path().join("settings.json.invalid-0")
        );
        std::fs::write(&path, b"2").unwrap();
        assert_eq!(
            move_aside(&path).unwrap(),
            dir.path().join("settings.json.invalid-1")
        );
        assert!(!path.exists());
    }

    #[test]
    fn a_normal_save_stamps_saved_at() {
        let dir = tempfile::tempdir().unwrap();
        let mut scene = Scene::default();
        scene.add(
            PanelKind::Terminal {
                command: None,
                cwd: None,
            },
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        let layout = scene.to_layout();
        assert_eq!(layout.saved_at_unix, None);
        let before = now_unix();
        save_layout_in(dir.path(), "named", &layout, BrowserQueryMode::StripAll).unwrap();
        let mut writer = SessionWriter::new(
            dir.path().join(SESSION_FILE_NAME),
            3,
            BrowserQueryMode::StripAll,
        );
        writer.save(&layout).unwrap();
        let after = now_unix();
        for name in ["named", "session"] {
            let stamp = load_layout_from(dir.path(), name).unwrap().saved_at_unix;
            assert!(
                stamp.is_some_and(|stamp| (before..=after).contains(&stamp)),
                "{name}: {stamp:?} not in {before}..={after}"
            );
        }
    }

    #[test]
    fn the_write_path_scrubs_browser_urls_per_mode() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://example.com/a?code=fixture-code#state=fixture-state";
        let layout = browser_layout(url);
        let strip =
            save_layout_in(dir.path(), "strip", &layout, BrowserQueryMode::StripAll).unwrap();
        let keep = save_layout_in(dir.path(), "keep", &layout, BrowserQueryMode::Keep).unwrap();
        let mut session = SessionWriter::new(
            dir.path().join(SESSION_FILE_NAME),
            0,
            BrowserQueryMode::StripAll,
        );
        session.save(&layout).unwrap();

        for path in [&strip, session.path()] {
            let bytes = std::fs::read_to_string(path).unwrap();
            assert!(
                bytes.contains("\"https://example.com/a\""),
                "{}",
                path.display()
            );
            assert!(!bytes.contains("fixture-code"), "{}", path.display());
            assert!(!bytes.contains("fixture-state"), "{}", path.display());
        }
        let kept = std::fs::read_to_string(&keep).unwrap();
        assert!(kept.contains(&format!("\"{url}\"")));
        // The caller's layout is not scrubbed in memory.
        assert_eq!(layout, browser_layout(url));
    }

    // Review fixes on the session write path.

    #[test]
    fn rotation_never_leaves_the_primary_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SESSION_FILE_NAME);
        launch(&path, 3, 1);
        let before = std::fs::read(&path).unwrap();
        // A crash or failed write right after rotation must still find the session.
        rotate_backups(&path, 3).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(load_layout_from(dir.path(), "session").is_ok());
        assert_eq!(std::fs::read(backup_path(&path, 1)).unwrap(), before);
    }

    #[test]
    fn a_write_failing_after_rotation_keeps_the_primary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SESSION_FILE_NAME);
        launch(&path, 3, 1);
        let before = std::fs::read(&path).unwrap();

        let mut writer = SessionWriter::new(path.clone(), 3, BrowserQueryMode::Keep);
        let error = writer
            .save_with(&browser_layout("https://example.com/new"), |path, _, _| {
                Err(LayoutError::Io {
                    path: path.to_path_buf(),
                    source: io::Error::from_raw_os_error(24), // EMFILE
                })
            })
            .unwrap_err();
        assert!(matches!(error, LayoutError::Io { .. }), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(saved_url(&path), "https://example.com/launch-1-c");
        assert_eq!(std::fs::read(backup_path(&path, 1)).unwrap(), before);
    }

    #[test]
    fn missing_dirs_are_created_top_down_and_their_parents_synced() {
        let root = tempfile::tempdir().unwrap();
        let leaf = root.path().join("a/b/c");
        let mut synced = Vec::new();
        create_private_dir_with(&leaf, |dir| {
            synced.push(dir.to_path_buf());
            Ok(())
        })
        .unwrap();
        let a = root.path().join("a");
        let b = a.join("b");
        assert_eq!(synced, [root.path().to_path_buf(), a.clone(), b.clone()]);
        for dir in [&a, &b, &leaf] {
            assert_eq!(mode_of(dir), 0o700, "{}", dir.display());
        }

        // Only the missing part of a path is created and synced.
        synced.clear();
        create_private_dir_with(&b.join("x/y"), |dir| {
            synced.push(dir.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(synced, [b.clone(), b.join("x")]);
        synced.clear();
        create_private_dir_with(&leaf, |dir| {
            synced.push(dir.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert!(synced.is_empty(), "{synced:?}");
    }

    #[test]
    fn rotated_backups_end_up_private() {
        use std::os::unix::fs::PermissionsExt;
        let wide = || std::fs::Permissions::from_mode(0o644);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SESSION_FILE_NAME);
        std::fs::write(&path, b"primary").unwrap();
        std::fs::set_permissions(&path, wide()).unwrap();
        std::fs::write(backup_path(&path, 1), b"older").unwrap();
        std::fs::set_permissions(backup_path(&path, 1), wide()).unwrap();

        rotate_backups(&path, 3).unwrap();
        assert_eq!(std::fs::read(backup_path(&path, 1)).unwrap(), b"primary");
        assert_eq!(std::fs::read(backup_path(&path, 2)).unwrap(), b"older");
        assert_eq!(mode_of(&backup_path(&path, 1)), 0o600);
        assert_eq!(mode_of(&backup_path(&path, 2)), 0o600);
    }

    fn saved_browser(path: &Path) -> (String, String) {
        let text = std::fs::read_to_string(path).unwrap();
        let layout: LayoutFile = serde_json::from_str(&text).unwrap();
        match &layout.panels[0].kind {
            PanelKind::Browser { url, .. } => (url.clone(), layout.panels[0].title.clone()),
            other => panic!("expected a browser, got {other:?}"),
        }
    }

    fn save_titled(dir: &Path, url: &str, title: &str, mode: BrowserQueryMode) -> (String, String) {
        let mut layout = browser_layout(url);
        layout.panels[0].title = title.to_string();
        let path = save_layout_in(dir, "titled", &layout, mode).unwrap();
        saved_browser(&path)
    }

    #[test]
    fn strip_all_scrubs_every_url_inside_a_browser_title() {
        use BrowserQueryMode::{Keep, StripAll};
        let dir = tempfile::tempdir().unwrap();
        let url = "https://example.test/a";
        let cases = [
            (
                "Viewing https://example.test/a?code=X done",
                "Viewing https://example.test/a done",
            ),
            (
                "From http://a.test/x?code=one to HTTPS://b.test/y#state=two (and file:///tmp/f?q=three)",
                "From http://a.test/x to HTTPS://b.test/y (and file:///tmp/f",
            ),
            (
                "Paste data:text/plain,hi#frag-four or about:blank?five",
                "Paste data:text/plain,hi or about:blank",
            ),
            ("Plain title, no address", "Plain title, no address"),
            ("https://u:pw@example.test/a?x", "https://example.test/a"),
        ];
        for (title, stripped) in cases {
            assert_eq!(
                save_titled(dir.path(), url, title, StripAll).1,
                stripped,
                "{title}"
            );
            assert_eq!(
                save_titled(dir.path(), url, title, Keep).1,
                title,
                "{title}"
            );
        }
    }

    #[test]
    fn urls_over_8_kib_save_as_about_blank_in_both_modes() {
        use BrowserQueryMode::{Keep, StripAll};
        let dir = tempfile::tempdir().unwrap();
        let sized = |len: usize| {
            let prefix = "data:text/plain,";
            format!("{prefix}{}", "a".repeat(len - prefix.len()))
        };
        for mode in [StripAll, Keep] {
            let at_limit = sized(8192);
            assert_eq!(
                save_titled(dir.path(), &at_limit, "Big page", mode),
                (at_limit.clone(), "Big page".to_string()),
                "{mode:?}"
            );
            assert_eq!(
                save_titled(dir.path(), &sized(8193), "Big page", mode),
                ("about:blank".to_string(), "Big page".to_string()),
                "{mode:?}"
            );
        }
        // The guard runs after scrubbing: a long query alone does not trigger it.
        let long_query = format!("https://example.test/a?q={}", "x".repeat(9000));
        assert_eq!(
            save_titled(dir.path(), &long_query, "Search", StripAll).0,
            "https://example.test/a"
        );
    }

    // Layout v2.

    fn fixture_v1_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/layout-v1.json")
    }

    fn assert_panels_match(restored: &Scene, original: &Scene) {
        assert_eq!(restored.panels.len(), original.panels.len());
        for (restored_panel, original) in restored.panels.iter().zip(&original.panels) {
            assert_eq!(restored_panel.kind, original.kind);
            assert_eq!(restored_panel.rect, original.rect);
            assert_eq!(restored_panel.title, original.title);
            assert_eq!(restored_panel.minimized, original.minimized);
            assert_eq!(restored_panel.overrides, original.overrides);
        }
    }

    #[test]
    fn v2_roundtrip_keeps_every_kind_order_and_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let mut scene = Scene::default();
        scene.camera = Camera {
            offset: Point::new(3.5, -7.0),
            zoom: 1.5,
        };
        // Every command and cwd combination.
        for (index, (command, cwd)) in [
            (None, None),
            (Some("htop"), None),
            (None, Some("/srv/work dir")),
            (Some("make -j4"), Some("/tmp")),
        ]
        .into_iter()
        .enumerate()
        {
            scene.add(
                PanelKind::Terminal {
                    command: command.map(String::from),
                    cwd: cwd.map(PathBuf::from),
                },
                Rect::new(index as f64 * 500.0, 0.0, 400.0, 300.0),
            );
        }
        let zoomed = scene.add(
            PanelKind::Browser {
                url: "https://docs.rs/gpui".into(),
                page_zoom: Some(1.25),
            },
            Rect::new(0.0, 400.0, 800.0, 600.0),
        );
        scene.add(
            PanelKind::Browser {
                url: "about:blank".into(),
                page_zoom: None,
            },
            Rect::new(900.0, 400.0, 300.0, 200.0),
        );
        let plugin = scene.add(
            PanelKind::Plugin {
                plugin_id: "github-prs".into(),
                plugin_version: Some("0.1.0".into()),
                settings: serde_json::json!({
                    "repos": ["kolezka/CALM"],
                    "deep": {"list": [1, {"x": null}], "flag": true, "ratio": 0.5}
                }),
            },
            Rect::new(-500.0, 0.0, 420.0, 300.0),
        );
        let tui = scene.add(
            PanelKind::Tui {
                plugin_id: "top-example".into(),
                plugin_version: Some("1.2.3".into()),
            },
            Rect::new(-500.0, 400.0, 480.0, 320.0),
        );
        if let Some(panel) = scene.get_mut(tui) {
            panel.overrides = PanelOverrides {
                title: Some("Top".into()),
                icon: Some("terminal".into()),
                accent_color: Some("#ff8800".into()),
            };
            panel.title = "top".into();
        }
        scene.get_mut(plugin).unwrap().minimized = true;
        scene.send_to_back(zoomed);
        scene.bring_to_front(plugin);

        let mut layout = scene.to_layout();
        // The writer stamps the save time over whatever the layout carried.
        layout.saved_at_unix = Some(1);
        save_layout_in(dir.path(), "all", &layout, BrowserQueryMode::Keep).unwrap();
        let loaded = load_layout_from(dir.path(), "all").unwrap();
        assert_eq!(loaded.version, LAYOUT_VERSION);
        assert!(loaded.saved_at_unix.is_some_and(|stamp| stamp > 1));
        layout.saved_at_unix = loaded.saved_at_unix;
        assert_eq!(loaded, layout);
        let restored = Scene::from_layout(loaded);
        assert_eq!(restored.camera, scene.camera);
        assert_panels_match(&restored, &scene);
    }

    #[test]
    fn from_layout_assigns_fresh_ids_and_derives_resize() {
        let mut scene = Scene::default();
        for _ in 0..3 {
            let id = scene.add(
                PanelKind::Terminal {
                    command: None,
                    cwd: None,
                },
                Rect::new(0.0, 0.0, 400.0, 300.0),
            );
            scene.remove(id);
        }
        let plugin_kind = PanelKind::Plugin {
            plugin_id: "fixed".into(),
            plugin_version: None,
            settings: serde_json::json!({}),
        };
        let locked = scene.add(plugin_kind.clone(), Rect::new(0.0, 0.0, 400.0, 300.0));
        scene.set_resize_policy(locked, crate::resize::ResizePolicy::LOCKED);
        assert_eq!(locked.0, 4);

        let json = serde_json::to_string(&scene.to_layout()).unwrap();
        assert!(!json.contains("resize"), "{json}");
        let restored = Scene::from_layout(serde_json::from_str(&json).unwrap());
        assert_eq!(restored.panels[0].id.0, 1);
        // Without a manifest the policy comes from the kind alone.
        assert_eq!(
            restored.panels[0].resize,
            crate::resize::ResizePolicy::derive(&plugin_kind, None)
        );
    }

    #[test]
    fn tui_panel_keeps_current_format() {
        let dir = tempfile::tempdir().unwrap();
        let kind_json = r#"{"type": "tui", "plugin_id": "top-example"}"#;
        for version in [1, 2] {
            std::fs::write(
                dir.path().join(format!("tui{version}.json")),
                format!(
                    r#"{{"version": {version},
                        "camera": {{"offset": {{"x": 0, "y": 0}}, "zoom": 1}},
                        "panels": [{{"kind": {kind_json},
                                     "rect": {{"origin": {{"x": 0, "y": 0}}, "size": {{"width": 400, "height": 300}}}},
                                     "title": "top", "minimized": false}}]}}"#
                ),
            )
            .unwrap();
            let loaded = load_layout_from(dir.path(), &format!("tui{version}")).unwrap();
            let expected = PanelKind::Tui {
                plugin_id: "top-example".into(),
                plugin_version: None,
            };
            assert_eq!(loaded.panels[0].kind, expected, "version {version}");
            let saved = serde_json::to_value(&loaded.panels[0].kind).unwrap();
            assert_eq!(
                saved,
                serde_json::json!({"type": "tui", "plugin_id": "top-example", "plugin_version": null})
            );
            save_layout_in(dir.path(), "again", &loaded, BrowserQueryMode::Keep).unwrap();
            let mut again = load_layout_from(dir.path(), "again").unwrap();
            assert!(again.saved_at_unix.is_some());
            again.saved_at_unix = loaded.saved_at_unix;
            assert_eq!(again, loaded);
        }
    }

    #[test]
    fn v1_fixture_migrates_with_defaults_and_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.json");
        std::fs::copy(fixture_v1_path(), &path).unwrap();
        let before = std::fs::read(&path).unwrap();
        let modified_before = std::fs::metadata(&path).unwrap().modified().unwrap();

        let loaded = load_layout_from(dir.path(), "old").unwrap();
        assert_eq!(loaded.version, LAYOUT_VERSION);
        assert_eq!(loaded.saved_at_unix, None);
        assert_eq!(
            loaded.camera,
            Camera {
                offset: Point::new(-40.0, 12.5),
                zoom: 0.75,
            }
        );
        let kinds: Vec<&PanelKind> = loaded.panels.iter().map(|panel| &panel.kind).collect();
        assert_eq!(
            kinds,
            vec![
                &PanelKind::Terminal {
                    command: Some("htop".into()),
                    cwd: Some(PathBuf::from("/tmp")),
                },
                &PanelKind::Plugin {
                    plugin_id: "github-prs".into(),
                    plugin_version: None,
                    settings: serde_json::json!({"repos": ["a/b"]}),
                },
                &PanelKind::Tui {
                    plugin_id: "top-example".into(),
                    plugin_version: None,
                },
                &PanelKind::Browser {
                    url: "https://example.com".into(),
                    page_zoom: None,
                },
            ]
        );
        assert!(
            loaded
                .panels
                .iter()
                .all(|panel| panel.overrides == PanelOverrides::default())
        );
        assert_eq!(loaded.panels[3].title, "Docs");
        assert!(loaded.panels[3].minimized);
        assert_eq!(
            loaded.panels[1].rect,
            Rect::new(-300.0, 400.0, 300.0, 200.0)
        );

        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "v1 file was rewritten"
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified_before
        );
        assert_eq!(std::fs::read(fixture_v1_path()).unwrap(), before);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// Key order, spacing, number spelling and escapes that a trip through
    /// `serde_json::Value` would all change.
    const UNKNOWN_KIND: &str =
        r#"{ "type":"hologram",  "zeta": 1.50, "alpha":"café", "list":[ 1,2 ,{"n":null}] }"#;

    fn v2_json_with_kinds(kinds: &[&str]) -> String {
        let panels: Vec<String> = kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                format!(
                    r#"{{"kind": {kind},
                         "rect": {{"origin": {{"x": {index}, "y": 0}}, "size": {{"width": 400, "height": 300}}}},
                         "title": "panel {index}", "minimized": false}}"#
                )
            })
            .collect();
        format!(
            r#"{{"version": 2, "saved_at_unix": 1,
                "camera": {{"offset": {{"x": 0, "y": 0}}, "zoom": 1}},
                "panels": [{}]}}"#,
            panels.join(",")
        )
    }

    #[test]
    fn unknown_kind_keeps_exact_bytes_through_load_and_save() {
        let dir = tempfile::tempdir().unwrap();
        let browser = r#"{"type": "browser", "url": "https://example.com"}"#;
        std::fs::write(
            dir.path().join("mixed.json"),
            v2_json_with_kinds(&[browser, UNKNOWN_KIND]),
        )
        .unwrap();
        let loaded = load_layout_from(dir.path(), "mixed").unwrap();
        // Positive control: the known panel next to it loads normally.
        assert!(
            matches!(&loaded.panels[0].kind, PanelKind::Browser { url, .. } if url == "https://example.com")
        );
        let PanelKind::Unsupported { raw } = &loaded.panels[1].kind else {
            panic!("expected Unsupported, got {:?}", loaded.panels[1].kind);
        };
        assert_eq!(raw.json(), UNKNOWN_KIND);
        assert_eq!(raw.type_name(), "hologram");
        assert_eq!(loaded.panels[1].rect, Rect::new(1.0, 0.0, 400.0, 300.0));

        let restored = Scene::from_layout(loaded.clone());
        assert_eq!(restored.panels[1].title, "panel 1");
        save_layout_in(
            dir.path(),
            "saved",
            &restored.to_layout(),
            BrowserQueryMode::Keep,
        )
        .unwrap();
        let saved_text = std::fs::read_to_string(dir.path().join("saved.json")).unwrap();
        assert!(
            saved_text.contains(UNKNOWN_KIND),
            "raw kind bytes changed on save:\n{saved_text}"
        );
        let reloaded = load_layout_from(dir.path(), "saved").unwrap();
        assert_eq!(reloaded.panels[1].kind, loaded.panels[1].kind);
        let PanelKind::Unsupported { raw } = &reloaded.panels[1].kind else {
            panic!("expected Unsupported after reload");
        };
        assert_eq!(raw.json().as_bytes(), UNKNOWN_KIND.as_bytes());
    }

    #[test]
    fn malformed_known_kind_is_an_error_not_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let good = r#"{"type": "browser", "url": "https://example.com"}"#;
        std::fs::write(dir.path().join("good.json"), v2_json_with_kinds(&[good])).unwrap();
        assert!(load_layout_from(dir.path(), "good").is_ok());

        for (name, kind, field) in [
            ("no_url", r#"{"type": "browser"}"#, "url"),
            (
                "bad_command",
                r#"{"type": "terminal", "command": 5, "cwd": null}"#,
                "invalid type",
            ),
            (
                "bad_zoom",
                r#"{"type": "browser", "url": "x", "page_zoom": "big"}"#,
                "invalid type",
            ),
            ("no_plugin_id", r#"{"type": "tui"}"#, "plugin_id"),
        ] {
            std::fs::write(
                dir.path().join(format!("{name}.json")),
                v2_json_with_kinds(&[kind]),
            )
            .unwrap();
            assert_malformed_naming(dir.path(), name, field);
        }
    }

    #[test]
    fn malformed_v2_files_fail_with_specific_errors() {
        let dir = tempfile::tempdir().unwrap();
        let good_kind = r#"{"type": "terminal", "command": null, "cwd": null}"#;
        let good = v2_json_with_kinds(&[good_kind]);
        std::fs::write(dir.path().join("good.json"), &good).unwrap();
        // Positive control from the same text every case below corrupts.
        let loaded = load_layout_from(dir.path(), "good").unwrap();
        assert_eq!(loaded.panels.len(), 1);

        let cases: [(&str, String, &str); 7] = [
            (
                "bad_json",
                good.replacen('{', "{{", 1),
                "key must be a string",
            ),
            (
                "no_version",
                good.replacen(r#""version": 2,"#, "", 1),
                "version",
            ),
            (
                "text_version",
                good.replacen(r#""version": 2"#, r#""version": "2""#, 1),
                "version",
            ),
            (
                "zoom",
                good.replacen(r#""zoom": 1"#, r#""zoom": 99"#, 1),
                "camera.zoom",
            ),
            (
                "panel_not_object",
                v2_json_with_kinds(&[]).replacen("[]", "[5]", 1),
                "invalid type",
            ),
            (
                "kind_no_type",
                v2_json_with_kinds(&[r#"{"url": "x"}"#]),
                "type",
            ),
            (
                "kind_type_number",
                v2_json_with_kinds(&[r#"{"type": 7}"#]),
                "invalid type",
            ),
        ];
        for (name, text, field) in &cases {
            assert_ne!(text, &good, "{name} did not change the fixture");
            std::fs::write(dir.path().join(format!("{name}.json")), text).unwrap();
            assert_malformed_naming(dir.path(), name, field);
        }

        for (name, found) in [("v3", 3u64), ("v0", 0)] {
            let text = good.replacen(r#""version": 2"#, &format!(r#""version": {found}"#), 1);
            std::fs::write(dir.path().join(format!("{name}.json")), text).unwrap();
            let error = load_layout_from(dir.path(), name).unwrap_err();
            assert!(
                matches!(error, LayoutError::UnsupportedVersion { found: f, .. } if f == found),
                "{name}: {error:?}"
            );
        }
    }

    #[test]
    fn overrides_key_is_written_only_when_set() {
        let mut scene = Scene::default();
        let plain = scene.add(
            PanelKind::Terminal {
                command: None,
                cwd: None,
            },
            Rect::new(0.0, 0.0, 400.0, 300.0),
        );
        let styled = scene.duplicate(plain, |_| None).unwrap();
        scene.get_mut(styled).unwrap().overrides.title = Some("Logs".into());
        let text = serde_json::to_string(&scene.to_layout()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(json["panels"][0].get("overrides").is_none(), "{json}");
        assert_eq!(
            json["panels"][1]["overrides"],
            serde_json::json!({"title": "Logs"})
        );
        let restored = Scene::from_layout(serde_json::from_str(&text).unwrap());
        assert_panels_match(&restored, &scene);
    }

    #[test]
    fn unknown_override_key_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let text = v2_json_with_kinds(&[r#"{"type": "terminal", "command": null, "cwd": null}"#]);
        // Positive control: the same file without overrides.
        std::fs::write(dir.path().join("plain.json"), &text).unwrap();
        assert!(load_layout_from(dir.path(), "plain").is_ok());

        let with_overrides = text.replacen(
            r#""minimized": false"#,
            r##""minimized": false, "overrides": {"accent_color": "#11223344", "title_bar_color": "#000000"}"##,
            1,
        );
        assert_ne!(with_overrides, text);
        std::fs::write(dir.path().join("newer.json"), &with_overrides).unwrap();
        let loaded = load_layout_from(dir.path(), "newer").unwrap();
        assert_eq!(
            loaded.panels[0].overrides,
            PanelOverrides {
                accent_color: Some("#11223344".into()),
                ..PanelOverrides::default()
            }
        );
    }

    #[test]
    fn missing_v2_file_is_not_found_not_malformed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(fixture_v1_path(), dir.path().join("present.json")).unwrap();
        assert!(load_layout_from(dir.path(), "present").is_ok());
        let error = load_layout_from(dir.path(), "session").unwrap_err();
        assert!(matches!(error, LayoutError::NotFound(_)), "{error:?}");
    }
}
