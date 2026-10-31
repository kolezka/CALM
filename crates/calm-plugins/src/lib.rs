//! Plugin discovery, the JS bridge protocol and the host side GitHub client.

pub mod assets;
pub mod bridge;
pub mod github;
pub mod manifest;

pub use assets::missing_scripts;
pub use bridge::{
    BRIDGE_METHODS, BridgeEvent, BridgeRequest, BridgeResponse, CALM_PRELUDE_JS, is_known_method,
};
pub use github::{GhClient, GhError};
pub use manifest::{
    Discovery, InstalledPlugin, NO_BUILTIN_ENV, PluginError, PluginManifest, PluginSearch,
    Shadowed, Size, discover, discover_search, plugin_dirs, plugin_search, workspace_plugins_dir,
};
