//! Plugin discovery, the JS bridge protocol and the host side GitHub client.

pub mod assets;
pub mod bridge;
pub mod contributes;
pub mod github;
pub mod manifest;
pub mod tui;

pub use assets::missing_scripts;
pub use bridge::{
    BRIDGE_METHODS, BridgeEvent, BridgeRequest, BridgeResponse, CALM_PRELUDE_JS, is_known_method,
};
pub use github::{GhClient, GhError};
pub use manifest::{
    Discovery, InstalledPlugin, NO_BUILTIN_ENV, PluginContent, PluginError, PluginManifest,
    PluginSearch, Shadowed, Size, discover, discover_search, plugin_dirs, plugin_search,
    workspace_plugins_dir,
};
pub use tui::{DEFAULT_TUI_SIZE, TuiIcon, TuiRestart, TuiSpec, TuiTerminalColors, parse_hex_color};
