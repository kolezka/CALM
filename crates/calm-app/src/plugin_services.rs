//! Bridge methods that block on the host: `gh api`, plugin storage files, `open` and
//! `process.exec`. `parse` only reads params and runs on the UI thread. `Services::run`
//! does the work and runs on the background executor, so a slow child never stalls a frame.

use std::io::Read;
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use calm_plugins::{ApiError, ErrorCode, GhClient, GhError};
use serde_json::{Map, Value};

/// Longest storage key, in bytes.
pub const STORAGE_MAX_KEY: usize = 256;
/// Largest plugin data file after a write.
pub const STORAGE_MAX_FILE: usize = 5 * 1024 * 1024;
/// Longest URL `openExternal` accepts.
const MAX_URL: usize = 4096;
/// `process.exec` without `timeoutMs`, from the spec.
pub const EXEC_DEFAULT_TIMEOUT_MS: u64 = 30_000;
/// Longest `timeoutMs` a plugin may ask for.
pub const EXEC_MAX_TIMEOUT_MS: u64 = 600_000;
/// Cap on stdout and on stderr, each, from the spec. Beyond it the call is `too_large`.
pub const EXEC_MAX_OUTPUT: usize = 4 * 1024 * 1024;

// One lock for every plugin data file: a get, set or remove reads and rewrites the whole
// file, and two of them racing on the background executor would lose a write.
static STORAGE_LOCK: Mutex<()> = Mutex::new(());

/// A bridge call that is served off the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceCall {
    GithubApi(String),
    StorageGet {
        plugin: String,
        key: String,
    },
    StorageSet {
        plugin: String,
        key: String,
        value: Value,
    },
    StorageRemove {
        plugin: String,
        key: String,
    },
    OpenExternal(String),
    ProcessExec(ExecSpec),
}

/// One `process.exec`: the program runs directly, never through a shell.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecSpec {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub timeout: Duration,
}

/// What the background calls run against. Tests and the smoke swap in fakes.
#[derive(Debug, Clone)]
pub struct Services {
    pub gh: GhClient,
    /// Holds `<plugin-id>.json` per plugin.
    pub data_dir: PathBuf,
    /// Run as `<opener> <url>`.
    pub opener: PathBuf,
}

impl Default for Services {
    /// Loads the `gh` token from env or `.env` (see `calm_plugins::gh_token`). Logs the
    /// source and any warnings, which name paths and line numbers, never the token.
    fn default() -> Self {
        let resolved = calm_plugins::load_token();
        for warning in &resolved.warnings {
            eprintln!("calm: gh token: {warning}");
        }
        match &resolved.source {
            calm_plugins::TokenSource::ProcessEnv(key) => {
                eprintln!("calm: gh token from {key} in the environment")
            }
            calm_plugins::TokenSource::File { path, key } => {
                eprintln!("calm: gh token from {key} in {}", path.display())
            }
            calm_plugins::TokenSource::GhLogin => {}
        }
        Self {
            gh: GhClient {
                token: resolved.token,
                ..GhClient::default()
            },
            data_dir: plugin_data_dir(),
            opener: PathBuf::from("/usr/bin/open"),
        }
    }
}

/// `$CALM_HOME/plugin-data` when `CALM_HOME` is set, else `$HOME/.config/calm/plugin-data`.
pub fn plugin_data_dir() -> PathBuf {
    let root = match std::env::var_os("CALM_HOME").filter(|v| !v.is_empty()) {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/calm"),
    };
    root.join("plugin-data")
}

/// `None` when `method` is not one of these services. `plugin_id` is the calling panel's
/// plugin, `None` for a panel that is not a plugin.
pub fn parse(
    plugin_id: Option<&str>,
    method: &str,
    params: &Value,
) -> Option<Result<ServiceCall, ApiError>> {
    let call = match method {
        "github.api" => string_param(params, "path").map(ServiceCall::GithubApi),
        "storage.get" | "storage.set" | "storage.remove" => storage_call(plugin_id, method, params),
        "openExternal" => string_param(params, "url")
            .and_then(|url| check_url(&url).map(|()| ServiceCall::OpenExternal(url))),
        "process.exec" => exec_spec(params).map(ServiceCall::ProcessExec),
        _ => return None,
    };
    Some(call)
}

fn exec_spec(params: &Value) -> Result<ExecSpec, ApiError> {
    let command = string_param(params, "command")?;
    if command.is_empty() || command.contains('\0') {
        return Err(ApiError::invalid_params(
            "command",
            "must be a non-empty string without NUL",
        ));
    }
    let args = match params.get("args") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, item)| match item.as_str() {
                Some(arg) if !arg.contains('\0') => Ok(arg.to_string()),
                _ => Err(ApiError::invalid_params(
                    &format!("args[{index}]"),
                    "must be a string without NUL",
                )),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(ApiError::invalid_params("args", "must be an array")),
    };
    let cwd = match params.get("cwd") {
        None | Some(Value::Null) => None,
        Some(Value::String(cwd)) if Path::new(cwd).is_absolute() => Some(PathBuf::from(cwd)),
        Some(_) => return Err(ApiError::invalid_params("cwd", "must be an absolute path")),
    };
    let timeout_ms = match params.get("timeoutMs") {
        None | Some(Value::Null) => EXEC_DEFAULT_TIMEOUT_MS,
        Some(value) => value
            .as_u64()
            .filter(|ms| (1..=EXEC_MAX_TIMEOUT_MS).contains(ms))
            .ok_or_else(|| {
                ApiError::invalid_params(
                    "timeoutMs",
                    &format!("must be an integer from 1 to {EXEC_MAX_TIMEOUT_MS}"),
                )
            })?,
    };
    Ok(ExecSpec {
        command,
        args,
        cwd,
        timeout: Duration::from_millis(timeout_ms),
    })
}

/// Runs `spec` and answers `{ code, stdout, stderr }`. The child gets its own process
/// group, so a timeout also ends what it started. A child killed by signal `n` reports
/// `128 + n`, as a shell does.
fn run_exec(spec: &ExecSpec, max_output: usize) -> Result<Value, ApiError> {
    if let Some(cwd) = &spec.cwd
        && !cwd.is_dir()
    {
        return Err(ApiError::invalid_params(
            "cwd",
            &format!("{} is not a directory", cwd.display()),
        ));
    }
    let mut command = Command::new(&spec.command);
    command
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    let mut child = command.spawn().map_err(|err| {
        ApiError::host_error(format!("process.exec: cannot run {}: {err}", spec.command))
    })?;
    let group = child.id();
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        kill_group(group);
        let _ = child.wait();
        return Err(ApiError::host_error("process.exec: pipes were not opened"));
    };
    // One past the cap tells an oversized stream apart from one that fits exactly.
    let limit = max_output as u64 + 1;
    let (sender, receiver) = mpsc::channel();
    for (stream, reader) in [
        (0, Box::new(stdout) as Box<dyn Read + Send>),
        (1, Box::new(stderr)),
    ] {
        let sender = sender.clone();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = reader.take(limit).read_to_end(&mut buf);
            let _ = sender.send((stream, buf));
        });
    }
    drop(sender);
    let deadline = Instant::now() + spec.timeout;
    let timed_out = |child: &mut std::process::Child| {
        kill_group(group);
        let _ = child.wait();
        ApiError::new(
            ErrorCode::Timeout,
            format!(
                "process.exec: {} ran past {} ms and was killed",
                spec.command,
                spec.timeout.as_millis()
            ),
        )
    };
    // A stream over the cap ends the call at once, whether or not the child has exited.
    let mut outputs: [Option<Vec<u8>>; 2] = [None, None];
    let too_large = |stream: usize, child: &mut std::process::Child| {
        kill_group(group);
        let _ = child.wait();
        let name = if stream == 0 { "stdout" } else { "stderr" };
        ApiError::new(
            ErrorCode::TooLarge,
            format!("process.exec: {name} is over the {max_output} byte limit"),
        )
    };
    let status = loop {
        while let Ok((stream, buf)) = receiver.try_recv() {
            if buf.len() > max_output {
                return Err(too_large(stream, &mut child));
            }
            outputs[stream] = Some(buf);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => return Err(timed_out(&mut child)),
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(err) => {
                kill_group(group);
                return Err(ApiError::host_error(format!("process.exec: {err}")));
            }
        }
    };
    // A grandchild can hold the pipes open after the child exits; it shares the deadline.
    while outputs.iter().any(Option::is_none) {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok((stream, buf)) if buf.len() > max_output => {
                return Err(too_large(stream, &mut child));
            }
            Ok((stream, buf)) => outputs[stream] = Some(buf),
            Err(_) => return Err(timed_out(&mut child)),
        }
    }
    let [Some(stdout), Some(stderr)] = outputs else {
        unreachable!("both streams arrived");
    };
    let code = status
        .code()
        .or_else(|| status.signal().map(|signal| 128 + signal))
        .ok_or_else(|| ApiError::host_error(format!("process.exec: no exit status: {status}")))?;
    Ok(serde_json::json!({
        "code": code,
        "stdout": String::from_utf8_lossy(&stdout),
        "stderr": String::from_utf8_lossy(&stderr),
    }))
}

/// `kill -KILL -<pgid>` through `/bin/kill`, which needs no unsafe code in this crate.
fn kill_group(group: u32) {
    let killed = Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{group}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Err(err) = killed {
        eprintln!("calm: process.exec: could not kill process group {group}: {err}");
    }
}

fn string_param(params: &Value, field: &str) -> Result<String, ApiError> {
    params
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ApiError::invalid_params(field, "must be a string"))
}

fn storage_call(
    plugin_id: Option<&str>,
    method: &str,
    params: &Value,
) -> Result<ServiceCall, ApiError> {
    let plugin = plugin_id
        .filter(|id| is_file_safe(id))
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::WrongKind,
                format!("{method}: not a plugin panel"),
            )
        })?
        .to_string();
    let key = string_param(params, "key")?;
    if key.is_empty() || key.len() > STORAGE_MAX_KEY {
        return Err(ApiError::invalid_params(
            "key",
            &format!("must be 1 to {STORAGE_MAX_KEY} bytes"),
        ));
    }
    Ok(match method {
        "storage.get" => ServiceCall::StorageGet { plugin, key },
        "storage.set" => {
            // A missing value is `set(k, undefined)` after JSON; storing null would erase `k`.
            let value = params
                .get("value")
                .cloned()
                .ok_or_else(|| ApiError::invalid_params("value", "is required"))?;
            ServiceCall::StorageSet { plugin, key, value }
        }
        _ => ServiceCall::StorageRemove { plugin, key },
    })
}

// Manifest ids are already restricted; this keeps a bad one from naming another path.
fn is_file_safe(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn check_url(url: &str) -> Result<(), ApiError> {
    let lower = url.to_ascii_lowercase();
    let has_host = ["http://", "https://"]
        .iter()
        .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len());
    if !has_host || url.len() > MAX_URL || url.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(ApiError::invalid_params(
            "url",
            &format!(
                "only http and https URLs open, got {:?}",
                url.chars().take(80).collect::<String>()
            ),
        ));
    }
    Ok(())
}

impl Services {
    /// Blocks. Call it on the background executor.
    pub fn run(&self, call: ServiceCall) -> Result<Value, ApiError> {
        match call {
            ServiceCall::GithubApi(path) => self.gh.api(&path).map_err(|err| gh_error(&err)),
            ServiceCall::StorageGet { plugin, key } => {
                let _guard = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                let data = read_data(&self.data_file(&plugin), "storage.get")?;
                Ok(data.get(&key).cloned().unwrap_or(Value::Null))
            }
            ServiceCall::StorageSet { plugin, key, value } => {
                let _guard = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                let path = self.data_file(&plugin);
                let mut data = read_data(&path, "storage.set")?;
                data.insert(key, value);
                write_data(&path, &data, "storage.set")?;
                Ok(Value::Null)
            }
            ServiceCall::StorageRemove { plugin, key } => {
                let _guard = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                let path = self.data_file(&plugin);
                let mut data = read_data(&path, "storage.remove")?;
                if data.remove(&key).is_some() {
                    write_data(&path, &data, "storage.remove")?;
                }
                Ok(Value::Null)
            }
            ServiceCall::OpenExternal(url) => {
                let status = Command::new(&self.opener)
                    .arg(&url)
                    .status()
                    .map_err(|err| {
                        ApiError::host_error(format!(
                            "openExternal: {}: {err}",
                            self.opener.display()
                        ))
                    })?;
                if !status.success() {
                    return Err(ApiError::host_error(format!(
                        "openExternal: open exited with {status}"
                    )));
                }
                Ok(Value::Null)
            }
            ServiceCall::ProcessExec(spec) => run_exec(&spec, EXEC_MAX_OUTPUT),
        }
    }

    fn data_file(&self, plugin: &str) -> PathBuf {
        self.data_dir.join(format!("{plugin}.json"))
    }
}

/// The `gh` client's failures, each with its own code. The message is the client's text,
/// which is already redacted.
fn gh_error(err: &GhError) -> ApiError {
    let code = match err {
        GhError::NotInstalled => ErrorCode::GhNotInstalled,
        GhError::NotAuthenticated(_) => ErrorCode::GhNotLoggedIn,
        GhError::Rejected(_) => ErrorCode::GhFailed,
        GhError::InvalidPath(_) => ErrorCode::GhInvalidPath,
        GhError::TimedOut(_) => ErrorCode::Timeout,
        GhError::TooLarge(_) => ErrorCode::TooLarge,
    };
    ApiError::new(code, err.to_string())
}

/// A missing file is empty data. A file that does not parse is an error, never `{}`, so the
/// next write cannot wipe it.
fn read_data(path: &Path, method: &str) -> Result<Map<String, Value>, ApiError> {
    let text = match std::fs::read(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(err) => {
            return Err(ApiError::host_error(format!(
                "{method}: {}: {err}",
                path.display()
            )));
        }
    };
    let corrupt = |why: String| {
        ApiError::new(
            ErrorCode::StorageCorrupt,
            format!("{method}: plugin data {} {why}", path.display()),
        )
    };
    match serde_json::from_slice(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(corrupt("is not a JSON object".to_string())),
        Err(err) => Err(corrupt(format!("does not parse: {err}"))),
    }
}

/// Writes a temp file next to `path`, then renames it over, so a crash never leaves half a file.
fn write_data(path: &Path, data: &Map<String, Value>, method: &str) -> Result<(), ApiError> {
    use std::io::Write as _;
    let text =
        serde_json::to_vec(data).map_err(|err| ApiError::host_error(format!("{method}: {err}")))?;
    if text.len() > STORAGE_MAX_FILE {
        return Err(ApiError::new(
            ErrorCode::TooLarge,
            format!(
                "{method}: plugin data would be {} bytes, over the {STORAGE_MAX_FILE} byte limit",
                text.len()
            ),
        ));
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let fail =
        |err: std::io::Error| ApiError::host_error(format!("{method}: {}: {err}", path.display()));
    std::fs::create_dir_all(dir).map_err(fail)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(fail)?;
    temp.write_all(&text).map_err(fail)?;
    temp.as_file().sync_all().map_err(fail)?;
    temp.persist(path).map_err(|err| fail(err.error))?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // Same reason as in calm-plugins: Linux can report ETXTBSY when parallel tests exec a
    // script while another writes one.
    pub(crate) static FAKE_GH_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn fake_program(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn services(dir: &Path) -> Services {
        Services {
            gh: GhClient::new(dir.join("no-gh")),
            data_dir: dir.join("plugin-data"),
            opener: dir.join("no-open"),
        }
    }

    fn storage(services: &Services, method: &str, params: Value) -> Result<Value, ApiError> {
        let call = parse(Some("github-prs"), method, &params).unwrap()?;
        services.run(call)
    }

    #[test]
    fn parse_leaves_other_methods_alone_and_checks_params() {
        assert_eq!(parse(Some("p"), "settings.get", &Value::Null), None);
        assert_eq!(
            parse(Some("p"), "github.api", &json!({ "path": "user" })),
            Some(Ok(ServiceCall::GithubApi("user".into())))
        );
        assert_eq!(
            parse(Some("p"), "github.api", &Value::Null),
            Some(Err(ApiError::invalid_params("path", "must be a string")))
        );
        assert_eq!(
            parse(None, "storage.get", &json!({ "key": "k" })),
            Some(Err(ApiError::new(
                ErrorCode::WrongKind,
                "storage.get: not a plugin panel"
            )))
        );
        assert!(matches!(
            parse(Some("../x"), "storage.get", &json!({ "key": "k" })),
            Some(Err(e)) if e.code == ErrorCode::WrongKind
        ));
        let long = "k".repeat(STORAGE_MAX_KEY + 1);
        assert!(matches!(
            parse(Some("p"), "storage.set", &json!({ "key": long, "value": 1 })),
            Some(Err(e)) if e.code == ErrorCode::InvalidParams && e.message.starts_with("key: must be")
        ));
    }

    #[test]
    fn gh_errors_map_to_their_codes() {
        let cases = [
            (GhError::NotInstalled, ErrorCode::GhNotInstalled),
            (
                GhError::NotAuthenticated("x".into()),
                ErrorCode::GhNotLoggedIn,
            ),
            (GhError::Rejected("x".into()), ErrorCode::GhFailed),
            (GhError::InvalidPath("--x".into()), ErrorCode::GhInvalidPath),
            (GhError::TimedOut(5), ErrorCode::Timeout),
            (GhError::TooLarge(9), ErrorCode::TooLarge),
        ];
        for (err, code) in cases {
            let api = gh_error(&err);
            assert_eq!(api.code, code, "{err:?}");
            assert_eq!(api.message, err.to_string());
        }
    }

    #[test]
    fn storage_set_needs_an_explicit_value() {
        let dir = tempfile::tempdir().unwrap();
        let services = services(dir.path());
        let set = |params| storage(&services, "storage.set", params);
        assert_eq!(set(json!({ "key": "a", "value": 1 })), Ok(Value::Null));
        // `calm.storage.set('a', undefined)` arrives without `value`; it must not erase `a`.
        let missing = set(json!({ "key": "a" }));
        assert_eq!(
            missing,
            Err(ApiError::invalid_params("value", "is required"))
        );
        assert_eq!(
            storage(&services, "storage.get", json!({ "key": "a" })),
            Ok(json!(1))
        );
        // An explicit null is a real value and is stored.
        assert_eq!(set(json!({ "key": "a", "value": null })), Ok(Value::Null));
        let file = dir.path().join("plugin-data/github-prs.json");
        let on_disk: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(on_disk, json!({ "a": null }));
    }

    #[test]
    fn storage_file_that_is_not_an_object_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let services = services(dir.path());
        fs::create_dir_all(&services.data_dir).unwrap();
        fs::write(services.data_dir.join("github-prs.json"), "[1]").unwrap();
        let get = storage(&services, "storage.get", json!({ "key": "a" }));
        assert!(
            matches!(&get, Err(e) if e.code == ErrorCode::StorageCorrupt && e.message.contains("not a JSON object")),
            "{get:?}"
        );
    }

    #[test]
    fn storage_round_trips_per_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let services = services(dir.path());
        assert_eq!(
            storage(&services, "storage.get", json!({ "key": "a" })),
            Ok(Value::Null)
        );
        let value = json!({ "repos": ["inkitt/flash"], "n": 2 });
        assert_eq!(
            storage(
                &services,
                "storage.set",
                json!({ "key": "a", "value": value })
            ),
            Ok(Value::Null)
        );
        assert_eq!(
            storage(&services, "storage.get", json!({ "key": "a" })),
            Ok(value)
        );
        let file = dir.path().join("plugin-data/github-prs.json");
        let on_disk: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(on_disk["a"]["n"], json!(2));
        assert_eq!(
            storage(&services, "storage.remove", json!({ "key": "a" })),
            Ok(Value::Null)
        );
        assert_eq!(
            storage(&services, "storage.get", json!({ "key": "a" })),
            Ok(Value::Null)
        );
        // No temp files left next to the data file.
        let names: Vec<_> = fs::read_dir(dir.path().join("plugin-data"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("github-prs.json")]);
    }

    #[test]
    fn storage_refuses_a_corrupt_file_and_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let services = services(dir.path());
        // Positive control: a valid file reads.
        fs::create_dir_all(&services.data_dir).unwrap();
        let file = services.data_dir.join("github-prs.json");
        fs::write(&file, r#"{"a":1}"#).unwrap();
        assert_eq!(
            storage(&services, "storage.get", json!({ "key": "a" })),
            Ok(json!(1))
        );

        fs::write(&file, "{not json").unwrap();
        let get = storage(&services, "storage.get", json!({ "key": "a" }));
        assert!(
            matches!(&get, Err(e) if e.code == ErrorCode::StorageCorrupt && e.message.contains("does not parse")),
            "{get:?}"
        );
        let set = storage(&services, "storage.set", json!({ "key": "b", "value": 2 }));
        assert!(
            matches!(&set, Err(e) if e.code == ErrorCode::StorageCorrupt && e.message.contains("does not parse")),
            "{set:?}"
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), "{not json");
    }

    #[test]
    fn storage_refuses_a_file_over_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let services = services(dir.path());
        let big = "x".repeat(STORAGE_MAX_FILE);
        let set = storage(
            &services,
            "storage.set",
            json!({ "key": "a", "value": big }),
        );
        assert!(
            matches!(&set, Err(e) if e.code == ErrorCode::TooLarge && e.message.contains("byte limit")),
            "{set:?}"
        );
        assert!(!services.data_dir.join("github-prs.json").exists());
    }

    fn exec(params: Value) -> Result<Value, ApiError> {
        let Some(Ok(ServiceCall::ProcessExec(spec))) = parse(Some("p"), "process.exec", &params)
        else {
            panic!("not an exec call: {params}");
        };
        run_exec(&spec, EXEC_MAX_OUTPUT)
    }

    #[test]
    fn process_exec_runs_without_a_shell_and_reports_code_and_output() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        // `$0` and the quotes reach the program as literal argv, so no shell parsed them.
        let result = exec(json!({
            "command": "/bin/sh",
            "args": ["-c", "pwd; printf '%s|' \"$@\"; echo oops >&2; exit 3", "sh", "a b", "$HOME"],
            "cwd": cwd.to_str().unwrap(),
            "timeoutMs": null,
        }))
        .unwrap();
        assert_eq!(
            result,
            json!({
                "code": 3,
                "stdout": format!("{}\na b|$HOME|", cwd.display()),
                "stderr": "oops\n",
            })
        );
    }

    #[test]
    fn process_exec_checks_its_params() {
        let bad = |params: Value, field: &str| {
            let got = parse(Some("p"), "process.exec", &params);
            assert!(
                matches!(&got, Some(Err(e)) if e.code == ErrorCode::InvalidParams && e.message.starts_with(field)),
                "{params}: {got:?}"
            );
        };
        bad(json!({ "args": [] }), "command");
        bad(json!({ "command": "" }), "command");
        bad(json!({ "command": "ls", "args": "-l" }), "args");
        bad(json!({ "command": "ls", "args": ["ok", 3] }), "args[1]");
        bad(json!({ "command": "ls", "cwd": "relative/dir" }), "cwd");
        bad(json!({ "command": "ls", "timeoutMs": 0 }), "timeoutMs");
        bad(json!({ "command": "ls", "timeoutMs": 1.5 }), "timeoutMs");
        bad(
            json!({ "command": "ls", "timeoutMs": EXEC_MAX_TIMEOUT_MS + 1 }),
            "timeoutMs",
        );
        let missing_dir = exec(json!({ "command": "/bin/pwd", "cwd": "/no/such/dir" }));
        assert!(
            matches!(&missing_dir, Err(e) if e.code == ErrorCode::InvalidParams && e.message.starts_with("cwd")),
            "{missing_dir:?}"
        );
        let not_found = exec(json!({ "command": "/no/such/program" }));
        assert!(
            matches!(&not_found, Err(e) if e.code == ErrorCode::HostError && e.message.contains("cannot run /no/such/program")),
            "{not_found:?}"
        );
    }

    #[test]
    fn process_exec_kills_the_whole_group_after_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("survived");
        // The grandchild writes the marker if it outlives the kill.
        let script = format!("(sleep 1; touch '{}') & sleep 5", marker.display());
        let started = Instant::now();
        let result =
            exec(json!({ "command": "/bin/sh", "args": ["-c", script], "timeoutMs": 200 }));
        assert!(
            matches!(&result, Err(e) if e.code == ErrorCode::Timeout && e.message.contains("200 ms")),
            "{result:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!marker.exists(), "a grandchild outlived the timeout");
    }

    #[test]
    fn process_exec_does_not_wait_for_a_grandchild_holding_the_pipes() {
        // The child exits at once; the background sleep keeps stdout open.
        let started = Instant::now();
        let result = exec(json!({
            "command": "/bin/sh",
            "args": ["-c", "sleep 5 & echo started"],
            "timeoutMs": 300,
        }));
        assert!(
            matches!(&result, Err(e) if e.code == ErrorCode::Timeout),
            "{result:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn process_exec_refuses_output_over_the_cap() {
        let spec = ExecSpec {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), "printf 12345".into()],
            cwd: None,
            timeout: Duration::from_secs(5),
        };
        // Positive control: exactly at the cap fits.
        assert_eq!(run_exec(&spec, 5).unwrap()["stdout"], json!("12345"));
        let over = run_exec(&spec, 4);
        assert!(
            matches!(&over, Err(e) if e.code == ErrorCode::TooLarge && e.message.contains("stdout")),
            "{over:?}"
        );
    }

    // Far more than a pipe buffer: the writer must not block until the timeout.
    #[test]
    fn process_exec_output_over_the_cap_is_too_large_not_a_timeout() {
        let spec = ExecSpec {
            command: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "dd if=/dev/zero bs=4096 count=256 2>/dev/null".into(),
            ],
            cwd: None,
            timeout: Duration::from_secs(10),
        };
        let started = Instant::now();
        let result = run_exec(&spec, 1024);
        assert!(
            matches!(&result, Err(e) if e.code == ErrorCode::TooLarge),
            "{result:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn process_exec_too_large_leaves_no_writer_running() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("writer.pid");
        // The writer runs in the background and ignores SIGPIPE, so only a kill stops it.
        let script = format!(
            "trap '' PIPE; (while :; do printf 'xxxxxxxxxxxxxxxx' 2>/dev/null; done) & echo $! > '{}'; wait",
            pid_file.display()
        );
        let spec = ExecSpec {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            cwd: None,
            timeout: Duration::from_secs(5),
        };
        let result = run_exec(&spec, 1024);
        assert!(
            matches!(&result, Err(e) if e.code == ErrorCode::TooLarge),
            "{result:?}"
        );
        let pid = fs::read_to_string(&pid_file).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let alive = Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .status()
            .unwrap()
            .success();
        if alive {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", pid.trim()])
                .status();
        }
        assert!(
            !alive,
            "the writer {} outlived a too_large reply",
            pid.trim()
        );
    }

    #[test]
    fn process_exec_reports_a_signal_as_128_plus_n() {
        let result =
            exec(json!({ "command": "/bin/sh", "args": ["-c", "kill -TERM $$"] })).unwrap();
        assert_eq!(result["code"], json!(128 + 15));
    }

    #[test]
    fn open_external_runs_the_opener_for_http_only() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("opened");
        let mut services = services(dir.path());
        services.opener = fake_program(
            dir.path(),
            "open",
            &format!("printf '%s\\n' \"$@\" >> '{}'", log.display()),
        );
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://",
            "https://a b",
            "-a Calculator",
            "mailto:x@y",
        ] {
            assert!(
                matches!(
                    parse(Some("p"), "openExternal", &json!({ "url": url })),
                    Some(Err(e)) if e.code == ErrorCode::InvalidParams
                ),
                "{url}"
            );
        }
        assert!(!log.exists(), "opener ran for a refused URL");

        let url = "https://github.com/inkitt/flash/pull/1";
        let call = parse(Some("p"), "openExternal", &json!({ "url": url }))
            .unwrap()
            .unwrap();
        assert_eq!(services.run(call), Ok(Value::Null));
        assert_eq!(fs::read_to_string(&log).unwrap(), format!("{url}\n"));
    }
}
