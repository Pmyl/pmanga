use std::rc::Rc;

use dioxus::prelude::*;

use crate::{
    components::importer::Importer,
    routes::Route,
    storage::{
        db::Db,
        models::{MangaId, MangaMeta},
    },
};

#[component]
pub fn LibraryPage(manga_id: String) -> Element {
    // Open the DB once per mount.
    let mut db_signal: Signal<Option<Rc<Db>>> = use_signal(|| None);

    // The resolved MangaMeta for the pre-filled manga context.
    let mut manga_meta_signal: Signal<Option<MangaMeta>> = use_signal(|| None);

    // Controls whether the importer modal is visible.
    let mut show_importer: Signal<bool> = use_signal(|| false);

    // Bump this to trigger a data refresh after import.
    let mut refresh_counter: Signal<u32> = use_signal(|| 0);

    // Open DB and load the manga meta on mount (or when manga_id changes).
    let manga_id_clone = manga_id.clone();
    use_effect(move || {
        let mid = manga_id_clone.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match Db::open().await {
                Ok(db) => {
                    let db = Rc::new(db);
                    // Try to load the MangaMeta for the given manga_id.
                    match db.load_all_mangas().await {
                        Ok(mangas) => {
                            let found = mangas.into_iter().find(|m| m.id.0 == mid);
                            *manga_meta_signal.write() = found;
                        }
                        Err(e) => {
                            web_sys::console::error_1(
                                &format!("Failed to load mangas: {e}").into(),
                            );
                        }
                    }
                    *db_signal.write() = Some(db);
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("DB open error: {e}").into());
                }
            }
        });
    });

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
                        *show_importer.write() = true;
                    },
                    "+ Import"
                }
            }
            div {
                class: "library-grid",
                // TODO: render tankobon + lone chapter cards interleaved in order
                // (refresh_counter used to trigger re-fetch)
                p {
                    class: "empty-state",
                    "No chapters yet. Import something to get started."
                }
            }

            // Importer modal — only rendered when DB is ready and modal is open.
            if *show_importer.read() {
                if let Some(db) = db_signal.read().clone() {
                    Importer {
                        preset_manga: manga_meta_signal.read().clone(),
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
