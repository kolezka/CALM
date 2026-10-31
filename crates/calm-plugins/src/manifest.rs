use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MANIFEST_FILE: &str = "calm-plugin.json";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub entry: PathBuf,
    pub default_size: Size,
}

impl PluginManifest {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let manifest: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), String> {
        let id_ok = !self.id.is_empty()
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !id_ok {
            return Err(format!("id {:?} must match [a-z0-9-]+", self.id));
        }
        // Only plain relative segments, so the entry cannot leave the plugin directory.
        let entry_ok = self.entry.components().next().is_some()
            && self
                .entry
                .components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if !entry_ok {
            return Err(format!(
                "entry {} must be a relative path inside the plugin directory",
                self.entry.display()
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

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Discovery {
    pub plugins: Vec<InstalledPlugin>,
    pub errors: Vec<PluginError>,
}

pub fn plugin_dirs() -> Vec<PathBuf> {
    plugin_dirs_from(
        std::env::var_os("CALM_PLUGIN_PATH"),
        std::env::var_os("CALM_HOME"),
        std::env::var_os("HOME"),
    )
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
    let mut found = Discovery::default();
    let mut seen: HashMap<String, PathBuf> = HashMap::new();
    for root in dirs {
        for dir in plugin_subdirs(root, &mut found.errors) {
            let manifest = match load_manifest(&dir) {
                Ok(manifest) => manifest,
                Err(err) => {
                    found.errors.push(err);
                    continue;
                }
            };
            if let Some(first) = seen.get(&manifest.id) {
                found.errors.push(PluginError::DuplicateId {
                    id: manifest.id,
                    first: first.clone(),
                    second: dir,
                });
                continue;
            }
            seen.insert(manifest.id.clone(), dir.clone());
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
    check_entry_on_disk(dir, &manifest.entry).map_err(invalid)?;
    Ok(manifest)
}

// The lexical check in from_json cannot see symlinks, so resolve the real path here.
fn check_entry_on_disk(dir: &Path, entry: &Path) -> Result<(), String> {
    let shown = entry.display();
    let real_dir = fs::canonicalize(dir).map_err(|e| format!("cannot resolve plugin dir: {e}"))?;
    let real_entry = fs::canonicalize(dir.join(entry))
        .map_err(|e| format!("cannot resolve entry {shown}: {e}"))?;
    if !real_entry.starts_with(&real_dir) {
        return Err(format!(
            "entry {shown} resolves to {}, outside the plugin directory",
            real_entry.display()
        ));
    }
    if !real_entry.is_file() {
        return Err(format!("entry {shown} is not a file"));
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
                    entry: PathBuf::from("index.html"),
                    default_size: Size {
                        width: 320.0,
                        height: 200.0
                    },
                },
                dir,
            }]
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
