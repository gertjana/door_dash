//! Font loading and text drawing primitives.
//!
//! Bundles Inter Regular + Bold via `include_bytes!` so the rendered
//! output is byte-identical across dev (macOS) and prod (Alpine in
//! HA). No system-font fallback path because the bundled TTFs are
//! always present in the binary.
//!
//! ## Anti-aliasing on a 1-bit panel
//!
//! `draw_text` blends glyph alpha into the 8-bit canvas; when the
//! canvas later gets thresholded to 1-bit, AA edges resolve to clean
//! black/white pixels. That works well for 14px+ text but eats thin
//! strokes (descenders, the dot on `i`) at small sizes. For small
//! text use `draw_crisp_text`, which binarises each pixel as it draws
//! — alpha ≥ 0.5 becomes the fill colour, everything else is left
//! alone. This mirrors the `draw.fontmode = "1"` toggle in Pillow.

use std::sync::OnceLock;

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use image::{GrayImage, Luma};

const INTER_REGULAR_TTF: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
const INTER_BOLD_TTF: &[u8] = include_bytes!("../../assets/fonts/Inter-Bold.ttf");

/// Common font sizes used across widgets. Tweak in one place to
/// retune the visual hierarchy. Values mirror `addon/app/render/fonts.py`.
pub const TITLE_SIZE: f32 = 20.0;
pub const BODY_SIZE: f32 = 14.0;
pub const SMALL_SIZE: f32 = 13.0;
pub const BADGE_SIZE: f32 = 10.0;

/// Font weight selector for `font_for`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Bold,
}

static REGULAR: OnceLock<FontRef<'static>> = OnceLock::new();
static BOLD: OnceLock<FontRef<'static>> = OnceLock::new();

/// Get a reference to a bundled font. Cheap to call repeatedly — the
/// `OnceLock` parses each TTF exactly once on first access.
///
/// Panics on construction failure, but only a corrupted asset bundle
/// could trigger that — and it'd surface immediately at startup, not
/// half-way through a render.
pub fn font_for(weight: Weight) -> &'static FontRef<'static> {
    match weight {
        Weight::Regular => REGULAR.get_or_init(|| {
            FontRef::try_from_slice(INTER_REGULAR_TTF).expect("Inter-Regular.ttf is valid")
        }),
        Weight::Bold => BOLD.get_or_init(|| {
            FontRef::try_from_slice(INTER_BOLD_TTF).expect("Inter-Bold.ttf is valid")
        }),
    }
}

/// Pixel width of `text` rendered with `font` at `size`.
///
/// Sums advance widths across all characters. Approximate for kerning
/// pairs (ab_glyph doesn't expose kern tables in the public API), but
/// the dashboard layout has no places where a few pixels of kerning
/// drift would matter.
pub fn text_width(font: &FontRef<'static>, size: f32, text: &str) -> f32 {
    let scaled = font.as_scaled(PxScale::from(size));
    text.chars()
        .map(|c| scaled.h_advance(scaled.glyph_id(c)))
        .sum()
}

/// Pixel height of a line of text rendered at `size` — ascent +
/// descent (descent is reported as negative by ab_glyph).
pub fn text_height(font: &FontRef<'static>, size: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(size));
    scaled.ascent() - scaled.descent()
}

/// Draw anti-aliased text into `canvas` at top-left `(x, y)`.
///
/// Positioning matches Pillow's `draw.text((x, y), ...)`: `(x, y)` is
/// the top-left of the metric bounding box, not the baseline. We
/// shift by `ascent` internally so callers don't have to think about
/// font metrics.
///
/// Glyph alpha is blended onto the canvas via straight-alpha
/// compositing with `fill` as the source colour. For pure-black text
/// on a white canvas (`fill = 0`), this produces darker pixels for
/// strong glyph coverage and lighter pixels for AA edges; the
/// final 1-bit threshold collapses both back to clean black/white.
pub fn draw_text(
    canvas: &mut GrayImage,
    x: i32,
    y: i32,
    text: &str,
    font: &FontRef<'static>,
    size: f32,
    fill: u8,
) {
    draw_glyphs(
        canvas,
        &TextRun {
            x,
            y,
            text,
            font,
            size,
            fill,
        },
        AntiAlias::Yes,
    );
}

/// Draw text with no anti-aliasing — alpha ≥ 0.5 becomes `fill`,
/// everything else is left alone. Use this for small text where AA
/// edges would otherwise get thresholded into broken strokes.
pub fn draw_crisp_text(
    canvas: &mut GrayImage,
    x: i32,
    y: i32,
    text: &str,
    font: &FontRef<'static>,
    size: f32,
    fill: u8,
) {
    draw_glyphs(
        canvas,
        &TextRun {
            x,
            y,
            text,
            font,
            size,
            fill,
        },
        AntiAlias::No,
    );
}

/// Bundle of "what to draw and how" — saves carrying half a dozen
/// positional args through `draw_glyphs`.
struct TextRun<'a> {
    x: i32,
    y: i32,
    text: &'a str,
    font: &'a FontRef<'static>,
    size: f32,
    fill: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AntiAlias {
    Yes,
    No,
}

/// Internal common path for `draw_text` and `draw_crisp_text`.
fn draw_glyphs(canvas: &mut GrayImage, run: &TextRun<'_>, aa: AntiAlias) {
    let scale = PxScale::from(run.size);
    let scaled = run.font.as_scaled(scale);
    let ascent = scaled.ascent();
    let mut cursor = run.x as f32;
    let baseline_y = run.y as f32 + ascent;

    let cw = canvas.width() as i32;
    let ch = canvas.height() as i32;
    let fill = run.fill;

    for c in run.text.chars() {
        let glyph_id = scaled.glyph_id(c);
        let advance = scaled.h_advance(glyph_id);
        let glyph = glyph_id.with_scale_and_position(scale, point(cursor, baseline_y));
        if let Some(outline) = run.font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            let bx = bounds.min.x as i32;
            let by = bounds.min.y as i32;
            outline.draw(|gx, gy, alpha| {
                let px = bx + gx as i32;
                let py = by + gy as i32;
                if px < 0 || py < 0 || px >= cw || py >= ch {
                    return;
                }
                match aa {
                    AntiAlias::No => {
                        if alpha >= 0.5 {
                            canvas.put_pixel(px as u32, py as u32, Luma([fill]));
                        }
                    }
                    AntiAlias::Yes => {
                        let canvas_px = canvas.get_pixel(px as u32, py as u32)[0];
                        let blended = blend(canvas_px, fill, alpha);
                        canvas.put_pixel(px as u32, py as u32, Luma([blended]));
                    }
                }
            });
        }
        cursor += advance;
    }
}

/// Straight-alpha blend of `fill` over `dst` with coverage `alpha`.
fn blend(dst: u8, fill: u8, alpha: f32) -> u8 {
    let a = alpha.clamp(0.0, 1.0);
    let result = f32::from(dst) * (1.0 - a) + f32::from(fill) * a;
    result.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_load_without_panic() {
        let _ = font_for(Weight::Regular);
        let _ = font_for(Weight::Bold);
    }

    #[test]
    fn text_width_is_positive_for_non_empty_text() {
        let f = font_for(Weight::Regular);
        let w = text_width(f, BODY_SIZE, "Hello");
        assert!(
            w > 0.0,
            "non-empty text should have positive width: got {w}"
        );
    }

    #[test]
    fn text_width_is_zero_for_empty_string() {
        let f = font_for(Weight::Regular);
        assert_eq!(text_width(f, BODY_SIZE, ""), 0.0);
    }

    #[test]
    fn bold_is_wider_than_regular_at_same_size() {
        // Heuristic but stable across Inter releases: bold glyphs have
        // wider advance widths than their regular counterparts.
        let r = font_for(Weight::Regular);
        let b = font_for(Weight::Bold);
        let wr = text_width(r, BODY_SIZE, "Hello, world!");
        let wb = text_width(b, BODY_SIZE, "Hello, world!");
        assert!(wb >= wr, "bold should be at least as wide as regular");
    }

    #[test]
    fn draw_text_writes_some_dark_pixels() {
        let f = font_for(Weight::Regular);
        let mut canvas = GrayImage::from_pixel(80, 30, Luma([255]));
        draw_text(&mut canvas, 2, 2, "Hello", f, BODY_SIZE, 0);
        let dark_count = canvas.pixels().filter(|p| p[0] < 200).count();
        assert!(dark_count > 0, "draw_text should leave dark pixels behind");
    }

    #[test]
    fn draw_crisp_text_writes_only_pure_black_or_white() {
        let f = font_for(Weight::Regular);
        let mut canvas = GrayImage::from_pixel(80, 30, Luma([255]));
        draw_crisp_text(&mut canvas, 2, 2, "Hello", f, SMALL_SIZE, 0);
        for p in canvas.pixels() {
            assert!(
                p[0] == 0 || p[0] == 255,
                "crisp text leaves no greys, got {}",
                p[0]
            );
        }
    }

    #[test]
    fn draw_text_off_canvas_is_safe() {
        // Negative origin and far-right origin both shouldn't panic.
        let f = font_for(Weight::Regular);
        let mut canvas = GrayImage::from_pixel(40, 20, Luma([255]));
        draw_text(&mut canvas, -100, -100, "off", f, BODY_SIZE, 0);
        draw_text(&mut canvas, 200, 200, "off", f, BODY_SIZE, 0);
        // Canvas should still be entirely white.
        for p in canvas.pixels() {
            assert_eq!(p[0], 255);
        }
    }

    #[test]
    fn blend_respects_alpha_endpoints() {
        assert_eq!(blend(255, 0, 0.0), 255);
        assert_eq!(blend(255, 0, 1.0), 0);
        // Mid-alpha collapses to roughly half-grey.
        let mid = blend(255, 0, 0.5);
        assert!(
            (120..=140).contains(&mid),
            "mid-alpha = {mid}, expected ~128"
        );
    }
}
