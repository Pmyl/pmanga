//! Viewport utilities for the reader: orientation, dimensions, spread-zoom
//! geometry helpers, and blob → object-URL conversion.

// ---------------------------------------------------------------------------
// Blob helper
// ---------------------------------------------------------------------------

/// Convert a [`web_sys::Blob`] into an object URL string.
pub fn blob_to_object_url(blob: &web_sys::Blob) -> Result<String, String> {
    web_sys::Url::create_object_url_with_blob(blob)
        .map_err(|e| format!("create_object_url failed: {:?}", e))
}

// ---------------------------------------------------------------------------
// Orientation / dimensions
// ---------------------------------------------------------------------------

/// Returns `true` if the viewport is currently in portrait orientation
/// (height > width).
pub fn is_portrait() -> bool {
    let Some(window) = web_sys::window() else {
        return true;
    };
    let w = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let h = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    h > w
}

/// Returns the current viewport width in CSS pixels.
pub fn viewport_width() -> f64 {
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(400.0)
}

/// Returns the current viewport height in CSS pixels.
pub fn viewport_height() -> f64 {
    web_sys::window()
        .and_then(|w| w.inner_height().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(800.0)
}

// ---------------------------------------------------------------------------
// Spread-zoom geometry
// ---------------------------------------------------------------------------

/// Given the image's natural pixel dimensions, compute its rendered width when
/// the image is height-fitted to the viewport (i.e. `height: 100vh; width: auto`).
pub fn rendered_width_when_height_fitted(natural_w: u32, natural_h: u32) -> f64 {
    if natural_h == 0 {
        return 0.0;
    }
    let vh = viewport_height();
    (natural_w as f64) * (vh / natural_h as f64)
}

/// How many pixels a single left/right pan tap moves the view.
/// ~40 % of the viewport width so three taps cover a typical double-spread.
pub fn pan_step() -> f64 {
    viewport_width() * 0.4
}
