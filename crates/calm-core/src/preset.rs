use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::layout::{LayoutError, LayoutFile, load_layout_from, valid_layout_name};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetOrigin {
    User,
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetEntry {
    pub name: String,
    pub origin: PresetOrigin,
}

pub fn find_preset(
    name: &str,
    user_dir: &Path,
    builtin_dirs: &[PathBuf],
) -> Result<(LayoutFile, PresetOrigin), LayoutError> {
    let missing = match load_layout_from(user_dir, name) {
        Ok(layout) => return Ok((layout, PresetOrigin::User)),
        Err(LayoutError::NotFound(path)) => path,
        Err(error) => return Err(error),
    };
    for dir in builtin_dirs {
        match load_layout_from(dir, name) {
            Ok(layout) => return Ok((layout, PresetOrigin::Builtin)),
            Err(LayoutError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }
    Err(LayoutError::NotFound(missing))
}

pub fn list_presets(user_dir: &Path, builtin_dirs: &[PathBuf]) -> Vec<PresetEntry> {
    fn names(dir: &Path) -> BTreeSet<String> {
        let Ok(entries) = fs::read_dir(dir) else {
            return BTreeSet::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if !path.is_file() || path.extension()? != "json" {
                    return None;
                }
                let name = path.file_stem()?.to_str()?;
                valid_layout_name(name).then(|| name.to_string())
            })
            .collect()
    }
    let user = names(user_dir);
    let mut builtin = BTreeSet::new();
    for dir in builtin_dirs {
        builtin.extend(names(dir));
    }
    builtin.retain(|name| !user.contains(name));
    user.into_iter()
        .map(|name| PresetEntry {
            name,
            origin: PresetOrigin::User,
        })
        .chain(builtin.into_iter().map(|name| PresetEntry {
            name,
            origin: PresetOrigin::Builtin,
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::scene::{PanelKind, Scene};
    use std::fs;

    fn write_layout(dir: &Path, name: &str, panels: usize) {
        let mut scene = Scene::default();
        for _ in 0..panels {
            scene.add(
                PanelKind::Terminal {
                    command: None,
                    cwd: None,
                    profile: None,
                    app: None,
                },
                Rect::new(0.0, 0.0, 400.0, 300.0),
            );
        }
        fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_string(&scene.to_layout()).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn user_preset_shadows_builtin() {
        let user = tempfile::tempdir().unwrap();
        let builtin = tempfile::tempdir().unwrap();
        write_layout(user.path(), "welcome", 1);
        write_layout(builtin.path(), "welcome", 2);
        let dirs = [builtin.path().to_path_buf()];
        let (layout, origin) = find_preset("welcome", user.path(), &dirs).unwrap();
        assert_eq!(layout.panels.len(), 1);
        assert_eq!(origin, PresetOrigin::User);
        let entries = list_presets(user.path(), &dirs);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "welcome");
        assert_eq!(entries[0].origin, PresetOrigin::User);
    }
    #[test]
    fn builtin_found_when_user_missing() {
        let user = tempfile::tempdir().unwrap();
        let builtin = tempfile::tempdir().unwrap();
        write_layout(builtin.path(), "welcome", 1);
        let (layout, origin) =
            find_preset("welcome", user.path(), &[builtin.path().to_path_buf()]).unwrap();
        assert_eq!(origin, PresetOrigin::Builtin);
        assert_eq!(layout.panels.len(), 1);
    }
    #[test]
    fn missing_everywhere_is_not_found() {
        let user = tempfile::tempdir().unwrap();
        let builtin = tempfile::tempdir().unwrap();
        assert!(
            matches!(find_preset("absent", user.path(), &[builtin.path().to_path_buf()]), Err(LayoutError::NotFound(path)) if path == user.path().join("absent.json"))
        );
    }
    #[test]
    fn malformed_user_preset_does_not_fall_back() {
        let user = tempfile::tempdir().unwrap();
        let builtin = tempfile::tempdir().unwrap();
        fs::write(user.path().join("welcome.json"), "{").unwrap();
        write_layout(builtin.path(), "welcome", 1);
        assert!(matches!(
            find_preset("welcome", user.path(), &[builtin.path().to_path_buf()]),
            Err(LayoutError::Malformed { .. })
        ));
    }
    #[test]
    fn list_skips_unsafe_names_and_sorts() {
        let user = tempfile::tempdir().unwrap();
        for name in ["b.json", "a.json", "bad name.json", "notes.txt"] {
            fs::write(user.path().join(name), "").unwrap();
        }
        let entries = list_presets(user.path(), &[]);
        assert_eq!(
            entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }
    #[test]
    fn builtin_order_and_errors_are_preserved() {
        let user = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let dirs = [first.path().to_path_buf(), second.path().to_path_buf()];
        write_layout(first.path(), "welcome", 1);
        write_layout(second.path(), "welcome", 2);
        assert_eq!(
            find_preset("welcome", user.path(), &dirs)
                .unwrap()
                .0
                .panels
                .len(),
            1
        );
        fs::write(first.path().join("welcome.json"), "{").unwrap();
        assert!(
            matches!(find_preset("welcome", user.path(), &dirs), Err(LayoutError::Malformed { path, .. }) if path == first.path().join("welcome.json"))
        );
        fs::write(first.path().join("welcome.json"), r#"{"version":999}"#).unwrap();
        assert!(
            matches!(find_preset("welcome", user.path(), &dirs), Err(LayoutError::UnsupportedVersion { path, .. }) if path == first.path().join("welcome.json"))
        );
        let not_dir = user.path().join("not-a-directory");
        fs::write(&not_dir, "").unwrap();
        assert!(matches!(
            find_preset("welcome", &not_dir, &dirs),
            Err(LayoutError::Io { .. })
        ));
    }

    #[test]
    fn list_groups_sorted_origins_and_ignores_unreadable_directories() {
        let user = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for name in ["z.json", "a.json"] {
            fs::write(user.path().join(name), "").unwrap();
        }
        for name in ["b.json", "z.json", "d.json"] {
            fs::write(first.path().join(name), "").unwrap();
        }
        for name in ["b.json", "c.json"] {
            fs::write(second.path().join(name), "").unwrap();
        }
        fs::create_dir(first.path().join("directory.json")).unwrap();
        let dirs = [
            first.path().to_path_buf(),
            user.path().join("missing"),
            second.path().to_path_buf(),
        ];
        let entries = list_presets(user.path(), &dirs);
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.name.as_str(), entry.origin))
                .collect::<Vec<_>>(),
            [
                ("a", PresetOrigin::User),
                ("z", PresetOrigin::User),
                ("b", PresetOrigin::Builtin),
                ("c", PresetOrigin::Builtin),
                ("d", PresetOrigin::Builtin),
            ]
        );
    }
}
