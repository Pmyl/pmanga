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

// ---------------------------------------------------------------------------
// Pure navigation decision (extracted for unit-testability)
// ---------------------------------------------------------------------------

/// The outcome of a chapter-boundary navigation calculation, independent of
/// the Dioxus router or any browser APIs.
#[derive(Debug, PartialEq)]
pub(crate) enum NavDecision {
    /// Stay in the same chapter and show `page`.
    SamePage(usize),
    /// Jump to chapter at `chapter_idx` (into `all_chapters`), landing on its
    /// last page (use for backwards navigation).
    PreviousChapter { chapter_idx: usize },
    /// Jump to chapter at `chapter_idx` (into `all_chapters`), landing on
    /// page 0 (use for forwards navigation).
    NextChapter { chapter_idx: usize },
    /// Already at the first / last chapter boundary — do nothing.
    Clamp,
}

/// Compute the navigation outcome for `target_page` without touching the
/// router, storage, or any browser API.
///
/// Mirrors the boundary logic in [`go_to_page`] so it can be tested natively.
pub(crate) fn resolve_nav(
    target_page: isize,
    chapter_pages: u32,
    current_chapter_idx: usize,
    chapter_count: usize,
) -> NavDecision {
    if target_page < 0 {
        if current_chapter_idx == 0 {
            NavDecision::Clamp
        } else {
            NavDecision::PreviousChapter {
                chapter_idx: current_chapter_idx - 1,
            }
        }
    } else if target_page >= chapter_pages as isize {
        if current_chapter_idx + 1 >= chapter_count {
            NavDecision::Clamp
        } else {
            NavDecision::NextChapter {
                chapter_idx: current_chapter_idx + 1,
            }
        }
    } else {
        NavDecision::SamePage(target_page as usize)
    }
}

// ---------------------------------------------------------------------------
// Cross-chapter navigation (browser-coupled)
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

    match resolve_nav(target_page, chapter_pages, current_chapter_idx, all_chapters.len()) {
        NavDecision::PreviousChapter { chapter_idx } => {
            let prev = &all_chapters[chapter_idx];
            let last_page = prev.page_count.saturating_sub(1) as usize;
            save_progress_fire_and_forget(db, prev.manga_id.0.clone(), prev.id.0.clone(), last_page);
            nav.replace(Route::Reader {
                manga_id: prev.manga_id.0.clone(),
                chapter_id: prev.id.0.clone(),
                page: last_page,
            });
        }
        NavDecision::NextChapter { chapter_idx } => {
            let next = &all_chapters[chapter_idx];
            save_progress_fire_and_forget(db, next.manga_id.0.clone(), next.id.0.clone(), 0);
            nav.replace(Route::Reader {
                manga_id: next.manga_id.0.clone(),
                chapter_id: next.id.0.clone(),
                page: 0,
            });
        }
        NavDecision::SamePage(target) => {
            save_progress_fire_and_forget(db, manga_id.to_string(), chapter_id.to_string(), target);
            nav.replace(Route::Reader {
                manga_id: manga_id.to_string(),
                chapter_id: chapter_id.to_string(),
                page: target,
            });
        }
        NavDecision::Clamp => {} // Already at the boundary — do nothing.
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{NavDecision, resolve_nav};

    // ----- Within-chapter navigation -----

    #[test]
    fn same_page_for_mid_chapter_target() {
        // 5 pages, currently at chapter 1 of 3.  Targeting page 3 stays in chapter.
        assert_eq!(resolve_nav(3, 5, 1, 3), NavDecision::SamePage(3));
    }

    #[test]
    fn same_page_for_first_page_of_chapter() {
        assert_eq!(resolve_nav(0, 5, 0, 1), NavDecision::SamePage(0));
    }

    #[test]
    fn same_page_for_last_page_of_chapter() {
        // page_count = 5; last valid page = 4.
        assert_eq!(resolve_nav(4, 5, 0, 3), NavDecision::SamePage(4));
    }

    // ----- Forward boundary: advance to next chapter -----

    #[test]
    fn next_chapter_when_target_equals_page_count() {
        // page_count = 3, target = 3 → one past the end → next chapter.
        assert_eq!(
            resolve_nav(3, 3, 0, 2),
            NavDecision::NextChapter { chapter_idx: 1 }
        );
    }

    #[test]
    fn next_chapter_index_is_current_plus_one() {
        // Currently at chapter index 2 of 5 → next is chapter index 3.
        assert_eq!(
            resolve_nav(10, 5, 2, 5),
            NavDecision::NextChapter { chapter_idx: 3 }
        );
    }

    // ----- Forward boundary: clamp at last chapter -----

    #[test]
    fn clamp_forward_when_already_at_last_chapter() {
        // Only one chapter; tapping Next should do nothing.
        assert_eq!(resolve_nav(5, 5, 0, 1), NavDecision::Clamp);
    }

    #[test]
    fn clamp_forward_when_at_last_of_many_chapters() {
        // 4 chapters, currently at index 3 (the last one).
        assert_eq!(resolve_nav(10, 5, 3, 4), NavDecision::Clamp);
    }

    // ----- Backward boundary: go to previous chapter -----

    #[test]
    fn previous_chapter_when_target_is_negative() {
        // target_page = -1 from chapter index 2 → previous chapter is index 1.
        assert_eq!(
            resolve_nav(-1, 5, 2, 3),
            NavDecision::PreviousChapter { chapter_idx: 1 }
        );
    }

    #[test]
    fn previous_chapter_index_is_current_minus_one() {
        assert_eq!(
            resolve_nav(-1, 3, 4, 5),
            NavDecision::PreviousChapter { chapter_idx: 3 }
        );
    }

    // ----- Backward boundary: clamp at first chapter -----

    #[test]
    fn clamp_backward_when_already_at_first_chapter() {
        // Already at chapter index 0; tapping Prev should do nothing.
        assert_eq!(resolve_nav(-1, 5, 0, 3), NavDecision::Clamp);
    }

    #[test]
    fn clamp_backward_when_only_one_chapter() {
        assert_eq!(resolve_nav(-1, 5, 0, 1), NavDecision::Clamp);
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
    save_last_opened(&LastOpened::Reader {
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
