//! TUI panels: how their command line is built and what happens when the process exits.
//!
//! Ghostty takes one command string and, on macOS, runs it as `bash -c "exec -l <string>"`.
//! So the argv built here is joined with POSIX single quotes, never by plain concatenation.

use std::time::Duration;

/// Exit code a shell or `env` uses when it cannot find the command.
pub const COMMAND_NOT_FOUND: i32 = 127;
/// A run shorter than this counts as a quick failure for `on-exit` restarts.
pub const QUICK_EXIT: Duration = Duration::from_secs(10);
/// `on-exit` gives up after this many quick exits in a row.
pub const MAX_QUICK_EXITS: u32 = 5;
const FIRST_BACKOFF: Duration = Duration::from_secs(1);

/// Runs the command, writes its exit code to `$1`, prints a closing line, exits 0.
/// Exiting 0 keeps Ghostty's own "exited abnormally" text out; the code is in the file.
/// The traps are handlers, not ignores, so the command still gets Ctrl-C.
pub const STATUS_SCRIPT: &str = r#"status=$1 name=$2 dir=$3
shift 3
trap : INT QUIT
"$@"
code=$?
printf '%s\n' "$code" > "$status"
if [ "$code" -eq 127 ]; then
  printf '\n[calm] command not found: %s\n' "$name"
  [ -z "$dir" ] || printf '[calm] plugin: %s\n' "$dir"
else
  printf '\n[calm] %s exited with code %s\n' "$name" "$code"
fi
exit 0"#;

/// Runs `"$0" "$@"` in the user's login shell, so its profile sets `PATH`.
pub const LOGIN_SHELL_SCRIPT: &str = r#"exec "$0" "$@""#;

#[derive(Clone, Debug, PartialEq)]
pub struct LaunchSpec {
    pub command: String,
    pub args: Vec<String>,
    /// Applied by `/usr/bin/env`, after the login profile, so these win.
    pub env: Vec<(String, String)>,
    pub shell: bool,
}

/// Where a launch reports, and what its closing line names.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchContext<'a> {
    pub login_shell: &'a str,
    pub status_file: &'a str,
    pub plugin_dir: &'a str,
}

/// The full argv: `/usr/bin/env /bin/sh -c STATUS_SCRIPT ... [login shell] [env] command args`.
/// The leading `/usr/bin/env` absorbs the `-` that `exec -l` puts on argv[0], so `/bin/sh`
/// does not start as a login shell and read profiles.
pub fn argv(spec: &LaunchSpec, context: &LaunchContext) -> Vec<String> {
    argv_named(spec, context, &spec.command)
}

/// `argv` with `name` in the closing line instead of the command.
pub fn argv_named(spec: &LaunchSpec, context: &LaunchContext, name: &str) -> Vec<String> {
    let mut argv: Vec<String> = [
        "/usr/bin/env",
        "/bin/sh",
        "-c",
        STATUS_SCRIPT,
        "calm-tui",
        context.status_file,
        name,
        context.plugin_dir,
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if spec.shell {
        argv.extend([
            context.login_shell.to_string(),
            "-lc".to_string(),
            LOGIN_SHELL_SCRIPT.to_string(),
        ]);
    }
    if !spec.env.is_empty() {
        argv.push("/usr/bin/env".to_string());
        argv.extend(spec.env.iter().map(|(key, value)| format!("{key}={value}")));
    }
    argv.push(spec.command.clone());
    argv.extend(spec.args.iter().cloned());
    argv
}

/// A typed command runs in the login shell like a plain terminal: env first, then
/// `$SHELL -lc <command>`, so pipes and quotes in it work as typed.
pub fn typed_command_spec(
    command: &str,
    login_shell: &str,
    env: Vec<(String, String)>,
) -> LaunchSpec {
    LaunchSpec {
        command: login_shell.to_string(),
        args: vec!["-lc".to_string(), command.to_string()],
        env,
        shell: false,
    }
}

/// What `{name}` and the exit line call a command: its first word without the folder.
pub fn command_name(command: &str) -> String {
    let first = command.split_whitespace().next().unwrap_or_default();
    first.rsplit('/').next().unwrap_or(first).to_string()
}

/// One POSIX shell word that stands for exactly `word`.
pub fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// The string Ghostty gets as its command.
pub fn command_line(argv: &[String]) -> String {
    argv.iter()
        .map(|word| shell_quote(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Expands a leading `~` or `~/`. Other paths come back unchanged.
pub fn expand_tilde(path: &str, home: &str) -> String {
    if path == "~" {
        home.to_string()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{rest}", home.trim_end_matches('/'))
    } else {
        path.to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    OnExit,
    Prompt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuiState {
    Running,
    /// `None` when the exit code could not be read.
    Exited(Option<i32>),
    NotFound,
    RestartingIn(Duration),
    GaveUp(Option<i32>),
}

/// What the app does after an exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitAction {
    Stay,
    RestartAfter(Duration),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Supervisor {
    pub policy: RestartPolicy,
    pub state: TuiState,
    quick_exits: u32,
}

impl Supervisor {
    pub fn new(policy: RestartPolicy) -> Self {
        Self {
            policy,
            state: TuiState::Running,
            quick_exits: 0,
        }
    }

    /// The process ended after `ran_for`.
    pub fn exited(&mut self, code: Option<i32>, ran_for: Duration) -> ExitAction {
        if code == Some(COMMAND_NOT_FOUND) {
            // Restarting cannot find it either; the user fixes PATH or installs it first.
            self.state = TuiState::NotFound;
            return ExitAction::Stay;
        }
        match self.policy {
            RestartPolicy::Never | RestartPolicy::Prompt => {
                self.state = TuiState::Exited(code);
                ExitAction::Stay
            }
            RestartPolicy::OnExit => {
                if ran_for < QUICK_EXIT {
                    self.quick_exits += 1;
                } else {
                    self.quick_exits = 0;
                }
                if self.quick_exits >= MAX_QUICK_EXITS {
                    self.state = TuiState::GaveUp(code);
                    return ExitAction::Stay;
                }
                // 1 s after a long run, then 1, 2, 4, 8 s for quick exits in a row.
                let doublings = self.quick_exits.saturating_sub(1);
                let delay = FIRST_BACKOFF * 2u32.pow(doublings);
                self.state = TuiState::RestartingIn(delay);
                ExitAction::RestartAfter(delay)
            }
        }
    }

    /// A new process started, by the user or after a backoff.
    pub fn started(&mut self) {
        self.state = TuiState::Running;
    }

    /// A manual restart starts a fresh run of quick-exit counting.
    pub fn restart_requested(&mut self) -> bool {
        if !self.can_restart() {
            return false;
        }
        self.quick_exits = 0;
        self.started();
        true
    }

    /// Whether the panel offers Restart (menu item and Enter). `never` stays exited.
    pub fn can_restart(&self) -> bool {
        self.policy != RestartPolicy::Never
            && matches!(
                self.state,
                TuiState::Exited(_) | TuiState::NotFound | TuiState::GaveUp(_)
            )
    }

    /// The chrome title for a panel named `name` running `command`.
    pub fn title(&self, name: &str, command: &str) -> String {
        let code = |code: Option<i32>| code.map_or("unknown".to_string(), |c| c.to_string());
        match self.state {
            TuiState::Running => name.to_string(),
            TuiState::Exited(c) => format!("{name}: exited, code {}", code(c)),
            TuiState::NotFound => format!("{name}: command not found: {command}"),
            TuiState::RestartingIn(delay) => {
                format!("{name}: restarting in {} s", delay.as_secs())
            }
            TuiState::GaveUp(c) => format!(
                "{name}: gave up after {MAX_QUICK_EXITS} quick exits, code {}",
                code(c)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn spec(shell: bool, env: &[(&str, &str)], args: &[&str]) -> LaunchSpec {
        LaunchSpec {
            command: "printf".to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            shell,
        }
    }

    const CONTEXT: LaunchContext = LaunchContext {
        login_shell: "/bin/zsh",
        status_file: "/tmp/s",
        plugin_dir: "/p",
    };

    #[test]
    fn argv_without_shell_runs_command_under_the_status_wrapper() {
        let argv = argv(&spec(false, &[], &["a b"]), &CONTEXT);
        assert_eq!(
            argv,
            [
                "/usr/bin/env",
                "/bin/sh",
                "-c",
                STATUS_SCRIPT,
                "calm-tui",
                "/tmp/s",
                "printf",
                "/p",
                "printf",
                "a b"
            ]
        );
    }

    #[test]
    fn argv_with_shell_and_env_puts_env_after_the_login_shell() {
        let argv = argv(&spec(true, &[("K", "v w")], &["x"]), &CONTEXT);
        assert_eq!(
            argv[8..],
            [
                "/bin/zsh",
                "-lc",
                LOGIN_SHELL_SCRIPT,
                "/usr/bin/env",
                "K=v w",
                "printf",
                "x"
            ]
        );
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    /// Runs the joined command line through a real shell, like Ghostty does, and checks
    /// each argument arrives as one intact word.
    #[test]
    fn command_line_keeps_spaces_quotes_and_dollars_through_a_real_shell() {
        let tricky = [
            "two words",
            "it's",
            r#"say "hi""#,
            "$HOME",
            "$(echo pwned)",
            "back\\slash",
            "*",
            "",
        ];
        let dir = tempfile::tempdir().unwrap();
        let status = dir.path().join("status");
        let status = status.to_str().unwrap();
        for shell in [false, true] {
            let mut launch = spec(shell, &[("CALM_TEST_VAR", "a 'b' $c")], &["[%s]\\n"]);
            launch.args.extend(tricky.iter().map(|s| s.to_string()));
            launch.args.push("$CALM_TEST_VAR".to_string());
            let context = LaunchContext {
                login_shell: "/bin/sh",
                status_file: status,
                plugin_dir: "/p",
            };
            let line = command_line(&argv(&launch, &context));
            // Ghostty on macOS: bash --noprofile --norc -c "exec -l <line>".
            let output = Command::new("/bin/bash")
                .args(["--noprofile", "--norc", "-c", &format!("exec -l {line}")])
                .env_remove("CALM_TEST_VAR")
                .output()
                .unwrap();
            let stdout = String::from_utf8(output.stdout).unwrap();
            let mut expected: String = tricky.iter().map(|s| format!("[{s}]\n")).collect();
            // A literal "$CALM_TEST_VAR" argument stays literal: nothing expands it.
            expected.push_str("[$CALM_TEST_VAR]\n");
            assert!(
                stdout.starts_with(&expected),
                "shell={shell}\n{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                stdout.contains("[calm] printf exited with code 0"),
                "{stdout}"
            );
            assert_eq!(std::fs::read_to_string(status).unwrap(), "0\n");
        }
    }

    #[test]
    fn env_values_reach_the_command_and_missing_command_writes_127() {
        let dir = tempfile::tempdir().unwrap();
        let status = dir.path().join("status");
        let status = status.to_str().unwrap();
        let context = LaunchContext {
            login_shell: "/bin/sh",
            status_file: status,
            plugin_dir: "/plugins/slack",
        };
        let run = |launch: &LaunchSpec| {
            let line = command_line(&argv(launch, &context));
            let output = Command::new("/bin/bash")
                .args(["--noprofile", "--norc", "-c", &format!("exec -l {line}")])
                .output()
                .unwrap();
            assert!(output.status.success(), "the wrapper always exits 0");
            String::from_utf8(output.stdout).unwrap()
        };
        let mut launch = spec(true, &[("CALM_TEST_VAR", "a 'b' $c")], &[]);
        launch.command = "/usr/bin/printenv".to_string();
        launch.args = vec!["CALM_TEST_VAR".to_string()];
        assert!(run(&launch).starts_with("a 'b' $c\n"));

        for shell in [false, true] {
            let mut missing = spec(shell, &[], &[]);
            missing.command = "calm-no-such-command-xyz".to_string();
            let stdout = run(&missing);
            assert_eq!(std::fs::read_to_string(status).unwrap(), "127\n");
            assert!(
                stdout.contains("[calm] command not found: calm-no-such-command-xyz"),
                "{stdout}"
            );
            assert!(stdout.contains("[calm] plugin: /plugins/slack"), "{stdout}");
        }
    }

    #[test]
    fn a_typed_command_runs_in_the_login_shell_with_env_and_reports_its_exit() {
        let dir = tempfile::tempdir().unwrap();
        let status = dir.path().join("status");
        let status = status.to_str().unwrap();
        let run = |command: &str| {
            let env = vec![("CALM_TEST_VAR".to_string(), "x y".to_string())];
            let launch = typed_command_spec(command, "/bin/sh", env);
            let name = command_name(command);
            let context = LaunchContext {
                login_shell: "/bin/sh",
                status_file: status,
                plugin_dir: "",
            };
            let line = command_line(&argv_named(&launch, &context, &name));
            let output = Command::new("/bin/bash")
                .args(["--noprofile", "--norc", "-c", &format!("exec -l {line}")])
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap()
        };
        let stdout = run("printf '%s\\n' \"$CALM_TEST_VAR\" | tr x z; exit 3");
        assert!(stdout.starts_with("z y\n"), "{stdout}");
        assert!(
            stdout.contains("[calm] printf exited with code 3"),
            "{stdout}"
        );
        assert_eq!(std::fs::read_to_string(status).unwrap(), "3\n");

        let stdout = run("calm-no-such-command-xyz --flag");
        assert_eq!(std::fs::read_to_string(status).unwrap(), "127\n");
        assert!(
            stdout.contains("[calm] command not found: calm-no-such-command-xyz"),
            "{stdout}"
        );
        assert!(!stdout.contains("[calm] plugin:"), "{stdout}");
    }

    #[test]
    fn command_name_is_the_first_word_without_its_folder() {
        assert_eq!(command_name("htop -d 5"), "htop");
        assert_eq!(command_name("  /usr/bin/top"), "top");
        assert_eq!(command_name(""), "");
    }

    #[test]
    fn expand_tilde_only_touches_a_leading_tilde() {
        assert_eq!(expand_tilde("~", "/Users/u"), "/Users/u");
        assert_eq!(expand_tilde("~/w", "/Users/u/"), "/Users/u/w");
        assert_eq!(expand_tilde("/a/~/b", "/Users/u"), "/a/~/b");
        assert_eq!(expand_tilde("~other/x", "/Users/u"), "~other/x");
    }

    const QUICK: Duration = Duration::from_secs(1);
    const LONG: Duration = Duration::from_secs(60);

    #[test]
    fn on_exit_backs_off_and_gives_up_after_five_quick_exits() {
        let mut supervisor = Supervisor::new(RestartPolicy::OnExit);
        let mut delays = Vec::new();
        for _ in 0..4 {
            match supervisor.exited(Some(1), QUICK) {
                ExitAction::RestartAfter(delay) => delays.push(delay.as_secs()),
                ExitAction::Stay => panic!("gave up early: {:?}", supervisor.state),
            }
            supervisor.started();
        }
        assert_eq!(delays, vec![1, 2, 4, 8]);
        assert_eq!(supervisor.exited(Some(1), QUICK), ExitAction::Stay);
        assert_eq!(supervisor.state, TuiState::GaveUp(Some(1)));
        assert_eq!(
            supervisor.title("Slack", "slk"),
            "Slack: gave up after 5 quick exits, code 1"
        );
        // Restart is offered once it gave up, and resets the count.
        assert!(supervisor.restart_requested());
        assert_eq!(
            supervisor.exited(Some(1), QUICK),
            ExitAction::RestartAfter(Duration::from_secs(1))
        );
    }

    #[test]
    fn on_exit_long_run_resets_the_quick_exit_count() {
        let mut supervisor = Supervisor::new(RestartPolicy::OnExit);
        for _ in 0..4 {
            supervisor.exited(Some(1), QUICK);
            supervisor.started();
        }
        assert_eq!(
            supervisor.exited(Some(0), LONG),
            ExitAction::RestartAfter(Duration::from_secs(1))
        );
        assert_eq!(
            supervisor.state,
            TuiState::RestartingIn(Duration::from_secs(1))
        );
        assert!(
            !supervisor.can_restart(),
            "no manual restart while one is pending"
        );
    }

    #[test]
    fn prompt_and_never_stay_exited_and_only_prompt_offers_restart() {
        let mut prompt = Supervisor::new(RestartPolicy::Prompt);
        assert!(!prompt.can_restart(), "running");
        assert_eq!(prompt.exited(Some(0), QUICK), ExitAction::Stay);
        assert_eq!(prompt.title("Top", "top"), "Top: exited, code 0");
        assert!(prompt.can_restart());
        assert!(prompt.restart_requested());
        assert_eq!(prompt.state, TuiState::Running);
        assert_eq!(prompt.title("Top", "top"), "Top");

        let mut never = Supervisor::new(RestartPolicy::Never);
        assert_eq!(never.exited(None, LONG), ExitAction::Stay);
        assert_eq!(never.title("Top", "top"), "Top: exited, code unknown");
        assert!(!never.can_restart());
        assert!(!never.restart_requested());
    }

    #[test]
    fn command_not_found_never_restarts_on_its_own() {
        for policy in [
            RestartPolicy::Never,
            RestartPolicy::OnExit,
            RestartPolicy::Prompt,
        ] {
            let mut supervisor = Supervisor::new(policy);
            assert_eq!(
                supervisor.exited(Some(COMMAND_NOT_FOUND), QUICK),
                ExitAction::Stay
            );
            assert_eq!(supervisor.state, TuiState::NotFound);
            assert_eq!(
                supervisor.title("Slack", "slk"),
                "Slack: command not found: slk"
            );
        }
    }
}
