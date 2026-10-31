//! Plugin discovery, the JS bridge protocol and the host side GitHub client.

pub mod bridge;
pub mod manifest;

pub use bridge::{BridgeRequest, BridgeResponse, CALM_PRELUDE_JS};
pub use manifest::{
    Discovery, InstalledPlugin, PluginError, PluginManifest, Size, discover, plugin_dirs,
};
