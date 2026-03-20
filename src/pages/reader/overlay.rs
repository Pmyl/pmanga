//! Reader info overlay — the semi-transparent top bar and backdrop shown over
//! the manga page when toggled.

use dioxus::prelude::*;

use crate::{routes::Route, storage::models::ChapterMeta};

/// The top-bar overlay rendered while reading.
///
/// The parent is responsible for only mounting this when the overlay is
/// visible (`if overlay_visible() { ... }`).
///
/// * `on_close` — called when the user taps the backdrop or the bar itself to
///   dismiss the overlay.
/// * `on_open_padding` — called when the user taps the padding/crop button.
#[component]
pub fn ReaderOverlay(
    manga_id: String,
    manga_title: String,
    chapter_meta: Option<ChapterMeta>,
    page: usize,
    on_close: EventHandler<()>,
    on_open_padding: EventHandler<()>,
) -> Element {
    rsx! {
        // Top bar — clicking anywhere on it closes the overlay.
        div {
            class: "fixed top-0 left-0 right-0 z-30 bg-black/85 backdrop-blur-sm cursor-pointer",
            onclick: move |_| on_close.call(()),

            div {
                class: "flex items-center gap-3 px-3 py-2",

                // Back button — returns to the library view for this manga.
                button {
                    class: "flex-shrink-0 w-8 h-8 flex items-center justify-center border-0 cursor-pointer rounded bg-transparent text-[#888] hover:text-[#ccc] active:text-[#f0f0f0]",
                    onclick: move |e| {
                        e.stop_propagation();
                        navigator().push(Route::Library {
                            manga_id: manga_id.clone(),
                        });
                    },
                    svg {
                        class: "w-5 h-5",
                        fill: "none",
                        stroke: "currentColor",
                        stroke_width: "2",
                        view_box: "0 0 24 24",
                        path { d: "M15 19l-7-7 7-7" }
                    }
                }

                // Manga / chapter / page info
                div {
                    class: "flex flex-col gap-0.5 min-w-0 flex-1",

                    // Line 1: title · volume · chapter
                    div {
                        class: "text-sm text-[#ccc] truncate",
                        span { "{manga_title}" }
                        if let Some(ref meta) = chapter_meta {
                            if let Some(vol) = meta.tankobon_number {
                                span { class: "text-[#555]", " · " }
                                span { "Vol. {vol}" }
                            }
                            span { class: "text-[#555]", " · " }
                            span { "Ch. {meta.chapter_number}" }
                        }
                    }

                    // Line 2: page counter · filename
                    div {
                        class: "text-xs text-[#666]",
                        if let Some(ref meta) = chapter_meta {
                            span { "p. {page + 1} / {meta.page_count}" }
                            span { class: "text-[#555]", " · " }
                            span { class: "truncate", "{meta.filename}" }
                        }
                    }
                }

                // Padding / crop button
                button {
                    class: "flex-shrink-0 w-8 h-8 flex items-center justify-center border-0 cursor-pointer rounded bg-transparent text-[#888] hover:text-[#ccc] active:text-[#f0f0f0]",
                    onclick: move |e| {
                        e.stop_propagation();
                        on_open_padding.call(());
                    },
                    svg {
                        class: "w-5 h-5",
                        fill: "none",
                        stroke: "currentColor",
                        stroke_width: "2",
                        view_box: "0 0 24 24",
                        path { d: "M6 2v4" }
                        path { d: "M6 14v8" }
                        path { d: "M2 6h4" }
                        path { d: "M14 6h8" }
                        path { d: "M6 6h12v12H6z" }
                    }
                }
            }
        }
    }
}
