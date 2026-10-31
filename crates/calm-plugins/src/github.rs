use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;
use thiserror::Error;

/// Runs `gh api` on the host. The GitHub token stays inside `gh` and never passes through CALM.
#[derive(Debug, Clone, PartialEq)]
pub struct GhClient {
    pub program: PathBuf,
}

impl Default for GhClient {
    fn default() -> Self {
        Self {
            program: PathBuf::from("gh"),
        }
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
}

impl GhClient {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    /// Runs `<program> api <path>` and parses stdout as JSON.
    pub fn api(&self, path: &str) -> Result<Value, GhError> {
        if !is_valid_path(path) {
            return Err(GhError::InvalidPath(path.to_string()));
        }
        let output = match Command::new(&self.program).arg("api").arg(path).output() {
            Ok(output) => output,
            Err(err) if err.kind() == ErrorKind::NotFound => return Err(GhError::NotInstalled),
            Err(err) => return Err(GhError::Rejected(err.to_string())),
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.contains("gh auth login") || stderr.contains("not logged") {
                return Err(GhError::NotAuthenticated(stderr));
            }
            return Err(GhError::Rejected(stderr));
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|err| GhError::Rejected(format!("gh returned invalid JSON: {err}")))
    }
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

    fn fake_gh(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("gh");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn api_runs_gh_and_parses_json() {
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
    fn api_rejects_paths_with_flags_or_spaces() {
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
