use crate::routes::Route;
use dioxus::prelude::*;

#[component]
pub fn ShelfPage() -> Element {
    rsx! {
        div {
            class: "page shelf-page",
            div {
                class: "shelf-header",
                h1 { "PManga" }
                div {
                    class: "shelf-header-actions",
                    button {
                        class: "btn btn-icon",
                        onclick: move |_| {
                            navigator().push(Route::Settings {});
                        },
                        "⚙"
                    }
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| {
                            // TODO: open import dialog (no manga context)
                        },
                        "+ Import"
                    }
                }
            }
            div {
                class: "shelf-grid",
                // TODO: render manga cards from storage
                p {
                    class: "empty-state",
                    "No manga yet. Import something to get started."
                }
            }
        }
    }
}
