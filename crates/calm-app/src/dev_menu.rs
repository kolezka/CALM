//! Menu bar: the app menu with Quit, the View menu with camera jumps and the Developer menu
//! with the FPS readout toggle.

use calm_core::frame_stats::FrameStats;
use gpui::{App, KeyBinding, Menu, MenuItem, actions};

use crate::canvas_view::{CenterOnOrigin, FitAll};

actions!(calm, [ToggleFps, ResetFps, Quit]);

pub fn key_bindings() -> Vec<KeyBinding> {
    // No context, so the shortcuts also work while a terminal has focus.
    vec![
        KeyBinding::new("cmd-shift-f", ToggleFps, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]
}

/// The first menu becomes the app menu on macOS, so View and Developer come after it.
pub fn menus(show_fps: bool) -> Vec<Menu> {
    vec![
        Menu::new("CALM").items([MenuItem::action("Quit CALM", Quit)]),
        Menu::new("View").items([
            MenuItem::action("Center on Origin", CenterOnOrigin),
            MenuItem::action("Fit All Panels", FitAll),
        ]),
        Menu::new("Developer").items([
            MenuItem::action("Show FPS", ToggleFps).checked(show_fps),
            MenuItem::action("Reset FPS Stats", ResetFps),
        ]),
    ]
}

pub fn install(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus(menus(false));
}

/// `FPS 60.0  p95 16.7ms`, or dashes until two frames are in the window.
pub fn fps_text(stats: &FrameStats) -> String {
    match (stats.fps(), stats.p95_ms()) {
        (Some(fps), Some(p95)) => format!("FPS {fps:.1}  p95 {p95:.1}ms"),
        _ => "FPS --  p95 --".to_string(),
    }
}
