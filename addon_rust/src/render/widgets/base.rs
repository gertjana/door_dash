//! Widget placement primitive — the [`Rect`] type.
//!
//! Every widget renders into a target rectangle on the parent canvas
//! and must NOT draw outside that rectangle. This mirrors the Python
//! widget contract from `addon/app/render/widgets/base.py`, where the
//! corresponding type is named `Box`. We use `Rect` here both because
//! `Box` collides with `std::boxed::Box` and because `Rect` is the
//! conventional graphics-programming term.

/// Axis-aligned rectangle on a 2D canvas.
///
/// Fields are signed (`i32`, not `u32`) so layout arithmetic like
/// `r.x2() - 12` doesn't underflow; coordinates are pixel indices on
/// the parent canvas (typically 800×480 for the reTerminal panel) but
/// the type is generic over canvas size. Negative values are illegal
/// — they'd point off-canvas — but we don't enforce that at
/// construction; widgets are expected to clip when they draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// Construct a new rectangle.
    #[must_use]
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// Right edge: `x + w`.
    #[must_use]
    pub const fn x2(&self) -> i32 {
        self.x + self.w
    }

    /// Bottom edge: `y + h`.
    #[must_use]
    pub const fn y2(&self) -> i32 {
        self.y + self.h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x2_y2_compute_far_edges() {
        let r = Rect::new(10, 20, 100, 50);
        assert_eq!(r.x2(), 110);
        assert_eq!(r.y2(), 70);
    }

    #[test]
    fn rect_supports_zero_size() {
        // A zero-size rect is legal — widgets just won't draw anything.
        let r = Rect::new(0, 0, 0, 0);
        assert_eq!(r.x2(), 0);
        assert_eq!(r.y2(), 0);
    }

    #[test]
    fn rect_is_copy() {
        let r = Rect::new(1, 2, 3, 4);
        let _r2 = r; // not a move
        assert_eq!(r.x, 1); // r still usable
    }
}
