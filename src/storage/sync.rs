//! Shared WeebCentral chapter sync helpers.
//!
//! This module contains the core logic for downloading WeebCentral chapters
//! and saving them to the database — logic that is shared between the initial
//! importer, the per-manga sync, and the bulk sync-all-caught-up feature.

use js_sys::Promise;
use wasm_bindgen_futures::JsFuture;

use crate::{
    bridge::weebcentral::{WcChapter, fetch_chapter_pages},
    storage::{
        db::Db,
        models::{ChapterId, ChapterMeta, ChapterSource, MangaId, MangaMeta},
        tankobon::{TankobonRow, lookup_tankobon},
    },
};

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Wait `ms` milliseconds (browser/WASM-compatible).
pub async fn sleep_ms(ms: i32) {
    let promise = Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .expect("no window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .expect("set_timeout failed");
    });
    JsFuture::from(promise).await.unwrap();
}

/// Extract the series ID from a WeebCentral series URL.
///
/// `https://weebcentral.com/series/01J76XY7E9FNDZ1DBBM6PBJPFK/one-piece`
/// → `Some("01J76XY7E9FNDZ1DBBM6PBJPFK")`
pub fn extract_series_id(url: &str) -> Option<String> {
    let after = url.split("/series/").nth(1)?;
    let id = after.split('/').next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

// ---------------------------------------------------------------------------
// Core sync
// ---------------------------------------------------------------------------

/// Download `chapters` from the proxy and save them to the database.
///
/// `on_progress(saved_so_far, total, status_message)` is called before each
/// chapter is fetched so the caller can update their UI.
///
/// Returns the number of chapters successfully saved, or an error message.
pub async fn download_chapters_to_db(
    db: &Db,
    proxy_url: &str,
    manga_id: &MangaId,
    manga_title: &str,
    chapters: &[WcChapter],
    csv_rows: &[TankobonRow],
    mut on_progress: impl FnMut(usize, usize, String),
) -> Result<usize, String> {
    let total = chapters.len();
    for (i, chapter) in chapters.iter().enumerate() {
        on_progress(
            i,
            total,
            format!(
                "Fetching pages for chapter {} ({}/{})…",
                chapter.number,
                i + 1,
                total
            ),
        );

        let pages = fetch_chapter_pages(proxy_url, &chapter.id)
            .await
            .map_err(|e| format!("Failed to fetch pages for chapter {}: {e}", chapter.number))?;

        let tankobon_number = lookup_tankobon(manga_title, chapter.number, csv_rows);

        let chapter_meta = ChapterMeta {
            id: ChapterId(chapter.id.clone()),
            manga_id: manga_id.clone(),
            chapter_number: chapter.number,
            tankobon_number,
            filename: format!("Chapter {}", chapter.number),
            page_count: pages.len() as u32,
            source: ChapterSource::WeebCentral {
                chapter_id: chapter.id.clone(),
            },
            page_urls: pages.into_iter().map(|p| p.url).collect(),
        };

        db.save_chapter(&chapter_meta)
            .await
            .map_err(|e| format!("Failed to save chapter {}: {e}", chapter.number))?;

        sleep_ms(500).await;
    }
    Ok(total)
}

/// Update `latest_downloaded_chapter` in `MangaMeta` after syncing new chapters.
///
/// The field only ever increases — an existing value is preserved if it is
/// already higher than the maximum chapter number in `new_chapters`.
///
/// Returns the updated `MangaMeta` if a DB write was made, `None` otherwise.
pub async fn update_latest_downloaded_chapter(
    db: &Db,
    manga_id: &MangaId,
    new_chapters: &[WcChapter],
) -> Option<MangaMeta> {
    let max_new = new_chapters
        .iter()
        .map(|c| c.number)
        .fold(f32::NEG_INFINITY, f32::max);

    if !max_new.is_finite() {
        return None;
    }

    match db.load_manga(manga_id).await {
        Ok(Some(mut meta)) => {
            let current = meta.latest_downloaded_chapter.unwrap_or(f32::NEG_INFINITY);
            if max_new <= current {
                return None;
            }
            meta.latest_downloaded_chapter = Some(max_new);
            match db.save_manga(&meta).await {
                Ok(()) => Some(meta),
                Err(e) => {
                    web_sys::console::error_1(&format!("save_manga error: {e}").into());
                    None
                }
            }
        }
        _ => None,
    }
}
