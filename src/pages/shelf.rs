use std::rc::Rc;

use crate::{
    bridge::weebcentral::fetch_chapter_list,
    components::{
        confirm_dialog::ConfirmDialog,
        import_source_dialog::ImportSourceDialog,
        importer::Importer,
        manga_card::MangaCard,
        weebcentral_importer::WeebCentralImporter,
    },
    routes::Route,
    storage::{
        db::Db,
        models::{LastOpened, MangaId, MangaSource},
        progress::{
            clear_last_opened, is_startup_redirect_done, load_last_opened,
            load_proxy_url, mark_startup_redirect_done, save_last_opened,
        },
        sync::{download_chapters_to_db, extract_series_id, next_chapter_after, update_latest_downloaded_chapter},
        tankobon::fetch_tankobon_csv,
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
    /// Highest chapter number ever downloaded (from MangaMeta).
    /// Used to show "All caught up to ch. XX" when the manga is empty.
    last_downloaded_chapter: Option<f32>,
    /// WeebCentral series URL, if applicable. Used for sync-all-caught-up.
    series_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Sync-all-caught-up status
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum SyncAllStatus {
    Idle,
    Syncing {
        status: String,
        current: usize,
        total: usize,
    },
    Done {
        total_new: usize,
    },
    Error {
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Page component
// ---------------------------------------------------------------------------

#[component]
pub fn ShelfPage() -> Element {
    // Open the DB once per mount.
    let mut db_signal: Signal<Option<Rc<Db>>> = use_signal(|| None);

    // Controls whether the import-source picker modal is visible.
    let mut show_import_source: Signal<bool> = use_signal(|| false);

    // Controls whether the importer modal is visible.
    let mut show_importer: Signal<bool> = use_signal(|| false);

    // Controls whether the WeebCentral importer modal is visible.
    let mut show_wc_importer: Signal<bool> = use_signal(|| false);

    // Bump this to trigger a data refresh after import or delete.
    let mut refresh_counter: Signal<u32> = use_signal(|| 0);

    // Assembled display data for the grid.
    let mut display_data: Signal<Vec<MangaDisplayData>> = use_signal(Vec::new);

    // manga_id pending deletion confirmation.
    let mut pending_delete_manga: Signal<Option<String>> = use_signal(|| None);

    // Status for the sync-all-caught-up operation.
    let mut sync_all_status: Signal<SyncAllStatus> = use_signal(|| SyncAllStatus::Idle);

    // Open DB on mount.
    use_effect(move || {
        spawn(async move {
            match Db::open().await {
                Ok(db) => *db_signal.write() = Some(Rc::new(db)),
                Err(e) => web_sys::console::error_1(&format!("DB open error: {e}").into()),
            }
        });
    });

    // Startup redirect: if there is a saved session state in localStorage,
    // navigate to the correct page on first mount — but only once per
    // browser session so that navigating *back* to the shelf does not
    // re-trigger the redirect.
    use_effect(move || {
        if !is_startup_redirect_done() {
            mark_startup_redirect_done();
            let nav = navigator();
            match load_last_opened() {
                Some(LastOpened::Reader { manga_id, chapter_id, page }) => {
                    nav.push(Route::Reader { manga_id, chapter_id, page });
                }
                Some(LastOpened::Library { manga_id }) => {
                    nav.push(Route::Library { manga_id });
                }
                Some(LastOpened::Shelf) | None => {
                    // Stay on the shelf.
                }
            }
        } else {
            save_last_opened(&LastOpened::Shelf);
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
                // Fall back to the stored cover_url_fallback when there are no chapters.
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
                    // No local chapters — use the stored fallback (WeebCentral only).
                    manga.cover_url_fallback.clone()
                };

                // Total pages across all chapters for this manga.
                let total_pages: u32 = chapters.iter().map(|c| c.page_count).sum();

                // Pages read: sum of (progress.page + 1) clamped to page_count across all
                // chapters of this manga.  p.page is a 0-based index, so +1 converts it to
                // a read-page count.  Clamping prevents an oversized saved value from
                // inflating the total beyond 100 %.
                let pages_read: u32 = all_progress
                    .iter()
                    .filter_map(|p| {
                        if p.manga_id != manga.id {
                            return None;
                        }
                        let chapter = chapters.iter().find(|c| c.id == p.chapter_id)?;
                        Some((p.page as u32 + 1).min(chapter.page_count))
                    })
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
                    last_downloaded_chapter: manga.latest_downloaded_chapter,
                    series_url: match &manga.source {
                        MangaSource::WeebCentral { series_url } => Some(series_url.clone()),
                        _ => None,
                    },
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
                        class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#5a9fd4] text-black font-semibold active:bg-[#4a8fc4] disabled:opacity-50",
                        disabled: matches!(*sync_all_status.read(), SyncAllStatus::Syncing { .. }),
                        onclick: move |_| {
                            if matches!(*sync_all_status.read(), SyncAllStatus::Syncing { .. }) {
                                return;
                            }

                            let Some(db) = db_signal.read().clone() else { return };

                            let proxy_url = match load_proxy_url() {
                                Some(u) if !u.trim().is_empty() => u,
                                _ => {
                                    sync_all_status.set(SyncAllStatus::Error {
                                        message: "Proxy URL not configured. Go to Settings first.".to_string(),
                                    });
                                    return;
                                }
                            };

                            // Collect all caught-up WeebCentral mangas.
                            // "Caught up" = no chapters (but was previously synced)
                            //            OR all downloaded chapters have been read.
                            let caught_up: Vec<(String, String, String, Option<f32>)> =
                                display_data
                                    .read()
                                    .iter()
                                    .filter_map(|item| {
                                        let series_url = item.series_url.clone()?;
                                        let is_caught_up =
                                            (item.total_pages == 0
                                                && item.last_downloaded_chapter.is_some())
                                                || (item.total_pages > 0
                                                    && item.progress_value >= 1.0);
                                        if is_caught_up {
                                            Some((
                                                item.manga_id.clone(),
                                                item.title.clone(),
                                                series_url,
                                                item.last_downloaded_chapter,
                                            ))
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();

                            if caught_up.is_empty() {
                                sync_all_status.set(SyncAllStatus::Error {
                                    message: "No caught-up WeebCentral series are eligible to sync."
                                        .to_string(),
                                });
                                return;
                            }

                            let total_mangas = caught_up.len();
                            sync_all_status.set(SyncAllStatus::Syncing {
                                status: "Starting…".to_string(),
                                current: 0,
                                total: total_mangas,
                            });

                            spawn(async move {
                                let csv_rows_snapshot = fetch_tankobon_csv().await;
                                let mut grand_total_new = 0usize;

                                for (idx, (manga_id, title, series_url, last_downloaded)) in
                                    caught_up.iter().enumerate()
                                {
                                    sync_all_status.set(SyncAllStatus::Syncing {
                                        status: format!("Fetching chapters for \"{title}\"…"),
                                        current: idx + 1,
                                        total: total_mangas,
                                    });

                                    let series_id = match extract_series_id(series_url) {
                                        Some(id) => id,
                                        None => {
                                            sync_all_status.set(SyncAllStatus::Error {
                                                message: format!(
                                                    "Could not extract series ID from URL for \"{title}\"."
                                                ),
                                            });
                                            return;
                                        }
                                    };

                                    let mut remote_chapters =
                                        match fetch_chapter_list(&proxy_url, &series_id).await {
                                            Ok(chs) => chs,
                                            Err(e) => {
                                                sync_all_status.set(SyncAllStatus::Error {
                                                    message: format!(
                                                        "Failed to fetch chapters for \"{title}\": {e}"
                                                    ),
                                                });
                                                return;
                                            }
                                        };

                                    remote_chapters
                                        .sort_by(|a, b| a.number.total_cmp(&b.number));

                                    let existing = match db
                                        .load_chapters_for_manga(&MangaId(manga_id.clone()))
                                        .await
                                    {
                                        Ok(chs) => chs,
                                        Err(e) => {
                                            sync_all_status.set(SyncAllStatus::Error {
                                                message: format!(
                                                    "Failed to load existing chapters for \"{title}\": {e}"
                                                ),
                                            });
                                            return;
                                        }
                                    };

                                    let existing_ids: std::collections::HashSet<String> =
                                        existing.iter().map(|c| c.id.0.clone()).collect();

                                    // Exclude chapters already known to have been downloaded
                                    // (and presumably deleted or still present) to avoid
                                    // re-downloading old chapters.  We use next_chapter_after
                                    // so fractional successors (e.g. 10.5 after
                                    // last_downloaded=10.0) are still included.
                                    // This bound applies regardless of whether the DB still
                                    // contains those chapters — the `existing_ids` check above
                                    // handles skipping chapters that are already present.
                                    let from_ch: Option<f32> = last_downloaded.map(next_chapter_after);

                                    let new_chapters: Vec<_> = remote_chapters
                                        .into_iter()
                                        .filter(|c| {
                                            if existing_ids.contains(&c.id) {
                                                return false;
                                            }
                                            if let Some(f) = from_ch {
                                                if c.number < f {
                                                    return false;
                                                }
                                            }
                                            true
                                        })
                                        .collect();

                                    let total_new = new_chapters.len();

                                    match download_chapters_to_db(
                                        &db,
                                        &proxy_url,
                                        &MangaId(manga_id.clone()),
                                        title,
                                        &new_chapters,
                                        &csv_rows_snapshot,
                                        |_saved, _total, status| {
                                            // Prefix the per-chapter status with the manga title
                                            // so the user knows which series is being processed.
                                            sync_all_status.set(SyncAllStatus::Syncing {
                                                status: format!("\"{title}\" — {status}"),
                                                current: idx + 1,
                                                total: total_mangas,
                                            });
                                        },
                                    )
                                    .await
                                    {
                                        Ok(_) => {}
                                        Err(e) => {
                                            sync_all_status.set(SyncAllStatus::Error {
                                                message: e,
                                            });
                                            return;
                                        }
                                    }

                                    update_latest_downloaded_chapter(
                                        &db,
                                        &MangaId(manga_id.clone()),
                                        &new_chapters,
                                    )
                                    .await;

                                    grand_total_new += total_new;
                                }

                                sync_all_status.set(SyncAllStatus::Done {
                                    total_new: grand_total_new,
                                });
                                *refresh_counter.write() += 1;
                            });
                        },
                        "↻ Sync"
                    }
                    button {
                        class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#e8b44a] text-black font-semibold active:bg-[#d4a03c]",
                        onclick: move |_| {
                            *show_import_source.write() = true;
                        },
                        "+ Import"
                    }
                }
            }

            // Sync-all status banner
            match sync_all_status.read().clone() {
                SyncAllStatus::Idle => rsx! {},
                SyncAllStatus::Syncing { ref status, current, total } => rsx! {
                    div {
                        class: "px-4 py-2 bg-[#1a2a3a] text-sm text-[#5a9fd4] border-b border-[#222] shrink-0",
                        "{status} ({current}/{total})"
                    }
                },
                SyncAllStatus::Done { total_new } => rsx! {
                    div {
                        class: "px-4 py-2 bg-[#1a2a1a] text-sm text-[#4caf50] border-b border-[#222] shrink-0 flex items-center justify-between",
                        if total_new > 0 {
                            {
                                let word = if total_new == 1 { "chapter" } else { "chapters" };
                                rsx! { span { "✓ Sync complete — {total_new} new {word} added." } }
                            }
                        } else {
                            span { "✓ All caught-up manga are up to date." }
                        }
                        button {
                            class: "border-0 cursor-pointer text-xs text-[#666] bg-transparent px-1",
                            onclick: move |_| sync_all_status.set(SyncAllStatus::Idle),
                            "✕"
                        }
                    }
                },
                SyncAllStatus::Error { ref message } => rsx! {
                    div {
                        class: "px-4 py-2 bg-[#2a1a1a] text-sm text-[#cf6679] border-b border-[#222] shrink-0 flex items-center justify-between",
                        span { "{message}" }
                        button {
                            class: "border-0 cursor-pointer text-xs text-[#666] bg-transparent px-1",
                            onclick: move |_| sync_all_status.set(SyncAllStatus::Idle),
                            "✕"
                        }
                    }
                },
            }

            div {
                class: "overflow-y-auto flex-1",
                div {
                class: "grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] items-start gap-3 p-4",
                if display_data.read().is_empty() {
                    p {
                        class: "text-center text-[#888] py-12 px-4",
                        "No manga yet. Import something to get started."
                    }
                } else {
                    for item in display_data.read().iter().cloned() {
                        {
                            let manga_id = item.manga_id.clone();
                            let manga_id_for_delete = item.manga_id.clone();
                            let nav2 = navigator();
                            rsx! {
                                MangaCard {
                                    key: "{item.manga_id}",
                                    manga: crate::storage::models::MangaMeta {
                                        id: MangaId(item.manga_id.clone()),
                                        title: item.title.clone(),
                                        mangadex_id: None,
                                        source: crate::storage::models::MangaSource::Local,
                                        latest_downloaded_chapter: item.last_downloaded_chapter,
                                        cover_url_fallback: None,
                                    },
                                    cover_url: item.cover_url.clone(),
                                    progress_value: item.progress_value,
                                    pages_read: item.pages_read,
                                    total_pages: item.total_pages,
                                    last_downloaded_chapter: item.last_downloaded_chapter,
                                    is_web: item.is_web,
                                    on_click: move |_| {
                                        nav2.push(Route::Library {
                                            manga_id: manga_id.clone(),
                                        });
                                    },
                                    on_delete: move |_| {
                                        *pending_delete_manga.write() =
                                            Some(manga_id_for_delete.clone());
                                    },
                                }
                            }
                        }
                    }
                }
                }
            }

            // Confirm-delete dialog for whole manga
            if pending_delete_manga.read().is_some() {
                ConfirmDialog {
                    message: "Delete this manga? All downloaded chapters and pages will be permanently removed.".to_string(),
                    on_confirm: move |_| {
                        let Some(mid) = pending_delete_manga.read().clone() else {
                            return;
                        };
                        *pending_delete_manga.write() = None;
                        let Some(db) = db_signal.read().clone() else { return };
                        spawn(async move {
                            let manga_id = MangaId(mid.clone());
                            // Delete all chapters (and their pages + progress).
                            let chapters =
                                db.load_chapters_for_manga(&manga_id).await.unwrap_or_default();
                            for chapter in &chapters {
                                if let Err(e) = db.delete_chapter(&chapter.id).await {
                                    web_sys::console::error_1(
                                        &format!("delete_chapter error: {e}").into(),
                                    );
                                }
                                if let Err(e) = db.delete_pages_for_chapter(&chapter.id).await {
                                    web_sys::console::error_1(
                                        &format!("delete_pages_for_chapter error: {e}").into(),
                                    );
                                }
                                if let Err(e) =
                                    db.delete_progress_for_chapter(&chapter.id).await
                                {
                                    web_sys::console::error_1(
                                        &format!("delete_progress_for_chapter error: {e}").into(),
                                    );
                                }
                            }
                            // Delete the manga record itself.
                            if let Err(e) = db.delete_manga(&manga_id).await {
                                web_sys::console::error_1(
                                    &format!("delete_manga error: {e}").into(),
                                );
                            }
                            // Clear last_opened if it referenced this manga.
                            match load_last_opened() {
                                Some(LastOpened::Library { manga_id: ref lid })
                                    if lid == &mid =>
                                {
                                    clear_last_opened();
                                }
                                Some(LastOpened::Reader { manga_id: ref rid, .. })
                                    if rid == &mid =>
                                {
                                    clear_last_opened();
                                }
                                _ => {}
                            }
                            *refresh_counter.write() += 1;
                        });
                    },
                    on_cancel: move |_| {
                        *pending_delete_manga.write() = None;
                    },
                }
            }

            // Import source picker modal
            if *show_import_source.read() {
                ImportSourceDialog {
                    on_local: move |_| {
                        *show_import_source.write() = false;
                        *show_importer.write() = true;
                    },
                    on_weebcentral: move |_| {
                        *show_import_source.write() = false;
                        *show_wc_importer.write() = true;
                    },
                    on_cancel: move |_| {
                        *show_import_source.write() = false;
                    },
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
