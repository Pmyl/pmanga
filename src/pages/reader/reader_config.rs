//! Reader configuration persisted to localStorage.
//!
//! Stores user preferences for the reader that should survive page reloads,
//! such as tap-zone reading direction.

use serde::{Deserialize, Serialize};

/// Per-user reader preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReaderConfig {
    /// When `true`, the LEFT tap zone advances to the NEXT page (manga / RTL
    /// style).  When `false` (default), LEFT = previous page (LTR style).
    pub rtl_taps: bool,
    /// When `true`, all pages of the chapter are rendered vertically for
    /// continuous (webtoon-style) scrolling.  When `false` (default), pages
    /// are shown one at a time with tap-zone navigation.
    #[serde(default)]
    pub vertical_scroll: bool,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            rtl_taps: false,
            vertical_scroll: false,
        }
    }
}

impl ReaderConfig {
    const STORAGE_KEY: &'static str = "pmanga_reader_config";

    /// Load from `localStorage`, falling back to defaults if absent or corrupt.
    pub fn load() -> Self {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            if let Ok(Some(raw)) = storage.get_item(Self::STORAGE_KEY) {
                if let Ok(cfg) = serde_json::from_str::<Self>(&raw) {
                    return cfg;
                }
            }
        }
        Self::default()
    }

    /// Persist to `localStorage`.
    pub fn save(&self) {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            if let Ok(json) = serde_json::to_string(self) {
                let _ = storage.set_item(Self::STORAGE_KEY, &json);
            }
        }
    }
}
