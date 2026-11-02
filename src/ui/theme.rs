use iced::{Color, Theme, theme::palette::Palette, widget::button};

/// WoW-inspired color constants for use outside the palette (e.g. status indicators).
pub const COLOR_SUCCESS: Color = Color::from_rgb(0.118, 1.0, 0.0); // #1eff00 — WoW uncommon green
pub const COLOR_DANGER: Color = Color::from_rgb(1.0, 0.2, 0.2); // #ff3333 — WoW red
pub const COLOR_WARNING: Color = Color::from_rgb(1.0, 0.8, 0.2); // #ffcc33 — matches the palette warning
pub const COLOR_TITLEBAR: Color = Color::from_rgb(0.027, 0.035, 0.055); // #07090e — darker than background
pub const COLOR_GOLD: Color = Color::from_rgb(0.910, 0.659, 0.145); // #e8a825 — primary accent
pub const COLOR_MUTED: Color = Color::from_rgb(0.667, 0.690, 0.741); // #aab0bd — secondary text (8.7:1 on panels)
pub const COLOR_HINT: Color = Color::from_rgb(0.835, 0.851, 0.886); // #d5d9e2 — text drawn over artwork
pub const COLOR_SURFACE: Color = Color::from_rgb(0.102, 0.129, 0.196); // #1a2132 — raised controls
pub const COLOR_SURFACE_HOVER: Color = Color::from_rgb(0.145, 0.180, 0.263); // #252e43
pub const COLOR_BORDER: Color = Color::from_rgb(0.227, 0.267, 0.345); // #3a4458 — control outlines
pub const COLOR_PANEL: Color = Color::from_rgb(0.051, 0.063, 0.090); // #0d1017 — bars and panels

/// Style for secondary buttons: a dark raised surface with light text and an outline,
/// instead of the palette's mid-grey, which has too little contrast.
#[must_use]
pub fn surface_button(_theme: &Theme, status: button::Status) -> button::Style {
    let (background, text_color) = match status {
        button::Status::Active => (COLOR_SURFACE, Color::from_rgb(0.91, 0.894, 0.847)),
        button::Status::Hovered | button::Status::Pressed => (COLOR_SURFACE_HOVER, Color::WHITE),
        button::Status::Disabled => (
            COLOR_SURFACE,
            Color {
                a: 0.55,
                ..COLOR_MUTED
            },
        ),
    };
    button::Style {
        background: Some(iced::Background::Color(background)),
        text_color,
        border: iced::Border {
            color: COLOR_BORDER,
            width: 1.0,
            radius: 4.into(),
        },
        ..button::Style::default()
    }
}

/// A stable, muted tint for a client's hero area, derived from its id so every
/// client (including custom ones) looks distinct without any artwork.
#[must_use]
pub fn tint_for(seed: &str) -> Color {
    let hash = seed.bytes().fold(0xCBF2_9CE4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3)
    });
    #[allow(clippy::cast_precision_loss)]
    let hue = (hash % 360) as f32 / 60.0;
    let (saturation, value) = (0.45_f32, 0.30_f32);
    let chroma = value * saturation;
    let second = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r, g, b) = match hue {
        h if h < 1.0 => (chroma, second, 0.0),
        h if h < 2.0 => (second, chroma, 0.0),
        h if h < 3.0 => (0.0, chroma, second),
        h if h < 4.0 => (0.0, second, chroma),
        h if h < 5.0 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let lift = value - chroma;
    Color::from_rgb(r + lift, g + lift, b + lift)
}

/// Create a WoW-inspired custom theme.
///
/// - Deep navy-black background
/// - Gold primary accent
/// - Warm parchment text
/// - `WoW` uncommon green for success
/// - `WoW` red for danger
#[must_use]
pub fn wow_theme() -> Theme {
    Theme::custom(
        "Arclight".to_string(),
        Palette {
            background: Color::from_rgb(0.043, 0.055, 0.082), // #0b0e15
            text: Color::from_rgb(0.878, 0.847, 0.784),       // #e0d8c8
            primary: COLOR_GOLD,
            success: COLOR_SUCCESS,
            warning: Color::from_rgb(1.0, 0.8, 0.2),
            danger: COLOR_DANGER,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color) -> f32 {
        let channel = |value: f32| {
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
    }

    fn contrast(foreground: Color, background: Color) -> f32 {
        let (a, b) = (luminance(foreground), luminance(background));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn text_colors_keep_readable_contrast() {
        let palette = wow_theme().palette();
        let button_text = Color::from_rgb(0.91, 0.894, 0.847);

        // Small text (12 px) on the panels should clear WCAG AAA (7:1).
        assert!(contrast(COLOR_MUTED, COLOR_PANEL) >= 7.0);
        assert!(contrast(palette.text, COLOR_PANEL) >= 7.0);
        assert!(contrast(COLOR_GOLD, COLOR_PANEL) >= 7.0);
        assert!(contrast(button_text, COLOR_SURFACE) >= 7.0);
        assert!(contrast(Color::WHITE, COLOR_SURFACE_HOVER) >= 7.0);
        assert!(contrast(Color::BLACK, COLOR_GOLD) >= 7.0);
    }

    #[test]
    fn control_outlines_are_distinguishable_from_the_panel() {
        assert!(contrast(COLOR_BORDER, COLOR_PANEL) >= 1.5);
    }
}
