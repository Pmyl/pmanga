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
    /// Highest chapter number ever downloaded.  Used when the manga is empty
    /// to show "All caught up to ch. XX" instead of a progress bar.
    pub last_downloaded_chapter: Option<f32>,
    /// When true, renders a 🌐 badge indicating a WeebCentral (web) manga.
    #[props(default = false)]
    pub is_web: bool,
    pub on_click: EventHandler<()>,
    /// When provided, renders a delete (🗑) button on the card.
    pub on_delete: Option<EventHandler<()>>,
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
                // Delete button (only rendered when a handler is provided)
                if let Some(on_delete) = props.on_delete.clone() {
                    button {
                        class: "absolute top-1.5 right-1.5 border-0 cursor-pointer text-xs px-1.5 py-0.5 rounded bg-[#8b1a1a]/70 text-[#f0f0f0] active:bg-[#8b1a1a]",
                        title: "Delete manga",
                        onclick: move |e| {
                            e.stop_propagation();
                            on_delete.call(());
                        },
                        "🗑"
                    }
                }
            }

            div {
                class: "p-2 flex flex-col gap-1",
                p {
                    class: "text-xs font-medium truncate",
                    "{props.manga.title}"
                }
                if props.total_pages == 0 {
                    small {
                        class: "text-[0.65rem] text-[#888]",
                        if let Some(n) = props.last_downloaded_chapter {
                            if n == n.floor() {
                                "All caught up to ch. {n:.0}"
                            } else {
                                "All caught up to ch. {n:.1}"
                            }
                        } else {
                            "New"
                        }
                    }
                } else {
                    ProgressBar {
                        value: props.progress_value,
                        pages_read: props.pages_read,
                        total_pages: props.total_pages,
                    }
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
    pub on_mark_read: EventHandler<()>,
    pub on_mark_unread: EventHandler<()>,
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

            // Action buttons (delete + mark read) — absolutely positioned over the cover
            if !props.in_select_mode {
                div {
                    class: "absolute top-1.5 right-1.5 flex gap-1",
                    if props.progress_value >= 1.0 {
                        button {
                            class: "border-0 cursor-pointer text-xs px-1.5 py-0.5 rounded bg-[#5c3d1a]/80 text-[#f0f0f0] active:bg-[#7a5a1a]",
                            title: "Mark as unread",
                            onclick: move |e| {
                                e.stop_propagation();
                                props.on_mark_unread.call(());
                            },
                            "↩"
                        }
                    } else {
                        button {
                            class: "border-0 cursor-pointer text-xs px-1.5 py-0.5 rounded bg-[#1a5c1a]/80 text-[#f0f0f0] active:bg-[#1a7a1a]",
                            title: "Mark as fully read",
                            onclick: move |e| {
                                e.stop_propagation();
                                props.on_mark_read.call(());
                            },
                            "✓"
                        }
                    }
                    button {
                        class: "border-0 cursor-pointer text-xs px-1.5 py-0.5 rounded bg-[#8b1a1a]/70 text-[#f0f0f0] active:bg-[#8b1a1a]",
                        title: "Delete",
                        onclick: move |e| {
                            e.stop_propagation();
                            props.on_delete.call(());
                        },
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
