use crate::{routes::Route, storage::models::ChapterMeta};
use dioxus::prelude::*;

#[component]
pub fn ReaderOverlay(
    manga_id: String,
    manga_title: String,
    chapter_meta: Option<ChapterMeta>,
    page: usize,
    on_close: EventHandler<()>,
    on_open_settings: EventHandler<()>,
) -> Element {
    rsx! {
        div {
            class: "fixed top-0 left-0 right-0 z-30 bg-black/85 backdrop-blur-sm cursor-pointer",
            onclick: move |_| on_close.call(()),
            div {
                class: "flex items-center gap-3 px-3 py-2",
                button {
                    class: "flex-shrink-0 w-8 h-8 flex items-center justify-center border-0 cursor-pointer rounded bg-transparent text-[#888] hover:text-[#ccc] active:text-[#f0f0f0]",
                    onclick: move |e| {
                        e.stop_propagation();
                        navigator().push(Route::Library { manga_id: manga_id.clone() });
                    },
                    svg {
                        class: "w-5 h-5", fill: "none", stroke: "currentColor", stroke_width: "2", view_box: "0 0 24 24",
                        path { d: "M15 19l-7-7 7-7" }
                    }
                }
                div {
                    class: "flex flex-col gap-0.5 min-w-0 flex-1",
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
                    div {
                        class: "text-xs text-[#666]",
                        if let Some(ref meta) = chapter_meta {
                            span { "p. {page + 1} / {meta.page_count}" }
                            span { class: "text-[#555]", " · " }
                            span { class: "truncate", "{meta.filename}" }
                        }
                    }
                }
                // Settings button
                button {
                    class: "flex-shrink-0 w-8 h-8 flex items-center justify-center border-0 cursor-pointer rounded bg-transparent text-[#888] hover:text-[#ccc] active:text-[#f0f0f0]",
                    onclick: move |e| {
                        e.stop_propagation();
                        on_open_settings.call(());
                    },
                    svg {
                        class: "w-5 h-5", fill: "none", stroke: "currentColor", stroke_width: "2", view_box: "0 0 24 24",
                        path { d: "M4 6h16M4 12h16M4 18h16" }
                    }
                }
            }
        }
    }
}
