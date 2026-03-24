//! Combined reader-options modal.
//!
//! Presents two sub-sections in a single overlay:
//!   • Interactions — RTL tap-direction toggle (saved to localStorage)
//!   • Crop / Padding — per-chapter padding controls (saved to sessionStorage)

use dioxus::prelude::*;

use crate::pages::padding::{ChapterPadding, Padding, PaddingControls, save_chapter_padding};

use super::reader_config::ReaderConfig;

#[component]
pub fn ReaderOptionsModal(
    chapter_id: String,
    padding: Signal<ChapterPadding>,
    reader_config: Signal<ReaderConfig>,
    on_close: EventHandler<()>,
) -> Element {
    let mut padding = padding;
    let mut reader_config = reader_config;
    let chapter_id_save = chapter_id.clone();

    rsx! {
        // Backdrop
        div {
            class: "fixed inset-0 z-40 bg-black/70",
            onclick: move |_| on_close.call(()),
        }

        // Modal panel
        div {
            class: "fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-50 \
                    bg-[#1a1a1a] rounded-lg p-4 min-w-[280px] max-w-[90vw] \
                    max-h-[80dvh] overflow-y-auto",
            onclick: move |e| e.stop_propagation(),

            // Header
            div {
                class: "flex items-center justify-between mb-4",
                h3 { class: "text-sm font-medium text-[#ccc] m-0", "Reader Options" }
                button {
                    class: "w-6 h-6 flex items-center justify-center rounded \
                            bg-transparent text-[#666] hover:text-[#ccc] \
                            border-0 cursor-pointer",
                    onclick: move |_| on_close.call(()),
                    "✕"
                }
            }

            // ── Interactions ──────────────────────────────────────────────
            div {
                class: "mb-4",
                div { class: "text-xs text-[#666] mb-2 uppercase tracking-wide", "Interactions" }

                div {
                    class: "flex items-center justify-between gap-3 py-1",
                    label {
                        class: "text-sm text-[#ccc] flex-1 cursor-pointer",
                        r#for: "rtl-taps-toggle",
                        "Left tap = Next Page / Zoom move right (manga style)"
                    }
                    input {
                        id: "rtl-taps-toggle",
                        r#type: "checkbox",
                        class: "w-4 h-4 cursor-pointer accent-[#ccc]",
                        checked: reader_config.read().rtl_taps,
                        oninput: move |_| {
                            let new_val = !reader_config.read().rtl_taps;
                            reader_config.write().rtl_taps = new_val;
                            reader_config.read().save();
                        },
                    }
                }
            }

            // Divider
            div { class: "border-t border-[#333] mb-4" }

            // ── Crop / Padding ────────────────────────────────────────────
            div { class: "text-xs text-[#666] mb-3 uppercase tracking-wide", "Crop / Padding" }

            // General
            div {
                class: "mb-4",
                div { class: "text-xs text-[#888] mb-2", "General" }
                PaddingControls {
                    padding_value: padding.read().general,
                    on_change: {
                        let chapter_id = chapter_id_save.clone();
                        move |p: Padding| {
                            padding.write().general = p;
                            save_chapter_padding(&chapter_id, &padding.read());
                        }
                    },
                }
            }

            // Odd pages
            div {
                class: "mb-4",
                div { class: "text-xs text-[#888] mb-2", "Odd Pages (1, 3, 5...)" }
                PaddingControls {
                    padding_value: padding.read().odd,
                    on_change: {
                        let chapter_id = chapter_id_save.clone();
                        move |p: Padding| {
                            padding.write().odd = p;
                            save_chapter_padding(&chapter_id, &padding.read());
                        }
                    },
                }
            }

            // Even pages
            div {
                class: "mb-4",
                div { class: "text-xs text-[#888] mb-2", "Even Pages (2, 4, 6...)" }
                PaddingControls {
                    padding_value: padding.read().even,
                    on_change: {
                        let chapter_id = chapter_id_save.clone();
                        move |p: Padding| {
                            padding.write().even = p;
                            save_chapter_padding(&chapter_id, &padding.read());
                        }
                    },
                }
            }

            // Reset All
            div {
                class: "pt-3 border-t border-[#333]",
                button {
                    class: "w-full py-2 rounded bg-[#333] text-[#888] \
                            hover:bg-[#444] hover:text-[#ccc] \
                            border-0 cursor-pointer text-sm",
                    onclick: {
                        let chapter_id = chapter_id_save.clone();
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
