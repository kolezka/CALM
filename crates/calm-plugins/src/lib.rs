//! Plugin discovery, the JS bridge protocol and the host side GitHub client.

pub mod assets;
pub mod bridge;
pub mod gh_token;
pub mod github;
pub mod manifest;
pub mod tui;

pub use assets::missing_scripts;
pub use bridge::{
    BRIDGE_METHODS, BridgeEvent, BridgeRequest, BridgeResponse, CALM_PRELUDE_JS, is_known_method,
};
pub use gh_token::{GhToken, TokenResolution, TokenSource, load_token};
pub use github::{GH_MAX_OUTPUT, GH_TIMEOUT, GhClient, GhError};
pub use manifest::{
    Discovery, InstalledPlugin, NO_BUILTIN_ENV, PluginContent, PluginError, PluginManifest,
    PluginSearch, Shadowed, Size, discover, discover_search, plugin_dirs, plugin_search,
    workspace_plugins_dir,
};
pub use tui::{DEFAULT_TUI_SIZE, TuiRestart, TuiSpec};
