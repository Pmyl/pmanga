use crate::routes::Route;
use dioxus::prelude::*;

#[component]
pub fn LibraryPage(manga_id: String) -> Element {
    rsx! {
        div {
            class: "page library-page",
            div {
                class: "library-header",
                button {
                    class: "btn btn-back",
                    onclick: move |_| {
                        navigator().push(Route::Shelf {});
                    },
                    "← Back"
                }
                h1 { class: "library-title", "Library" }
                button {
                    class: "btn btn-primary",
                    onclick: move |_| {
                        // TODO: open import dialog (manga pre-filled)
                    },
                    "+ Import"
                }
            }
            div {
                class: "library-grid",
                // TODO: render tankobon + lone chapter cards interleaved in order
                p {
                    class: "empty-state",
                    "No chapters yet. Import something to get started."
                }
            }
        }
    }
}
