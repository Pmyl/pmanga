use dioxus::prelude::*;

use crate::routes::Route;

#[component]
pub fn SettingsPage() -> Element {
    rsx! {
        div {
            class: "page settings-page",
            div {
                class: "settings-header",
                button {
                    class: "btn btn-back",
                    onclick: move |_| {
                        navigator().push(Route::Shelf {});
                    },
                    "← Back"
                }
                h1 { "Settings" }
            }
            div {
                class: "settings-section",
                h2 { "Gamepad Bindings" }
                p {
                    class: "settings-hint",
                    "Press \"Remap\" next to an action, then press the desired button on your gamepad."
                }
                // TODO: render gamepad binding rows from input config
                p {
                    class: "empty-state",
                    "Gamepad bindings will appear here."
                }
            }
        }
    }
}
