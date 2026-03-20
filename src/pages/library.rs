use std::rc::Rc;

use crate::storage::progress::clear_last_opened;
use dioxus::prelude::*;

use crate::{
    components::{confirm_dialog::ConfirmDialog, importer::Importer, manga_card::LibraryEntryCard},
    routes::Route,
    storage::{
        db::Db,
        models::{ChapterId, LibraryEntry, MangaId, MangaMeta, build_library_entries},
    },
};

// ---------------------------------------------------------------------------
// Per-entry display data (assembled asynchronously after DB load)
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
struct EntryDisplayData {
    entry: LibraryEntry,
    cover_url: Option<String>,
    progress_value: f32,
    pages_read: u32,
    total_pages: u32,
    /// chapter_id of the first chapter in the entry, used for navigation.
    first_chapter_id: String,
    /// Page to resume from (last saved progress for the first chapter).
    resume_page: usize,
}

// ---------------------------------------------------------------------------
// Page component
// ---------------------------------------------------------------------------

#[component]
pub fn LibraryPage(manga_id: String) -> Element {
    // Open the DB once per mount.
    let mut db_signal: Signal<Option<Rc<Db>>> = use_signal(|| None);

    // The resolved MangaMeta for the pre-filled manga context.
    let mut manga_meta_signal: Signal<Option<MangaMeta>> = use_signal(|| None);

    // Controls whether the importer modal is visible.
    let mut show_importer: Signal<bool> = use_signal(|| false);

    // Bump this to trigger a data refresh after import or delete.
    let mut refresh_counter: Signal<u32> = use_signal(|| 0);

    // Assembled display data for the grid.
    let mut display_data: Signal<Vec<EntryDisplayData>> = use_signal(Vec::new);

    // Which entry is pending deletion (index into display_data).
    let mut pending_delete: Signal<Option<usize>> = use_signal(|| None);

    // Open DB and load the manga meta on mount (or when manga_id changes).
    let manga_id_for_db = manga_id.clone();
    use_effect(move || {
        let mid = manga_id_for_db.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match Db::open().await {
                Ok(db) => {
                    let db = Rc::new(db);
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

    // Load (or re-load) the entry grid whenever DB becomes ready or
    // refresh_counter is bumped.
    let manga_id_for_load = manga_id.clone();
    use_effect(move || {
        let counter = *refresh_counter.read();
        let _ = counter; // read so the effect re-runs when it changes

        let Some(db) = db_signal.read().clone() else {
            return;
        };

        let mid = manga_id_for_load.clone();

        wasm_bindgen_futures::spawn_local(async move {
            // Revoke old blob URLs to avoid memory leaks.
            for old in display_data.read().iter() {
                if let Some(url) = &old.cover_url {
                    let _ = web_sys::Url::revoke_object_url(url);
                }
            }

            let chapters = match db.load_chapters_for_manga(&MangaId(mid.clone())).await {
                Ok(v) => v,
                Err(e) => {
                    web_sys::console::error_1(
                        &format!("load_chapters_for_manga error: {e}").into(),
                    );
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

            let entries = build_library_entries(chapters);
            let mut items: Vec<EntryDisplayData> = Vec::new();

            for entry in entries {
                // Get the first chapter (sorted by chapter_number) for cover + navigation.
                let chapter_list: Vec<_> = match &entry {
                    LibraryEntry::Tankobon { chapters, .. } => chapters.clone(),
                    LibraryEntry::LoneChapter(ch) => vec![ch.clone()],
                };

                let first_chapter = chapter_list
                    .iter()
                    .min_by(|a, b| a.chapter_number.total_cmp(&b.chapter_number));

                let first_chapter_id = match first_chapter {
                    Some(ch) => ch.id.0.clone(),
                    None => continue,
                };

                // Cover URL: page 0 of the first chapter.
                let cover_url: Option<String> = match first_chapter {
                    Some(ch) => match db.load_page(&ch.id, 0).await {
                        Ok(Some(blob)) => web_sys::Url::create_object_url_with_blob(&blob).ok(),
                        _ => None,
                    },
                    None => None,
                };

                // Progress: sum across all chapters in this entry.
                let total_pages: u32 = chapter_list.iter().map(|c| c.page_count).sum();

                let pages_read: u32 = all_progress
                    .iter()
                    .filter(|p| chapter_list.iter().any(|c| c.id == p.chapter_id))
                    .map(|p| p.page as u32)
                    .sum();

                let progress_value = if total_pages > 0 {
                    (pages_read as f32 / total_pages as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                // Resume page: last saved progress for the first chapter.
                let resume_page = all_progress
                    .iter()
                    .find(|p| p.chapter_id.0 == first_chapter_id)
                    .map(|p| p.page)
                    .unwrap_or(0);

                items.push(EntryDisplayData {
                    entry,
                    cover_url,
                    progress_value,
                    pages_read,
                    total_pages,
                    first_chapter_id,
                    resume_page,
                });
            }

            *display_data.write() = items;
        });
    });

    // -----------------------------------------------------------------------
    // Delete handler
    // -----------------------------------------------------------------------
    let manga_id_for_delete = manga_id.clone();
    let on_confirm_delete = move |_| {
        let Some(idx) = *pending_delete.read() else {
            return;
        };
        *pending_delete.write() = None;

        let Some(db) = db_signal.read().clone() else {
            return;
        };

        let entry = match display_data.read().get(idx) {
            Some(d) => d.entry.clone(),
            None => return,
        };

        let mid = manga_id_for_delete.clone();

        wasm_bindgen_futures::spawn_local(async move {
            let chapter_ids: Vec<ChapterId> = match &entry {
                LibraryEntry::Tankobon { chapters, .. } => {
                    chapters.iter().map(|c| c.id.clone()).collect()
                }
                LibraryEntry::LoneChapter(ch) => vec![ch.id.clone()],
            };

            for cid in &chapter_ids {
                if let Err(e) = db.delete_chapter(cid).await {
                    web_sys::console::error_1(&format!("delete_chapter error: {e}").into());
                }
                if let Err(e) = db.delete_pages_for_chapter(cid).await {
                    web_sys::console::error_1(
                        &format!("delete_pages_for_chapter error: {e}").into(),
                    );
                }
            }

            // Check if any chapters remain; if none, delete the manga entirely.
            match db.load_chapters_for_manga(&MangaId(mid.clone())).await {
                Ok(remaining) if remaining.is_empty() => {
                    if let Err(e) = db.delete_manga(&MangaId(mid.clone())).await {
                        web_sys::console::error_1(&format!("delete_manga error: {e}").into());
                    }
                    // Clear the last-opened position so the app doesn't
                    // try to reopen a page from a manga that no longer exists.
                    clear_last_opened();
                    // Navigate back to shelf since the manga is gone.
                    navigator().push(Route::Shelf {});
                    return;
                }
                Err(e) => {
                    web_sys::console::error_1(
                        &format!("load_chapters_for_manga error: {e}").into(),
                    );
                }
                _ => {}
            }

            *refresh_counter.write() += 1;
        });
    };

    let nav = navigator();
    let manga_id_for_nav = manga_id.clone();

    rsx! {
        div {
            class: "h-screen flex flex-col overflow-hidden",
            div {
                class: "flex items-center gap-2 px-4 py-3 border-b border-[#222] shrink-0",
                button {
                    class: "border-0 cursor-pointer text-sm px-2 py-1.5 rounded bg-transparent text-[#888] active:text-[#f0f0f0]",
                    onclick: move |_| {
                        nav.push(Route::Shelf {});
                    },
                    "← Back"
                }
                h1 { class: "text-base font-semibold flex-1 truncate", "Library" }
                button {
                    class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#e8b44a] text-black font-semibold active:bg-[#d4a03c]",
                    onclick: move |_| {
                        *show_importer.write() = true;
                    },
                    "+ Import"
                }
            }

            div {
                class: "grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] items-start gap-3 p-4 overflow-y-auto flex-1",
                if display_data.read().is_empty() {
                    p {
                        class: "text-center text-[#888] py-12 px-4",
                        "No chapters yet. Import something to get started."
                    }
                } else {
                    for (idx, item) in display_data.read().iter().cloned().enumerate() {
                        {
                            let first_chapter_id = item.first_chapter_id.clone();
                            let resume_page = item.resume_page;
                            let mid_click = manga_id_for_nav.clone();
                            let nav2 = navigator();
                            rsx! {
                                LibraryEntryCard {
                                    key: "{idx}",
                                    entry: item.entry.clone(),
                                    cover_url: item.cover_url.clone(),
                                    progress_value: item.progress_value,
                                    pages_read: item.pages_read,
                                    total_pages: item.total_pages,
                                    on_click: move |_| {
                                        nav2.push(Route::Reader {
                                            manga_id: mid_click.clone(),
                                            chapter_id: first_chapter_id.clone(),
                                            page: resume_page,
                                        });
                                    },
                                    on_delete: move |_| {
                                        *pending_delete.write() = Some(idx);
                                    },
                                }
                            }
                        }
                    }
                }
            }

            // Confirm-delete dialog
            if pending_delete.read().is_some() {
                ConfirmDialog {
                    message: "Delete this entry? All pages will be removed and cannot be recovered.".to_string(),
                    on_confirm: on_confirm_delete,
                    on_cancel: move |_| {
                        *pending_delete.write() = None;
                    },
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
