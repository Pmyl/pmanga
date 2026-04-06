//! Vertical scroll reading mode — all pages rendered top-to-bottom for
//! continuous (webtoon-style) reading.
//!
//! Navigation model:
//! - Natural touch/mouse scroll reads through the chapter.
//! - Right tap / gamepad Next → scroll down by a step; at the very bottom of
//!   the last page, advance to the next chapter.
//! - Left tap / gamepad Prev → scroll up by a step; at the very top, go to
//!   the previous chapter.
//! - Top-strip tap / gamepad ToggleOverlay → show / hide the info overlay.
//! - Reading progress (current visible page) is saved on every page change.

use std::cell::Cell;
use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::JsCast;

use crate::{
    input::gamepad::use_gamepad,
    input::{Action, config::GamepadConfig},
    pages::padding::{ChapterPadding, load_chapter_padding},
    routes::Route,
    storage::{
        db::Db,
        models::{ChapterId, ChapterMeta, MangaId},
    },
};

use super::navigation::{go_to_page, save_progress_fire_and_forget};
use super::options_modal::ReaderOptionsModal;
use super::overlay::ReaderOverlay;
use super::reader_config::ReaderConfig;
use super::viewport::{blob_to_object_url, viewport_height, viewport_width};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// ID of the scrollable container element.
const SCROLL_CONTAINER_ID: &str = "pmanga-scroll-container";

/// ID prefix for individual page wrapper elements (suffixed with the index).
const SCROLL_PAGE_ID_PREFIX: &str = "pmanga-scroll-page-";

/// How far one "step" scrolls as a fraction of the viewport height.
/// ~35 % means roughly 2–3 steps to cross a viewport-height's worth of page.
const SCROLL_STEP_FRACTION: f64 = 0.35;

/// Pixel tolerance (integer) used to detect "at top" and "at bottom" of the
/// scroll container.  Needed because fractional pixel rounding can keep the
/// `scrollTop` value a few pixels short of the exact boundary.
const SCROLL_BOUNDARY_THRESHOLD_PX: i32 = 8;

/// Top tap-zone height as a fraction of the viewport height.
/// Tapping the top 15 % of the screen toggles the info overlay.
const TOP_ZONE_HEIGHT_RATIO: f64 = 0.15;

/// Horizontal boundary (as a fraction of viewport width) between the left
/// tap-zone and the middle pass-through zone.
const LEFT_ZONE_RATIO: f64 = 1.0 / 3.0;

/// Horizontal boundary (as a fraction of viewport width) between the middle
/// pass-through zone and the right tap-zone.
const RIGHT_ZONE_RATIO: f64 = 2.0 / 3.0;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the scroll container element, if it exists in the DOM.
fn get_scroll_container() -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id(SCROLL_CONTAINER_ID)?
        .dyn_into::<web_sys::HtmlElement>()
        .ok()
}

/// Scroll the container by `delta` pixels (positive = down).
fn scroll_by(delta: f64) {
    if let Some(el) = get_scroll_container() {
        let new_top = (el.scroll_top() as f64 + delta).max(0.0);
        el.set_scroll_top(new_top as i32);
    }
}

/// Return the viewport height of the scroll container (its `clientHeight`).
fn container_height() -> f64 {
    get_scroll_container()
        .map(|el| el.client_height() as f64)
        .unwrap_or(800.0)
}

/// Compute the `offsetTop` for each page element and return them as a
/// `Vec<i32>`.  Called **once** after pages are first rendered to the DOM so
/// the scroll handler can determine the current page via a binary search with
/// zero per-event allocations and zero per-event DOM queries.
///
/// `offsetTop` is relative to the scroll container and is not affected by
/// the current scroll position, so the values remain valid throughout the
/// lifetime of the current chapter.
fn compute_page_tops(page_count: usize) -> Vec<i32> {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return Vec::new();
    };
    let mut tops = Vec::with_capacity(page_count);
    for i in 0..page_count {
        let id = format!("{SCROLL_PAGE_ID_PREFIX}{i}");
        let top = doc
            .get_element_by_id(&id)
            .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
            .map(|el| el.offset_top())
            .unwrap_or(0);
        tops.push(top);
    }
    tops
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[component]
pub fn ScrollReaderView(
    manga_id: String,
    chapter_id: String,
    page: usize,
    reader_config: Signal<ReaderConfig>,
) -> Element {
    // ----- Alive guard -----
    let component_alive = Rc::new(Cell::new(true));
    {
        let alive_for_drop = component_alive.clone();
        use_drop(move || alive_for_drop.set(false));
    }

    // ----- Signals -----
    let mut overlay_visible = use_signal(|| false);
    let mut settings_modal_open = use_signal(|| false);

    let mut page_signal = use_signal(|| page);
    let mut chapter_id_signal = use_signal(|| chapter_id.clone());

    if *page_signal.peek() != page {
        page_signal.set(page);
    }
    if *chapter_id_signal.peek() != chapter_id {
        chapter_id_signal.set(chapter_id.clone());
    }

    let db_signal: Signal<Option<Rc<Db>>> = use_signal(|| None);
    let chapters_signal: Signal<Vec<ChapterMeta>> = use_signal(Vec::new);
    let manga_title_signal: Signal<String> = use_signal(String::new);

    // All page URLs for the current chapter (None = not yet loaded).
    let page_urls_signal: Signal<Vec<Option<String>>> = use_signal(Vec::new);

    let chapter_meta_signal = use_memo(move || {
        chapters_signal
            .read()
            .iter()
            .find(|c| c.id.0 == chapter_id_signal())
            .cloned()
    });

    let mut padding_signal: Signal<ChapterPadding> =
        use_signal(|| load_chapter_padding(&chapter_id));

    // Sync padding when the chapter changes.
    {
        let mut prev_chapter = use_signal(|| chapter_id.clone());
        if *prev_chapter.peek() != chapter_id {
            let loaded = load_chapter_padding(&chapter_id);
            if *padding_signal.peek() != loaded {
                padding_signal.set(loaded);
            }
            prev_chapter.set(chapter_id.clone());
        }
    }

    // Current visible page as tracked by the scroll handler.
    let mut current_visible_page: Signal<usize> = use_signal(|| page);

    // Whether the container is scrolled to its very top or very bottom.
    let mut at_top_signal: Signal<bool> = use_signal(|| true);
    let mut at_bottom_signal: Signal<bool> = use_signal(|| false);

    // Cached scroll container element (populated once after first render).
    // Avoids repeated window → document → getElementById → dyn_into chains
    // inside the hot-path scroll handler.
    let mut container_signal: Signal<Option<web_sys::HtmlElement>> = use_signal(|| None);

    // Pre-computed offsetTop for each page element (populated once after the
    // page URLs are available and pages are rendered to the DOM).
    // The scroll handler uses binary search on this Vec to find the visible
    // page in O(log N) with zero allocations and zero DOM queries.
    let mut page_tops_signal: Signal<Vec<i32>> = use_signal(Vec::new);

    // ----- Save progress when visible page changes -----
    {
        let manga_id_for_progress = manga_id.clone();
        use_effect(move || {
            let p = current_visible_page();
            let db = db_signal.read().clone();
            let chapter_id = chapter_id_signal();
            save_progress_fire_and_forget(db, manga_id_for_progress.clone(), chapter_id, p);
        });
    }

    // ----- Scroll event handler (hot path — keep allocation-free) -----
    //
    // The scroll event fires at up to 60 fps while the user is scrolling.
    // Every operation here must be O(1) with no heap allocations and no DOM
    // queries beyond reading three integer properties from the cached element.
    //
    // Page detection uses a binary search on `page_tops_signal` (a Vec<i32> of
    // pre-computed offsetTop values) so it is O(log N) with zero JS calls.
    let handle_scroll = {
        move |_: Event<ScrollData>| {
            let container_guard = container_signal.read();
            let Some(container) = container_guard.as_ref() else {
                return;
            };

            let scroll_top = container.scroll_top();    // i32
            let client_height = container.client_height(); // i32
            let scroll_height = container.scroll_height(); // i32

            // Only write to signals when the value actually changes to avoid
            // triggering spurious Dioxus re-renders.
            let new_at_top = scroll_top <= SCROLL_BOUNDARY_THRESHOLD_PX;
            if *at_top_signal.peek() != new_at_top {
                at_top_signal.set(new_at_top);
            }

            let new_at_bottom =
                scroll_top + client_height >= scroll_height - SCROLL_BOUNDARY_THRESHOLD_PX;
            if *at_bottom_signal.peek() != new_at_bottom {
                at_bottom_signal.set(new_at_bottom);
            }

            // Determine the visible page: last page whose top ≤ midpoint.
            // Binary search on the cached tops Vec — no DOM queries, no allocs.
            let tops = page_tops_signal.read();
            if !tops.is_empty() {
                let midpoint = scroll_top + client_height / 2;
                let new_visible = tops
                    .partition_point(|&t| t <= midpoint)
                    .saturating_sub(1)
                    .min(tops.len() - 1);
                if *current_visible_page.peek() != new_visible {
                    current_visible_page.set(new_visible);
                }
            }
        }
    };

    // ----- Navigate left / right -----

    // Shared helper: snapshots the current chapter navigation context from
    // reactive signals.  Both direction handlers call this to avoid repeating
    // the same signal reads.
    let nav_ctx = move || {
        let current_chapter_id = chapter_id_signal();
        let all_chapters = chapters_signal.read().clone();
        let current_idx = all_chapters
            .iter()
            .position(|c| c.id.0 == current_chapter_id)
            .unwrap_or(0);
        let chapter_pages = chapter_meta_signal
            .read()
            .as_ref()
            .map(|c| c.page_count)
            .unwrap_or(1);
        let db = db_signal.read().clone();
        (chapter_pages, all_chapters, current_chapter_id, current_idx, db)
    };

    let handle_navigate_right = {
        let manga_id = manga_id.clone();
        move || {
            let (chapter_pages, all_chapters, current_chapter_id, current_idx, db) = nav_ctx();
            if at_bottom_signal() {
                // At the bottom → advance to the next chapter.
                go_to_page(
                    chapter_pages as isize,
                    &manga_id,
                    &current_chapter_id,
                    chapter_pages,
                    &all_chapters,
                    current_idx,
                    db,
                );
            } else {
                scroll_by(container_height() * SCROLL_STEP_FRACTION);
            }
        }
    };

    let handle_navigate_left = {
        let manga_id = manga_id.clone();
        move || {
            let (chapter_pages, all_chapters, current_chapter_id, current_idx, db) = nav_ctx();
            if at_top_signal() {
                // At the top → go to the previous chapter.
                go_to_page(
                    -1,
                    &manga_id,
                    &current_chapter_id,
                    chapter_pages,
                    &all_chapters,
                    current_idx,
                    db,
                );
            } else {
                scroll_by(-(container_height() * SCROLL_STEP_FRACTION));
            }
        }
    };

    // ----- Gamepad -----
    let gamepad_config = use_signal(GamepadConfig::load);
    let gp_manga_id = manga_id.clone();

    use_gamepad(gamepad_config, {
        let mut overlay_visible = overlay_visible;
        let gp_navigate_right = handle_navigate_right.clone();
        let gp_navigate_left = handle_navigate_left.clone();

        move |action| match action {
            Action::NextPage => gp_navigate_right(),
            Action::PreviousPage => gp_navigate_left(),
            Action::ToggleOverlay => {
                overlay_visible.set(!overlay_visible());
            }
            Action::GoBack => {
                navigator().push(Route::Library {
                    manga_id: gp_manga_id.clone(),
                });
            }
            // Spread zoom is not used in scroll mode.
            Action::ToggleSpreadZoom | Action::Confirm => {}
            Action::Refresh => {
                crate::bridge::js::reload_page();
            }
        }
    });

    // ----- Resource: open the database -----
    {
        let mut db_signal = db_signal;
        use_resource(move || async move {
            match Db::open().await {
                Ok(db) => *db_signal.write() = Some(Rc::new(db)),
                Err(e) => {
                    web_sys::console::error_1(&format!("DB open error: {e}").into());
                }
            }
        });
    }

    // ----- Resource: sync page with saved progress -----
    {
        let manga_id_for_progress = manga_id.clone();
        let alive = component_alive.clone();

        use_resource(move || {
            let current_chapter_id = chapter_id_signal();
            let db = db_signal.read().clone();
            let manga_id_for_progress = manga_id_for_progress.clone();
            let alive = alive.clone();
            async move {
                let Some(db) = db else { return };
                let current_page = *page_signal.peek();

                if let Ok(Some(saved)) = db
                    .load_progress(&ChapterId(current_chapter_id.clone()))
                    .await
                {
                    if alive.get() && saved.page != current_page {
                        navigator().replace(Route::Reader {
                            manga_id: manga_id_for_progress,
                            chapter_id: current_chapter_id,
                            page: saved.page,
                        });
                    }
                }
            }
        });
    }

    // ----- Resource: load chapters + manga title -----
    {
        let manga_id_clone = manga_id.clone();
        let mut chapters_signal = chapters_signal;
        let mut manga_title_signal = manga_title_signal;

        use_resource(move || {
            let manga_id_clone = manga_id_clone.clone();
            async move {
                let db = db_signal.read().clone();
                let Some(db) = db else { return };

                if let Ok(mangas) = db.load_all_mangas().await {
                    if let Some(m) = mangas.into_iter().find(|m| m.id.0 == manga_id_clone) {
                        *manga_title_signal.write() = m.title;
                    }
                }

                match db
                    .load_chapters_for_manga(&MangaId(manga_id_clone.clone()))
                    .await
                {
                    Ok(mut chs) => {
                        chs.sort_by(|a, b| a.chapter_number.total_cmp(&b.chapter_number));
                        *chapters_signal.write() = chs;
                    }
                    Err(e) => {
                        web_sys::console::error_1(&format!("load_chapters error: {e}").into());
                    }
                }
            }
        });
    }

    // ----- Resource: load all page URLs for the chapter -----
    {
        let mut page_urls_signal = page_urls_signal;

        use_resource(move || async move {
            let current_chapter_id = chapter_id_signal();
            let chapter_meta = chapter_meta_signal.read().clone();
            let db = db_signal.read().clone();
            let Some(db) = db else { return };

            // WeebCentral: use CDN URLs directly.
            if let Some(ref meta) = chapter_meta {
                if !meta.page_urls.is_empty() {
                    *page_urls_signal.write() =
                        meta.page_urls.iter().map(|u| Some(u.clone())).collect();
                    return;
                }

                // Local: load all blobs from IndexedDB.
                let count = meta.page_count;
                let mut urls: Vec<Option<String>> = Vec::with_capacity(count as usize);
                for i in 0..count {
                    match db
                        .load_page(&ChapterId(current_chapter_id.clone()), i)
                        .await
                    {
                        Ok(Some(blob)) => match blob_to_object_url(&blob) {
                            Ok(url) => urls.push(Some(url)),
                            Err(_) => urls.push(None),
                        },
                        _ => urls.push(None),
                    }
                }
                *page_urls_signal.write() = urls;
            }
        });
    }

    // ----- One-time setup: cache container, compute page tops, initial scroll -----
    // Triggered whenever page_urls_signal becomes non-empty (i.e., the chapter
    // data finishes loading). Runs inside `spawn` so the DOM elements exist.
    //
    // This is also where the scroll container element is cached into
    // `container_signal` so the hot-path scroll handler never has to call
    // window → document → getElementById again.
    {
        let initial_page = page;
        let mut scrolled = use_signal(|| false);

        // Reset both the scroll guard and the cached tops when the chapter changes
        // so the effect re-runs and recomputes everything for the new chapter.
        {
            let mut prev_chapter = use_signal(|| chapter_id.clone());
            if *prev_chapter.peek() != chapter_id {
                scrolled.set(false);
                *page_tops_signal.write() = Vec::new();
                prev_chapter.set(chapter_id.clone());
            }
        }

        use_effect(move || {
            let urls = page_urls_signal.read();
            if urls.is_empty() {
                return;
            }
            let count = urls.len();
            drop(urls); // release borrow before spawn captures signals

            // Use spawn so elements are in the DOM when we query offsetTop.
            spawn(async move {
                // Step 1: cache the container element (once per component lifetime).
                if container_signal.read().is_none() {
                    *container_signal.write() = get_scroll_container();
                }

                // Step 2: compute and cache page tops.
                let tops = compute_page_tops(count);
                // Snapshot the initial page's top before the write borrow.
                let initial_top = tops.get(initial_page).copied().unwrap_or(0);
                *page_tops_signal.write() = tops;

                // Step 3: scroll to the starting page (only once per chapter).
                if !scrolled() {
                    scrolled.set(true);
                    if initial_top > 0 {
                        if let Some(container) = container_signal.read().as_ref() {
                            container.set_scroll_top(initial_top);
                        }
                    }
                }
            });
        });
    }

    // ----- Derived data for render -----
    let db_ready = db_signal.read().is_some();
    let page_urls = page_urls_signal.read().clone();
    let chapter_meta = chapter_meta_signal.read().clone();
    let manga_title = manga_title_signal.read().clone();

    let tap_navigate_left = handle_navigate_left;
    let tap_navigate_right = handle_navigate_right;

    // ----- Render -----
    rsx! {
        div {
            // select-none prevents accidental text selection when tapping
            // repeatedly in quick succession (issue #1).
            class: "fixed inset-0 bg-black select-none",

            // ---- Scrollable page strip ----
            div {
                id: SCROLL_CONTAINER_ID,
                // Native overflow-y scroll is the only scroll mechanism in this
                // mode.  There is no separate tap-zone overlay that would block
                // touch scroll events (issue #4).
                class: "w-full h-dvh overflow-y-auto",
                onscroll: handle_scroll,
                // Position-based click routing replaces the fixed tap-zone
                // overlay.  Using onclick directly on the scroll container avoids
                // the iOS-Safari `pointer-events: none` parent /
                // `pointer-events: all` child bug that made the top-bar click
                // area unreliable in landscape (issue #2).
                // The browser only fires onclick for taps (no significant pointer
                // movement), so scroll gestures are never mis-routed (issue #4).
                // `rtl_taps` is respected for left/right direction (issue #3).
                onclick: move |e| {
                    let coords = e.client_coordinates();
                    let x = coords.x;
                    let y = coords.y;

                    let vw = viewport_width();
                    let vh = viewport_height();

                    let rtl = reader_config.peek().rtl_taps;

                    if y < vh * TOP_ZONE_HEIGHT_RATIO {
                        // Top zone: toggle the info overlay.
                        overlay_visible.set(!overlay_visible());
                    } else if x < vw * LEFT_ZONE_RATIO {
                        // Left zone.
                        if rtl {
                            tap_navigate_right();
                        } else {
                            tap_navigate_left();
                        }
                    } else if x > vw * RIGHT_ZONE_RATIO {
                        // Right zone.
                        if rtl {
                            tap_navigate_left();
                        } else {
                            tap_navigate_right();
                        }
                    }
                    // Middle zone: no action — let the user scroll freely.
                },

                if !db_ready || page_urls.is_empty() && chapter_meta.is_none() {
                    div {
                        class: "flex items-center justify-center h-dvh text-[#555] text-base",
                        "Loading..."
                    }
                } else if page_urls.is_empty() {
                    div {
                        class: "flex items-center justify-center h-dvh text-[#555] text-base",
                        "No pages available"
                    }
                } else {
                    for (i, url) in page_urls.iter().enumerate() {
                        {
                            let p = padding_signal.read().effective_for_page(i);
                            let page_id = format!("{SCROLL_PAGE_ID_PREFIX}{i}");
                            rsx! {
                                div {
                                    id: "{page_id}",
                                    class: "w-full",
                                    if let Some(src) = url {
                                        if p.is_zero() {
                                            img {
                                                class: "w-full h-auto block select-none",
                                                src: "{src}",
                                                alt: "Manga page {i}",
                                            }
                                        } else {
                                            div {
                                                class: "overflow-hidden",
                                                img {
                                                    style: "display: block; width: calc(100% + {p.left}px + {p.right}px); \
                                                            max-width: none; height: auto; user-select: none; \
                                                            margin-left: -{p.left}px; margin-right: -{p.right}px; \
                                                            margin-top: -{p.up}px; margin-bottom: -{p.down}px;",
                                                    src: "{src}",
                                                    alt: "Manga page {i}",
                                                }
                                            }
                                        }
                                    } else {
                                        div {
                                            class: "w-full h-32 bg-[#111] flex items-center justify-center text-[#444] text-sm",
                                            "Page {i + 1} unavailable"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ---- Info overlay ----
            if overlay_visible() {
                ReaderOverlay {
                    manga_id: manga_id.clone(),
                    manga_title: manga_title.clone(),
                    chapter_meta: chapter_meta.clone(),
                    page: current_visible_page(),
                    on_close: move |_| overlay_visible.set(false),
                    on_open_settings: move |_| settings_modal_open.set(true),
                }
            }

            // ---- Options modal ----
            if settings_modal_open() {
                ReaderOptionsModal {
                    chapter_id: chapter_id.clone(),
                    padding: padding_signal,
                    reader_config,
                    on_close: move |_| settings_modal_open.set(false),
                }
            }
        }
    }
}
