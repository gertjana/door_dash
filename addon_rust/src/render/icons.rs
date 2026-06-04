//! Material Design Icons (MDI) glyph rendering.
//!
//! Uses the MDI webfont bundled in `assets/icons/`. Icon names match
//! the HA / `mdi:...` convention (e.g. `weather-partly-cloudy`).
//!
//! The codepoint table below is a curated subset — we only ship the
//! names actually used by the dashboard so unknown lookups fail
//! loudly during development. Extend `codepoint` to add more.
//!
//! Glyphs are drawn through the same `draw_text` path as regular
//! text so positioning, AA, and 1-bit thresholding behave identically.

use std::sync::OnceLock;

use ab_glyph::FontRef;
use image::GrayImage;

use crate::render::fonts::draw_text;

const MDI_TTF: &[u8] = include_bytes!("../../assets/icons/materialdesignicons-webfont.ttf");

static MDI: OnceLock<FontRef<'static>> = OnceLock::new();

fn mdi_font() -> &'static FontRef<'static> {
    MDI.get_or_init(|| {
        FontRef::try_from_slice(MDI_TTF).expect("materialdesignicons-webfont.ttf is valid")
    })
}

/// Codepoint lookup for an MDI icon name. Strips a leading `mdi:` if
/// present, mirroring the Python implementation. Returns `None` for
/// unknown names so callers can substitute a `help-circle` fallback.
///
/// The table is a `match` so the compiler emits an efficient jump
/// table; lookup cost is irrelevant compared with the glyph raster.
pub fn codepoint(name: &str) -> Option<u32> {
    let name = name.strip_prefix("mdi:").unwrap_or(name);
    Some(match name {
        // Weather (HA `weather.*` state values map here directly)
        "weather-cloudy" => 0xF0590,
        "weather-fog" => 0xF0591,
        "weather-hail" => 0xF0592,
        "weather-hazy" => 0xF0F30,
        "weather-hurricane" => 0xF0898,
        "weather-lightning" => 0xF0593,
        "weather-lightning-rainy" => 0xF067E,
        "weather-night" => 0xF0594,
        "weather-night-partly-cloudy" => 0xF0F31,
        "weather-partly-cloudy" => 0xF0595,
        "weather-partly-lightning" => 0xF0F32,
        "weather-partly-rainy" => 0xF0F33,
        "weather-partly-snowy" => 0xF0F34,
        "weather-partly-snowy-rainy" => 0xF0F35,
        "weather-pouring" => 0xF0596,
        "weather-rainy" => 0xF0597,
        "weather-snowy" => 0xF0598,
        "weather-snowy-heavy" => 0xF0F36,
        "weather-snowy-rainy" => 0xF067F,
        "weather-sunny" => 0xF0599,
        "weather-sunset" => 0xF059A,
        "weather-tornado" => 0xF0F38,
        "weather-windy" => 0xF059D,
        "weather-windy-variant" => 0xF059E,
        // General
        "alert-circle" => 0xF0028,
        "battery" => 0xF0079,
        "battery-charging" => 0xF0084,
        "battery-outline" => 0xF008E,
        "calendar" => 0xF00ED,
        "cloud" => 0xF015F,
        "gauge" => 0xF0269,
        "help-circle" => 0xF02D7,
        "home" => 0xF02DC,
        "snowflake" => 0xF0717,
        "thermometer" => 0xF050F,
        "umbrella" => 0xF054A,
        "water-percent" => 0xF058E,
        _ => return None,
    })
}

/// Map an HA `weather.*` entity state value to the right MDI icon
/// name. Falls back to `help-circle` for unknown states (mirrors the
/// Python addon so unexpected payloads stay loudly visible).
///
/// Reference: https://www.home-assistant.io/integrations/weather/
pub fn icon_for_weather_state(state: &str) -> &'static str {
    match state.to_ascii_lowercase().as_str() {
        "clear-night" => "weather-night",
        "cloudy" => "weather-cloudy",
        "exceptional" => "alert-circle",
        "fog" => "weather-fog",
        "hail" => "weather-hail",
        "lightning" => "weather-lightning",
        "lightning-rainy" => "weather-lightning-rainy",
        "partlycloudy" => "weather-partly-cloudy",
        "pouring" => "weather-pouring",
        "rainy" => "weather-rainy",
        "snowy" => "weather-snowy",
        "snowy-rainy" => "weather-snowy-rainy",
        "sunny" => "weather-sunny",
        "windy" => "weather-windy",
        "windy-variant" => "weather-windy-variant",
        _ => "help-circle",
    }
}

/// Draw an MDI glyph at `(x, y)` sized roughly `size`×`size`.
///
/// MDI is designed on a 24×24 grid; using `size` directly as the
/// font px size yields a glyph close to that square box with the
/// usual font-metric slack.
///
/// Falls back to `help-circle` when the name is unknown. If even
/// `help-circle` is missing (corrupted MDI table — shouldn't happen
/// in practice) we silently no-op rather than panic.
pub fn draw_icon(canvas: &mut GrayImage, name: &str, x: i32, y: i32, size: f32, fill: u8) {
    let cp = codepoint(name).or_else(|| codepoint("help-circle"));
    let Some(cp) = cp else {
        return;
    };
    let Some(ch) = char::from_u32(cp) else {
        return;
    };
    let mut buf = [0u8; 4];
    let s = ch.encode_utf8(&mut buf);
    draw_text(canvas, x, y, s, mdi_font(), size, fill);
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    #[test]
    fn known_icon_returns_codepoint() {
        assert_eq!(codepoint("weather-sunny"), Some(0xF0599));
        assert_eq!(codepoint("home"), Some(0xF02DC));
    }

    #[test]
    fn mdi_prefix_is_stripped() {
        assert_eq!(codepoint("mdi:home"), Some(0xF02DC));
        assert_eq!(codepoint("mdi:weather-cloudy"), Some(0xF0590));
    }

    #[test]
    fn unknown_icon_returns_none() {
        assert_eq!(codepoint("definitely-not-a-real-icon"), None);
    }

    #[test]
    fn weather_state_maps_known_values() {
        assert_eq!(icon_for_weather_state("sunny"), "weather-sunny");
        assert_eq!(
            icon_for_weather_state("partlycloudy"),
            "weather-partly-cloudy"
        );
        assert_eq!(icon_for_weather_state("CLEAR-NIGHT"), "weather-night");
    }

    #[test]
    fn weather_state_unknown_falls_back_to_help_circle() {
        assert_eq!(icon_for_weather_state("plasma-storm"), "help-circle");
        assert_eq!(icon_for_weather_state(""), "help-circle");
    }

    #[test]
    fn help_circle_is_in_the_table() {
        // The `draw_icon` fallback assumes `help-circle` is always
        // resolvable. Pin that contract.
        assert!(codepoint("help-circle").is_some());
    }

    #[test]
    fn draw_icon_writes_some_dark_pixels() {
        let mut canvas = GrayImage::from_pixel(48, 48, Luma([255]));
        draw_icon(&mut canvas, "weather-sunny", 8, 8, 32.0, 0);
        let dark = canvas.pixels().filter(|p| p[0] < 200).count();
        assert!(dark > 0, "drawing an icon should leave non-white pixels");
    }

    #[test]
    fn draw_icon_unknown_name_falls_back_to_help_circle() {
        // Should still draw _something_ rather than no-op.
        let mut canvas = GrayImage::from_pixel(48, 48, Luma([255]));
        draw_icon(&mut canvas, "made-up-icon", 8, 8, 32.0, 0);
        let dark = canvas.pixels().filter(|p| p[0] < 200).count();
        assert!(
            dark > 0,
            "unknown icon should fall back to help-circle, got blank canvas"
        );
    }
}
