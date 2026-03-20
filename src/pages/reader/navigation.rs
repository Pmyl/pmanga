//! Navigation helpers for the reader: debounce, cross-chapter page jumps, and
//! progress persistence.

use std::cell::Cell;
use std::rc::Rc;

use dioxus::prelude::*;

use crate::{
    routes::Route,
    storage::{
        db::Db,
        models::{ChapterId, ChapterMeta, LastOpened, MangaId, ReadingProgress},
        progress::save_last_opened,
    },
};

// ---------------------------------------------------------------------------
// Navigation debounce
// ---------------------------------------------------------------------------

/// Minimum milliseconds between accepted navigations.
const DEBOUNCE_MS: f64 = 100.0;

thread_local! {
    static LAST_NAV_MS: Cell<f64> = const { Cell::new(0.0) };
}

/// Returns `true` if enough time has passed since the last navigation and
/// resets the debounce timer.  Returns `false` (and does NOT update the timer)
/// when called too soon.
pub fn nav_debounce_ok() -> bool {
    let now = js_sys::Date::now();
    LAST_NAV_MS.with(|last| {
        let prev = last.get();
        if now - prev > DEBOUNCE_MS {
            last.set(now);
            true
        } else {
            false
        }
    })
}

// ---------------------------------------------------------------------------
// Cross-chapter navigation
// ---------------------------------------------------------------------------

/// Navigate to `target_page` within the current chapter, crossing chapter
/// boundaries as needed.
///
/// * Negative `target_page` → last page of the previous chapter (or no-op if
///   already at the first chapter).
/// * `target_page >= chapter_pages` → page 0 of the next chapter (or no-op if
///   already at the last chapter).
/// * Otherwise → that page within the current chapter.
///
/// Progress is saved on every successful navigation.
pub fn go_to_page(
    target_page: isize,
    manga_id: &str,
    chapter_id: &str,
    chapter_pages: u32,
    all_chapters: &[ChapterMeta],
    current_chapter_idx: usize,
    db: Option<Rc<Db>>,
) {
    if !nav_debounce_ok() {
        return;
    }

    let nav = navigator();

    if target_page < 0 {
        // Go to last page of the previous chapter.
        if current_chapter_idx == 0 {
            return; // Already at the first chapter — clamp.
        }
        let prev = &all_chapters[current_chapter_idx - 1];
        let last_page = prev.page_count.saturating_sub(1) as usize;
        save_progress_fire_and_forget(db, prev.manga_id.0.clone(), prev.id.0.clone(), last_page);
        nav.replace(Route::Reader {
            manga_id: prev.manga_id.0.clone(),
            chapter_id: prev.id.0.clone(),
            page: last_page,
        });
    } else if target_page >= chapter_pages as isize {
        // Go to page 0 of the next chapter.
        if current_chapter_idx + 1 >= all_chapters.len() {
            return; // Already at the last chapter — clamp.
        }
        let next = &all_chapters[current_chapter_idx + 1];
        save_progress_fire_and_forget(db, next.manga_id.0.clone(), next.id.0.clone(), 0);
        nav.replace(Route::Reader {
            manga_id: next.manga_id.0.clone(),
            chapter_id: next.id.0.clone(),
            page: 0,
        });
    } else {
        let target = target_page as usize;
        save_progress_fire_and_forget(db, manga_id.to_string(), chapter_id.to_string(), target);
        nav.replace(Route::Reader {
            manga_id: manga_id.to_string(),
            chapter_id: chapter_id.to_string(),
            page: target,
        });
    }
}

// ---------------------------------------------------------------------------
// Progress persistence
// ---------------------------------------------------------------------------

/// Persist reading progress synchronously to `localStorage` and
/// asynchronously to IndexedDB (fire-and-forget).
pub fn save_progress_fire_and_forget(
    db: Option<Rc<Db>>,
    manga_id: String,
    chapter_id: String,
    page: usize,
) {
    // localStorage — synchronous, do it immediately.
    save_last_opened(&LastOpened {
        manga_id: manga_id.clone(),
        chapter_id: chapter_id.clone(),
        page,
    });

    // IndexedDB — async, fire-and-forget.
    if let Some(db) = db {
        let progress = ReadingProgress {
            manga_id: MangaId(manga_id),
            chapter_id: ChapterId(chapter_id),
            page,
        };
        wasm_bindgen_futures::spawn_local(async move {
            let _ = db.save_progress(&progress).await;
        });
    }
}
