use std::rc::Rc;

use crate::{
    components::{
        importer::Importer, manga_card::MangaCard, weebcentral_importer::WeebCentralImporter,
    },
    routes::Route,
    storage::{
        db::Db,
        models::{MangaId, MangaSource},
        progress::{is_startup_redirect_done, load_last_opened, mark_startup_redirect_done},
    },
};
use dioxus::prelude::*;

// ---------------------------------------------------------------------------
// Per-manga display data (assembled asynchronously after DB load)
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
struct MangaDisplayData {
    manga_id: String,
    title: String,
    cover_url: Option<String>,
    progress_value: f32,
    pages_read: u32,
    total_pages: u32,
    /// True for WeebCentral manga — renders a 🌐 badge on the card.
    is_web: bool,
}

// ---------------------------------------------------------------------------
// Page component
// ---------------------------------------------------------------------------

#[component]
pub fn ShelfPage() -> Element {
    // Open the DB once per mount.
    let mut db_signal: Signal<Option<Rc<Db>>> = use_signal(|| None);

    // Controls whether the importer modal is visible.
    let mut show_importer: Signal<bool> = use_signal(|| false);

    // Controls whether the WeebCentral importer modal is visible.
    let mut show_wc_importer: Signal<bool> = use_signal(|| false);

    // Bump this to trigger a data refresh after import.
    let mut refresh_counter: Signal<u32> = use_signal(|| 0);

    // Assembled display data for the grid.
    let mut display_data: Signal<Vec<MangaDisplayData>> = use_signal(Vec::new);

    // Open DB on mount.
    use_effect(move || {
        spawn(async move {
            match Db::open().await {
                Ok(db) => *db_signal.write() = Some(Rc::new(db)),
                Err(e) => web_sys::console::error_1(&format!("DB open error: {e}").into()),
            }
        });
    });

    // Startup redirect: if there is a last-opened position in localStorage,
    // navigate straight to the reader on first mount — but only once per
    // browser session so that navigating *back* to the shelf does not
    // re-trigger the redirect.
    use_effect(move || {
        if !is_startup_redirect_done() {
            mark_startup_redirect_done();
            if let Some(last) = load_last_opened() {
                let nav = navigator();
                nav.push(Route::Reader {
                    manga_id: last.manga_id,
                    chapter_id: last.chapter_id,
                    page: last.page,
                });
            }
        }
    });

    // Load (or re-load) the manga grid whenever the DB becomes ready or
    // refresh_counter is bumped.
    use_effect(move || {
        let counter = *refresh_counter.read();
        let _ = counter; // read so the effect re-runs when it changes

        let Some(db) = db_signal.read().clone() else {
            return;
        };

        spawn(async move {
            // Revoke old blob URLs to avoid memory leaks.
            // Never revoke CDN URLs (WeebCentral) — only blob: object URLs.
            for old in display_data.read().iter() {
                if let Some(url) = &old.cover_url {
                    if url.starts_with("blob:") {
                        let _ = web_sys::Url::revoke_object_url(url);
                    }
                }
            }

            let mangas = match db.load_all_mangas().await {
                Ok(v) => v,
                Err(e) => {
                    web_sys::console::error_1(&format!("load_all_mangas error: {e}").into());
                    return;
                }
            };

            let all_progress = match db.load_all_progress().await {
                Ok(v) => v,
                Err(e) => {
                    web_sys::console::error_1(&format!("load_all_progress error: {e}").into());
                    return;
                }
            };

            let mut items: Vec<MangaDisplayData> = Vec::new();

            for manga in mangas {
                let chapters = match db.load_chapters_for_manga(&manga.id).await {
                    Ok(v) => v,
                    Err(e) => {
                        web_sys::console::error_1(
                            &format!("load_chapters error for {}: {e}", manga.id.0).into(),
                        );
                        continue;
                    }
                };

                // Sort chapters by chapter_number to find the "first" one.
                let mut sorted = chapters.clone();
                sorted.sort_by(|a, b| a.chapter_number.total_cmp(&b.chapter_number));

                // Cover URL: CDN URL for WeebCentral, blob from IDB for local.
                let cover_url: Option<String> = if let Some(first) = sorted.first() {
                    if !first.page_urls.is_empty() {
                        // WeebCentral: use the first CDN URL directly
                        first.page_urls.first().cloned()
                    } else {
                        // Local: load blob from IndexedDB
                        match db.load_page(&first.id, 0).await {
                            Ok(Some(blob)) => web_sys::Url::create_object_url_with_blob(&blob).ok(),
                            _ => None,
                        }
                    }
                } else {
                    None
                };

                // Total pages across all chapters for this manga.
                let total_pages: u32 = chapters.iter().map(|c| c.page_count).sum();

                // Pages read: sum of progress.page across all chapters of this manga.
                let pages_read: u32 = all_progress
                    .iter()
                    .filter(|p| p.manga_id == manga.id)
                    .map(|p| p.page as u32)
                    .sum();

                let progress_value = if total_pages > 0 {
                    (pages_read as f32 / total_pages as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                items.push(MangaDisplayData {
                    manga_id: manga.id.0.clone(),
                    title: manga.title.clone(),
                    cover_url,
                    progress_value,
                    pages_read,
                    total_pages,
                    is_web: matches!(manga.source, MangaSource::WeebCentral { .. }),
                });
            }

            *display_data.write() = items;
        });
    });

    let nav = navigator();

    rsx! {
        div {
            class: "h-screen flex flex-col overflow-hidden",
            div {
                class: "flex items-center justify-between px-4 py-3 border-b border-[#222] shrink-0",
                h1 { class: "text-lg font-semibold", "PManga" }
                div {
                    class: "flex flex-row gap-2 items-center",
                    button {
                        class: "border-0 cursor-pointer text-lg px-2 py-1 rounded bg-transparent text-[#888] active:text-[#f0f0f0]",
                        onclick: move |_| {
                            nav.push(Route::Settings {});
                        },
                        "⚙"
                    }
                    button {
                        class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#5a9fd4] text-black font-semibold active:bg-[#4a8fc4]",
                        onclick: move |_| {
                            *show_wc_importer.write() = true;
                        },
                        "🌐 WeebCentral"
                    }
                    button {
                        class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#e8b44a] text-black font-semibold active:bg-[#d4a03c]",
                        onclick: move |_| {
                            *show_importer.write() = true;
                        },
                        "+ Import"
                    }
                }
            }

            div {
                class: "grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] items-start gap-3 p-4 overflow-y-auto flex-1",
                if display_data.read().is_empty() {
                    p {
                        class: "text-center text-[#888] py-12 px-4",
                        "No manga yet. Import something to get started."
                    }
                } else {
                    for item in display_data.read().iter().cloned() {
                        {
                            let manga_id = item.manga_id.clone();
                            let nav2 = navigator();
                            rsx! {
                                MangaCard {
                                    key: "{item.manga_id}",
                                    manga: crate::storage::models::MangaMeta {
                                        id: MangaId(item.manga_id.clone()),
                                        title: item.title.clone(),
                                        mangadex_id: None,
                                        source: crate::storage::models::MangaSource::Local,
                                    },
                                    cover_url: item.cover_url.clone(),
                                    progress_value: item.progress_value,
                                    pages_read: item.pages_read,
                                    total_pages: item.total_pages,
                                    is_web: item.is_web,
                                    on_click: move |_| {
                                        nav2.push(Route::Library {
                                            manga_id: manga_id.clone(),
                                        });
                                    },
                                }
                            }
                        }
                    }
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

            // WeebCentral importer modal
            if *show_wc_importer.read() {
                if let Some(db) = db_signal.read().clone() {
                    WeebCentralImporter {
                        db,
                        on_complete: move |_manga_id: MangaId| {
                            *show_wc_importer.write() = false;
                            *refresh_counter.write() += 1;
                        },
                        on_cancel: move |_| {
                            *show_wc_importer.write() = false;
                        },
                    }
                }
            }
        }
    }
}
