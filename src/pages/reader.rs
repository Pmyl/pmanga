use crate::routes::Route;
use dioxus::prelude::*;

#[component]
pub fn ReaderPage(manga_id: String, chapter_id: String, page: usize) -> Element {
    let overlay_visible = use_signal(|| false);

    rsx! {
        div {
            class: "page reader-page",

            // Page image area
            div {
                class: "reader-image-container",
                // TODO: render current page image from IndexedDB
                div {
                    class: "reader-placeholder",
                    "Page {page}"
                }
            }

            // Tap zones (overlay on top of the image)
            div {
                class: "reader-tap-zones",

                // Left zone: previous page
                div {
                    class: "tap-zone tap-zone-left",
                    onclick: move |_| {
                        // TODO: go to previous page
                    }
                }

                // Top zone: toggle overlay
                div {
                    class: "tap-zone tap-zone-top",
                    onclick: move |_| {
                        let mut v = overlay_visible;
                        v.set(!v());
                    }
                }

                // Right zone: next page
                div {
                    class: "tap-zone tap-zone-right",
                    onclick: move |_| {
                        // TODO: go to next page
                    }
                }
            }

            // Info overlay (top bar)
            if overlay_visible() {
                div {
                    class: "reader-overlay",
                    div {
                        class: "reader-overlay-bar",
                        button {
                            class: "btn btn-back",
                            onclick: move |_| {
                                navigator().push(Route::Library {
                                    manga_id: manga_id.clone(),
                                });
                            },
                            "← Library"
                        }
                        div {
                            class: "reader-overlay-info",
                            // TODO: fill from chapter metadata
                            span { class: "overlay-manga-name", "Manga name" }
                            span { class: "overlay-separator", " · " }
                            span { class: "overlay-volume", "Vol. ?" }
                            span { class: "overlay-separator", " · " }
                            span { class: "overlay-chapter", "Ch. ?" }
                            span { class: "overlay-separator", " · " }
                            span { class: "overlay-page", "p. {page}" }
                        }
                        div {
                            class: "reader-overlay-filename",
                            // TODO: show actual filename
                            span { "filename.pdf" }
                        }
                    }
                }
            }
        }
    }
}
