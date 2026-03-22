//! Manga card components for Shelf and Library pages.

use dioxus::prelude::*;

use crate::{
    components::progress_bar::ProgressBar,
    storage::models::{LibraryEntry, MangaMeta},
};

// ---------------------------------------------------------------------------
// MangaCard — used on the Shelf page (one card per manga series)
// ---------------------------------------------------------------------------

#[derive(Props, Clone, PartialEq)]
pub struct MangaCardProps {
    pub manga: MangaMeta,
    /// Blob URL of the first page of the first chapter (if available).
    pub cover_url: Option<String>,
    /// Reading progress 0.0–1.0.
    pub progress_value: f32,
    pub pages_read: u32,
    pub total_pages: u32,
    /// When true, renders a 🌐 badge indicating a WeebCentral (web) manga.
    #[props(default = false)]
    pub is_web: bool,
    pub on_click: EventHandler<()>,
}

#[component]
pub fn MangaCard(props: MangaCardProps) -> Element {
    rsx! {
        div {
            class: "bg-[#1a1a1a] rounded-lg overflow-hidden cursor-pointer flex flex-col active:bg-[#222]",
            onclick: move |_| props.on_click.call(()),

            // Cover thumbnail or placeholder
            div {
                class: "relative",
                if let Some(url) = props.cover_url.clone() {
                    img {
                        class: "w-full aspect-[2/3] object-cover bg-[#111]",
                        src: "{url}",
                        alt: "{props.manga.title}",
                    }
                } else {
                    div {
                        class: "w-full aspect-[2/3] bg-[#111] flex items-center justify-center text-[#333] text-3xl",
                        "?"
                    }
                }
                // Web source badge
                if props.is_web {
                    div {
                        class: "absolute top-1.5 left-1.5 text-xs bg-black/60 rounded px-1 py-0.5 leading-none",
                        "🌐"
                    }
                }
            }

            div {
                class: "p-2 flex flex-col gap-1",
                p {
                    class: "text-xs font-medium truncate",
                    "{props.manga.title}"
                }
                ProgressBar {
                    value: props.progress_value,
                    pages_read: props.pages_read,
                    total_pages: props.total_pages,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// LibraryEntryCard — used on the Library page (one tankobon or lone chapter)
// ---------------------------------------------------------------------------

#[derive(Props, Clone, PartialEq)]
pub struct LibraryEntryCardProps {
    pub entry: LibraryEntry,
    /// Blob URL of the first page of the entry's first chapter (if available).
    pub cover_url: Option<String>,
    /// Reading progress 0.0–1.0.
    pub progress_value: f32,
    pub pages_read: u32,
    pub total_pages: u32,
    pub on_click: EventHandler<()>,
    pub on_delete: EventHandler<()>,
    #[props(default = false)]
    pub in_select_mode: bool,
    #[props(default = false)]
    pub is_selected: bool,
}

#[component]
pub fn LibraryEntryCard(props: LibraryEntryCardProps) -> Element {
    let label = match &props.entry {
        LibraryEntry::Tankobon { number, .. } => format!("Vol. {number}"),
        LibraryEntry::LoneChapter(ch) => format!("Ch. {:.1}", ch.chapter_number),
    };

    rsx! {
        div {
            class: "bg-[#1a1a1a] rounded-lg overflow-hidden cursor-pointer flex flex-col relative active:bg-[#222]",

            // Cover thumbnail or placeholder — clicking opens the reader
            div {
                class: "relative",
                onclick: move |_| props.on_click.call(()),
                if let Some(url) = props.cover_url.clone() {
                    img {
                        class: "w-full aspect-[2/3] object-cover bg-[#111]",
                        src: "{url}",
                        alt: "{label}",
                    }
                } else {
                    div {
                        class: "w-full aspect-[2/3] bg-[#111] flex items-center justify-center text-[#333] text-3xl",
                        "?"
                    }
                }
                if props.is_selected {
                    div {
                        class: "absolute inset-0 bg-[#4caf50]/30 flex items-center justify-center",
                        span { class: "text-white text-3xl font-bold", "✓" }
                    }
                }
            }

            // Delete button — absolutely positioned over the cover
            if !props.in_select_mode {
                div {
                    class: "absolute top-1.5 right-1.5",
                    button {
                        class: "border-0 cursor-pointer text-xs px-1.5 py-0.5 rounded bg-[#8b1a1a]/70 text-[#f0f0f0] active:bg-[#8b1a1a]",
                        title: "Delete",
                        onclick: move |_| props.on_delete.call(()),
                        "🗑"
                    }
                }
            }

            // Label + progress bar — matches MangaCard's p-2 layout
            div {
                class: "p-2 flex flex-col gap-1",
                onclick: move |_| props.on_click.call(()),
                p {
                    class: "text-xs font-medium truncate",
                    "{label}"
                }
                ProgressBar {
                    value: props.progress_value,
                    pages_read: props.pages_read,
                    total_pages: props.total_pages,
                }
            }
        }
    }
}
