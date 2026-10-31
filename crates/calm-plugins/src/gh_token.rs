//! A GitHub token for the `gh` child process only. It comes from CALM's own env or a
//! `.env` file and goes to `gh` as `GH_TOKEN` in its env, never in argv, plugin JS, logs or
//! error text. Without one, `gh` uses its own login (`gh auth login`).

use std::fmt;
use std::path::{Path, PathBuf};

/// The keys read from env and `.env` files, in the order `gh` itself prefers them.
pub const TOKEN_KEYS: [&str; 2] = ["GH_TOKEN", "GITHUB_TOKEN"];
/// What a token is replaced with wherever text could carry it.
pub const REDACTED: &str = "<redacted>";

/// A token whose Debug and Display never show it.
#[derive(Clone, PartialEq, Eq)]
pub struct GhToken(String);

impl GhToken {
    /// `None` for an empty or blank value.
    pub fn new(value: impl Into<String>) -> Option<GhToken> {
        let value = value.into();
        (!value.trim().is_empty()).then_some(GhToken(value))
    }

    /// The secret itself. Only for the child env and for redaction.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// `text` with every copy of the token replaced by `<redacted>`.
    pub fn redact(&self, text: &str) -> String {
        text.replace(&self.0, REDACTED)
    }
}

impl fmt::Debug for GhToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GhToken({REDACTED})")
    }
}

impl fmt::Display for GhToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// Where the token came from. Safe to log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    /// `GH_TOKEN` or `GITHUB_TOKEN` in CALM's process env (the child inherits it anyway).
    ProcessEnv(&'static str),
    /// A key in a `.env` file.
    File { path: PathBuf, key: &'static str },
    /// None found: `gh` uses its own login.
    GhLogin,
}

/// A resolved token plus warnings that name paths and line numbers only.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenResolution {
    pub token: Option<GhToken>,
    pub source: TokenSource,
    pub warnings: Vec<String>,
}

/// The token keys a dotenv text sets, plus the 1-based numbers of lines that do not parse.
/// Other keys are skipped and never returned.
pub fn parse_dotenv(text: &str) -> (Vec<(&'static str, String)>, Vec<usize>) {
    let mut found = Vec::new();
    let mut malformed = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);
        let Some((key, value)) = line.split_once('=') else {
            malformed.push(index + 1);
            continue;
        };
        let key = key.trim();
        let Some(value) = (is_key(key)).then(|| dotenv_value(value.trim())).flatten() else {
            malformed.push(index + 1);
            continue;
        };
        if let Some(known) = TOKEN_KEYS.iter().find(|k| **k == key) {
            found.push((*known, value));
        }
    }
    (found, malformed)
}

fn is_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

// Quoted values are taken as is; unquoted ones lose a trailing ` # comment`.
fn dotenv_value(value: &str) -> Option<String> {
    for quote in ['"', '\''] {
        if let Some(rest) = value.strip_prefix(quote) {
            let (inner, after) = rest.split_once(quote)?;
            let after = after.trim();
            return (after.is_empty() || after.starts_with('#')).then(|| inner.to_string());
        }
    }
    let value = match value.find(" #") {
        Some(at) => &value[..at],
        None => value,
    };
    Some(value.trim_end().to_string())
}

/// Picks the token: `GH_TOKEN` then `GITHUB_TOKEN` from `env`, then each file in order
/// (`GH_TOKEN` before `GITHUB_TOKEN` within one file). Missing files are skipped quietly.
pub fn resolve_token(env: impl Fn(&str) -> Option<String>, files: &[PathBuf]) -> TokenResolution {
    let mut warnings = Vec::new();
    for key in TOKEN_KEYS {
        if let Some(token) = env(key).and_then(GhToken::new) {
            return TokenResolution {
                token: Some(token),
                source: TokenSource::ProcessEnv(key),
                warnings,
            };
        }
    }
    for path in files {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                warnings.push(format!("{}: not read: {}", path.display(), err.kind()));
                continue;
            }
        };
        if let Some(warning) = permission_warning(path) {
            warnings.push(warning);
        }
        let (found, malformed) = parse_dotenv(&text);
        for line in malformed {
            warnings.push(format!(
                "{}: line {line} is not KEY=VALUE, skipped",
                path.display()
            ));
        }
        for key in TOKEN_KEYS {
            let token = found
                .iter()
                .rev()
                .find(|(k, _)| *k == key)
                .and_then(|(_, v)| GhToken::new(v.clone()));
            if let Some(token) = token {
                return TokenResolution {
                    token: Some(token),
                    source: TokenSource::File {
                        path: path.clone(),
                        key,
                    },
                    warnings,
                };
            }
        }
    }
    TokenResolution {
        token: None,
        source: TokenSource::GhLogin,
        warnings,
    }
}

fn permission_warning(path: &Path) -> Option<String> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path).ok()?.permissions().mode();
    (mode & 0o044 != 0).then(|| {
        format!(
            "{} is readable by other users, run chmod 600 on it",
            path.display()
        )
    })
}

/// `$CALM_HOME/.env` (default `~/.config/calm/.env`), then in dev builds the workspace `.env`.
pub fn token_files() -> Vec<PathBuf> {
    let home = match std::env::var_os("CALM_HOME").filter(|v| !v.is_empty()) {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/calm"),
    };
    let mut files = vec![home.join(".env")];
    if cfg!(debug_assertions) {
        files.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env"));
    }
    files
}

/// Resolves from the real process env and `token_files()`.
pub fn load_token() -> TokenResolution {
    resolve_token(|key| std::env::var(key).ok(), &token_files())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // Plainly fake, so no scanner or reader mistakes it for a credential.
    const FAKE: &str = "fake-token-for-tests";

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn env_file(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    #[test]
    fn dotenv_parses_quotes_comments_and_export() {
        let text = "# comment\n\nOTHER=x\nexport GITHUB_TOKEN=\"a b\" # note\nGH_TOKEN='c#d'\n";
        let (found, malformed) = parse_dotenv(text);
        assert_eq!(
            found,
            vec![
                ("GITHUB_TOKEN", "a b".to_string()),
                ("GH_TOKEN", "c#d".to_string())
            ]
        );
        assert!(malformed.is_empty(), "{malformed:?}");

        let (found, _) = parse_dotenv("GITHUB_TOKEN=plain # trailing\nGH_TOKEN = spaced \n");
        assert_eq!(
            found,
            vec![
                ("GITHUB_TOKEN", "plain".to_string()),
                ("GH_TOKEN", "spaced".to_string())
            ]
        );
    }

    #[test]
    fn dotenv_reports_malformed_lines_by_number_only() {
        let text =
            "GITHUB_TOKEN=ok\nno equals here\n1BAD=x\nGH_TOKEN=\"unclosed\nGH_TOKEN=\"a\" junk\n";
        let (found, malformed) = parse_dotenv(text);
        assert_eq!(found, vec![("GITHUB_TOKEN", "ok".to_string())]);
        assert_eq!(malformed, vec![2, 3, 4, 5]);

        let dir = tempfile::tempdir().unwrap();
        let path = env_file(
            dir.path(),
            ".env",
            &format!("GITHUB_TOKEN={FAKE}\n{FAKE} oops\n"),
        );
        let resolved = resolve_token(no_env, &[path]);
        assert_eq!(resolved.warnings.len(), 1, "{:?}", resolved.warnings);
        assert!(resolved.warnings[0].ends_with("line 2 is not KEY=VALUE, skipped"));
        assert!(!format!("{resolved:?}").contains(FAKE));
    }

    #[test]
    fn precedence_is_process_env_then_files_in_order_then_gh_login() {
        let dir = tempfile::tempdir().unwrap();
        let home = env_file(dir.path(), "home.env", "GITHUB_TOKEN=from-home\n");
        let workspace = env_file(dir.path(), "ws.env", "GITHUB_TOKEN=from-workspace\n");
        let files = vec![home.clone(), workspace.clone()];
        let token = |r: &TokenResolution| r.token.as_ref().map(|t| t.expose().to_string());

        let env = |key: &str| match key {
            "GH_TOKEN" => Some("from-gh-env".to_string()),
            "GITHUB_TOKEN" => Some("from-github-env".to_string()),
            _ => None,
        };
        let r = resolve_token(env, &files);
        assert_eq!(token(&r).as_deref(), Some("from-gh-env"));
        assert_eq!(r.source, TokenSource::ProcessEnv("GH_TOKEN"));

        let env = |key: &str| (key == "GITHUB_TOKEN").then(|| "from-github-env".to_string());
        let r = resolve_token(env, &files);
        assert_eq!(token(&r).as_deref(), Some("from-github-env"));

        // A blank env value counts as unset.
        let r = resolve_token(|_| Some(" ".to_string()), &files);
        assert_eq!(token(&r).as_deref(), Some("from-home"));
        assert_eq!(
            r.source,
            TokenSource::File {
                path: home.clone(),
                key: "GITHUB_TOKEN"
            }
        );

        let missing = dir.path().join("missing.env");
        let r = resolve_token(no_env, &[missing.clone(), workspace.clone()]);
        assert_eq!(token(&r).as_deref(), Some("from-workspace"));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);

        // GH_TOKEN wins inside one file, as it does for gh.
        let both = env_file(dir.path(), "both.env", "GITHUB_TOKEN=b\nGH_TOKEN=a\n");
        assert_eq!(token(&resolve_token(no_env, &[both])).as_deref(), Some("a"));

        let r = resolve_token(no_env, &[missing]);
        assert_eq!((r.token, r.source), (None, TokenSource::GhLogin));
    }

    #[test]
    fn loose_file_permissions_warn_but_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = env_file(dir.path(), ".env", &format!("GITHUB_TOKEN={FAKE}\n"));
        // Positive control: 0600 gives no warning.
        assert!(
            resolve_token(no_env, std::slice::from_ref(&path))
                .warnings
                .is_empty()
        );

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let r = resolve_token(no_env, std::slice::from_ref(&path));
        assert_eq!(r.token.as_ref().map(GhToken::expose), Some(FAKE));
        assert_eq!(
            r.warnings,
            vec![format!(
                "{} is readable by other users, run chmod 600 on it",
                path.display()
            )]
        );
    }

    #[test]
    fn debug_and_display_never_show_the_token() {
        let token = GhToken::new(FAKE).unwrap();
        assert_eq!(format!("{token}"), REDACTED);
        assert_eq!(format!("{token:?}"), "GhToken(<redacted>)");
        let resolution = TokenResolution {
            token: Some(token.clone()),
            source: TokenSource::ProcessEnv("GH_TOKEN"),
            warnings: vec![],
        };
        assert!(!format!("{resolution:?}").contains(FAKE));
        assert!(!format!("{resolution:#?}").contains(FAKE));
        assert_eq!(
            token.redact(&format!("x {FAKE} y {FAKE}")),
            "x <redacted> y <redacted>"
        );
        assert_eq!(GhToken::new("  "), None);
    }
}
