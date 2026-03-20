use dioxus::prelude::*;

use crate::pages::{
    library::LibraryPage, reader::ReaderPage, settings::SettingsPage, shelf::ShelfPage,
};

#[derive(Routable, Clone, PartialEq)]
pub enum Route {
    #[route("/")]
    Shelf {},
    #[route("/library/:manga_id")]
    Library { manga_id: String },
    #[route("/read/:manga_id/:chapter_id/:page")]
    Reader {
        manga_id: String,
        chapter_id: String,
        page: usize,
    },
    #[route("/settings")]
    Settings {},
}

#[component]
fn Shelf() -> Element {
    rsx! { ShelfPage {} }
}

#[component]
fn Library(manga_id: String) -> Element {
    rsx! { LibraryPage { manga_id } }
}

#[component]
fn Reader(manga_id: String, chapter_id: String, page: usize) -> Element {
    rsx! { ReaderPage { manga_id, chapter_id, page } }
}

#[component]
fn Settings() -> Element {
    rsx! { SettingsPage {} }
}

#[component]
pub fn App() -> Element {
    rsx! {
        document::Stylesheet { href: asset!("/assets/main.css") }
        // JSZip must be loaded as a plain script before the bridge module runs.
        document::Script {
            src: "https://cdnjs.cloudflare.com/ajax/libs/jszip/3.10.1/jszip.min.js"
        }
        // Bridge loaded as type="module" so it can use ES module imports (PDF.js).
        document::Script {
            src: asset!("/assets/pmanga_bridge.js"),
            r#type: "module"
        }
        Router::<Route> {}
    }
}
