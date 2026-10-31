use std::io::{ErrorKind, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use thiserror::Error;

use crate::gh_token::GhToken;

/// How long one `gh api` call may run before it is killed.
pub const GH_TIMEOUT: Duration = Duration::from_secs(30);
/// The largest `gh api` response CALM parses.
pub const GH_MAX_OUTPUT: usize = 8 * 1024 * 1024;
// Enough for any gh error message; the rest is dropped.
const GH_MAX_STDERR: u64 = 64 * 1024;

/// Runs `gh api` on the host. The GitHub token stays inside `gh` and never passes through CALM.
#[derive(Debug, Clone, PartialEq)]
pub struct GhClient {
    pub program: PathBuf,
    pub timeout: Duration,
    pub max_output: usize,
    /// Passed to `gh` as `GH_TOKEN` in its env. `None` leaves `gh` on its own login.
    pub token: Option<GhToken>,
}

impl Default for GhClient {
    fn default() -> Self {
        Self::new("gh")
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum GhError {
    #[error("gh is not installed, install it from https://cli.github.com")]
    NotInstalled,
    #[error("gh is not logged in, run gh auth login ({0})")]
    NotAuthenticated(String),
    #[error("gh api failed: {0}")]
    Rejected(String),
    #[error("invalid gh api path {0:?}")]
    InvalidPath(String),
    #[error("gh api timed out after {0} ms")]
    TimedOut(u128),
    #[error("gh api response is over the {0} byte limit")]
    TooLarge(usize),
}

impl GhClient {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            timeout: GH_TIMEOUT,
            max_output: GH_MAX_OUTPUT,
            token: None,
        }
    }

    /// Runs `<program> api <path>` and parses stdout as JSON. Blocks, so call it off the UI
    /// thread. Kills `gh` after `timeout` and refuses output over `max_output` bytes.
    pub fn api(&self, path: &str) -> Result<Value, GhError> {
        if !is_valid_path(path) {
            return Err(GhError::InvalidPath(path.to_string()));
        }
        let mut command = Command::new(&self.program);
        command
            .arg("api")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Env, never argv: argv is visible to every user through `ps`.
        if let Some(token) = &self.token {
            command.env("GH_TOKEN", token.expose());
        }
        let spawned = command.spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(err) if err.kind() == ErrorKind::NotFound => return Err(GhError::NotInstalled),
            Err(err) => return Err(GhError::Rejected(err.to_string())),
        };
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(GhError::Rejected("gh pipes were not opened".into()));
        };
        // Reading one byte past the limit is how an oversized answer is told apart.
        let stdout_limit = self.max_output as u64 + 1;
        let stdout = thread::spawn(move || read_capped(stdout, stdout_limit));
        let stderr = thread::spawn(move || read_capped(stderr, GH_MAX_STDERR));
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    // The reader threads end when the pipes close; nothing waits on them.
                    return Err(GhError::TimedOut(self.timeout.as_millis()));
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(err) => return Err(GhError::Rejected(err.to_string())),
            }
        };
        let stdout = stdout.join().unwrap_or_default();
        let stderr = stderr.join().unwrap_or_default();
        if stdout.len() > self.max_output {
            return Err(GhError::TooLarge(self.max_output));
        }
        if !status.success() {
            let stderr = self.redact(String::from_utf8_lossy(&stderr).trim());
            if stderr.contains("gh auth login")
                || stderr.contains("not logged")
                || stderr.contains("HTTP 401")
            {
                return Err(GhError::NotAuthenticated(stderr));
            }
            return Err(GhError::Rejected(stderr));
        }
        // GitHub does not echo tokens, but this answer goes to plugin JS, so make sure.
        let stdout = self.redact(&String::from_utf8_lossy(&stdout));
        serde_json::from_str(&stdout)
            .map_err(|err| GhError::Rejected(format!("gh returned invalid JSON: {err}")))
    }

    fn redact(&self, text: &str) -> String {
        match &self.token {
            Some(token) => token.redact(text),
            None => text.to_string(),
        }
    }
}

// Stops at `limit` bytes and drops the pipe, so a runaway writer gets EPIPE.
fn read_capped(reader: impl Read, limit: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = reader.take(limit).read_to_end(&mut buf);
    buf
}

// A leading `-` would turn the path into a gh flag.
fn is_valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('-')
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./?=&%-".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    // Linux can report ETXTBSY when parallel tests exec a script while another writes one.
    static FAKE_GH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn fake_gh(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("gh");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn api_runs_gh_and_parses_json() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let gh = fake_gh(
            dir.path(),
            r#"if [ "$#" -eq 2 ] && [ "$1" = api ] && [ "$2" = "repos/o/r/pulls?state=open&per_page=30" ]; then
  echo '[{"number":1,"title":"Fix","draft":false}]'
else
  echo "unexpected args: $*" >&2
  exit 1
fi"#,
        );

        let value = GhClient::new(gh)
            .api("repos/o/r/pulls?state=open&per_page=30")
            .unwrap();

        assert_eq!(
            value,
            json!([{ "number": 1, "title": "Fix", "draft": false }])
        );
    }

    #[test]
    fn api_reports_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let client = GhClient::new(dir.path().join("no-such-gh"));

        assert_eq!(client.api("user"), Err(GhError::NotInstalled));
        assert!(
            GhError::NotInstalled
                .to_string()
                .contains("gh is not installed")
        );
    }

    #[test]
    fn api_reports_not_authenticated_from_stderr() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let gh = fake_gh(
            dir.path(),
            r#"echo "To get started with GitHub CLI, please run:  gh auth login" >&2
exit 4"#,
        );

        let err = GhClient::new(gh).api("user").unwrap_err();

        assert!(
            matches!(&err, GhError::NotAuthenticated(s) if s.contains("gh auth login")),
            "{err:?}"
        );
        assert!(err.to_string().contains("run gh auth login"));
    }

    #[test]
    fn api_reports_rejected_for_other_failures() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let gh = fake_gh(
            dir.path(),
            r#"echo "gh: Not Found (HTTP 404)" >&2
exit 1"#,
        );

        let err = GhClient::new(gh).api("repos/o/missing").unwrap_err();

        assert_eq!(err, GhError::Rejected("gh: Not Found (HTTP 404)".into()));
    }

    #[test]
    fn api_reports_bad_credentials_and_rate_limits() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let gh = fake_gh(
            dir.path(),
            r#"echo "gh: Bad credentials (HTTP 401)" >&2
exit 1"#,
        );
        let err = GhClient::new(gh).api("user").unwrap_err();
        assert!(matches!(err, GhError::NotAuthenticated(_)), "{err:?}");

        let gh = fake_gh(
            dir.path(),
            r#"echo "gh: API rate limit exceeded for user ID 1. (HTTP 403)" >&2
exit 1"#,
        );
        let err = GhClient::new(gh).api("user").unwrap_err();
        assert_eq!(
            err.to_string(),
            "gh api failed: gh: API rate limit exceeded for user ID 1. (HTTP 403)"
        );
    }

    #[test]
    fn api_kills_gh_after_the_timeout() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let gh = fake_gh(dir.path(), "exec sleep 10");
        let mut client = GhClient::new(gh);
        client.timeout = Duration::from_millis(200);

        let started = Instant::now();
        assert_eq!(client.api("user"), Err(GhError::TimedOut(200)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn api_refuses_output_over_the_limit() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let gh = fake_gh(
            dir.path(),
            r#"echo '{"a":"0123456789012345678901234567890"}'"#,
        );
        let mut client = GhClient::new(gh);
        client.max_output = 16;
        assert_eq!(client.api("user"), Err(GhError::TooLarge(16)));

        // Positive control: the same answer fits a larger limit.
        client.max_output = 1024;
        assert_eq!(
            client.api("user"),
            Ok(json!({ "a": "0123456789012345678901234567890" }))
        );
    }

    const FAKE_TOKEN: &str = "fake-token-for-tests";

    #[test]
    fn token_goes_to_gh_env_never_argv_or_output() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let args = dir.path().join("args");
        let env = dir.path().join("env");
        let gh = fake_gh(
            dir.path(),
            &format!(
                "printf '%s\\n' \"$@\" > '{}'\nenv > '{}'\necho \"{{\\\"echo\\\":\\\"$GH_TOKEN\\\"}}\"",
                args.display(),
                env.display()
            ),
        );
        let mut client = GhClient::new(gh);
        client.token = GhToken::new(FAKE_TOKEN);

        let value = client.api("user").unwrap();

        assert_eq!(fs::read_to_string(&args).unwrap(), "api\nuser\n");
        let env = fs::read_to_string(&env).unwrap();
        assert!(
            env.lines().any(|l| l == format!("GH_TOKEN={FAKE_TOKEN}")),
            "GH_TOKEN not in the child env"
        );
        assert_eq!(value, json!({ "echo": "<redacted>" }));
        assert!(!format!("{client:?}").contains(FAKE_TOKEN));
    }

    #[test]
    fn error_text_never_echoes_the_token() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let gh = fake_gh(
            dir.path(),
            r#"echo "gh: bad token $GH_TOKEN (HTTP 403)" >&2; exit 1"#,
        );
        let mut client = GhClient::new(gh);
        client.token = GhToken::new(FAKE_TOKEN);

        let err = client.api("user").unwrap_err();

        // Positive control: the fake did print the token, and it was replaced.
        assert_eq!(
            err.to_string(),
            "gh api failed: gh: bad token <redacted> (HTTP 403)"
        );
        assert!(!format!("{err:?}").contains(FAKE_TOKEN));
    }

    #[test]
    fn api_rejects_paths_with_flags_or_spaces() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let gh = fake_gh(
            dir.path(),
            &format!("touch '{}'\necho '{{}}'", marker.display()),
        );
        let client = GhClient::new(gh);

        for path in ["-X", "--paginate", "repos/o r", "user;rm", "user\n", ""] {
            assert_eq!(
                client.api(path),
                Err(GhError::InvalidPath(path.into())),
                "path {path:?}"
            );
        }
        assert!(!marker.exists(), "gh ran for an invalid path");

        // Positive control: the same fake runs for a valid path.
        assert_eq!(client.api("user"), Ok(json!({})));
        assert!(marker.exists());
    }
}
