//! Sparkline rendering for the 1-bit ePaper canvas.
//!
//! A sparkline is a small inline chart showing the trajectory of a
//! value over a window of time. Each input point is one resampled
//! bucket; `None` means the value was unknown for that bucket and
//! renders as a gap in the line.
//!
//! Y-axis behaviour:
//!
//! * `include_zero = true` — clamp y-min to 0. Use for power and
//!   current where zero is a meaningful reference (you want to see
//!   "the line went up from idle" rather than auto-zoom into the
//!   noise floor).
//! * `include_zero = false` — auto-zoom to the data's own min/max
//!   with a one-pixel margin top and bottom. Use for voltage
//!   (~230 V steady) and indoor temperature where the absolute scale
//!   is uninteresting.
//!
//! Drawing is done with `imageproc::drawing::draw_line_segment_mut`
//! (Bresenham, no AA) at the fill colour. The 1-bit threshold in
//! `image_io::to_mono` keeps the line crisp; we never draw at
//! intermediate grey values because they get thresholded
//! unpredictably.

use image::{GrayImage, Luma};
use imageproc::drawing::draw_line_segment_mut;

use crate::render::widgets::Rect;

/// Below this many valid (non-`None`) buckets a "trace" is just a
/// stray dot or pair of dots, which combined with the baseline rule
/// looks like a misleading flat-line chart with one artefact at the
/// edge. We drop the baseline in that case so the cell reads as
/// "almost no data" rather than "zero across the window with a
/// glitch". Three points is enough to draw two line segments — the
/// visual minimum of an actual trend.
pub const MIN_TRACE_POINTS: usize = 3;

/// Optional knobs for [`draw_sparkline`]. All fields have sensible
/// defaults via [`Default`]; spread them with `..SparklineOptions::default()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparklineOptions {
    /// Line colour. `0` = black (the usual choice on a white canvas),
    /// `255` = white.
    pub fill: u8,
    /// Clamp the y-axis minimum to zero. Use for power and current.
    pub include_zero: bool,
    /// Draw a thin bottom rule across the rect so the sparkline reads
    /// as a chart even when the line happens to sit near the top of
    /// the cell. Suppressed automatically when fewer than
    /// [`MIN_TRACE_POINTS`] valid samples are present, so a freshly
    /// deployed entity (with only 1–2 buckets of recorded history)
    /// doesn't look like a finished flat-line chart.
    pub show_baseline: bool,
}

impl Default for SparklineOptions {
    fn default() -> Self {
        Self {
            fill: 0,
            include_zero: false,
            show_baseline: true,
        }
    }
}

/// Draw `points` as a polyline inside `rect` (in place).
///
/// Mirrors `addon/app/render/sparkline.py` exactly:
///
/// 1. Compute valid (non-`None`) sample count. If `< MIN_TRACE_POINTS`
///    or `show_baseline = false`, skip the bottom rule.
/// 2. With no valid samples, draw a short dashed line through the
///    middle of the rect so empty cells stay visually distinguishable
///    from real flat traces.
/// 3. With all-equal samples (e.g. voltage flat-lined at 230.0), draw
///    the trace centred vertically — auto-scaling would map a single
///    value to the bottom edge and look like a deliberate downward
///    trend.
/// 4. Otherwise, scale `y` linearly from `[y_min, y_max]` to the
///    plotting region (rect minus a 1-px top/bottom margin), inverted
///    so larger values draw higher on the canvas.
///
/// `None` entries break the line so we don't draw across regions
/// with no real samples.
pub fn draw_sparkline(
    canvas: &mut GrayImage,
    rect: Rect,
    points: &[Option<f64>],
    options: SparklineOptions,
) {
    if rect.w <= 1 || rect.h <= 1 {
        return;
    }

    let valid: Vec<f64> = points.iter().filter_map(|p| *p).collect();
    let has_meaningful_trace = valid.len() >= MIN_TRACE_POINTS;

    let fill_pixel = Luma([options.fill]);

    // Subtle bottom rule. Top rule is omitted on purpose — adding
    // both makes the cell read like a heavy table border, which
    // clashes with the actual table rules drawn by the page above.
    if options.show_baseline && has_meaningful_trace {
        let baseline_y = rect.y2() - 1;
        draw_line_segment_mut(
            canvas,
            (rect.x as f32, baseline_y as f32),
            ((rect.x2() - 1) as f32, baseline_y as f32),
            fill_pixel,
        );
    }

    if valid.is_empty() {
        // No data yet (entity unconfigured, or just powered up).
        // Draw a short dashed line through the middle so the cell
        // isn't visually empty but is clearly distinguishable from a
        // real flat trace.
        let midy = rect.y + rect.h / 2;
        let mut dx = 0;
        while dx < rect.w {
            put_pixel_clipped(canvas, rect.x + dx, midy, options.fill);
            dx += 4;
        }
        return;
    }

    let mut y_min = valid.iter().copied().fold(f64::INFINITY, f64::min);
    let mut y_max = valid.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if options.include_zero {
        y_min = y_min.min(0.0);
        y_max = y_max.max(0.0);
    }

    let n = points.len();
    // Constant-series special case (e.g. voltage flat-lined at 230.0).
    // Auto-scaling would map the single value to the bottom edge by
    // convention; centring it is more useful and clearly says "no
    // variation in this window".
    if y_max <= y_min {
        let flat_y = rect.y + rect.h / 2;
        let mut prev: Option<(i32, i32)> = None;
        for (i, &p) in points.iter().enumerate() {
            if p.is_none() {
                prev = None;
                continue;
            }
            let x = sample_x(rect.x, rect.w, i, n);
            match prev {
                None => put_pixel_clipped(canvas, x, flat_y, options.fill),
                Some((px, py)) => draw_line_segment_mut(
                    canvas,
                    (px as f32, py as f32),
                    (x as f32, flat_y as f32),
                    fill_pixel,
                ),
            }
            prev = Some((x, flat_y));
        }
        return;
    }

    let span = y_max - y_min;
    let pad = 1; // 1-px margin top + bottom so the line doesn't touch the rect edge
    let plot_h = (rect.h - 2 * pad).max(1);

    let mut prev: Option<(i32, i32)> = None;
    for (i, &p) in points.iter().enumerate() {
        let Some(p) = p else {
            // Gap in the data — break the line so we don't draw
            // across a region with no real samples.
            prev = None;
            continue;
        };
        let x = sample_x(rect.x, rect.w, i, n);
        // Map p in [y_min, y_max] -> y in [rect.y+pad, rect.y+pad+plot_h-1]
        // inverted (top of rect = y_max; bottom of rect = y_min).
        let norm = (p - y_min) / span;
        let y = rect.y + pad + ((1.0 - norm) * (plot_h - 1) as f64).round() as i32;
        match prev {
            None => {
                // Single-pixel point so the trace doesn't disappear at
                // the start, or when isolated between gaps.
                put_pixel_clipped(canvas, x, y, options.fill);
            }
            Some((px, py)) => {
                draw_line_segment_mut(
                    canvas,
                    (px as f32, py as f32),
                    (x as f32, y as f32),
                    fill_pixel,
                );
            }
        }
        prev = Some((x, y));
    }
}

/// Compute the x coordinate of the i-th sample within a rect of
/// width `w` starting at `x0`, evenly distributed across `n` total
/// samples. Mirrors Python's `x = x0 + int(i * (w - 1) / max(1, n - 1))`.
fn sample_x(x0: i32, w: i32, i: usize, n: usize) -> i32 {
    let denom = n.saturating_sub(1).max(1) as i32;
    x0 + (i as i32 * (w - 1)) / denom
}

/// Set a single pixel iff it's inside the canvas. Avoids panics on
/// off-canvas writes, which can happen at the dashed-line endpoints
/// for zero-size rects.
fn put_pixel_clipped(canvas: &mut GrayImage, x: i32, y: i32, fill: u8) {
    if x < 0 || y < 0 || x >= canvas.width() as i32 || y >= canvas.height() as i32 {
        return;
    }
    canvas.put_pixel(x as u32, y as u32, Luma([fill]));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(w: u32, h: u32) -> GrayImage {
        GrayImage::from_pixel(w, h, Luma([255]))
    }

    fn dark_pixel_count(canvas: &GrayImage) -> usize {
        canvas.pixels().filter(|p| p[0] < 200).count()
    }

    #[test]
    fn min_trace_points_is_three() {
        // The constant matters for "is this a real trace" gating.
        // Pin it so future tweaks land deliberately.
        assert_eq!(MIN_TRACE_POINTS, 3);
    }

    #[test]
    fn empty_rect_renders_no_pixels() {
        let mut canvas = blank(50, 50);
        let rect = Rect::new(10, 10, 0, 0);
        draw_sparkline(
            &mut canvas,
            rect,
            &[Some(1.0), Some(2.0), Some(3.0)],
            SparklineOptions::default(),
        );
        assert_eq!(dark_pixel_count(&canvas), 0);
    }

    #[test]
    fn no_valid_samples_draws_dashed_line() {
        let mut canvas = blank(60, 30);
        let rect = Rect::new(2, 2, 50, 20);
        draw_sparkline(
            &mut canvas,
            rect,
            &[None, None, None, None],
            SparklineOptions::default(),
        );
        // Dashed line is sparse but non-empty.
        let dark = dark_pixel_count(&canvas);
        assert!(
            (1..=20).contains(&dark),
            "expected a few dashed-line pixels, got {dark}"
        );
    }

    #[test]
    fn flat_trace_is_centred_vertically() {
        let mut canvas = blank(60, 30);
        let rect = Rect::new(2, 2, 50, 20);
        draw_sparkline(
            &mut canvas,
            rect,
            &[Some(5.0); 10],
            // No baseline so we only see the trace itself.
            SparklineOptions {
                show_baseline: false,
                ..Default::default()
            },
        );
        // All dark pixels should lie on the centre row of the rect.
        let mid_y = rect.y + rect.h / 2;
        for (x, y, p) in canvas.enumerate_pixels() {
            if p[0] < 200 {
                assert_eq!(
                    y as i32, mid_y,
                    "off-centre dark pixel at ({x}, {y}); flat trace should be centred at {mid_y}"
                );
            }
        }
    }

    #[test]
    fn baseline_skipped_when_too_few_samples() {
        let mut canvas = blank(60, 30);
        let rect = Rect::new(2, 2, 50, 20);
        // Only 2 valid samples — under MIN_TRACE_POINTS.
        draw_sparkline(
            &mut canvas,
            rect,
            &[Some(1.0), Some(2.0)],
            SparklineOptions::default(),
        );
        // Bottom rule would put dark pixels at y == rect.y2() - 1.
        let baseline_y = (rect.y2() - 1) as u32;
        for x in rect.x as u32..rect.x2() as u32 {
            let p = canvas.get_pixel(x, baseline_y)[0];
            assert!(p > 200, "baseline shouldn't be drawn for sparse traces");
        }
    }

    #[test]
    fn baseline_drawn_when_enough_samples() {
        let mut canvas = blank(60, 30);
        let rect = Rect::new(2, 2, 50, 20);
        draw_sparkline(
            &mut canvas,
            rect,
            &[Some(1.0), Some(2.0), Some(3.0), Some(4.0)],
            SparklineOptions::default(),
        );
        let baseline_y = (rect.y2() - 1) as u32;
        let baseline_dark = (rect.x as u32..rect.x2() as u32)
            .filter(|&x| canvas.get_pixel(x, baseline_y)[0] < 200)
            .count();
        assert!(
            baseline_dark > 10,
            "expected a continuous baseline rule, got {baseline_dark} dark pixels"
        );
    }

    #[test]
    fn ascending_trace_starts_low_ends_high() {
        let mut canvas = blank(120, 60);
        let rect = Rect::new(10, 10, 100, 40);
        // Strictly ascending series — last point should sit near the
        // top of the rect, first point near the bottom.
        let pts: Vec<Option<f64>> = (0..10).map(|i| Some(i as f64)).collect();
        draw_sparkline(
            &mut canvas,
            rect,
            &pts,
            SparklineOptions {
                show_baseline: false,
                ..Default::default()
            },
        );
        // Find the dark-pixel y at the leftmost and rightmost trace columns.
        let first_x = rect.x as u32;
        let last_x = (rect.x2() - 1) as u32;
        let first_y = (0..canvas.height())
            .find(|&y| canvas.get_pixel(first_x, y)[0] < 200)
            .expect("first sample column should have a pixel");
        let last_y = (0..canvas.height())
            .find(|&y| canvas.get_pixel(last_x, y)[0] < 200)
            .expect("last sample column should have a pixel");
        assert!(
            last_y < first_y,
            "ascending series: expected last_y={last_y} above first_y={first_y}"
        );
    }

    #[test]
    fn include_zero_clamps_y_min() {
        // Without include_zero, a [10..20] series fills the rect from
        // top to bottom. With include_zero, the same series should
        // sit near the top (because 0 is now the implicit floor).
        let mut canvas_a = blank(120, 60);
        let mut canvas_b = blank(120, 60);
        let rect = Rect::new(10, 10, 100, 40);
        let pts: Vec<Option<f64>> = (10..20).map(|i| Some(i as f64)).collect();
        draw_sparkline(
            &mut canvas_a,
            rect,
            &pts,
            SparklineOptions {
                include_zero: false,
                show_baseline: false,
                ..Default::default()
            },
        );
        draw_sparkline(
            &mut canvas_b,
            rect,
            &pts,
            SparklineOptions {
                include_zero: true,
                show_baseline: false,
                ..Default::default()
            },
        );
        // In auto-zoom mode, the lowest sample sits near the bottom edge.
        // In include_zero mode, the lowest sample sits well above it.
        let bottom = (rect.y2() - 2) as u32;
        let auto_low =
            (rect.x as u32..rect.x2() as u32).any(|x| canvas_a.get_pixel(x, bottom)[0] < 200);
        let zero_low =
            (rect.x as u32..rect.x2() as u32).any(|x| canvas_b.get_pixel(x, bottom)[0] < 200);
        assert!(auto_low, "auto-zoom should reach bottom of rect");
        assert!(!zero_low, "include_zero should keep trace above bottom");
    }

    #[test]
    fn gap_breaks_the_line() {
        // None in the middle should not produce a connecting segment
        // across the gap; verified indirectly via dark-pixel count
        // staying below "fully connected polyline" levels.
        let mut canvas_with_gap = blank(120, 60);
        let mut canvas_no_gap = blank(120, 60);
        let rect = Rect::new(10, 10, 100, 40);
        let with_gap = vec![
            Some(1.0),
            Some(1.0),
            Some(1.0),
            None,
            None,
            None,
            Some(5.0),
            Some(5.0),
            Some(5.0),
        ];
        let no_gap = vec![
            Some(1.0),
            Some(1.0),
            Some(1.0),
            Some(5.0),
            Some(5.0),
            Some(5.0),
        ];
        draw_sparkline(
            &mut canvas_with_gap,
            rect,
            &with_gap,
            SparklineOptions {
                show_baseline: false,
                ..Default::default()
            },
        );
        draw_sparkline(
            &mut canvas_no_gap,
            rect,
            &no_gap,
            SparklineOptions {
                show_baseline: false,
                ..Default::default()
            },
        );
        // The gap version should have fewer dark pixels (no diagonal
        // segment bridging the discontinuity).
        let with_gap_dark = dark_pixel_count(&canvas_with_gap);
        let no_gap_dark = dark_pixel_count(&canvas_no_gap);
        assert!(
            with_gap_dark < no_gap_dark,
            "gap version should draw fewer pixels: with_gap={with_gap_dark}, no_gap={no_gap_dark}"
        );
    }

    #[test]
    fn off_canvas_rect_does_not_panic() {
        let mut canvas = blank(40, 40);
        let rect = Rect::new(-100, -100, 50, 50);
        draw_sparkline(
            &mut canvas,
            rect,
            &[Some(1.0), Some(2.0), Some(3.0)],
            SparklineOptions::default(),
        );
        // No assertion — just shouldn't panic.
    }

    #[test]
    fn options_default_matches_python_kwargs() {
        let o = SparklineOptions::default();
        assert_eq!(o.fill, 0);
        assert!(!o.include_zero);
        assert!(o.show_baseline);
    }
}
