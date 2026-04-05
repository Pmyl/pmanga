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
use super::viewport::blob_to_object_url;

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

/// Pixel tolerance used to detect "at top" and "at bottom" of the scroll
/// container.  Needed because fractional pixel rounding can keep the value
/// a few pixels short of the exact boundary.
const SCROLL_BOUNDARY_THRESHOLD: f64 = 8.0;

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

/// Determine the index of the currently visible page.
///
/// **Algorithm**: iterates through page elements in order; the "current page"
/// is the *last* one whose top edge (from `getBoundingClientRect().top`) is
/// at or above the vertical midpoint of the container's visible area.
/// That is, we advance the current-page pointer for every page whose top has
/// scrolled past the midpoint, which naturally yields the page whose content
/// occupies the largest portion of the upper half of the screen.
fn visible_page_index(page_count: usize) -> usize {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return 0;
    };
    let midpoint = container_height() / 2.0;
    let mut visible = 0usize;
    for i in 0..page_count {
        let id = format!("{SCROLL_PAGE_ID_PREFIX}{i}");
        if let Some(el) = doc.get_element_by_id(&id) {
            let rect = el.get_bounding_client_rect();
            if rect.top() <= midpoint {
                visible = i;
            } else {
                break;
            }
        }
    }
    visible
}

/// Scroll the container so that the element with the given `id` is at the
/// top of the visible area.
fn scroll_to_element(id: &str) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(el) = doc.get_element_by_id(id) else {
        return;
    };
    let Ok(html_el) = el.dyn_into::<web_sys::HtmlElement>() else {
        return;
    };
    if let Some(container) = get_scroll_container() {
        container.set_scroll_top(html_el.offset_top());
    }
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

    // ----- Scroll event handler -----
    let handle_scroll = {
        move |_: Event<ScrollData>| {
            let Some(container) = get_scroll_container() else {
                return;
            };
            let scroll_top = container.scroll_top() as f64;
            let scroll_height = container.scroll_height() as f64;
            let client_height = container.client_height() as f64;

            at_top_signal.set(scroll_top <= SCROLL_BOUNDARY_THRESHOLD);
            at_bottom_signal
                .set(scroll_top + client_height >= scroll_height - SCROLL_BOUNDARY_THRESHOLD);

            let page_count = chapter_meta_signal
                .read()
                .as_ref()
                .map(|m| m.page_count as usize)
                .unwrap_or(0);

            if page_count > 0 {
                let new_visible = visible_page_index(page_count);
                if new_visible != current_visible_page() {
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

    // ----- Scroll to initial page once images are available -----
    // Fires every time page_urls_signal changes; the `scrolled` guard ensures
    // we only perform the initial scroll once per chapter.
    {
        let initial_page = page;
        let mut scrolled = use_signal(|| false);

        // Reset scrolled flag when chapter changes.
        {
            let mut prev_chapter = use_signal(|| chapter_id.clone());
            if *prev_chapter.peek() != chapter_id {
                scrolled.set(false);
                prev_chapter.set(chapter_id.clone());
            }
        }

        use_effect(move || {
            let urls = page_urls_signal.read();
            if !urls.is_empty() && !scrolled() {
                scrolled.set(true);
                let id = format!("{SCROLL_PAGE_ID_PREFIX}{initial_page}");
                // Use spawn so the scroll runs after the current render cycle
                // has placed the elements in the DOM.
                spawn(async move {
                    scroll_to_element(&id);
                });
            }
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
            class: "fixed inset-0 bg-black",

            // ---- Scrollable page strip ----
            div {
                id: SCROLL_CONTAINER_ID,
                class: "w-full h-dvh overflow-y-auto",
                onscroll: handle_scroll,

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

            // ---- Tap zones ----
            // Left/right zones scroll by a step (or cross chapter boundaries).
            // The top strip toggles the info overlay.
            // The middle zone is intentionally left transparent so users can
            // interact with the scrollable content underneath.
            div {
                class: "reader-tap-zones",

                div {
                    class: "tap-zone tap-zone-left",
                    onclick: move |_| tap_navigate_left(),
                }

                // Middle zone: pass-through in scroll mode (no action).
                div {
                    class: "tap-zone tap-zone-middle",
                }

                div {
                    class: "tap-zone tap-zone-top",
                    onclick: move |_| overlay_visible.set(!overlay_visible()),
                }

                div {
                    class: "tap-zone tap-zone-right",
                    onclick: move |_| tap_navigate_right(),
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
