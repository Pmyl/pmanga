//! Padding adjustment for reader pages.
//!
//! Allows cropping white borders from manga pages with per-chapter settings
//! stored in session storage.

use serde::{Deserialize, Serialize};
use web_sys::window;

/// Padding values for cropping edges (in pixels).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Padding {
    pub up: i32,
    pub down: i32,
    pub left: i32,
    pub right: i32,
}

impl Padding {
    /// Add two padding values together.
    pub fn add(&self, other: &Padding) -> Padding {
        Padding {
            up: self.up + other.up,
            down: self.down + other.down,
            left: self.left + other.left,
            right: self.right + other.right,
        }
    }

    /// Check if all values are zero.
    pub fn is_zero(&self) -> bool {
        self.up == 0 && self.down == 0 && self.left == 0 && self.right == 0
    }
}

/// Per-chapter padding configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChapterPadding {
    /// Applied to all pages.
    pub general: Padding,
    /// Applied to odd pages (1, 3, 5, ...) - additive with general.
    pub odd: Padding,
    /// Applied to even pages (2, 4, 6, ...) - additive with general.
    pub even: Padding,
}

impl ChapterPadding {
    /// Compute effective padding for a given page number (0-based).
    /// Returns padding with all values clamped to >= 0.
    pub fn effective_for_page(&self, page: usize) -> Padding {
        // Page 0 is "page 1" visually, which is odd
        let is_odd = page % 2 == 0;
        let raw = if is_odd {
            self.general.add(&self.odd)
        } else {
            self.general.add(&self.even)
        };
        // Clamp to non-negative values to avoid invalid clip-path
        Padding {
            up: raw.up.max(0),
            down: raw.down.max(0),
            left: raw.left.max(0),
            right: raw.right.max(0),
        }
    }

    /// Check if all padding values are zero.
    pub fn is_zero(&self) -> bool {
        self.general.is_zero() && self.odd.is_zero() && self.even.is_zero()
    }
}

/// Session storage key for chapter padding.
fn storage_key(chapter_id: &str) -> String {
    format!("pmanga_padding_{}", chapter_id)
}

/// Load padding settings from session storage for a chapter.
pub fn load_chapter_padding(chapter_id: &str) -> ChapterPadding {
    let Some(window) = window() else {
        return ChapterPadding::default();
    };
    let Ok(Some(storage)) = window.session_storage() else {
        return ChapterPadding::default();
    };
    let key = storage_key(chapter_id);
    let Ok(Some(json)) = storage.get_item(&key) else {
        return ChapterPadding::default();
    };
    serde_json::from_str(&json).unwrap_or_default()
}

/// Save padding settings to session storage for a chapter.
pub fn save_chapter_padding(chapter_id: &str, padding: &ChapterPadding) {
    let Some(window) = window() else { return };
    let Ok(Some(storage)) = window.session_storage() else {
        return;
    };
    let key = storage_key(chapter_id);

    // If all zero, remove the key to save space
    if padding.is_zero() {
        let _ = storage.remove_item(&key);
        return;
    }

    if let Ok(json) = serde_json::to_string(padding) {
        let _ = storage.set_item(&key, &json);
    }
}
