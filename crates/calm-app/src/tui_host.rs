//! TUI plugin panels: turns a manifest into a Ghostty launch and tracks each run.

use std::path::{Path, PathBuf};
use std::time::Instant;

use calm_core::scene::PanelId;
use calm_core::tui::{self, LaunchContext, LaunchSpec, RestartPolicy, Supervisor};
use calm_plugins::{InstalledPlugin, TuiRestart, TuiSpec};
use gpui::Subscription;
use gpui_libghostty::TerminalOptions;

use crate::terminal;

/// One TUI panel: its plugin, restart state and the current launch.
pub struct TuiRun {
    pub plugin: InstalledPlugin,
    pub supervisor: Supervisor,
    pub started_at: Instant,
    /// Counts launches, so each one writes its own status file.
    pub launches: u64,
    pub status_file: PathBuf,
    /// Exit and Enter events of the current terminal. Replaced on restart.
    pub subscription: Option<Subscription>,
    /// Appended to the manifest's `args`, from the plugin menu item that opened the panel.
    pub extra_args: Vec<String>,
}

impl TuiRun {
    pub fn new(plugin: InstalledPlugin) -> Result<Self, String> {
        let spec = plugin
            .manifest
            .tui()
            .ok_or_else(|| format!("plugin {} is not a tui plugin", plugin.manifest.id))?;
        let policy = match spec.restart {
            TuiRestart::Never => RestartPolicy::Never,
            TuiRestart::OnExit => RestartPolicy::OnExit,
            TuiRestart::Prompt => RestartPolicy::Prompt,
        };
        Ok(Self {
            supervisor: Supervisor::new(policy),
            plugin,
            started_at: Instant::now(),
            launches: 0,
            status_file: PathBuf::new(),
            subscription: None,
            extra_args: Vec::new(),
        })
    }

    pub fn spec(&self) -> &TuiSpec {
        self.plugin
            .manifest
            .tui()
            .expect("TuiRun::new only accepts tui plugins")
    }

    pub fn title(&self) -> String {
        self.supervisor
            .title(&self.plugin.manifest.name, &self.spec().command)
    }

    pub fn glyph(&self) -> Option<&str> {
        self.spec().icon.as_deref()
    }

    /// Options for the next launch. Removes a stale status file first, so a missing file
    /// after exit means the wrapper did not finish, not an old code.
    pub fn next_launch(&mut self, id: PanelId) -> Result<TerminalOptions, String> {
        self.launches += 1;
        let dir = runtime_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
        self.status_file = dir.join(format!("tui-{}-{}.status", id.0, self.launches));
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
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        let spec = self.spec();
        let launch = LaunchSpec {
            command: spec.command.clone(),
            args: spec.args.iter().chain(&self.extra_args).cloned().collect(),
            env: spec.env.clone().into_iter().collect(),
            shell: spec.shell,
        };
        // No cwd: home, like a new terminal.
        let cwd = spec
            .cwd
            .as_deref()
            .map(|cwd| PathBuf::from(tui::expand_tilde(cwd, &home)));
        let shell = login_shell();
        let status = self.status_file.to_string_lossy().into_owned();
        let plugin_dir = self.plugin.dir.to_string_lossy().into_owned();
        let argv = tui::argv(
            &launch,
            &LaunchContext {
                login_shell: &shell,
                status_file: &status,
                plugin_dir: &plugin_dir,
            },
        );
        self.started_at = Instant::now();
        self.supervisor.started();
        Ok(terminal::options(Some(&tui::command_line(&argv)), cwd))
    }
}

/// The exit code the wrapper wrote, or `None` if it never got there (e.g. killed).
pub fn read_exit_code(path: &Path) -> Option<i32> {
    match std::fs::read_to_string(path) {
        Ok(text) => match text.trim().parse() {
            Ok(code) => Some(code),
            Err(_) => {
                eprintln!(
                    "calm: tui status {} is not a number: {text:?}",
                    path.display()
                );
                None
            }
        },
        Err(err) => {
            eprintln!("calm: tui status {} unreadable: {err}", path.display());
            None
        }
    }
}

/// `$SHELL`, else zsh, the macOS default. Apps started from Finder may lack `$SHELL`.
fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_string())
}

/// Per app run, so two CALM instances never read each other's status files.
fn runtime_dir() -> PathBuf {
    std::env::temp_dir().join(format!("calm-{}", std::process::id()))
}
