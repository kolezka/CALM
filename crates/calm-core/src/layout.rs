use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::camera::{Camera, MAX_ZOOM, MIN_ZOOM};
use crate::geometry::Rect;
use crate::scene::{PanelKind, Scene};

pub const LAYOUT_VERSION: u32 = 1;

/// Makes temp file names unique per save call, not just per process.
static SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutFile {
    pub version: u32,
    pub camera: Camera,
    /// Back to front, same order as `Scene::panels`.
    pub panels: Vec<LayoutPanel>,
}

/// Configuration only. Session state (scrollback, history, cookies) never goes here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutPanel {
    pub kind: PanelKind,
    pub rect: Rect,
    pub title: String,
    pub minimized: bool,
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
                "layout {} has version {found}, only version {LAYOUT_VERSION} is supported",
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
            camera: self.camera,
            panels: self
                .panels
                .iter()
                .map(|panel| LayoutPanel {
                    kind: panel.kind.clone(),
                    rect: panel.rect,
                    title: panel.title.clone(),
                    minimized: panel.minimized,
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
    // instead of an unrelated field error.
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| malformed(error.to_string()))?;
    let found = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| malformed("missing or non-integer \"version\"".to_string()))?;
    if found != u64::from(LAYOUT_VERSION) {
        return Err(LayoutError::UnsupportedVersion { path, found });
    }
    let layout: LayoutFile =
        serde_json::from_value(value).map_err(|error| malformed(error.to_string()))?;
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
            },
            Rect::new(500.0, 20.0, 640.0, 480.0),
        );
        scene.add(
            PanelKind::Plugin {
                plugin_id: "github-prs".into(),
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
        assert_eq!(loaded.version, 1);
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
            r#"{"version": 2, "camera": {"offset": {"x": 0, "y": 0}, "zoom": 1}, "panels": []}"#,
        )
        .unwrap();
        let error = load_layout_from(dir.path(), "future").unwrap_err();
        assert!(
            matches!(&error, LayoutError::UnsupportedVersion { path: p, found: 2 } if *p == path),
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
            camera: Camera::default(),
            panels: vec![
                LayoutPanel {
                    kind: PanelKind::Browser {
                        url: "https://example.com".into(),
                    },
                    rect: Rect::new(5.0, 6.0, 10.0, 10.0),
                    title: "Tiny".into(),
                    minimized: false,
                },
                LayoutPanel {
                    kind: PanelKind::Browser {
                        url: "https://example.com".into(),
                    },
                    rect: Rect::new(0.0, 0.0, 640.0, 40.0),
                    title: "Short".into(),
                    minimized: true,
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
}
