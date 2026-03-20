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
    pub on_click: EventHandler<()>,
}

#[component]
pub fn MangaCard(props: MangaCardProps) -> Element {
    rsx! {
        div {
            class: "manga-card",
            onclick: move |_| props.on_click.call(()),

            // Cover thumbnail or placeholder
            if let Some(url) = props.cover_url.clone() {
                img {
                    class: "manga-card-cover",
                    src: "{url}",
                    alt: "{props.manga.title}",
                }
            } else {
                div {
                    class: "manga-card-cover manga-card-cover-placeholder",
                    "?"
                }
            }

            div {
                class: "manga-card-info",
                p {
                    class: "manga-card-title",
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
}

#[component]
pub fn LibraryEntryCard(props: LibraryEntryCardProps) -> Element {
    let label = match &props.entry {
        LibraryEntry::Tankobon { number, .. } => format!("Vol. {number}"),
        LibraryEntry::LoneChapter(ch) => format!("Ch. {:.1}", ch.chapter_number),
    };

    rsx! {
        div {
            class: "library-entry-card",

            // Cover thumbnail or placeholder — clicking opens the reader
            div {
                class: "library-entry-cover-wrapper",
                onclick: move |_| props.on_click.call(()),
                if let Some(url) = props.cover_url.clone() {
                    img {
                        class: "manga-card-cover",
                        src: "{url}",
                        alt: "{label}",
                    }
                } else {
                    div {
                        class: "manga-card-cover manga-card-cover-placeholder",
                        "?"
                    }
                }
            }

            // Info row: label + delete button
            div {
                class: "library-entry-label",
                span {
                    onclick: move |_| props.on_click.call(()),
                    "{label}"
                }
                div {
                    class: "library-entry-actions",
                    button {
                        class: "btn btn-icon btn-delete",
                        title: "Delete",
                        onclick: move |_| props.on_delete.call(()),
                        "🗑"
                    }
                }
            }

            // Progress bar — clicking opens the reader
            div {
                onclick: move |_| props.on_click.call(()),
                ProgressBar {
                    value: props.progress_value,
                    pages_read: props.pages_read,
                    total_pages: props.total_pages,
                }
            }
        }
    }
}
