use iced::{Color, Theme, theme::palette::Palette};

/// WoW-inspired color constants for use outside the palette (e.g. status indicators).
pub const COLOR_SUCCESS: Color = Color::from_rgb(0.118, 1.0, 0.0); // #1eff00 — WoW uncommon green
pub const COLOR_DANGER: Color = Color::from_rgb(1.0, 0.2, 0.2); // #ff3333 — WoW red
pub const COLOR_TITLEBAR: Color = Color::from_rgb(0.067, 0.067, 0.133); // #111122 — darker than background

/// Create a WoW-inspired custom theme.
///
/// - Deep navy-black background
/// - Warm parchment text
/// - Blizzard blue primary (rare item color)
/// - `WoW` uncommon green for success
/// - `WoW` red for danger
#[must_use]
pub fn wow_theme() -> Theme {
    Theme::custom(
        "Arclight".to_string(),
        Palette {
            background: Color::from_rgb(0.102, 0.102, 0.180), // #1a1a2e
            text: Color::from_rgb(0.878, 0.847, 0.784),       // #e0d8c8
            primary: Color::from_rgb(0.0, 0.439, 0.867),      // #0070dd
            success: COLOR_SUCCESS,
            warning: Color::from_rgb(1.0, 0.8, 0.2),
            danger: COLOR_DANGER,
        },
    )
}
