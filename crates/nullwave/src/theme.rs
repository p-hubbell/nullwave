//! Colors and sizes. Dark, calm, VS Code/Zed-like (values follow the layout A mockup).

use gpui::{Hsla, Rgba, rgb};

pub const ACTIVITY_BAR_WIDTH: f32 = 36.0;
pub const TAB_BAR_HEIGHT: f32 = 30.0;
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
pub const PANEL_TABS_HEIGHT: f32 = 26.0;
pub const RESIZE_HANDLE: f32 = 4.0;

pub const SIDEBAR_MIN_WIDTH: f32 = 160.0;
pub const SIDEBAR_MAX_WIDTH: f32 = 600.0;
pub const PANEL_MIN_HEIGHT: f32 = 60.0;
/// The panel never grows past the window height minus this much.
pub const PANEL_RESERVED_HEIGHT: f32 = 140.0;

pub const FONT_SIZE: f32 = 12.0;
pub const SMALL_FONT_SIZE: f32 = 10.0;

pub fn editor_bg() -> Rgba {
    rgb(0x1b1d22)
}
pub fn chrome_bg() -> Rgba {
    rgb(0x16181c)
}
pub fn sidebar_bg() -> Rgba {
    rgb(0x1e2025)
}
pub fn border() -> Rgba {
    rgb(0x2c2f36)
}
pub fn text() -> Rgba {
    rgb(0xc9ced8)
}
pub fn text_strong() -> Rgba {
    rgb(0xe5e7eb)
}
pub fn text_muted() -> Rgba {
    rgb(0x7c8494)
}
pub fn text_faint() -> Rgba {
    rgb(0x565c68)
}
pub fn accent() -> Rgba {
    rgb(0x6aa0ff)
}
pub fn selection_bg() -> Rgba {
    rgb(0x2a3a55)
}
pub fn hover_bg() -> Rgba {
    rgb(0x262930)
}
pub fn input_bg() -> Rgba {
    rgb(0x22252b)
}
pub fn status_bg() -> Rgba {
    rgb(0x2a3a55)
}
pub fn status_text() -> Rgba {
    rgb(0xdbe6ff)
}
pub fn warning() -> Rgba {
    rgb(0xffb86b)
}

/// Fully transparent, for handles that only need a hitbox.
pub fn transparent() -> Hsla {
    gpui::transparent_black()
}
