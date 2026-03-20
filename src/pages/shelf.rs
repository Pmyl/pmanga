use std::rc::Rc;

use dioxus::prelude::*;

use crate::{
    components::importer::Importer,
    routes::Route,
    storage::{db::Db, models::MangaId},
};

#[component]
pub fn ShelfPage() -> Element {
    // Open the DB once per mount.
    let mut db_signal: Signal<Option<Rc<Db>>> = use_signal(|| None);

    // Controls whether the importer modal is visible.
    let mut show_importer: Signal<bool> = use_signal(|| false);

    // Bump this to trigger a data refresh after import.
    let mut refresh_counter: Signal<u32> = use_signal(|| 0);

    // Open DB on mount.
    use_effect(move || {
        wasm_bindgen_futures::spawn_local(async move {
            match Db::open().await {
                Ok(db) => *db_signal.write() = Some(Rc::new(db)),
                Err(e) => web_sys::console::error_1(&format!("DB open error: {e}").into()),
            }
        });
    });

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
                            *show_importer.write() = true;
                        },
                        "+ Import"
                    }
                }
            }
            div {
                class: "shelf-grid",
                // TODO: render manga cards from storage (refresh_counter used to trigger re-fetch)
                p {
                    class: "empty-state",
                    "No manga yet. Import something to get started."
                }
            }

            // Importer modal — only rendered when DB is ready and modal is open.
            if *show_importer.read() {
                if let Some(db) = db_signal.read().clone() {
                    Importer {
                        preset_manga: None,
                        db,
                        on_complete: move |_manga_id: MangaId| {
                            *show_importer.write() = false;
                            *refresh_counter.write() += 1;
                        },
                        on_cancel: move |_| {
                            *show_importer.write() = false;
                        },
                    }
                }
            }
        }
    }
}
