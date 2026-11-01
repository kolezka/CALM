//! What a launch restores. Pure: the app does the reads, the move aside and the panel
//! creation, and this module decides from flags, the setting and the read results.

use std::io;
use std::path::{Path, PathBuf};

use crate::layout::{LayoutError, LayoutFile};
use crate::scene::PanelKind;
use crate::settings::RestoreOnLaunch;

/// Shown when `session.restore_on_launch` is `ask`: there is no launch dialog yet, so
/// `ask` restores like `on` and says so.
pub const ASK_FALLBACK_NOTICE: &str =
    "session.restore_on_launch is \"ask\", which has no dialog yet: restored the last session";

/// The launch flags that pick what to restore.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchFlags {
    /// `--layout NAME`.
    pub layout: Option<String>,
    /// `--no-restore`.
    pub no_restore: bool,
}

/// Where the first canvas of a launch comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    NamedLayout(String),
    Session,
    Empty,
}

/// `--layout NAME` wins, then `--no-restore`, then the setting. `ask` falls back to `on`
/// with a notice. The smoke never calls this: it always runs isolated in a temp dir.
pub fn pick_source(flags: &LaunchFlags, setting: RestoreOnLaunch) -> (Source, Option<String>) {
    if let Some(name) = &flags.layout {
        return (Source::NamedLayout(name.clone()), None);
    }
    if flags.no_restore {
        return (Source::Empty, None);
    }
    match setting {
        RestoreOnLaunch::On => (Source::Session, None),
        RestoreOnLaunch::Off => (Source::Empty, None),
        RestoreOnLaunch::Ask => (Source::Session, Some(ASK_FALLBACK_NOTICE.to_string())),
    }
}

/// Whether session autosave may run this launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutosaveStart {
    On,
    /// The reason, for the status pill and stderr.
    Off(String),
}

impl AutosaveStart {
    /// The autosave gate for the rest of this launch. Off closes it, so a later Settings
    /// toggle or reset cannot start a writer over a session file this launch could not
    /// read or move aside.
    pub fn launch_gate(&self, gate: bool) -> bool {
        gate && *self == AutosaveStart::On
    }
}

/// The launch, decided. The app moves `quarantine` aside first (then calls
/// `after_quarantine`), builds `layout` back to front, and starts autosave on the
/// resulting scene, so an untouched start never writes the session file.
#[derive(Clone, Debug, PartialEq)]
pub struct RestorePlan {
    /// Panels and camera to build. `None` is an empty start.
    pub layout: Option<LayoutFile>,
    /// The named layout this canvas belongs to, for a later save. Kept when the file is
    /// missing.
    pub layout_name: Option<String>,
    /// A session file that failed to load and must leave its place before autosave.
    pub quarantine: Option<PathBuf>,
    pub autosave: AutosaveStart,
    /// One line each for the status pill and stderr. Paths only, never file contents.
    pub notices: Vec<String>,
}

/// A launch that must not start: a named layout the user asked for is unusable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchExit {
    pub code: i32,
    pub message: String,
}

/// Decides the launch. `session` is the read of the session file, made on every launch
/// because autosave writes that file whatever the source. `layout` is the read of the
/// named layout, when the source is one.
///
/// - A missing session is a normal empty start.
/// - A malformed or unsupported session is quarantined; autosave stays on, and since the
///   file is gone from its place, the first save has nothing to rotate into the backups.
/// - A session that cannot be read turns autosave off for this launch.
/// - A missing named layout is an empty start that remembers the name. Any other error
///   on a named layout exits with code 2 and leaves the file where it is.
pub fn plan_restore(
    source: &Source,
    notice: Option<String>,
    session: Result<LayoutFile, LayoutError>,
    layout: Option<Result<LayoutFile, LayoutError>>,
) -> Result<RestorePlan, LaunchExit> {
    let mut plan = RestorePlan {
        layout: None,
        layout_name: None,
        quarantine: None,
        autosave: AutosaveStart::On,
        notices: notice.into_iter().collect(),
    };
    if let Source::NamedLayout(name) = source {
        plan.layout_name = Some(name.clone());
        match layout {
            Some(Ok(file)) => plan.layout = Some(file),
            Some(Err(LayoutError::NotFound(path))) => plan.notices.push(format!(
                "layout {} not found: started empty, saving uses the name {name}",
                path.display()
            )),
            Some(Err(error)) => {
                return Err(LaunchExit {
                    code: 2,
                    message: error.to_string(),
                });
            }
            None => {
                return Err(LaunchExit {
                    code: 2,
                    message: format!("layout {name} was not read"),
                });
            }
        }
    }
    match session {
        Ok(file) => {
            if *source == Source::Session {
                plan.layout = Some(file);
            }
        }
        Err(LayoutError::NotFound(_)) => {}
        Err(LayoutError::Malformed { path, .. } | LayoutError::UnsupportedVersion { path, .. }) => {
            plan.quarantine = Some(path)
        }
        Err(error @ LayoutError::Io { .. }) => {
            let reason = format!("autosave off for this launch: {error}");
            plan.notices.push(reason.clone());
            plan.autosave = AutosaveStart::Off(reason);
        }
    }
    Ok(plan)
}

/// Records the move aside of `plan.quarantine`. A failed move keeps the bad file in
/// place, so autosave is turned off rather than writing over it.
pub fn after_quarantine(plan: &mut RestorePlan, moved: io::Result<PathBuf>) {
    let Some(from) = plan.quarantine.take() else {
        return;
    };
    match moved {
        Ok(to) => plan.notices.push(quarantine_notice(&from, &to)),
        Err(error) => {
            let reason = format!(
                "autosave off for this launch: session {} could not be loaded or moved aside: {error}",
                from.display()
            );
            plan.notices.push(reason.clone());
            plan.autosave = AutosaveStart::Off(reason);
        }
    }
}

/// Names both paths and nothing from inside the file.
pub fn quarantine_notice(from: &Path, to: &Path) -> String {
    format!(
        "session {} could not be loaded, moved to {}: started empty",
        from.display(),
        to.display()
    )
}

/// Why a restored panel cannot run and draws as a placeholder, or `None` when it can.
/// `web` and `tui` say whether a plugin id is installed as that kind.
pub fn placeholder_reason(
    kind: &PanelKind,
    web: impl Fn(&str) -> bool,
    tui: impl Fn(&str) -> bool,
) -> Option<String> {
    match kind {
        PanelKind::Plugin { plugin_id, .. } if !web(plugin_id) => {
            Some(format!("Plugin {plugin_id} is not installed"))
        }
        PanelKind::Tui { plugin_id, .. } if !tui(plugin_id) => {
            Some(format!("TUI app {plugin_id} is not installed"))
        }
        PanelKind::Unsupported { raw } => Some(format!(
            "Unknown panel type {}, saved by a newer CALM",
            raw.type_name()
        )),
        _ => None,
    }
}

/// Shown after a restored terminal's title in its chrome. Never saved with the title.
pub const CWD_MISSING_NOTE: &str = "cwd missing, started in ~";

/// The cwd a restored terminal starts in. A saved cwd that is gone or not a directory
/// starts in `home` (not the profile's cwd), with a notice that names the path.
pub fn restored_cwd(
    cwd: Option<&Path>,
    is_dir: impl Fn(&Path) -> bool,
    home: &Path,
) -> (Option<PathBuf>, Option<String>) {
    match cwd {
        Some(path) if !is_dir(path) => (
            Some(home.to_path_buf()),
            Some(format!(
                "terminal cwd {} missing or not a directory, started in ~",
                path.display()
            )),
        ),
        other => (other.map(Path::to_path_buf), None),
    }
}

/// A log line when a plugin panel was saved by another version of its plugin. Versions
/// only: the panel settings are never logged.
pub fn version_note(
    plugin_id: &str,
    saved: Option<&str>,
    installed: Option<&str>,
) -> Option<String> {
    match (saved, installed) {
        (Some(saved), Some(installed)) if saved != installed => Some(format!(
            "plugin {plugin_id}: panel saved with version {saved}, installed {installed}"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autosave::AutosaveClock;
    use crate::browser_url::BrowserQueryMode;
    use crate::geometry::Rect;
    use crate::layout::{SessionWriter, backup_path, load_layout_from, move_aside};
    use crate::scene::Scene;
    use std::time::Duration;

    fn flags(layout: Option<&str>, no_restore: bool) -> LaunchFlags {
        LaunchFlags {
            layout: layout.map(str::to_string),
            no_restore,
        }
    }

    fn sample() -> LayoutFile {
        let mut scene = Scene::default();
        scene.add(
            PanelKind::Terminal {
                command: None,
                cwd: None,
            },
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        scene.to_layout()
    }

    fn not_found() -> Result<LayoutFile, LayoutError> {
        Err(LayoutError::NotFound(PathBuf::from("/s/session.json")))
    }

    fn malformed() -> Result<LayoutFile, LayoutError> {
        Err(LayoutError::Malformed {
            path: PathBuf::from("/s/session.json"),
            message: "secret-looking-content".to_string(),
        })
    }

    fn io_error() -> Result<LayoutFile, LayoutError> {
        Err(LayoutError::Io {
            path: PathBuf::from("/s/session.json"),
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        })
    }

    #[test]
    fn source_precedence_layout_then_no_restore_then_setting() {
        use RestoreOnLaunch::*;
        for setting in [On, Off, Ask] {
            assert_eq!(
                pick_source(&flags(Some("work"), true), setting),
                (Source::NamedLayout("work".to_string()), None)
            );
            assert_eq!(
                pick_source(&flags(None, true), setting),
                (Source::Empty, None)
            );
        }
        assert_eq!(
            pick_source(&flags(None, false), On),
            (Source::Session, None)
        );
        assert_eq!(pick_source(&flags(None, false), Off), (Source::Empty, None));
    }

    #[test]
    fn ask_restores_with_a_visible_notice() {
        let (source, notice) = pick_source(&flags(None, false), RestoreOnLaunch::Ask);
        assert_eq!(source, Source::Session);
        assert_eq!(notice.as_deref(), Some(ASK_FALLBACK_NOTICE));
        let plan = plan_restore(&source, notice, Ok(sample()), None).unwrap();
        assert!(plan.layout.is_some());
        assert_eq!(plan.notices, vec![ASK_FALLBACK_NOTICE.to_string()]);
    }

    #[test]
    fn valid_session_restores_with_autosave_on() {
        let plan = plan_restore(&Source::Session, None, Ok(sample()), None).unwrap();
        assert_eq!(plan.layout, Some(sample()));
        assert_eq!(plan.autosave, AutosaveStart::On);
        assert_eq!(plan.quarantine, None);
        assert!(plan.notices.is_empty());
    }

    #[test]
    fn missing_session_is_a_quiet_empty_start() {
        let plan = plan_restore(&Source::Session, None, not_found(), None).unwrap();
        assert_eq!(plan.layout, None);
        assert_eq!(plan.quarantine, None);
        assert_eq!(plan.autosave, AutosaveStart::On);
        assert!(plan.notices.is_empty());
    }

    #[test]
    fn malformed_or_unsupported_session_is_quarantined_with_autosave_on() {
        let unsupported = Err(LayoutError::UnsupportedVersion {
            path: PathBuf::from("/s/session.json"),
            found: 9,
        });
        for read in [malformed(), unsupported] {
            let plan = plan_restore(&Source::Session, None, read, None).unwrap();
            assert_eq!(plan.layout, None);
            assert_eq!(plan.quarantine, Some(PathBuf::from("/s/session.json")));
            assert_eq!(plan.autosave, AutosaveStart::On);
        }
    }

    #[test]
    fn a_bad_session_is_quarantined_even_when_restore_is_off() {
        // Autosave would rotate it into the backups otherwise.
        for source in [Source::Empty, Source::NamedLayout("w".to_string())] {
            let layout = matches!(source, Source::NamedLayout(_)).then(|| Ok(sample()));
            let plan = plan_restore(&source, None, malformed(), layout).unwrap();
            assert_eq!(plan.quarantine, Some(PathBuf::from("/s/session.json")));
        }
    }

    #[test]
    fn unreadable_session_turns_autosave_off() {
        let plan = plan_restore(&Source::Session, None, io_error(), None).unwrap();
        assert_eq!(plan.layout, None);
        assert_eq!(plan.quarantine, None);
        assert!(
            matches!(plan.autosave, AutosaveStart::Off(ref r) if r.starts_with("autosave off"))
        );
        assert_eq!(plan.notices.len(), 1);
    }

    #[test]
    fn restore_off_ignores_a_valid_session() {
        let plan = plan_restore(&Source::Empty, None, Ok(sample()), None).unwrap();
        assert_eq!(plan.layout, None);
        assert_eq!(plan.autosave, AutosaveStart::On);
    }

    #[test]
    fn named_layout_wins_and_missing_one_remembers_the_name() {
        let source = Source::NamedLayout("work".to_string());
        let plan = plan_restore(
            &source,
            None,
            Ok(Scene::default().to_layout()),
            Some(Ok(sample())),
        )
        .unwrap();
        assert_eq!(plan.layout, Some(sample()));
        assert_eq!(plan.layout_name.as_deref(), Some("work"));

        let missing = Err(LayoutError::NotFound(PathBuf::from("/s/layouts/work.json")));
        let plan = plan_restore(&source, None, not_found(), Some(missing)).unwrap();
        assert_eq!(plan.layout, None);
        assert_eq!(plan.layout_name.as_deref(), Some("work"));
        assert_eq!(plan.autosave, AutosaveStart::On);
    }

    #[test]
    fn malformed_named_layout_exits_2_and_quarantines_nothing() {
        let source = Source::NamedLayout("work".to_string());
        let bad = Err(LayoutError::Malformed {
            path: PathBuf::from("/s/layouts/work.json"),
            message: "expected value".to_string(),
        });
        // Even a bad session is left alone: the launch stops before anything moves.
        let exit = plan_restore(&source, None, malformed(), Some(bad)).unwrap_err();
        assert_eq!(exit.code, 2);
        assert!(
            exit.message.contains("/s/layouts/work.json"),
            "{}",
            exit.message
        );
    }

    #[test]
    fn quarantine_notice_names_paths_not_contents() {
        let mut plan = plan_restore(&Source::Session, None, malformed(), None).unwrap();
        after_quarantine(&mut plan, Ok(PathBuf::from("/s/session.json.invalid-0")));
        assert_eq!(plan.quarantine, None);
        assert_eq!(plan.autosave, AutosaveStart::On);
        let notice = plan.notices.join("\n");
        assert!(notice.contains("/s/session.json.invalid-0"), "{notice}");
        assert!(!notice.contains("secret-looking-content"), "{notice}");
    }

    #[test]
    fn failed_quarantine_turns_autosave_off() {
        let mut plan = plan_restore(&Source::Session, None, malformed(), None).unwrap();
        after_quarantine(
            &mut plan,
            Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        );
        assert!(matches!(plan.autosave, AutosaveStart::Off(_)));
    }

    /// Quarantine on disk: the bad file moves to `.invalid-0` unchanged, and the first
    /// autosave after it rotates nothing into the backups.
    #[test]
    fn quarantined_session_never_enters_the_backups() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let mut writer = SessionWriter::new(path.clone(), 3, BrowserQueryMode::StripAll);
        writer.save(&sample()).unwrap();
        // Positive control: the same file, intact, loads.
        assert!(load_layout_from(dir.path(), "session").is_ok());
        let text = std::fs::read(&path).unwrap();
        std::fs::write(&path, &text[..text.len() - 1]).unwrap();
        let corrupt = std::fs::read(&path).unwrap();

        let read = load_layout_from(dir.path(), "session");
        assert!(matches!(read, Err(LayoutError::Malformed { .. })));
        let mut plan = plan_restore(&Source::Session, None, read, None).unwrap();
        let from = plan.quarantine.clone().unwrap();
        after_quarantine(&mut plan, move_aside(&from));
        assert_eq!(plan.autosave, AutosaveStart::On);
        let moved = dir.path().join("session.json.invalid-0");
        assert_eq!(std::fs::read(&moved).unwrap(), corrupt);

        let mut writer = SessionWriter::new(path.clone(), 3, BrowserQueryMode::StripAll);
        writer.save(&Scene::default().to_layout()).unwrap();
        assert!(
            !backup_path(&path, 1).exists(),
            "a failed state was rotated"
        );
        assert_eq!(std::fs::read(&moved).unwrap(), corrupt);
    }

    /// Empty start guard: with restore off, the old session stays byte for byte until the
    /// first real change, and that first save rotates it into `.1`.
    #[test]
    fn empty_start_keeps_the_last_session_until_the_first_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        SessionWriter::new(path.clone(), 3, BrowserQueryMode::StripAll)
            .save(&sample())
            .unwrap();
        let last = std::fs::read(&path).unwrap();

        let plan = plan_restore(
            &Source::Empty,
            None,
            load_layout_from(dir.path(), "session"),
            None,
        )
        .unwrap();
        assert_eq!(plan.layout, None);
        assert_eq!(plan.autosave, AutosaveStart::On);

        let mut scene = Scene::default();
        let debounce = Duration::from_millis(100);
        let mut clock = AutosaveClock::new(debounce, scene.sync_revision());
        let mut writer = SessionWriter::new(path.clone(), 3, BrowserQueryMode::StripAll);
        for t in [0, 200, 20_000] {
            clock.observe(scene.sync_revision(), Duration::from_millis(t));
            assert_eq!(clock.due(Duration::from_millis(t)), None);
        }
        assert_eq!(std::fs::read(&path).unwrap(), last);

        scene.add(
            PanelKind::Browser {
                url: "about:blank".to_string(),
                page_zoom: None,
            },
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        let now = Duration::from_millis(30_000);
        clock.observe(scene.sync_revision(), now);
        let due = clock.due(now + debounce).expect("a change is saved");
        clock.submitted(due);
        writer.save(&scene.to_layout()).unwrap();
        assert_eq!(std::fs::read(backup_path(&path, 1)).unwrap(), last);
    }

    #[test]
    fn placeholders_for_missing_plugins_and_unknown_kinds() {
        let web = |id: &str| id == "github-prs";
        let tui = |id: &str| id == "top-example";
        let plugin = |id: &str| PanelKind::Plugin {
            plugin_id: id.to_string(),
            plugin_version: None,
            settings: serde_json::json!({}),
        };
        let app = |id: &str| PanelKind::Tui {
            plugin_id: id.to_string(),
            plugin_version: None,
        };
        assert_eq!(placeholder_reason(&plugin("github-prs"), web, tui), None);
        assert_eq!(
            placeholder_reason(&plugin("gone"), web, tui).as_deref(),
            Some("Plugin gone is not installed")
        );
        assert_eq!(placeholder_reason(&app("top-example"), web, tui), None);
        // A web plugin id does not satisfy a TUI panel.
        assert!(placeholder_reason(&app("github-prs"), web, tui).is_some());
        let unknown: PanelKind = serde_json::from_str(r#"{"type":"hologram","x":1}"#).unwrap();
        assert_eq!(
            placeholder_reason(&unknown, web, tui).as_deref(),
            Some("Unknown panel type hologram, saved by a newer CALM")
        );
        let terminal = PanelKind::Terminal {
            command: None,
            cwd: None,
        };
        assert_eq!(placeholder_reason(&terminal, web, tui), None);
    }

    #[test]
    fn missing_cwd_falls_back_to_home_with_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, "x").unwrap();
        let home = Path::new("/Users/someone");
        let is_dir = |p: &Path| p.is_dir();
        assert_eq!(
            restored_cwd(Some(dir.path()), is_dir, home),
            (Some(dir.path().to_path_buf()), None)
        );
        assert_eq!(restored_cwd(None, is_dir, home), (None, None));
        for gone in [file.as_path(), Path::new("/no/such/calm/dir")] {
            let (cwd, notice) = restored_cwd(Some(gone), is_dir, home);
            assert_eq!(cwd.as_deref(), Some(home));
            let notice = notice.unwrap();
            assert!(notice.contains(&gone.display().to_string()), "{notice}");
            assert!(notice.contains("started in ~"), "{notice}");
        }
    }

    #[test]
    fn unreadable_session_keeps_autosave_off_for_the_launch() {
        use crate::autosave::{AutosaveChange, autosave_change};
        use crate::settings::SessionSettings;
        let io = LayoutError::Io {
            path: PathBuf::from("/s/session.json"),
            source: io::Error::from(io::ErrorKind::PermissionDenied),
        };
        let plan = plan_restore(&Source::Session, None, Err(io), None).unwrap();
        let gate = plan.autosave.launch_gate(true);
        let on = SessionSettings::default();
        let off = SessionSettings {
            autosave: false,
            ..SessionSettings::default()
        };
        // Toggle off then on, and a reset to defaults: both flip the setting on.
        assert_eq!(
            autosave_change(gate, false, &off, &on),
            AutosaveChange::Keep
        );
        // Positive control: a readable session lets the same toggle start the writer.
        let gate = AutosaveStart::On.launch_gate(true);
        assert_eq!(
            autosave_change(gate, false, &off, &on),
            AutosaveChange::Start
        );
        assert!(!AutosaveStart::On.launch_gate(false));
    }

    #[test]
    fn version_note_only_on_a_mismatch() {
        assert_eq!(version_note("p", Some("1"), Some("1")), None);
        assert_eq!(version_note("p", None, Some("1")), None);
        assert_eq!(
            version_note("p", Some("1"), Some("2")).as_deref(),
            Some("plugin p: panel saved with version 1, installed 2")
        );
    }
}
