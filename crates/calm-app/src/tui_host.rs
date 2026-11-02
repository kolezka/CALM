//! Terminals that run one command under a supervisor: TUI apps, whose command comes from
//! their manifest, and typed commands. Builds each launch and tracks its exit.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use calm_core::chrome::ChromeOverrides;
use calm_core::scene::PanelId;
use calm_core::terminal_profile::{AppLayer, CommandRestart, PLACEHOLDER_NAME, TerminalIcon};
use calm_core::tui::{self, LaunchContext, LaunchSpec, RestartPolicy, Supervisor};
use calm_plugins::{InstalledPlugin, TuiIcon, TuiRestart, TuiSpec};
use gpui::Subscription;

/// Where a run's command comes from.
pub enum RunSource {
    App {
        /// Boxed: a manifest is much larger than a typed command.
        plugin: Box<InstalledPlugin>,
        /// Appended to the manifest's `args`, from the plugin menu item that opened it.
        extra_args: Vec<String>,
    },
    /// Typed by the user, run by the login shell.
    Typed { command: String },
}

/// The supervised command of one terminal: restart state and the current launch.
pub struct CommandRun {
    pub source: RunSource,
    pub supervisor: Supervisor,
    pub started_at: Instant,
    /// Counts launches, so each one writes its own status file.
    pub launches: u64,
    pub status_file: PathBuf,
    /// Exit and Enter events of the current terminal. Replaced on restart.
    pub subscription: Option<Subscription>,
}

/// The supervisor policy for a typed command.
pub fn typed_policy(restart: CommandRestart) -> RestartPolicy {
    match restart {
        CommandRestart::Never => RestartPolicy::Never,
        CommandRestart::OnExit => RestartPolicy::OnExit,
        CommandRestart::Prompt => RestartPolicy::Prompt,
    }
}

impl CommandRun {
    fn new(source: RunSource, policy: RestartPolicy) -> Self {
        Self {
            source,
            supervisor: Supervisor::new(policy),
            started_at: Instant::now(),
            launches: 0,
            status_file: PathBuf::new(),
            subscription: None,
        }
    }

    pub fn app(plugin: InstalledPlugin, extra_args: Vec<String>) -> Result<Self, String> {
        let spec = plugin
            .manifest
            .tui()
            .ok_or_else(|| format!("plugin {} is not a tui plugin", plugin.manifest.id))?;
        let policy = match spec.restart {
            TuiRestart::Never => RestartPolicy::Never,
            TuiRestart::OnExit => RestartPolicy::OnExit,
            TuiRestart::Prompt => RestartPolicy::Prompt,
        };
        let plugin = Box::new(plugin);
        Ok(Self::new(RunSource::App { plugin, extra_args }, policy))
    }

    pub fn typed(command: String, restart: CommandRestart) -> Self {
        Self::new(RunSource::Typed { command }, typed_policy(restart))
    }

    pub fn plugin(&self) -> Option<&InstalledPlugin> {
        match &self.source {
            RunSource::App { plugin, .. } => Some(plugin.as_ref()),
            RunSource::Typed { .. } => None,
        }
    }

    pub fn spec(&self) -> Option<&TuiSpec> {
        self.plugin().and_then(|plugin| plugin.manifest.tui())
    }

    pub fn is_app(&self) -> bool {
        self.plugin().is_some()
    }

    /// What `{name}` becomes: the app's name, or the typed command's first word.
    pub fn name(&self) -> String {
        match &self.source {
            RunSource::App { plugin, .. } => plugin.manifest.name.clone(),
            RunSource::Typed { command } => tui::command_name(command),
        }
    }

    /// The command a "command not found" title or log names. A typed command shows only
    /// its name, since the full line may carry env values.
    pub fn command(&self) -> String {
        match &self.source {
            RunSource::App { .. } => self
                .spec()
                .map_or(String::new(), |spec| spec.command.clone()),
            RunSource::Typed { command } => tui::command_name(command),
        }
    }

    /// The stderr line for panel `id` when its command was not found. A typed command
    /// is named by its panel only, since its text may hold env values.
    pub fn not_found_log(&self, id: PanelId) -> String {
        match self.plugin() {
            Some(plugin) => format!(
                "calm: tui {}: command not found: {} (plugin dir {})",
                plugin.manifest.id,
                self.command(),
                plugin.dir.display()
            ),
            None => format!("calm: panel {}: typed command not found", id.0),
        }
    }

    /// The chrome title for a terminal whose own title is `base`.
    pub fn title(&self, base: &str) -> String {
        self.supervisor.title(base, &self.command())
    }

    /// The layer between the profile and the overrides: an app's manifest look and env,
    /// or a typed command's `{name}` title.
    pub fn app_layer(&self) -> Option<AppLayer> {
        let RunSource::App { plugin, .. } = &self.source else {
            return Some(AppLayer::typed_command());
        };
        let spec = plugin.manifest.tui()?;
        Some(AppLayer {
            title_template: Some(
                spec.title
                    .clone()
                    .unwrap_or_else(|| PLACEHOLDER_NAME.to_string()),
            ),
            icon: spec.icon.as_ref().map(|icon| match icon {
                TuiIcon::Text(text) => TerminalIcon::Text(text.clone()),
                TuiIcon::Image(path) => TerminalIcon::AppImage(plugin.dir.join(path)),
            }),
            chrome: ChromeOverrides {
                accent: spec.accent_color,
                title_bar: spec.title_bar_color,
                title: spec.title_color,
            },
            colors: spec.terminal_colors.as_deref().copied(),
            env: spec.env.clone(),
        })
    }

    /// An app's start directory from its manifest, else home. `None` for typed commands,
    /// which start where a plain terminal would.
    pub fn app_start_dir(&self, home: &Path) -> Option<PathBuf> {
        let spec = self.spec()?;
        let home_text = home.to_string_lossy();
        Some(spec.cwd.as_deref().map_or_else(
            || home.to_path_buf(),
            |cwd| PathBuf::from(tui::expand_tilde(cwd, &home_text)),
        ))
    }

    /// The command line for the next launch, with `env` already merged. Removes a stale
    /// status file first, so a missing file after exit means the wrapper did not finish.
    pub fn next_launch(
        &mut self,
        id: PanelId,
        env: &BTreeMap<String, String>,
        login_shell: &str,
    ) -> Result<String, String> {
        self.launches += 1;
        let dir = runtime_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
        self.status_file = dir.join(format!("run-{}-{}.status", id.0, self.launches));
        match std::fs::remove_file(&self.status_file) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(format!(
                    "cannot clear {}: {err}",
                    self.status_file.display()
                ));
            }
        }
        let env: Vec<(String, String)> = env.clone().into_iter().collect();
        let (launch, name, plugin_dir) = match &self.source {
            RunSource::App { plugin, extra_args } => {
                let spec = plugin
                    .manifest
                    .tui()
                    .ok_or_else(|| format!("plugin {} is not a tui plugin", plugin.manifest.id))?;
                let launch = LaunchSpec {
                    command: spec.command.clone(),
                    args: spec.args.iter().chain(extra_args).cloned().collect(),
                    env,
                    shell: spec.shell,
                };
                let dir = plugin.dir.to_string_lossy().into_owned();
                (launch, spec.command.clone(), dir)
            }
            RunSource::Typed { command } => (
                tui::typed_command_spec(command, login_shell, env),
                tui::command_name(command),
                String::new(),
            ),
        };
        let status = self.status_file.to_string_lossy().into_owned();
        let argv = tui::argv_named(
            &launch,
            &LaunchContext {
                login_shell,
                status_file: &status,
                plugin_dir: &plugin_dir,
            },
            &name,
        );
        self.started_at = Instant::now();
        self.supervisor.started();
        Ok(tui::command_line(&argv))
    }
}

/// The exit code the wrapper wrote, or `None` if it never got there (e.g. killed).
pub fn read_exit_code(path: &Path) -> Option<i32> {
    match std::fs::read_to_string(path) {
        Ok(text) => match text.trim().parse() {
            Ok(code) => Some(code),
            Err(_) => {
                eprintln!(
                    "calm: run status {} is not a number: {text:?}",
                    path.display()
                );
                None
            }
        },
        Err(err) => {
            eprintln!("calm: run status {} unreadable: {err}", path.display());
            None
        }
    }
}

/// Per app run, so two CALM instances never read each other's status files.
fn runtime_dir() -> PathBuf {
    std::env::temp_dir().join(format!("calm-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_command_title_and_name_never_show_its_env_values() {
        let mut run = CommandRun::typed(
            "API_TOKEN=calm-secret-value calm-no-such-command-xyz --flag".to_string(),
            CommandRestart::Never,
        );
        assert_eq!(run.name(), "calm-no-such-command-xyz");
        run.supervisor
            .exited(Some(tui::COMMAND_NOT_FOUND), std::time::Duration::ZERO);
        assert_eq!(run.supervisor.state, tui::TuiState::NotFound);
        let title = run.title("Run");
        assert_eq!(
            title, "Run: command not found: calm-no-such-command-xyz",
            "{title}"
        );
    }

    #[test]
    fn a_typed_not_found_log_names_the_panel_and_no_command_text() {
        let run = CommandRun::typed(
            "API_TOKEN=SECRETVAL calm-no-such-command-xyz".to_string(),
            CommandRestart::Never,
        );
        let line = run.not_found_log(PanelId(42));
        assert!(line.contains("panel 42"), "{line}");
        assert!(!line.contains("calm-no-such-command-xyz"), "{line}");
        assert!(!line.contains("SECRETVAL"), "{line}");
    }
}
