//! Colors shared by the canvas and the panel chrome.
//! The canvas background is a setting: `Settings::background` in calm-core.
//! Values ending in an alpha byte are for `rgba`, the rest for `rgb`.

pub const TEXT: u32 = 0xe6e6e6;
pub const DRAFT_OUTLINE: u32 = 0x6fa8dc;

/// Accent for focus. Also the focused panel's border.
pub const ACCENT: u32 = 0x5b8def;

/// Title bar fill. The focused panel's bar is a step brighter.
pub const PANEL_BAR: u32 = 0x1d1e22;
pub const PANEL_BAR_FOCUSED: u32 = 0x26282e;
/// 1 px ring just outside the panel.
pub const PANEL_BORDER: u32 = 0x34363d;
/// Hairline between the title bar and the content.
pub const PANEL_DIVIDER: u32 = 0x2c2e34;
/// Flat fill where a native view will sit.
pub const CONTENT_PLACEHOLDER: u32 = 0x151619;

pub const TITLE_TEXT: u32 = 0x8b8f99;
pub const TITLE_TEXT_FOCUSED: u32 = 0xe4e6eb;
pub const STALE_TEXT: u32 = 0xc9a24a;

pub const ICON_TERMINAL: u32 = 0x5fbf8f;
pub const ICON_BROWSER: u32 = 0x6aa6f8;
pub const ICON_PLUGIN: u32 = 0xb48cf2;

/// Button circle while the panel is hovered but the button is not.
pub const BUTTON_IDLE: u32 = 0x3b3e46;
pub const BUTTON_GLYPH: u32 = 0xb9bdc6;
pub const BUTTON_CLOSE_HOVER: u32 = 0xe5534b;
pub const BUTTON_MINIMIZE_HOVER: u32 = 0xe0a83a;
/// Glyph on a colored (hovered) button.
pub const BUTTON_GLYPH_ON_COLOR: u32 = 0x1a1a1a;

/// Close confirmation: tinted bar, red ring, light text.
pub const CONFIRM_BAR: u32 = 0x3a1f23;
pub const CONFIRM_BORDER: u32 = 0xe5534b;
pub const CONFIRM_TEXT: u32 = 0xffd9d6;

/// Drop shadow under a panel, with alpha.
pub const SHADOW: u32 = 0x0000_0059;
pub const SHADOW_FOCUSED: u32 = 0x0000_0080;
