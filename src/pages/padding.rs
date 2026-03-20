//! Padding adjustment for reader pages.
//!
//! Allows cropping white borders from manga pages with per-chapter settings
//! stored in session storage.  Also contains the `PaddingModal` and
//! `PaddingControls` UI components that live in the reader overlay.

use dioxus::prelude::*;
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

// ---------------------------------------------------------------------------
// UI components
// ---------------------------------------------------------------------------

/// Modal dialog for adjusting per-chapter padding settings.
#[component]
pub fn PaddingModal(
    chapter_id: String,
    padding: Signal<ChapterPadding>,
    on_close: EventHandler<()>,
) -> Element {
    let mut padding = padding;
    let chapter_id_for_save = chapter_id.clone();

    rsx! {
        // Backdrop
        div {
            class: "fixed inset-0 z-40 bg-black/70",
            onclick: move |_| on_close.call(()),
        }
        // Modal
        div {
            class: "fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-50 bg-[#1a1a1a] rounded-lg p-4 min-w-[280px] max-w-[90vw]",
            onclick: move |e| e.stop_propagation(),

            // Header
            div {
                class: "flex items-center justify-between mb-4",
                h3 { class: "text-sm font-medium text-[#ccc] m-0", "Padding Adjustment" }
                button {
                    class: "w-6 h-6 flex items-center justify-center rounded bg-transparent text-[#666] hover:text-[#ccc] border-0 cursor-pointer",
                    onclick: move |_| on_close.call(()),
                    "✕"
                }
            }

            // General section
            div {
                class: "mb-4",
                div { class: "text-xs text-[#666] mb-2 uppercase tracking-wide", "General" }
                PaddingControls {
                    padding_value: padding.read().general,
                    on_change: {
                        let chapter_id = chapter_id_for_save.clone();
                        move |p: Padding| {
                            padding.write().general = p;
                            save_chapter_padding(&chapter_id, &padding.read());
                        }
                    },
                }
            }

            // Odd pages section
            div {
                class: "mb-4",
                div { class: "text-xs text-[#666] mb-2 uppercase tracking-wide", "Odd Pages (1, 3, 5...)" }
                PaddingControls {
                    padding_value: padding.read().odd,
                    on_change: {
                        let chapter_id = chapter_id_for_save.clone();
                        move |p: Padding| {
                            padding.write().odd = p;
                            save_chapter_padding(&chapter_id, &padding.read());
                        }
                    },
                }
            }

            // Even pages section
            div {
                class: "mb-4",
                div { class: "text-xs text-[#666] mb-2 uppercase tracking-wide", "Even Pages (2, 4, 6...)" }
                PaddingControls {
                    padding_value: padding.read().even,
                    on_change: {
                        let chapter_id = chapter_id_for_save.clone();
                        move |p: Padding| {
                            padding.write().even = p;
                            save_chapter_padding(&chapter_id, &padding.read());
                        }
                    },
                }
            }

            // Reset button
            div {
                class: "pt-3 border-t border-[#333]",
                button {
                    class: "w-full py-2 rounded bg-[#333] text-[#888] hover:bg-[#444] hover:text-[#ccc] border-0 cursor-pointer text-sm",
                    onclick: {
                        let chapter_id = chapter_id_for_save.clone();
                        move |_| {
                            padding.set(ChapterPadding::default());
                            save_chapter_padding(&chapter_id, &ChapterPadding::default());
                        }
                    },
                    "Reset All"
                }
            }
        }
    }
}

/// A grid of increment/decrement controls for a single [`Padding`] value.
#[component]
pub fn PaddingControls(padding_value: Padding, on_change: EventHandler<Padding>) -> Element {
    let up = padding_value.up;
    let down = padding_value.down;
    let left = padding_value.left;
    let right = padding_value.right;

    rsx! {
        div {
            class: "grid grid-cols-2 gap-2",

            // UP
            div {
                class: "flex items-center gap-2",
                span { class: "w-14 text-xs text-[#888]", "UP" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up: (up - 1).max(0), down, left, right }),
                    "-"
                }
                span { class: "w-8 text-center text-sm text-[#ccc]", "{up}" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up: up + 1, down, left, right }),
                    "+"
                }
            }

            // DOWN
            div {
                class: "flex items-center gap-2",
                span { class: "w-14 text-xs text-[#888]", "DOWN" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down: (down - 1).max(0), left, right }),
                    "-"
                }
                span { class: "w-8 text-center text-sm text-[#ccc]", "{down}" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down: down + 1, left, right }),
                    "+"
                }
            }

            // LEFT
            div {
                class: "flex items-center gap-2",
                span { class: "w-14 text-xs text-[#888]", "LEFT" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down, left: (left - 1).max(0), right }),
                    "-"
                }
                span { class: "w-8 text-center text-sm text-[#ccc]", "{left}" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down, left: left + 1, right }),
                    "+"
                }
            }

            // RIGHT
            div {
                class: "flex items-center gap-2",
                span { class: "w-14 text-xs text-[#888]", "RIGHT" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down, left, right: (right - 1).max(0) }),
                    "-"
                }
                span { class: "w-8 text-center text-sm text-[#ccc]", "{right}" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down, left, right: right + 1 }),
                    "+"
                }
            }
        }
    }
}
