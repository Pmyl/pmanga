use std::rc::Rc;

use crate::storage::progress::{clear_last_opened, load_proxy_url};
use dioxus::prelude::*;
use js_sys::Promise;
use wasm_bindgen_futures::JsFuture;

use crate::{
    bridge::weebcentral::{fetch_chapter_list, fetch_chapter_pages},
    components::{confirm_dialog::ConfirmDialog, importer::Importer, manga_card::LibraryEntryCard},
    routes::Route,
    storage::tankobon::{fetch_tankobon_csv, lookup_tankobon},
    storage::{
        db::Db,
        models::{
            ChapterId, ChapterMeta, ChapterSource, LibraryEntry, MangaId, MangaMeta, MangaSource,
            build_library_entries,
        },
    },
};

// ---------------------------------------------------------------------------
// sleep_ms — same pattern as settings.rs
// ---------------------------------------------------------------------------

async fn sleep_ms(ms: i32) {
    let promise = Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .expect("no window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .expect("set_timeout failed");
    });
    JsFuture::from(promise).await.unwrap();
}

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
// Sync status
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum SyncStatus {
    Idle,
    /// Range panel is open; fields are pre-filled but sync not yet started.
    RangeInput,
    Syncing {
        status: String,
    },
    Done {
        new_chapters: usize,
    },
    Error {
        message: String,
    },
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

    // WeebCentral sync status.
    let mut sync_status: Signal<SyncStatus> = use_signal(|| SyncStatus::Idle);

    // Sync range inputs — populated with defaults when the panel opens.
    let mut sync_from: Signal<String> = use_signal(String::new);
    let mut sync_to: Signal<String> = use_signal(String::new);

    // Assembled display data for the grid.
    let mut display_data: Signal<Vec<EntryDisplayData>> = use_signal(Vec::new);

    // Which entry is pending deletion (index into display_data).
    let mut pending_delete: Signal<Option<usize>> = use_signal(|| None);

    // Open DB and load the manga meta on mount (or when manga_id changes).
    let manga_id_for_db = manga_id.clone();
    use_effect(move || {
        let mid = manga_id_for_db.clone();
        spawn(async move {
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

        spawn(async move {
            // Revoke old blob URLs — never revoke CDN URLs (WeebCentral).
            for old in display_data.read().iter() {
                if let Some(url) = &old.cover_url {
                    if url.starts_with("blob:") {
                        let _ = web_sys::Url::revoke_object_url(url);
                    }
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

                // Cover URL: CDN URL for WeebCentral, blob from IDB for local.
                let cover_url: Option<String> = match first_chapter {
                    Some(ch) if !ch.page_urls.is_empty() => {
                        // WeebCentral: use first CDN URL directly
                        ch.page_urls.first().cloned()
                    }
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

        spawn(async move {
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

                // Show Sync for WeebCentral manga, Import for local manga.
                if matches!(
                    manga_meta_signal.read().as_ref().map(|m| &m.source),
                    Some(MangaSource::WeebCentral { .. })
                ) {
                    button {
                        class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#5a9fd4] text-black font-semibold active:bg-[#4a8fc4] disabled:opacity-50",
                        disabled: matches!(*sync_status.read(), SyncStatus::Syncing { .. } | SyncStatus::RangeInput),
                        onclick: {
                            let manga_id_open = manga_id.clone();
                            move |_| {
                                // Compute default From = last existing chapter + 1.
                                let Some(db) = db_signal.read().clone() else { return };
                                let manga_id_open = manga_id_open.clone();
                                spawn(async move {
                                    let existing = db
                                        .load_chapters_for_manga(&MangaId(manga_id_open))
                                        .await
                                        .unwrap_or_default();
                                    let default_from = existing
                                        .iter()
                                        .map(|c| c.chapter_number)
                                        .fold(f32::NEG_INFINITY, f32::max);
                                    if default_from.is_finite() {
                                        // Round up to next whole chapter number.
                                        let next = (default_from.floor() as u32) + 1;
                                        *sync_from.write() = next.to_string();
                                    } else {
                                        sync_from.write().clear();
                                    }
                                    sync_to.write().clear();
                                    sync_status.set(SyncStatus::RangeInput);
                                });
                            }
                        },
                        "↻ Sync"
                    }
                } else {
                    button {
                        class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#e8b44a] text-black font-semibold active:bg-[#d4a03c]",
                        onclick: move |_| {
                            *show_importer.write() = true;
                        },
                        "+ Import"
                    }
                }
            }

            // Sync range panel
            if matches!(*sync_status.read(), SyncStatus::RangeInput) {
                div {
                    class: "px-4 py-3 bg-[#111] border-b border-[#222] shrink-0 flex flex-col gap-3",

                    div { class: "flex gap-2 items-center",
                        label { class: "text-xs text-[#888] shrink-0", "From ch." }
                        input {
                            class: "bg-[#1a1a1a] border border-[#333] rounded px-2 py-1 text-sm w-20 focus:outline-none focus:border-[#555]",
                            r#type: "number",
                            min: "1",
                            step: "0.1",
                            placeholder: "any",
                            value: "{sync_from}",
                            oninput: move |e| *sync_from.write() = e.value(),
                        }
                        label { class: "text-xs text-[#888] shrink-0", "to" }
                        input {
                            class: "bg-[#1a1a1a] border border-[#333] rounded px-2 py-1 text-sm w-20 focus:outline-none focus:border-[#555]",
                            r#type: "number",
                            min: "1",
                            step: "0.1",
                            placeholder: "any",
                            value: "{sync_to}",
                            oninput: move |e| *sync_to.write() = e.value(),
                        }
                        div { class: "flex gap-1 ml-auto",
                            button {
                                class: "border-0 cursor-pointer text-xs px-2 py-1 rounded bg-transparent border border-[#333] text-[#ccc] active:bg-[#222]",
                                onclick: move |_| sync_status.set(SyncStatus::Idle),
                                "Cancel"
                            }
                            button {
                                class: "border-0 cursor-pointer text-xs px-2 py-1 rounded bg-[#5a9fd4] text-black font-semibold active:bg-[#4a8fc4]",
                                onclick: {
                                    let manga_id_sync = manga_id.clone();
                                    move |_| {
                                        let Some(db) = db_signal.read().clone() else { return };
                                        let Some(manga_meta) = manga_meta_signal.read().clone() else { return };
                                        let manga_id_sync = manga_id_sync.clone();
                                        let series_url = match &manga_meta.source {
                                            MangaSource::WeebCentral { series_url } => series_url.clone(),
                                            _ => return,
                                        };

                                        let proxy_url = match load_proxy_url() {
                                            Some(u) if !u.trim().is_empty() => u,
                                            _ => {
                                                sync_status.set(SyncStatus::Error {
                                                    message: "Proxy URL not configured. Go to Settings first.".to_string(),
                                                });
                                                return;
                                            }
                                        };

                                        let series_id = series_url
                                            .split("/series/")
                                            .nth(1)
                                            .and_then(|s| s.split('/').next())
                                            .unwrap_or("")
                                            .to_string();

                                        // Parse range inputs.
                                        let from_raw = sync_from.read().trim().to_string();
                                        let to_raw = sync_to.read().trim().to_string();

                                        let from_ch: Option<f32> = if from_raw.is_empty() {
                                            None
                                        } else {
                                            match from_raw.parse::<f32>() {
                                                Ok(n) => Some(n),
                                                Err(_) => {
                                                    sync_status.set(SyncStatus::Error {
                                                        message: format!("\"From\" must be a number, got \"{from_raw}\"."),
                                                    });
                                                    return;
                                                }
                                            }
                                        };

                                        let to_ch: Option<f32> = if to_raw.is_empty() {
                                            None
                                        } else {
                                            match to_raw.parse::<f32>() {
                                                Ok(n) => Some(n),
                                                Err(_) => {
                                                    sync_status.set(SyncStatus::Error {
                                                        message: format!("\"To\" must be a number, got \"{to_raw}\"."),
                                                    });
                                                    return;
                                                }
                                            }
                                        };

                                        if let (Some(f), Some(t)) = (from_ch, to_ch) {
                                            if f > t {
                                                sync_status.set(SyncStatus::Error {
                                                    message: format!("\"From\" ({f}) must not be greater than \"To\" ({t})."),
                                                });
                                                return;
                                            }
                                        }

                                        sync_status.set(SyncStatus::Syncing {
                                            status: "Fetching chapter list…".to_string(),
                                        });

                                        spawn(async move {
                                            let manga_title = manga_meta.title.clone();
                                            let csv_rows_snapshot = fetch_tankobon_csv().await;
                                            // Fetch all chapters from proxy
                                            let mut remote_chapters =
                                                match fetch_chapter_list(&proxy_url, &series_id).await {
                                                    Ok(chs) => chs,
                                                    Err(e) => {
                                                        sync_status.set(SyncStatus::Error {
                                                            message: format!("Failed to fetch chapters: {e}"),
                                                        });
                                                        return;
                                                    }
                                                };

                                            remote_chapters
                                                .sort_by(|a, b| a.number.total_cmp(&b.number));

                                            // Load existing chapter IDs from IDB
                                            let existing = match db
                                                .load_chapters_for_manga(&MangaId(manga_id_sync.clone()))
                                                .await
                                            {
                                                Ok(chs) => chs,
                                                Err(e) => {
                                                    sync_status.set(SyncStatus::Error {
                                                        message: format!("Failed to load existing chapters: {e}"),
                                                    });
                                                    return;
                                                }
                                            };

                                            let existing_ids: std::collections::HashSet<String> =
                                                existing.iter().map(|c| c.id.0.clone()).collect();

                                            // Filter: not already in IDB + within requested range.
                                            let new_chapters: Vec<_> = remote_chapters
                                                .into_iter()
                                                .filter(|c| {
                                                    if existing_ids.contains(&c.id) {
                                                        return false;
                                                    }
                                                    if let Some(f) = from_ch {
                                                        if c.number < f { return false; }
                                                    }
                                                    if let Some(t) = to_ch {
                                                        if c.number > t { return false; }
                                                    }
                                                    true
                                                })
                                                .collect();

                                            let total_new = new_chapters.len();
                                            let mut saved = 0usize;

                                            for chapter in &new_chapters {
                                                sync_status.set(SyncStatus::Syncing {
                                                    status: format!(
                                                        "Fetching pages for chapter {} ({}/{})…",
                                                        chapter.number,
                                                        saved + 1,
                                                        total_new
                                                    ),
                                                });

                                                let pages =
                                                    match fetch_chapter_pages(&proxy_url, &chapter.id)
                                                        .await
                                                    {
                                                        Ok(p) => p,
                                                        Err(e) => {
                                                            sync_status.set(SyncStatus::Error {
                                                                message: format!(
                                                                    "Failed to fetch pages for chapter {}: {e}",
                                                                    chapter.number
                                                                ),
                                                            });
                                                            return;
                                                        }
                                                    };

                                                let tankobon_number = lookup_tankobon(
                                                    &manga_title,
                                                    chapter.number,
                                                    &csv_rows_snapshot,
                                                );

                                                let chapter_meta = ChapterMeta {
                                                    id: ChapterId(chapter.id.clone()),
                                                    manga_id: MangaId(manga_id_sync.clone()),
                                                    chapter_number: chapter.number,
                                                    tankobon_number,
                                                    filename: format!("Chapter {}", chapter.number),
                                                    page_count: pages.len() as u32,
                                                    source: ChapterSource::WeebCentral {
                                                        chapter_id: chapter.id.clone(),
                                                    },
                                                    page_urls: pages.into_iter().map(|p| p.url).collect(),
                                                };

                                                if let Err(e) = db.save_chapter(&chapter_meta).await {
                                                    sync_status.set(SyncStatus::Error {
                                                        message: format!(
                                                            "Failed to save chapter {}: {e}",
                                                            chapter.number
                                                        ),
                                                    });
                                                    return;
                                                }

                                                saved += 1;
                                                sleep_ms(500).await;
                                            }

                                            sync_status.set(SyncStatus::Done {
                                                new_chapters: total_new,
                                            });
                                            *refresh_counter.write() += 1;
                                        });
                                    }
                                },
                                "Go"
                            }
                        }
                    }
                }
            }

            // Sync status banner
            match sync_status.read().clone() {
                SyncStatus::RangeInput => rsx! {},
                SyncStatus::Syncing { ref status } => rsx! {
                    div {
                        class: "px-4 py-2 bg-[#1a2a3a] text-sm text-[#5a9fd4] border-b border-[#222] shrink-0",
                        "{status}"
                    }
                },
                SyncStatus::Done { new_chapters } => rsx! {
                    div {
                        class: "px-4 py-2 bg-[#1a2a1a] text-sm text-[#4caf50] border-b border-[#222] shrink-0 flex items-center justify-between",
                        if new_chapters > 0 {
                            span { "✓ Sync complete — {new_chapters} new chapter(s) added." }
                        } else {
                            span { "✓ Already up to date." }
                        }
                        button {
                            class: "border-0 cursor-pointer text-xs text-[#666] bg-transparent px-1",
                            onclick: move |_| sync_status.set(SyncStatus::Idle),
                            "✕"
                        }
                    }
                },
                SyncStatus::Error { ref message } => rsx! {
                    div {
                        class: "px-4 py-2 bg-[#2a1a1a] text-sm text-[#cf6679] border-b border-[#222] shrink-0 flex items-center justify-between",
                        span { "{message}" }
                        button {
                            class: "border-0 cursor-pointer text-xs text-[#666] bg-transparent px-1",
                            onclick: move |_| sync_status.set(SyncStatus::Idle),
                            "✕"
                        }
                    }
                },
                SyncStatus::Idle => rsx! {},
            }

            div {
                class: "overflow-y-auto flex-1",
                div {
                class: "grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] items-start gap-3 p-4",
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
