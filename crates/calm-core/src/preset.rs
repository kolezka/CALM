use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::layout::{LayoutError, LayoutFile, load_layout_from, valid_layout_name};
use crate::scene::PanelKind;

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

/// What a panel is, for the Load Preset confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosingKind {
    Terminal,
    App,
    Browser,
    Plugin,
    Markdown,
}

/// The Load Preset kind of a panel, or `None` for a kind this build cannot read: it
/// closes too but never confirms, so it is not counted.
pub fn closing_kind(kind: &PanelKind) -> Option<ClosingKind> {
    match kind {
        PanelKind::Terminal { app: Some(_), .. } => Some(ClosingKind::App),
        PanelKind::Terminal { app: None, .. } => Some(ClosingKind::Terminal),
        PanelKind::Browser { .. } => Some(ClosingKind::Browser),
        PanelKind::Plugin { .. } => Some(ClosingKind::Plugin),
        PanelKind::Markdown { .. } => Some(ClosingKind::Markdown),
        PanelKind::Unsupported { .. } => None,
    }
}

/// Whether closing a panel of `kind` asks first when `panels.confirm_close` is on. `live`
/// is whether its terminal runs or its browser page is open. A markdown panel asks when it
/// has text: a preset load or a slip of the key would drop notes the user wrote.
pub fn close_confirms(kind: &PanelKind, live: bool) -> bool {
    match kind {
        PanelKind::Markdown { text } => !text.trim().is_empty(),
        _ => live,
    }
}

/// A panel that loading a preset would close. `confirms` is what a normal close asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosingPanel {
    pub kind: ClosingKind,
    pub confirms: bool,
}

/// The Load Preset question, or `None` when loading needs no confirmation: `confirm_close`
/// is off or no closing panel would confirm on its own close.
pub fn load_confirmation(
    preset: &str,
    closing: &[ClosingPanel],
    confirm_close: bool,
) -> Option<String> {
    if !confirm_close || !closing.iter().any(|panel| panel.confirms) {
        return None;
    }
    let nouns = [
        (ClosingKind::Terminal, "terminal"),
        (ClosingKind::App, "app"),
        (ClosingKind::Browser, "browser"),
        (ClosingKind::Plugin, "plugin"),
        (ClosingKind::Markdown, "markdown panel"),
    ];
    let parts: Vec<String> = nouns
        .iter()
        .filter_map(|(kind, noun)| {
            let count = closing.iter().filter(|panel| panel.kind == *kind).count();
            match count {
                0 => None,
                1 => Some(format!("1 {noun}")),
                n => Some(format!("{n} {noun}s")),
            }
        })
        .collect();
    let list = match parts.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => return None,
    };
    Some(format!("Load preset {preset}? This closes {list}."))
}

/// Whether to go on with a load: no question was needed, or the user said OK to it.
pub fn decide_load(confirmation: Option<&str>, user_said_ok: bool) -> bool {
    confirmation.is_none() || user_said_ok
}

/// The trimmed name a preset is saved under, or the message for a name that is not a safe
/// file name.
pub fn save_name(input: &str) -> Result<String, String> {
    let name = input.trim();
    if valid_layout_name(name) {
        Ok(name.to_string())
    } else {
        Err("Preset names use letters, digits, - and _.".to_string())
    }
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

    fn closing(kind: ClosingKind, n: usize, confirms: bool) -> Vec<ClosingPanel> {
        vec![ClosingPanel { kind, confirms }; n]
    }

    #[test]
    fn load_confirmation_counts_and_plurals() {
        let mut panels = closing(ClosingKind::Browser, 1, true);
        panels.extend(closing(ClosingKind::Terminal, 3, true));
        assert_eq!(
            load_confirmation("welcome", &panels, true).as_deref(),
            Some("Load preset welcome? This closes 3 terminals and 1 browser.")
        );
        let mut panels = closing(ClosingKind::Plugin, 1, false);
        panels.extend(closing(ClosingKind::Browser, 2, true));
        panels.extend(closing(ClosingKind::App, 1, true));
        panels.extend(closing(ClosingKind::Terminal, 1, false));
        assert_eq!(
            load_confirmation("x", &panels, true).as_deref(),
            Some("Load preset x? This closes 1 terminal, 1 app, 2 browsers and 1 plugin.")
        );
        assert_eq!(
            load_confirmation("x", &closing(ClosingKind::App, 2, true), true).as_deref(),
            Some("Load preset x? This closes 2 apps.")
        );
    }

    #[test]
    fn load_confirmation_none_when_nothing_confirms() {
        let mut panels = closing(ClosingKind::Plugin, 2, false);
        panels.extend(closing(ClosingKind::Terminal, 1, false));
        assert_eq!(load_confirmation("x", &panels, true), None);
        assert_eq!(load_confirmation("x", &[], true), None);
    }

    #[test]
    fn markdown_with_text_confirms_and_counts_like_a_browser() {
        let note = PanelKind::Markdown {
            text: "# Notes".into(),
        };
        let empty = PanelKind::Markdown {
            text: String::new(),
        };
        let blank = PanelKind::Markdown { text: " \n".into() };
        assert!(close_confirms(&note, false), "a note with text confirms");
        assert!(!close_confirms(&empty, false), "an empty note does not");
        assert!(!close_confirms(&blank, false), "whitespace is empty");
        assert_eq!(closing_kind(&note), Some(ClosingKind::Markdown));
        let closing: Vec<ClosingPanel> = [&note, &empty]
            .into_iter()
            .map(|kind| ClosingPanel {
                kind: closing_kind(kind).unwrap(),
                confirms: close_confirms(kind, false),
            })
            .collect();
        assert_eq!(
            load_confirmation("welcome", &closing, true).as_deref(),
            Some("Load preset welcome? This closes 2 markdown panels.")
        );
        let only_empty = [ClosingPanel {
            kind: closing_kind(&empty).unwrap(),
            confirms: close_confirms(&empty, false),
        }];
        assert_eq!(load_confirmation("x", &only_empty, true), None);
        // Other kinds still follow `live`.
        assert!(close_confirms(&PanelKind::terminal(), true));
        assert!(!close_confirms(&PanelKind::terminal(), false));
    }

    #[test]
    fn load_confirmation_none_when_confirm_close_off() {
        let panels = closing(ClosingKind::Terminal, 2, true);
        assert_eq!(load_confirmation("x", &panels, false), None);
    }

    #[test]
    fn load_preset_cancel_changes_nothing() {
        assert!(!decide_load(
            Some("Load preset x? This closes 1 terminal."),
            false
        ));
        assert!(decide_load(
            Some("Load preset x? This closes 1 terminal."),
            true
        ));
        assert!(decide_load(None, false));
        assert!(decide_load(None, true));
    }

    #[test]
    fn preset_name_rejects_unsafe_names() {
        let error = Err("Preset names use letters, digits, - and _.".to_string());
        for bad in ["", "   ", "my preset", "../x", "a/b", "x.json"] {
            assert_eq!(save_name(bad), error, "{bad:?}");
        }
        assert_eq!(save_name(" demo "), Ok("demo".to_string()));
        assert_eq!(save_name("my_layout-2"), Ok("my_layout-2".to_string()));
    }
}
