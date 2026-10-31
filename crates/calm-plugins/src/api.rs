//! Plugin API v2 types shared by the bridge, the host and later transports: the API
//! version, error codes, the caller and the capability words. Pure, no host state.

use std::fmt;

use calm_core::scene::PanelId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The plugin API version this host serves. A manifest above it is skipped.
pub const API_VERSION: u32 = 2;

/// Every reject code from the spec's error table. The wire word is `as_str()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UnknownMethod,
    NotImplemented,
    InvalidParams,
    NotFound,
    WrongKind,
    NotAlive,
    Busy,
    Cancelled,
    Unavailable,
    TooLarge,
    StorageCorrupt,
    GhNotInstalled,
    GhNotLoggedIn,
    GhFailed,
    GhInvalidPath,
    Timeout,
    NotAllowed,
    NotInstalled,
    NotRunning,
    MethodNotFound,
    RemoteError,
    HostError,
}

impl ErrorCode {
    pub const ALL: &'static [ErrorCode] = &[
        ErrorCode::UnknownMethod,
        ErrorCode::NotImplemented,
        ErrorCode::InvalidParams,
        ErrorCode::NotFound,
        ErrorCode::WrongKind,
        ErrorCode::NotAlive,
        ErrorCode::Busy,
        ErrorCode::Cancelled,
        ErrorCode::Unavailable,
        ErrorCode::TooLarge,
        ErrorCode::StorageCorrupt,
        ErrorCode::GhNotInstalled,
        ErrorCode::GhNotLoggedIn,
        ErrorCode::GhFailed,
        ErrorCode::GhInvalidPath,
        ErrorCode::Timeout,
        ErrorCode::NotAllowed,
        ErrorCode::NotInstalled,
        ErrorCode::NotRunning,
        ErrorCode::MethodNotFound,
        ErrorCode::RemoteError,
        ErrorCode::HostError,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::UnknownMethod => "unknown_method",
            ErrorCode::NotImplemented => "not_implemented",
            ErrorCode::InvalidParams => "invalid_params",
            ErrorCode::NotFound => "not_found",
            ErrorCode::WrongKind => "wrong_kind",
            ErrorCode::NotAlive => "not_alive",
            ErrorCode::Busy => "busy",
            ErrorCode::Cancelled => "cancelled",
            ErrorCode::Unavailable => "unavailable",
            ErrorCode::TooLarge => "too_large",
            ErrorCode::StorageCorrupt => "storage_corrupt",
            ErrorCode::GhNotInstalled => "gh_not_installed",
            ErrorCode::GhNotLoggedIn => "gh_not_logged_in",
            ErrorCode::GhFailed => "gh_failed",
            ErrorCode::GhInvalidPath => "gh_invalid_path",
            ErrorCode::Timeout => "timeout",
            ErrorCode::NotAllowed => "not_allowed",
            ErrorCode::NotInstalled => "not_installed",
            ErrorCode::NotRunning => "not_running",
            ErrorCode::MethodNotFound => "method_not_found",
            ErrorCode::RemoteError => "remote_error",
            ErrorCode::HostError => "host_error",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A reject as it crosses the bridge: `{ "code": "...", "message": "..." }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Error)]
#[error("{code}: {message}")]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The message names the field, as the spec requires.
    pub fn invalid_params(field: &str, why: &str) -> Self {
        Self::new(ErrorCode::InvalidParams, format!("{field}: {why}"))
    }

    pub fn not_found(id: PanelId) -> Self {
        Self::new(ErrorCode::NotFound, format!("no panel with id {}", id.0))
    }

    pub fn wrong_kind(id: PanelId, expected: &str) -> Self {
        Self::new(
            ErrorCode::WrongKind,
            format!("panel {} is not a {expected} panel", id.0),
        )
    }

    pub fn not_alive(id: PanelId) -> Self {
        Self::new(
            ErrorCode::NotAlive,
            format!("panel {}: the process has exited", id.0),
        )
    }

    pub fn unknown_method(method: &str) -> Self {
        Self::new(
            ErrorCode::UnknownMethod,
            format!("unknown method: {method}"),
        )
    }

    pub fn not_implemented(method: &str) -> Self {
        Self::new(
            ErrorCode::NotImplemented,
            format!("not implemented: {method}"),
        )
    }

    pub fn host_error(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::HostError, message)
    }
}

/// An external control client (phase 11), numbered by the transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub u64);

/// Who made a call. Set by the transport, never parsed from params, so no `Deserialize`.
#[derive(Debug, Clone, PartialEq)]
pub enum Caller {
    Panel {
        panel: PanelId,
        plugin_id: String,
    },
    /// A plugin's bun process.
    Backend {
        plugin_id: String,
    },
    External {
        client: ClientId,
    },
}

impl Caller {
    pub fn plugin_id(&self) -> Option<&str> {
        match self {
            Caller::Panel { plugin_id, .. } | Caller::Backend { plugin_id } => Some(plugin_id),
            Caller::External { .. } => None,
        }
    }
}

/// The words of the spec's capability table. Declarative: shown to the user, never a gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    PanelsCreate,
    PanelsControl,
    PanelsClose,
    CameraControl,
    TerminalRead,
    TerminalWrite,
    TerminalControl,
    BrowserControl,
    BrowserScript,
    BrowserData,
    PluginsCall,
    SettingsWrite,
    ProcessExec,
    Github,
    OpenExternal,
    Layouts,
    Backend,
}

impl Capability {
    pub const ALL: &'static [Capability] = &[
        Capability::PanelsCreate,
        Capability::PanelsControl,
        Capability::PanelsClose,
        Capability::CameraControl,
        Capability::TerminalRead,
        Capability::TerminalWrite,
        Capability::TerminalControl,
        Capability::BrowserControl,
        Capability::BrowserScript,
        Capability::BrowserData,
        Capability::PluginsCall,
        Capability::SettingsWrite,
        Capability::ProcessExec,
        Capability::Github,
        Capability::OpenExternal,
        Capability::Layouts,
        Capability::Backend,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Capability::PanelsCreate => "panels.create",
            Capability::PanelsControl => "panels.control",
            Capability::PanelsClose => "panels.close",
            Capability::CameraControl => "camera.control",
            Capability::TerminalRead => "terminal.read",
            Capability::TerminalWrite => "terminal.write",
            Capability::TerminalControl => "terminal.control",
            Capability::BrowserControl => "browser.control",
            Capability::BrowserScript => "browser.script",
            Capability::BrowserData => "browser.data",
            Capability::PluginsCall => "plugins.call",
            Capability::SettingsWrite => "settings.write",
            Capability::ProcessExec => "process.exec",
            Capability::Github => "github",
            Capability::OpenExternal => "open.external",
            Capability::Layouts => "layouts",
            Capability::Backend => "backend",
        }
    }

    pub fn parse(word: &str) -> Option<Capability> {
        Self::ALL.iter().copied().find(|c| c.as_str() == word)
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The words `Capability::parse` rejects, in manifest order, for the discovery warning.
pub fn unknown_capabilities(words: &[String]) -> Vec<String> {
    words
        .iter()
        .filter(|word| Capability::parse(word).is_none())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    /// The spec's error table, row by row.
    const ERROR_WORDS: &[(ErrorCode, &str)] = &[
        (ErrorCode::UnknownMethod, "unknown_method"),
        (ErrorCode::NotImplemented, "not_implemented"),
        (ErrorCode::InvalidParams, "invalid_params"),
        (ErrorCode::NotFound, "not_found"),
        (ErrorCode::WrongKind, "wrong_kind"),
        (ErrorCode::NotAlive, "not_alive"),
        (ErrorCode::Busy, "busy"),
        (ErrorCode::Cancelled, "cancelled"),
        (ErrorCode::Unavailable, "unavailable"),
        (ErrorCode::TooLarge, "too_large"),
        (ErrorCode::StorageCorrupt, "storage_corrupt"),
        (ErrorCode::GhNotInstalled, "gh_not_installed"),
        (ErrorCode::GhNotLoggedIn, "gh_not_logged_in"),
        (ErrorCode::GhFailed, "gh_failed"),
        (ErrorCode::GhInvalidPath, "gh_invalid_path"),
        (ErrorCode::Timeout, "timeout"),
        (ErrorCode::NotAllowed, "not_allowed"),
        (ErrorCode::NotInstalled, "not_installed"),
        (ErrorCode::NotRunning, "not_running"),
        (ErrorCode::MethodNotFound, "method_not_found"),
        (ErrorCode::RemoteError, "remote_error"),
        (ErrorCode::HostError, "host_error"),
    ];

    #[test]
    fn every_error_code_round_trips_to_its_wire_word() {
        assert_eq!(ERROR_WORDS.len(), ErrorCode::ALL.len());
        for (code, word) in ERROR_WORDS {
            assert_eq!(serde_json::to_value(code).unwrap(), json!(word), "{code:?}");
            assert_eq!(
                serde_json::from_value::<ErrorCode>(json!(word)).unwrap(),
                *code
            );
            assert_eq!(code.as_str(), *word);
            assert_eq!(code.to_string(), *word);
        }
        let unique: HashSet<_> = ErrorCode::ALL.iter().collect();
        assert_eq!(unique.len(), ErrorCode::ALL.len());
    }

    #[test]
    fn api_error_serializes_code_and_message() {
        assert_eq!(
            serde_json::to_value(ApiError::invalid_params("title", "must be a string")).unwrap(),
            json!({ "code": "invalid_params", "message": "title: must be a string" })
        );
        assert_eq!(
            serde_json::to_value(ApiError::not_found(PanelId(42))).unwrap(),
            json!({ "code": "not_found", "message": "no panel with id 42" })
        );
        assert_eq!(
            ApiError::not_implemented("panel.close").to_string(),
            "not_implemented: not implemented: panel.close"
        );
        assert_eq!(
            ApiError::unknown_method("nope").code,
            ErrorCode::UnknownMethod
        );
    }

    #[test]
    fn caller_names_its_plugin() {
        let panel = Caller::Panel {
            panel: PanelId(1),
            plugin_id: "a".into(),
        };
        assert_eq!(panel.plugin_id(), Some("a"));
        let backend = Caller::Backend {
            plugin_id: "b".into(),
        };
        assert_eq!(backend.plugin_id(), Some("b"));
        let external = Caller::External {
            client: ClientId(3),
        };
        assert_eq!(external.plugin_id(), None);
    }

    /// The spec's capability table, row by row.
    const CAPABILITY_WORDS: &[&str] = &[
        "panels.create",
        "panels.control",
        "panels.close",
        "camera.control",
        "terminal.read",
        "terminal.write",
        "terminal.control",
        "browser.control",
        "browser.script",
        "browser.data",
        "plugins.call",
        "settings.write",
        "process.exec",
        "github",
        "open.external",
        "layouts",
        "backend",
    ];

    #[test]
    fn every_capability_word_parses_and_round_trips() {
        assert_eq!(CAPABILITY_WORDS.len(), Capability::ALL.len());
        for word in CAPABILITY_WORDS {
            let capability = Capability::parse(word).unwrap_or_else(|| panic!("{word}"));
            assert_eq!(capability.as_str(), *word);
        }
        let unique: HashSet<_> = Capability::ALL.iter().collect();
        assert_eq!(unique.len(), Capability::ALL.len());
    }

    #[test]
    fn unknown_capabilities_returns_typos_only() {
        let words = vec![
            "panels.create".to_string(),
            "panel.create".to_string(),
            "github".to_string(),
            "Github".to_string(),
        ];
        assert_eq!(
            unknown_capabilities(&words),
            vec!["panel.create".to_string(), "Github".to_string()]
        );
        assert!(unknown_capabilities(&[]).is_empty());
    }
}
