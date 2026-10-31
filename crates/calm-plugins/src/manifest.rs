use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use calm_core::resize::PluginResize;
use calm_core::scene::MIN_PANEL_SIZE;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::tui::{self, TuiSpec};

pub const MANIFEST_FILE: &str = "calm-plugin.json";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    fn core(self) -> calm_core::geometry::Size {
        calm_core::geometry::Size::new(self.width, self.height)
    }
}

/// What a plugin panel shows: an HTML page, or a terminal running one command.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginContent {
    Web { entry: PathBuf },
    Tui(TuiSpec),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub default_size: Size,
    pub content: PluginContent,
    /// `false` locks the panel at the size it was created with. Absent means true.
    pub resizable: Option<bool>,
    /// Smallest size a user resize may reach, chrome included.
    pub min_size: Option<Size>,
    /// Largest size a user resize may reach, chrome included.
    pub max_size: Option<Size>,
}

/// The `"kind": "web"` manifest as it was before `kind` existed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebManifestJson {
    id: String,
    name: String,
    version: String,
    entry: PathBuf,
    default_size: Size,
    #[serde(default, deserialize_with = "present")]
    resizable: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    min_size: Option<Size>,
    #[serde(default, deserialize_with = "present")]
    max_size: Option<Size>,
}

/// An optional field that, when written, must hold a value: `null` is an error, not absent.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl PluginManifest {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let kind = match value.get("kind") {
            None => "web",
            Some(serde_json::Value::String(kind)) => kind.as_str(),
            Some(other) => return Err(format!("kind must be a string, got {other}")),
        };
        let manifest = match kind {
            "web" => {
                let web: WebManifestJson =
                    serde_json::from_value(value).map_err(|e| e.to_string())?;
                Self {
                    id: web.id,
                    name: web.name,
                    version: web.version,
                    default_size: web.default_size,
                    content: PluginContent::Web { entry: web.entry },
                    resizable: web.resizable,
                    min_size: web.min_size,
                    max_size: web.max_size,
                }
            }
            "tui" => {
                let object = value.as_object().ok_or("manifest must be a JSON object")?;
                let tui = tui::parse(object)?;
                Self {
                    id: tui.id,
                    name: tui.name,
                    version: tui.version,
                    default_size: tui.default_size,
                    content: PluginContent::Tui(tui.spec),
                    // TUI panels resize freely, like terminals.
                    resizable: None,
                    min_size: None,
                    max_size: None,
                }
            }
            other => {
                return Err(format!(
                    "unknown kind {other:?}, expected \"web\" or \"tui\""
                ));
            }
        };
        manifest.validate()?;
        Ok(manifest)
    }

    /// The HTML entry of a web plugin.
    pub fn entry(&self) -> Option<&Path> {
        match &self.content {
            PluginContent::Web { entry } => Some(entry),
            PluginContent::Tui(_) => None,
        }
    }

    pub fn tui(&self) -> Option<&TuiSpec> {
        match &self.content {
            PluginContent::Tui(spec) => Some(spec),
            PluginContent::Web { .. } => None,
        }
    }

    /// The validated resize settings, as calm-core's policy takes them.
    pub fn resize(&self) -> PluginResize {
        PluginResize {
            resizable: self.resizable.unwrap_or(true),
            min: self.min_size.map(Size::core),
            max: self.max_size.map(Size::core),
        }
    }

    fn validate_resize(&self) -> Result<(), String> {
        let limited = self.min_size.is_some() || self.max_size.is_some();
        if self.resizable == Some(false) && limited {
            return Err(
                "minSize and maxSize apply only to a resizable panel; remove them or \
                 \"resizable\": false"
                    .to_string(),
            );
        }
        for (field, size) in [("minSize", self.min_size), ("maxSize", self.max_size)] {
            let Some(size) = size else { continue };
            for (axis, value) in [("width", size.width), ("height", size.height)] {
                if !value.is_finite() || value <= 0.0 {
                    return Err(format!(
                        "{field}.{axis} must be a positive number, got {value}"
                    ));
                }
            }
            if size.width < MIN_PANEL_SIZE.width || size.height < MIN_PANEL_SIZE.height {
                return Err(format!(
                    "{field} {}x{} is below the smallest panel, {}x{}",
                    size.width, size.height, MIN_PANEL_SIZE.width, MIN_PANEL_SIZE.height
                ));
            }
        }
        let min = self.min_size.map(Size::core).unwrap_or(MIN_PANEL_SIZE);
        if let Some(max) = self.max_size
            && (max.width < min.width || max.height < min.height)
        {
            return Err(format!(
                "maxSize {}x{} is smaller than minSize {}x{}",
                max.width, max.height, min.width, min.height
            ));
        }
        let size = self.default_size;
        let below = size.width < min.width || size.height < min.height;
        let above = self
            .max_size
            .is_some_and(|max| size.width > max.width || size.height > max.height);
        if limited && (below || above) {
            return Err(format!(
                "defaultSize {}x{} is outside minSize and maxSize",
                size.width, size.height
            ));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        self.validate_resize()?;
        let id_ok = !self.id.is_empty()
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !id_ok {
            return Err(format!("id {:?} must match [a-z0-9-]+", self.id));
        }
        let Some(entry) = self.entry() else {
            return Ok(());
        };
        // Only plain relative segments, so the entry cannot leave the plugin directory.
        let entry_ok = entry.components().next().is_some()
            && entry
                .components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if !entry_ok {
            return Err(format!(
                "entry {} must be a relative path inside the plugin directory",
                entry.display()
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstalledPlugin {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum PluginError {
    #[error("{0}: no {MANIFEST_FILE}")]
    MissingManifest(PathBuf),
    #[error("{path}: {message}")]
    Invalid { path: PathBuf, message: String },
    #[error("duplicate plugin id {id:?}: {first} and {second}")]
    DuplicateId {
        id: String,
        first: PathBuf,
        second: PathBuf,
    },
}

/// A built-in plugin hidden because a user dir has a plugin with the same id.
#[derive(Debug, Clone, PartialEq)]
pub struct Shadowed {
    pub id: String,
    pub kept: PathBuf,
    pub hidden: PathBuf,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Discovery {
    pub plugins: Vec<InstalledPlugin>,
    pub errors: Vec<PluginError>,
    pub shadowed: Vec<Shadowed>,
    /// How many of `plugins` came from built-in dirs. They come after the user plugins.
    pub builtin_count: usize,
}

impl Discovery {
    pub fn user_count(&self) -> usize {
        self.plugins.len() - self.builtin_count
    }
}

/// Where plugins are looked for. User dirs come first, so a user copy wins over a built-in.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PluginSearch {
    pub user: Vec<PathBuf>,
    pub builtin: Vec<PathBuf>,
    /// Why built-ins are off, if they are.
    pub builtin_disabled_by: Option<&'static str>,
}

/// Env var that turns built-in plugin dirs off when set to anything but empty or `0`.
pub const NO_BUILTIN_ENV: &str = "CALM_NO_BUILTIN_PLUGINS";

/// The repo's own `plugins/` folder, fixed at compile time.
pub fn workspace_plugins_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

/// The search for this process. `builtin_setting` is the `plugins.builtin` setting.
pub fn plugin_search(builtin_setting: bool) -> PluginSearch {
    let user = plugin_dirs_from(
        std::env::var_os("CALM_PLUGIN_PATH"),
        std::env::var_os("CALM_HOME"),
        std::env::var_os("HOME"),
    );
    let env_off = std::env::var_os(NO_BUILTIN_ENV).is_some_and(|v| !v.is_empty() && v != "0");
    let builtin_disabled_by = if env_off {
        Some(NO_BUILTIN_ENV)
    } else if !builtin_setting {
        Some("setting plugins.builtin=false")
    } else {
        None
    };
    let builtin = match builtin_disabled_by {
        Some(_) => Vec::new(),
        None => builtin_dirs_from(
            cfg!(debug_assertions),
            std::env::current_exe().ok().as_deref(),
            &workspace_plugins_dir(),
        ),
    };
    PluginSearch {
        user,
        builtin,
        builtin_disabled_by,
    }
}

/// User dirs, then built-in dirs, with built-ins on.
pub fn plugin_dirs() -> Vec<PathBuf> {
    let search = plugin_search(true);
    search.user.into_iter().chain(search.builtin).collect()
}

/// Built-in dirs that exist: the workspace `plugins/` in a dev build (debug, or a binary
/// inside a cargo `target` dir), and `Contents/Resources/plugins` next to a bundled binary.
fn builtin_dirs_from(
    debug_build: bool,
    exe: Option<&Path>,
    workspace_plugins: &Path,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let in_target_dir = exe.is_some_and(|exe| {
        exe.ancestors()
            .any(|a| a.file_name().is_some_and(|name| name == "target"))
    });
    if (debug_build || in_target_dir) && workspace_plugins.is_dir() {
        dirs.push(fs::canonicalize(workspace_plugins).unwrap_or(workspace_plugins.to_path_buf()));
    }
    let macos_dir = exe.and_then(Path::parent);
    if let Some(contents) = macos_dir
        .filter(|dir| dir.file_name().is_some_and(|name| name == "MacOS"))
        .and_then(Path::parent)
    {
        let bundled = contents.join("Resources/plugins");
        if bundled.is_dir() {
            dirs.push(bundled);
        }
    }
    dirs
}

fn plugin_dirs_from(
    plugin_path: Option<OsString>,
    calm_home: Option<OsString>,
    home: Option<OsString>,
) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = plugin_path
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|d| !d.as_os_str().is_empty())
                .collect()
        })
        .unwrap_or_default();
    match (calm_home, home) {
        (Some(calm_home), _) => dirs.push(PathBuf::from(calm_home).join("plugins")),
        (None, Some(home)) => dirs.push(PathBuf::from(home).join(".config/calm/plugins")),
        (None, None) => {}
    }
    dirs
}

/// Scans each root for plugin subdirectories. The first plugin with a given id wins.
pub fn discover(dirs: &[PathBuf]) -> Discovery {
    discover_search(&PluginSearch {
        user: dirs.to_vec(),
        ..PluginSearch::default()
    })
}

/// Scans user dirs, then built-in dirs. A built-in whose id a user plugin already has is
/// recorded in `shadowed`, not as an error. Other repeated ids are errors.
pub fn discover_search(search: &PluginSearch) -> Discovery {
    let mut found = Discovery::default();
    // id -> (dir, came from a built-in dir)
    let mut seen: HashMap<String, (PathBuf, bool)> = HashMap::new();
    let roots = search
        .user
        .iter()
        .map(|root| (root, false))
        .chain(search.builtin.iter().map(|root| (root, true)));
    for (root, builtin) in roots {
        for dir in plugin_subdirs(root, &mut found.errors) {
            let manifest = match load_manifest(&dir) {
                Ok(manifest) => manifest,
                Err(err) => {
                    found.errors.push(err);
                    continue;
                }
            };
            if let Some((first, first_builtin)) = seen.get(&manifest.id) {
                if builtin && !first_builtin {
                    found.shadowed.push(Shadowed {
                        id: manifest.id,
                        kept: first.clone(),
                        hidden: dir,
                    });
                } else {
                    found.errors.push(PluginError::DuplicateId {
                        id: manifest.id,
                        first: first.clone(),
                        second: dir,
                    });
                }
                continue;
            }
            seen.insert(manifest.id.clone(), (dir.clone(), builtin));
            if builtin {
                found.builtin_count += 1;
            }
            found.plugins.push(InstalledPlugin { manifest, dir });
        }
    }
    found
}

fn plugin_subdirs(root: &Path, errors: &mut Vec<PluginError>) -> Vec<PathBuf> {
    // A root that does not exist is normal, e.g. a fresh machine without a plugin folder.
    if !root.exists() {
        return Vec::new();
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) => {
            errors.push(PluginError::Invalid {
                path: root.to_path_buf(),
                message: err.to_string(),
            });
            return Vec::new();
        }
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.is_dir())
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .collect();
    dirs.sort();
    dirs
}

fn load_manifest(dir: &Path) -> Result<PluginManifest, PluginError> {
    let path = dir.join(MANIFEST_FILE);
    if !path.is_file() {
        return Err(PluginError::MissingManifest(dir.to_path_buf()));
    }
    let invalid = |message: String| PluginError::Invalid {
        path: path.clone(),
        message,
    };
    let text = fs::read_to_string(&path).map_err(|e| invalid(e.to_string()))?;
    let manifest = PluginManifest::from_json(&text).map_err(invalid)?;
    if let Some(entry) = manifest.entry() {
        check_file_on_disk(dir, entry, "entry").map_err(invalid)?;
    }
    if let Some(icon) = manifest.tui().and_then(TuiSpec::icon_image) {
        check_file_on_disk(dir, icon, "icon").map_err(invalid)?;
    }
    Ok(manifest)
}

// The lexical check in from_json cannot see symlinks, so resolve the real path here.
fn check_file_on_disk(dir: &Path, file: &Path, field: &str) -> Result<(), String> {
    let shown = file.display();
    let real_dir = fs::canonicalize(dir).map_err(|e| format!("cannot resolve plugin dir: {e}"))?;
    let real_file = fs::canonicalize(dir.join(file))
        .map_err(|e| format!("cannot resolve {field} {shown}: {e}"))?;
    if !real_file.starts_with(&real_dir) {
        return Err(format!(
            "{field} {shown} resolves to {}, outside the plugin directory",
            real_file.display()
        ));
    }
    if !real_file.is_file() {
        return Err(format!("{field} {shown} is not a file"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn write_plugin(root: &Path, dir: &str, manifest: &str) -> PathBuf {
        let plugin_dir = root.join(dir);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join(MANIFEST_FILE), manifest).unwrap();
        fs::write(plugin_dir.join("index.html"), "<p>hi</p>").unwrap();
        plugin_dir
    }

    fn only_invalid_path(found: &Discovery) -> Option<&Path> {
        match &found.errors[..] {
            [PluginError::Invalid { path, .. }] => Some(path),
            _ => None,
        }
    }

    #[test]
    fn discover_rejects_entry_symlink_escaping_plugin_dir() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.html");
        fs::write(&secret, "<p>outside</p>").unwrap();
        let dir = root.path().join("escape");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(MANIFEST_FILE),
            manifest_json("escape", "index.html"),
        )
        .unwrap();
        std::os::unix::fs::symlink(&secret, dir.join("index.html")).unwrap();
        write_plugin(root.path(), "good", &manifest_json("good", "index.html"));

        let found = discover(&[root.path().to_path_buf()]);

        let ids: Vec<&str> = found
            .plugins
            .iter()
            .map(|p| p.manifest.id.as_str())
            .collect();
        assert_eq!(ids, vec!["good"]);
        assert_eq!(
            only_invalid_path(&found),
            Some(dir.join(MANIFEST_FILE).as_path()),
            "{:?}",
            found.errors
        );
    }

    #[test]
    fn discover_rejects_entry_that_is_a_directory() {
        let root = tempfile::tempdir().unwrap();
        let dir = write_plugin(root.path(), "dot", &manifest_json("dot", "."));
        write_plugin(root.path(), "good", &manifest_json("good", "index.html"));

        let found = discover(&[root.path().to_path_buf()]);

        let ids: Vec<&str> = found
            .plugins
            .iter()
            .map(|p| p.manifest.id.as_str())
            .collect();
        assert_eq!(ids, vec!["good"]);
        assert_eq!(
            only_invalid_path(&found),
            Some(dir.join(MANIFEST_FILE).as_path()),
            "{:?}",
            found.errors
        );
    }

    fn manifest_json(id: &str, entry: &str) -> String {
        format!(
            r#"{{ "id": "{id}", "name": "Name {id}", "version": "0.1.0", "entry": "{entry}",
                 "defaultSize": {{ "width": 320, "height": 200 }} }}"#
        )
    }

    #[test]
    fn discover_reads_valid_manifest() {
        let root = tempfile::tempdir().unwrap();
        let dir = write_plugin(
            root.path(),
            "hello",
            &manifest_json("hello-world", "index.html"),
        );

        let found = discover(&[root.path().to_path_buf()]);

        assert_eq!(found.errors, vec![]);
        assert_eq!(
            found.plugins,
            vec![InstalledPlugin {
                manifest: PluginManifest {
                    id: "hello-world".into(),
                    name: "Name hello-world".into(),
                    version: "0.1.0".into(),
                    default_size: Size {
                        width: 320.0,
                        height: 200.0
                    },
                    content: PluginContent::Web {
                        entry: PathBuf::from("index.html")
                    },
                    resizable: None,
                    min_size: None,
                    max_size: None,
                },
                dir,
            }]
        );
    }

    fn with_fields(extra: &str) -> String {
        format!(
            r#"{{ "id": "p", "name": "P", "version": "0.1.0", "entry": "index.html",
                 "defaultSize": {{ "width": 320, "height": 200 }}{extra} }}"#
        )
    }

    #[test]
    fn resize_fields_are_optional_and_default_to_free() {
        let manifest = PluginManifest::from_json(&with_fields("")).unwrap();
        assert_eq!(manifest.resize(), PluginResize::default());
    }

    #[test]
    fn resize_fields_parse_opt_out_and_limits() {
        let locked = PluginManifest::from_json(&with_fields(r#", "resizable": false"#)).unwrap();
        assert!(!locked.resize().resizable);
        let limited = PluginManifest::from_json(&with_fields(
            r#", "resizable": true, "minSize": { "width": 200, "height": 120 },
               "maxSize": { "width": 900, "height": 700 }"#,
        ))
        .unwrap();
        assert_eq!(
            limited.resize(),
            PluginResize {
                resizable: true,
                min: Some(calm_core::geometry::Size::new(200.0, 120.0)),
                max: Some(calm_core::geometry::Size::new(900.0, 700.0)),
            }
        );
    }

    /// Each bad value is its own explicit error, never a quiet fallback to free resizing.
    #[test]
    fn bad_resize_fields_are_manifest_errors() {
        let cases = [
            (r#", "resizable": "no""#, "invalid type"),
            (r#", "resizable": null"#, "invalid type: null"),
            (r#", "minSize": null"#, "invalid type: null"),
            (r#", "minSize": { "w": 200, "h": 120 }"#, "unknown field"),
            (r#", "minSize": { "width": 200 }"#, "missing field `height`"),
            (
                r#", "minSize": { "width": -5, "height": 120 }"#,
                "minSize.width must be a positive",
            ),
            (
                r#", "maxSize": { "width": 300, "height": 0 }"#,
                "maxSize.height must be a positive",
            ),
            (
                r#", "minSize": { "width": 60, "height": 120 }"#,
                "below the smallest panel",
            ),
            (
                r#", "minSize": { "width": 300, "height": 150 }, "maxSize": { "width": 250, "height": 400 }"#,
                "smaller than minSize",
            ),
            (
                r#", "resizable": false, "maxSize": { "width": 400, "height": 400 }"#,
                "apply only to a resizable panel",
            ),
            (
                r#", "minSize": { "width": 400, "height": 120 }"#,
                "defaultSize 320x200 is outside",
            ),
            (
                r#", "maxSize": { "width": 300, "height": 400 }"#,
                "defaultSize 320x200 is outside",
            ),
        ];
        for (extra, want) in cases {
            let err = PluginManifest::from_json(&with_fields(extra)).unwrap_err();
            assert!(err.contains(want), "{extra}: {err}");
        }
    }

    #[test]
    fn bad_resize_fields_skip_the_plugin_at_discovery() {
        let root = tempfile::tempdir().unwrap();
        let dir = write_plugin(
            root.path(),
            "bad",
            &with_fields(r#", "resizable": "sometimes""#).replace(r#""id": "p""#, r#""id": "bad""#),
        );
        let found = discover(&[root.path().to_path_buf()]);
        assert!(found.plugins.is_empty());
        assert_eq!(
            only_invalid_path(&found),
            Some(dir.join(MANIFEST_FILE).as_path()),
            "{:?}",
            found.errors
        );
    }

    #[test]
    fn discover_skips_broken_manifest_and_reports_path() {
        let root = tempfile::tempdir().unwrap();
        write_plugin(root.path(), "good", &manifest_json("good", "index.html"));
        let broken = write_plugin(root.path(), "broken", "{ not json");
        let empty = root.path().join("empty");
        fs::create_dir_all(&empty).unwrap();

        let found = discover(&[root.path().to_path_buf()]);

        let ids: Vec<&str> = found
            .plugins
            .iter()
            .map(|p| p.manifest.id.as_str())
            .collect();
        assert_eq!(ids, vec!["good"]);
        assert_eq!(found.errors.len(), 2, "{:?}", found.errors);
        assert!(found.errors.iter().any(|e| matches!(
            e,
            PluginError::Invalid { path, .. } if *path == broken.join(MANIFEST_FILE)
        )));
        assert!(found.errors.contains(&PluginError::MissingManifest(empty)));
    }

    #[test]
    fn discover_reports_duplicate_ids() {
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let first = write_plugin(first_root.path(), "a", &manifest_json("same", "index.html"));
        let second = write_plugin(
            second_root.path(),
            "b",
            &manifest_json("same", "index.html"),
        );

        let found = discover(&[
            first_root.path().to_path_buf(),
            second_root.path().to_path_buf(),
        ]);

        assert_eq!(found.plugins.len(), 1);
        assert_eq!(found.plugins[0].dir, first);
        assert_eq!(
            found.errors,
            vec![PluginError::DuplicateId {
                id: "same".into(),
                first,
                second,
            }]
        );
    }

    #[test]
    fn manifest_rejects_entry_outside_plugin_dir() {
        assert!(PluginManifest::from_json(&manifest_json("ok", "index.html")).is_ok());
        for entry in ["../x.html", "/etc/x.html", "sub/../../x.html"] {
            let result = PluginManifest::from_json(&manifest_json("ok", entry));
            assert!(result.is_err(), "entry {entry:?} was accepted");
        }

        let root = tempfile::tempdir().unwrap();
        let dir = write_plugin(root.path(), "escape", &manifest_json("escape", "../x.html"));
        let found = discover(&[root.path().to_path_buf()]);
        assert_eq!(found.plugins, vec![]);
        assert!(matches!(
            &found.errors[..],
            [PluginError::Invalid { path, .. }] if *path == dir.join(MANIFEST_FILE)
        ));
    }

    #[test]
    fn manifest_rejects_bad_ids() {
        for id in ["", "Hello", "has space", "under_score"] {
            let result = PluginManifest::from_json(&manifest_json(id, "index.html"));
            assert!(result.is_err(), "id {id:?} was accepted");
        }
    }

    #[test]
    fn discover_ignores_missing_root() {
        let root = tempfile::tempdir().unwrap();
        write_plugin(root.path(), "good", &manifest_json("good", "index.html"));
        let missing = root.path().join("does-not-exist");

        let found = discover(&[missing, root.path().to_path_buf()]);

        assert_eq!(found.errors, vec![]);
        assert_eq!(found.plugins.len(), 1);
    }

    const DEV_CHILD_ENV: &str = "CALM_TEST_DEV_DISCOVERY_CHILD";

    /// Runs one test of this binary in a child process with `env`, so no test mutates its
    /// own environment. Returns the child's combined output and whether it passed.
    fn run_child_test(name: &str, env: &[(&str, &std::ffi::OsStr)]) -> (bool, String) {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([name, "--exact", "--nocapture", "--test-threads=1"])
            .env(DEV_CHILD_ENV, "1")
            .env_remove("CALM_PLUGIN_PATH")
            .env_remove("CALM_NO_BUILTIN_PLUGINS");
        for (key, value) in env {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        (output.status.success(), text)
    }

    fn discovered_ids_in_child() -> Vec<String> {
        discover(&plugin_dirs())
            .plugins
            .into_iter()
            .map(|p| p.manifest.id)
            .collect()
    }

    #[test]
    fn child_dev_discovery_finds_workspace_plugins() {
        if std::env::var_os(DEV_CHILD_ENV).is_none() {
            return;
        }
        let ids = discovered_ids_in_child();
        for id in ["hello-world", "github-prs"] {
            assert!(ids.iter().any(|found| found == id), "{id} missing: {ids:?}");
        }
    }

    // Regression: a dev build with an empty CALM_HOME found no plugins, because the repo's
    // own plugins/ folder was never searched.
    #[test]
    fn dev_build_discovers_workspace_plugins_with_empty_user_dir() {
        let home = tempfile::tempdir().unwrap();
        let (passed, output) = run_child_test(
            "manifest::tests::child_dev_discovery_finds_workspace_plugins",
            &[("CALM_HOME", home.path().as_os_str())],
        );
        assert!(passed, "{output}");
        assert!(
            output.contains("1 passed"),
            "child test did not run: {output}"
        );
    }

    #[test]
    fn child_no_builtin_env_hides_workspace_plugins() {
        if std::env::var_os(DEV_CHILD_ENV).is_none() {
            return;
        }
        let search = plugin_search(true);
        assert_eq!(search.builtin, Vec::<PathBuf>::new());
        assert_eq!(search.builtin_disabled_by, Some(NO_BUILTIN_ENV));
        assert_eq!(discovered_ids_in_child(), Vec::<String>::new());
    }

    #[test]
    fn no_builtin_env_disables_builtin_dirs() {
        let home = tempfile::tempdir().unwrap();
        let (passed, output) = run_child_test(
            "manifest::tests::child_no_builtin_env_hides_workspace_plugins",
            &[
                ("CALM_HOME", home.path().as_os_str()),
                (NO_BUILTIN_ENV, std::ffi::OsStr::new("1")),
            ],
        );
        assert!(passed, "{output}");
        assert!(
            output.contains("1 passed"),
            "child test did not run: {output}"
        );
    }

    #[test]
    fn builtin_setting_off_disables_builtin_dirs() {
        let search = plugin_search(false);
        assert_eq!(search.builtin, Vec::<PathBuf>::new());
        assert!(search.builtin_disabled_by.is_some());
        assert!(plugin_search(true).builtin.len() <= 2);
    }

    #[test]
    fn builtin_dirs_cover_dev_build_target_dir_and_bundle() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("ws/plugins");
        fs::create_dir_all(&workspace).unwrap();
        let workspace = fs::canonicalize(&workspace).unwrap();
        let app = root.path().join("CALM.app/Contents");
        fs::create_dir_all(app.join("MacOS")).unwrap();
        fs::create_dir_all(app.join("Resources/plugins")).unwrap();
        let bundled_exe = app.join("MacOS/calm-app");
        let cargo_exe = root.path().join("ws/target/release/calm-app");
        let other_exe = root.path().join("bin/calm-app");

        assert_eq!(
            builtin_dirs_from(true, Some(&other_exe), &workspace),
            vec![workspace.clone()]
        );
        assert_eq!(
            builtin_dirs_from(false, Some(&cargo_exe), &workspace),
            vec![workspace.clone()]
        );
        assert_eq!(
            builtin_dirs_from(false, Some(&bundled_exe), &workspace),
            vec![app.join("Resources/plugins")]
        );
        assert_eq!(
            builtin_dirs_from(false, Some(&other_exe), &workspace),
            Vec::<PathBuf>::new()
        );
        // Dirs that do not exist at runtime are left out, not reported.
        let missing = root.path().join("gone/plugins");
        assert_eq!(
            builtin_dirs_from(true, Some(&other_exe), &missing),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn user_plugin_shadows_builtin_with_same_id() {
        let user = tempfile::tempdir().unwrap();
        let builtin = tempfile::tempdir().unwrap();
        let mine = write_plugin(
            user.path(),
            "mine",
            &manifest_json("hello-world", "index.html"),
        );
        let shipped = write_plugin(
            builtin.path(),
            "hello-world",
            &manifest_json("hello-world", "index.html"),
        );
        write_plugin(
            builtin.path(),
            "other",
            &manifest_json("other", "index.html"),
        );

        let found = discover_search(&PluginSearch {
            user: vec![user.path().to_path_buf()],
            builtin: vec![builtin.path().to_path_buf()],
            builtin_disabled_by: None,
        });

        let dirs: Vec<&Path> = found.plugins.iter().map(|p| p.dir.as_path()).collect();
        assert_eq!(
            dirs,
            vec![mine.as_path(), builtin.path().join("other").as_path()]
        );
        assert_eq!(found.errors, vec![]);
        assert_eq!(
            found.shadowed,
            vec![Shadowed {
                id: "hello-world".into(),
                kept: mine,
                hidden: shipped,
            }]
        );
        assert_eq!((found.user_count(), found.builtin_count), (1, 1));
    }

    #[test]
    fn duplicate_within_builtin_dirs_is_still_an_error() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        write_plugin(first.path(), "a", &manifest_json("same", "index.html"));
        write_plugin(second.path(), "b", &manifest_json("same", "index.html"));

        let found = discover_search(&PluginSearch {
            user: vec![],
            builtin: vec![first.path().to_path_buf(), second.path().to_path_buf()],
            builtin_disabled_by: None,
        });

        assert_eq!(found.plugins.len(), 1);
        assert_eq!(found.shadowed, vec![]);
        assert!(matches!(
            &found.errors[..],
            [PluginError::DuplicateId { .. }]
        ));
    }

    #[test]
    fn plugin_dirs_lists_user_dirs_before_builtin_dirs() {
        let search = plugin_search(true);
        let dirs = plugin_dirs();
        assert_eq!(dirs[..search.user.len()], search.user[..]);
        assert_eq!(dirs[search.user.len()..], search.builtin[..]);
    }

    #[test]
    fn plugin_dirs_lists_plugin_path_then_calm_home() {
        let dirs = plugin_dirs_from(
            Some("/a:/b".into()),
            Some("/calm".into()),
            Some("/home/u".into()),
        );
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/calm/plugins")
            ]
        );

        let dirs = plugin_dirs_from(None, None, Some("/home/u".into()));
        assert_eq!(dirs, vec![PathBuf::from("/home/u/.config/calm/plugins")]);
    }
}
