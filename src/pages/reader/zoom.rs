//! Zoom rendering helpers for the reader page.
//!
//! Two zoom modes are supported:
//!
//! - **Spread zoom**: landscape (double-spread) pages are fitted to viewport
//!   height so their full width overflows; the user pans left/right.
//! - **Portrait zoom**: portrait (single-page) images on a portrait screen are
//!   zoomed to 3× width and displayed one sextant at a time.  Reading order:
//!   top-right → top-middle → top-left → bottom-right → bottom-middle → bottom-left.

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

/// Sextant index for portrait zoom mode.
///
/// | Value | Position      |
/// |-------|---------------|
/// | 0     | Top-right     |
/// | 1     | Top-middle    |
/// | 2     | Top-left      |
/// | 3     | Bottom-right  |
/// | 4     | Bottom-middle |
/// | 5     | Bottom-left   |
pub type PortraitQuadrant = u8;

/// Total number of sextants in portrait zoom mode.
pub const PORTRAIT_QUADRANT_COUNT: PortraitQuadrant = 6;

/// Returns the CSS `style` attribute string for a portrait-zoom image.
///
/// The image is displayed at 3× viewport width (one column per sextant column)
/// and translated so that `sextant` fills the visible area.
pub fn portrait_zoom_image_style(
    quadrant: PortraitQuadrant,
    natural_w: u32,
    natural_h: u32,
) -> String {
    let (tx, ty) = portrait_quadrant_translate(quadrant, natural_w, natural_h);
    let tripled_w = viewport_width() * 3.0;
    format!(
        "width: {tripled_w}px; height: auto; max-width: none; max-height: none; \
         position: absolute; left: 0; top: 0; \
         transform: translate({tx}px, {ty}px); \
         user-select: none; display: block;"
    )
}

/// Computes the CSS `translate(x, y)` offset (in pixels) that brings `sextant`
/// into the visible viewport.
///
/// The image is assumed to be rendered at `width = 3 × viewport_width`.
/// Column layout (left-to-right in the rendered image):
///   sextant % 3 == 0 → right column  (x = -2×vw)
///   sextant % 3 == 1 → middle column (x = -vw)
///   sextant % 3 == 2 → left column   (x = 0)
fn portrait_quadrant_translate(
    quadrant: PortraitQuadrant,
    natural_w: u32,
    natural_h: u32,
) -> (f64, f64) {
    let vw = viewport_width();
    let vh = viewport_height();
    let rendered_width = vw * 3.0;
    // rendered_height is only computed when natural_w > 0, so no division by zero.
    let rendered_height = if natural_w > 0 {
        (natural_h as f64) * (rendered_width / natural_w as f64)
    } else {
        0.0
    };

    // Horizontal: column = sextant % 3.
    //   0 (right)  → x = -(rendered_width - vw) = -2×vw
    //   1 (middle) → x = -vw
    //   2 (left)   → x = 0
    let x = match quadrant % 3 {
        0 => -(rendered_width - vw), // -2×vw: right edge aligns with viewport right
        1 => -vw,                    // middle column centred
        _ => 0.0,                    // left edge aligns with viewport left
    };

    // Vertical: sextants 0–2 → top row; sextants 3–5 → bottom row.
    let y = if quadrant < 3 {
        0.0
    } else {
        -(rendered_height - vh).max(0.0)
    };

    (x, y)
}
