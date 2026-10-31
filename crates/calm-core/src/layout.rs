use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

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
    /// A missing key means no overrides.
    #[serde(default)]
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

pub fn save_layout(name: &str, layout: &LayoutFile) -> Result<PathBuf, LayoutError> {
    save_layout_in(&layouts_dir(), name, layout)
}

pub fn load_layout(name: &str) -> Result<LayoutFile, LayoutError> {
    load_layout_from(&layouts_dir(), name)
}

/// Writes `<dir>/<name>.json` through a temp file and a rename, so a crash never
/// leaves a half-written layout behind.
pub fn save_layout_in(dir: &Path, name: &str, layout: &LayoutFile) -> Result<PathBuf, LayoutError> {
    let path = layout_path(dir, name)?;
    let io_error = |source| LayoutError::Io {
        path: path.clone(),
        source,
    };
    let json = serde_json::to_string_pretty(layout).map_err(|error| LayoutError::Malformed {
        path: path.clone(),
        message: error.to_string(),
    })?;
    fs::create_dir_all(dir).map_err(io_error)?;
    let temp_path = dir.join(format!(
        ".{name}.json.tmp-{}-{}",
        std::process::id(),
        SAVE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(source) = fs::write(&temp_path, json).and_then(|()| fs::rename(&temp_path, &path)) {
        // Best effort cleanup; the write or rename error is the one worth reporting.
        let _ = fs::remove_file(&temp_path);
        return Err(io_error(source));
    }
    Ok(path)
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
        let path = save_layout_in(dir.path(), "work", &scene.to_layout()).unwrap();
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
        assert_eq!(
            keys,
            vec!["kind", "minimized", "overrides", "rect", "title"]
        );
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
        save_layout_in(dir.path(), "plugins", &scene.to_layout()).unwrap();
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
            let error = save_layout_in(dir.path(), name, &layout).unwrap_err();
            assert!(
                matches!(error, LayoutError::Malformed { .. }),
                "{name}: {error:?}"
            );
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(save_layout_in(dir.path(), "My_layout-2", &layout).is_ok());
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
                        let saved = save_layout_in(dir, "shared", layout);
                        assert!(saved.is_ok(), "save failed: {saved:?}");
                    }
                });
            }
        });

        let loaded = load_layout_from(dir.path(), "shared").unwrap();
        assert!(loaded == big || loaded == small, "final file mixes writers");
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
                accent: Some("#ff8800".into()),
            };
            panel.title = "top".into();
        }
        scene.get_mut(plugin).unwrap().minimized = true;
        scene.send_to_back(zoomed);
        scene.bring_to_front(plugin);

        let mut layout = scene.to_layout();
        layout.saved_at_unix = Some(1_793_450_000);
        save_layout_in(dir.path(), "all", &layout).unwrap();
        let loaded = load_layout_from(dir.path(), "all").unwrap();
        assert_eq!(loaded.version, LAYOUT_VERSION);
        assert_eq!(loaded.saved_at_unix, Some(1_793_450_000));
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
            save_layout_in(dir.path(), "again", &loaded).unwrap();
            assert_eq!(load_layout_from(dir.path(), "again").unwrap(), loaded);
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
        save_layout_in(dir.path(), "saved", &restored.to_layout()).unwrap();
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
    fn missing_v2_file_is_not_found_not_malformed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(fixture_v1_path(), dir.path().join("present.json")).unwrap();
        assert!(load_layout_from(dir.path(), "present").is_ok());
        let error = load_layout_from(dir.path(), "session").unwrap_err();
        assert!(matches!(error, LayoutError::NotFound(_)), "{error:?}");
    }
}
