//! Plugin discovery, the JS bridge protocol and the host side GitHub client.

pub mod api;
pub mod assets;
pub mod bridge;
pub mod contributes;
pub mod events;
pub mod gh_token;
pub mod github;
pub mod manifest;
pub mod tui;

pub use api::{API_VERSION, ApiError, Caller, Capability, ClientId, ErrorCode};
pub use assets::missing_scripts;
pub use bridge::{
    BRIDGE_METHODS, BridgeEvent, BridgeRequest, BridgeResponse, CALM_PRELUDE_JS, host_info_js,
    is_known_method,
};
pub use gh_token::{GhToken, TokenResolution, TokenSource, load_token};
pub use github::{GH_MAX_OUTPUT, GH_TIMEOUT, GhClient, GhError};
pub use manifest::{
    BackendRestart, BackendSpec, BackendStart, Discovery, Engines, InstalledPlugin, NO_BUILTIN_ENV,
    PluginContent, PluginError, PluginManifest, PluginSearch, PluginWarning, Shadowed, Size,
    discover, discover_search, plugin_dirs, plugin_search, workspace_plugins_dir,
};
pub use tui::{DEFAULT_TUI_SIZE, TuiIcon, TuiRestart, TuiSpec, TuiTerminalColors, parse_hex_color};
