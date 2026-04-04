//! Zoom rendering helpers for the reader page.
//!
//! Two zoom modes are supported:
//!
//! - **Spread zoom**: landscape (double-spread) pages are fitted to viewport
//!   height so their full width overflows; the user pans left/right.
//! - **Portrait zoom**: portrait (single-page) images on a portrait screen are
//!   zoomed to 2× width and displayed one quadrant at a time.  Reading order
//!   of quadrants: top-right → top-left → bottom-right → bottom-left.

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

/// Quadrant index for portrait zoom mode.
///
/// | Value | Position     |
/// |-------|--------------|
/// | 0     | Top-right    |
/// | 1     | Top-left     |
/// | 2     | Bottom-right |
/// | 3     | Bottom-left  |
pub type PortraitQuadrant = u8;

/// Total number of quadrants in portrait zoom mode.
pub const PORTRAIT_QUADRANT_COUNT: PortraitQuadrant = 4;

/// Returns the CSS `style` attribute string for a portrait-zoom image.
///
/// The image is displayed at 2× viewport width (overflowing both axes when
/// the natural dimensions are comparable to the viewport) and translated so
/// that `quadrant` fills the visible area.
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

/// Computes the CSS `translate(x, y)` offset (in pixels) that brings `quadrant`
/// into the visible viewport.
///
/// The image is assumed to be rendered at `width = 2 × viewport_width`.
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

    // Horizontal: even quadrant index (0, 2) → right side; odd (1, 3) → left side.
    let x = if quadrant & 1 == 0 {
        -(rendered_width - vw) // = -vw: right edge of image aligns with viewport right
    } else {
        0.0 // left edge of image aligns with viewport left
    };

    // Vertical: quadrants 0–1 → top; quadrants 2–3 → bottom.
    let y = if quadrant < 2 {
        0.0
    } else {
        -(rendered_height - vh).max(0.0)
    };

    (x, y)
}
