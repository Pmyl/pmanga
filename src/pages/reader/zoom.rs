//! Zoom rendering helpers for the reader page.
//!
//! Two zoom modes are supported:
//!
//! - **Spread zoom**: landscape (double-spread) pages are fitted to viewport
//!   height so their full width overflows; the user pans left/right.
//! - **Portrait zoom**: portrait (single-page) images on a portrait screen are
//!   zoomed to 2× width and stepped through 9 nonet positions (3 cols × 3 rows).
//!   The middle column shows the image at x = -vw/2, overlapping both the right
//!   and left columns.  Reading order:
//!   top-right → top-middle → top-left →
//!   middle-right → middle-middle → middle-left →
//!   bottom-right → bottom-middle → bottom-left.

use super::viewport::{viewport_height, viewport_width};

// ---------------------------------------------------------------------------
// Spread zoom
// ---------------------------------------------------------------------------

/// Returns the CSS `style` attribute string for a spread-zoom image.
///
/// The image is anchored `right: 0` and shifted horizontally by `pan_x`
/// pixels (positive value moves the image rightward, revealing the left side
/// of the spread).
pub fn spread_zoom_image_style(pan_x: f64) -> String {
    format!(
        "height: 100dvh; width: auto; max-width: none; position: absolute; \
         right: 0; transform: translateX({pan_x}px); user-select: none; display: block;"
    )
}

// ---------------------------------------------------------------------------
// Portrait zoom
// ---------------------------------------------------------------------------

/// Nonet index for portrait zoom mode.
///
/// | Value | Position        |
/// |-------|-----------------|
/// | 0     | Top-right       |
/// | 1     | Top-middle      |
/// | 2     | Top-left        |
/// | 3     | Middle-right    |
/// | 4     | Middle-middle   |
/// | 5     | Middle-left     |
/// | 6     | Bottom-right    |
/// | 7     | Bottom-middle   |
/// | 8     | Bottom-left     |
pub type PortraitQuadrant = u8;

/// Total number of nonets in portrait zoom mode.
pub const PORTRAIT_QUADRANT_COUNT: PortraitQuadrant = 9;

/// Returns the CSS `style` attribute string for a portrait-zoom image.
///
/// The image is displayed at 2× viewport width (same zoom as the original
/// quadrant mode) and translated to one of nine nonet positions.  The middle
/// column position (`nonet % 3 == 1`) sits at x = -vw/2, intentionally
/// overlapping both the right and left columns.
pub fn portrait_zoom_image_style(
    quadrant: PortraitQuadrant,
    natural_w: u32,
    natural_h: u32,
) -> String {
    let (tx, ty) = portrait_quadrant_translate(quadrant, natural_w, natural_h);
    let doubled_w = viewport_width() * 2.0;
    format!(
        "width: {doubled_w}px; height: auto; max-width: none; max-height: none; \
         position: absolute; left: 0; top: 0; \
         transform: translate({tx}px, {ty}px); \
         user-select: none; display: block;"
    )
}

/// Computes the CSS `translate(x, y)` offset (in pixels) that brings `nonet`
/// into the visible viewport.
///
/// The image is rendered at `width = 2 × viewport_width`.
/// Column positions (nonet % 3):
///   0 (right)  → x = -vw      (right edge of image at viewport right)
///   1 (middle) → x = -vw/2    (overlaps both right and left columns)
///   2 (left)   → x = 0        (left edge of image at viewport left)
/// Row positions (nonet / 3):
///   0 (top)    → y = 0
///   1 (middle) → y = -(rendered_h - vh) / 2
///   2 (bottom) → y = -(rendered_h - vh)
fn portrait_quadrant_translate(
    quadrant: PortraitQuadrant,
    natural_w: u32,
    natural_h: u32,
) -> (f64, f64) {
    let vw = viewport_width();
    let vh = viewport_height();
    let rendered_width = vw * 2.0;
    // rendered_height is only computed when natural_w > 0, so no division by zero.
    let rendered_height = if natural_w > 0 {
        (natural_h as f64) * (rendered_width / natural_w as f64)
    } else {
        0.0
    };

    // Horizontal: column = nonet % 3.
    //   0 (right)  → x = -vw      (same as original right quadrant)
    //   1 (middle) → x = -vw/2    (midpoint, shows overlap with both sides)
    //   2 (left)   → x = 0        (same as original left quadrant)
    let x = match quadrant % 3 {
        0 => -vw,             // right edge of 2×vw image aligns with viewport right
        1 => -(vw / 2.0),     // midpoint of 2×vw image centred in viewport
        _ => 0.0,             // left edge of image aligns with viewport left
    };

    // Vertical: row = nonet / 3.
    //   0 (top)    → y = 0
    //   1 (middle) → y = -(rendered_h - vh) / 2
    //   2 (bottom) → y = -(rendered_h - vh)
    let overflow = (rendered_height - vh).max(0.0);
    let y = match quadrant / 3 {
        0 => 0.0,
        1 => -(overflow / 2.0),
        _ => -overflow,
    };

    (x, y)
}
