//! Plugin discovery, the JS bridge protocol and the host side GitHub client.

pub mod assets;
pub mod bridge;
pub mod github;
pub mod manifest;
pub mod tui;

pub use assets::missing_scripts;
pub use bridge::{
    BRIDGE_METHODS, BridgeEvent, BridgeRequest, BridgeResponse, CALM_PRELUDE_JS, is_known_method,
};
pub use github::{GhClient, GhError};
pub use manifest::{
    Discovery, InstalledPlugin, PluginContent, PluginError, PluginManifest, Size, discover,
    plugin_dirs,
};
pub use tui::{DEFAULT_TUI_SIZE, TuiRestart, TuiSpec};
