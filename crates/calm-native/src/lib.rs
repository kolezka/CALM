//! AppKit side of CALM: the per-panel container view and the WKWebView panel.
//!
//! Everything here is main-thread only. All `unsafe` in the workspace lives in this crate.

#![cfg(target_os = "macos")]

pub mod app_icon;
pub mod browser;
pub mod child;
pub mod content_gesture;
pub mod menu;
pub mod process_stats;
pub mod settings_window;
pub mod status_bar;
pub mod webview;

pub use browser::{BROWSER_BAR_HEIGHT, BrowserEvent, BrowserView};
pub use child::{
    NativeChild, clip_frame, focus_host_view, host_view_is_first_responder, window_visible,
    zoom_bounds, zoom_frame,
};
pub use content_gesture::{ContentDecision, ContentEvent, ContentGestureMonitor, GestureMods};
pub use menu::{ContextMenu, HostView, show_context_menu};
pub use process_stats::{ProcessSample, ProcessSampler};
pub use settings_window::{SettingsWindow, WindowLayout, choose_image_file, settings_window_count};
pub use status_bar::StatusBar;
pub use webview::{DEFAULT_BROWSER_URL, WebPanel};

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeError {
    /// Called off the main thread.
    NotMainThread,
    /// The window handle could not be read or is not an AppKit handle.
    UnsupportedWindowHandle(String),
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NativeError::NotMainThread => {
                write!(f, "AppKit views must be created on the main thread")
            }
            NativeError::UnsupportedWindowHandle(reason) => {
                write!(f, "unsupported window handle: {reason}")
            }
        }
    }
}

impl std::error::Error for NativeError {}
