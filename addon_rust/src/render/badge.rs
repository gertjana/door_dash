//! Shared version-badge helper.
//!
//! Every page draws a tiny `vX.Y.Z · fwA.B.C` badge in the top-right
//! corner so the running addon + firmware versions are visible at a
//! glance. The drawing primitive is identical across pages — only the
//! content above/around it changes — so it lives here rather than
//! being copy-pasted into each page module.

use image::{GrayImage, Luma};

use crate::config::Settings;
use crate::render::fonts::{
    draw_crisp_text, font_for, text_height, text_width, Weight, BADGE_SIZE,
};
use crate::ADDON_VERSION;

/// Render the addon + firmware version badge in the top-right corner.
///
/// Drawn on a small white pad so the badge stays legible if a page
/// chooses to draw content right up to the canvas edge.
pub fn draw_version_badge(canvas: &mut GrayImage, settings: &Settings, fw_version: Option<&str>) {
    let fw = fw_version.unwrap_or("?");
    // U+00B7 MIDDLE DOT — same separator the Python addon uses.
    let text = format!("v{ADDON_VERSION} \u{00b7} fw{fw}");
    let f = font_for(Weight::Regular);
    let w = text_width(f, BADGE_SIZE, &text).ceil() as i32;
    let h = text_height(f, BADGE_SIZE).ceil() as i32;
    let pad = 2i32;
    let x = settings.width as i32 - w - 4;
    let y = 2i32;
    fill_rect(canvas, x - pad, y - pad, w + 2 * pad, h + 2 * pad, 255);
    draw_crisp_text(canvas, x, y, &text, f, BADGE_SIZE, 0);
}

/// Fill an axis-aligned rectangle with a solid grayscale value.
/// Clipped to canvas bounds; out-of-range coordinates are ignored
/// rather than panicking.
fn fill_rect(canvas: &mut GrayImage, x: i32, y: i32, w: i32, h: i32, fill: u8) {
    let cw = canvas.width() as i32;
    let ch = canvas.height() as i32;
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(cw);
    let y1 = (y + h).min(ch);
    for py in y0..y1 {
        for px in x0..x1 {
            canvas.put_pixel(px as u32, py as u32, Luma([fill]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn settings_for(width: u32, height: u32) -> Settings {
        Settings {
            width,
            height,
            ..Settings::default()
        }
    }

    #[test]
    fn badge_writes_pixels_in_top_right() {
        let mut canvas = GrayImage::from_pixel(800, 480, Luma([255]));
        draw_version_badge(&mut canvas, &settings_for(800, 480), Some("2025.10.0"));
        // Expect at least some dark pixels in the top-right quadrant.
        let mut dark_top_right = 0;
        for y in 0..30 {
            for x in 600..800 {
                if canvas.get_pixel(x, y)[0] < 200 {
                    dark_top_right += 1;
                }
            }
        }
        assert!(
            dark_top_right > 0,
            "expected version badge pixels in top-right"
        );
    }

    #[test]
    fn badge_does_not_touch_bottom_left() {
        let mut canvas = GrayImage::from_pixel(800, 480, Luma([255]));
        draw_version_badge(&mut canvas, &settings_for(800, 480), None);
        // Bottom-left should be untouched (still white).
        for y in 100..480 {
            for x in 0..400 {
                assert_eq!(canvas.get_pixel(x, y)[0], 255, "badge bled into ({x}, {y})");
            }
        }
    }

    #[test]
    fn badge_handles_missing_fw_version() {
        let mut canvas = GrayImage::from_pixel(800, 480, Luma([255]));
        // Should render `fw?` without panicking.
        draw_version_badge(&mut canvas, &settings_for(800, 480), None);
    }

    #[test]
    fn badge_fits_inside_small_canvases() {
        // Sanity: a tiny canvas should still render something rather
        // than panic on out-of-bounds writes.
        let mut canvas = GrayImage::from_pixel(120, 40, Luma([255]));
        draw_version_badge(&mut canvas, &settings_for(120, 40), Some("0.1"));
    }
}
