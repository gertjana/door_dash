//! Rendering primitives — canvas types, image I/O, fonts, icons, the
//! shared version badge, and (in later phases) widgets and pages.
//!
//! The composition flow mirrors the Python addon:
//!
//! 1. Allocate an L-mode `GrayImage` filled white (255).
//! 2. Pages draw widgets; widgets call `draw_text` / `draw_icon` /
//!    `imageproc::drawing::*` against the canvas.
//! 3. `image_io::to_mono` hard-thresholds at 128 to produce a 1-bit
//!    image, then `to_png_bytes` / `to_bmp_bytes` serialize for the
//!    HTTP responses.
//!
//! All drawing happens on 8-bit grayscale until the very last step;
//! anti-aliased glyphs and lines blend into the L canvas, then get
//! collapsed to pure black/white. Small text uses `draw_crisp_text` to
//! avoid the AA-edge thresholding artefacts that eat thin strokes on a
//! 1-bit panel.

pub mod badge;
pub mod fonts;
pub mod icons;
pub mod image_io;

use image::GrayImage;

/// Allocate a fresh canvas filled white. Used by every page as the
/// drawing target. Returns an 8-bit grayscale image because we need
/// AA blending headroom during composition; the final 1-bit collapse
/// happens at serialize time.
pub fn blank_canvas(width: u32, height: u32) -> GrayImage {
    GrayImage::from_pixel(width, height, image::Luma([255]))
}
