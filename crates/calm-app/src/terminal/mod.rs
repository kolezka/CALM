//! Live Ghostty terminals hosted in calm-native container views.

mod adapter;

use std::path::PathBuf;
use std::sync::Arc;

use gpui_libghostty::{TerminalConfiguration, TerminalOptions};

pub use adapter::{KEY_CONTEXT, TerminalEvent, TerminalPanel};

/// `command` and `cwd` come from the panel; missing ones fall back to `$SHELL` and `$HOME`.
pub fn options(command: Option<&str>, cwd: Option<PathBuf>) -> TerminalOptions {
    let command = command
        .map(str::to_string)
        .or_else(|| {
            std::env::var("SHELL")
                .ok()
                .filter(|shell| !shell.is_empty())
        })
        .unwrap_or_else(|| "/bin/zsh".to_string());
    let cwd = cwd
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"));
    let mut options = TerminalOptions::new(command, cwd);
    options.configuration = TerminalConfiguration::UserDefault;
    options.quiet_login = true;
    options.clipboard_approval = Some(Arc::new(|_| true));
    options.focus_on_spawn = true;
    options
}
