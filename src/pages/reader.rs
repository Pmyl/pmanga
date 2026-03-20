//! Reader page — displays a single manga page from IndexedDB with tap-zone
//! navigation, an info overlay, and gamepad support.

use std::cell::Cell;
use std::rc::Rc;

use dioxus::prelude::*;

// Debounce navigation to prevent double-triggers (e.g. from touch events or
// rapid re-renders). We use a thread-local timestamp to track the last nav.
thread_local! {
    static LAST_NAV_MS: Cell<f64> = const { Cell::new(0.0) };
}

/// Returns true if enough time has passed since the last navigation (100ms).
/// Also updates the timestamp if returning true.
fn nav_debounce_ok() -> bool {
    let now = js_sys::Date::now();

    LAST_NAV_MS.with(|last| {
        let prev = last.get();
        if now - prev > 100.0 {
            last.set(now);
            true
        } else {
            false
        }
    })
}

use crate::{
    input::gamepad::use_gamepad,
    input::{Action, config::GamepadConfig},
    routes::Route,
    storage::{
        db::Db,
        models::{ChapterId, ChapterMeta, LastOpened, MangaId, ReadingProgress},
        progress::save_last_opened,
    },
};

// ---------------------------------------------------------------------------
// Helper: convert a Blob to an object URL
// ---------------------------------------------------------------------------

fn blob_to_object_url(blob: &web_sys::Blob) -> Result<String, String> {
    web_sys::Url::create_object_url_with_blob(blob)
        .map_err(|e| format!("create_object_url failed: {:?}", e))
}

// ---------------------------------------------------------------------------
// Navigation helper
// ---------------------------------------------------------------------------

/// Navigate to `target_page` within the current chapter, or cross chapter
/// boundaries.  Progress is saved on every successful navigation.
fn go_to_page(
    target_page: isize,
    manga_id: &str,
    chapter_id: &str,
    chapter_pages: u32,
    all_chapters: &[ChapterMeta],
    current_chapter_idx: usize,
    db: Option<Rc<Db>>,
) {
    // Debounce to prevent double navigation from rapid clicks or re-renders.
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
        // Use replace() instead of push() to avoid polluting history and to
        // prevent double-navigation issues (if called twice with same target,
        // replace is idempotent).
        save_progress_fire_and_forget(db, manga_id.to_string(), chapter_id.to_string(), target);
        nav.replace(Route::Reader {
            manga_id: manga_id.to_string(),
            chapter_id: chapter_id.to_string(),
            page: target,
        });
    }
}

/// Fire-and-forget progress save (both IndexedDB and localStorage).
fn save_progress_fire_and_forget(
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

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[component]
pub fn ReaderPage(manga_id: String, chapter_id: String, page: usize) -> Element {
    // ----- Signals -----
    let mut overlay_visible = use_signal(|| false);

    // Signals to track props reactively. We sync them directly during render
    // by comparing with peek() and updating if different. This ensures the
    // resource re-runs when navigating to a different page or chapter.
    let mut page_signal = use_signal(|| page);
    let mut chapter_id_signal = use_signal(|| chapter_id.clone());

    // Sync props to signals during render (compare with peek to avoid subscription).
    if *page_signal.peek() != page {
        page_signal.set(page);
    }
    if *chapter_id_signal.peek() != chapter_id {
        chapter_id_signal.set(chapter_id.clone());
    }

    // Holds the open Db once the async open completes.
    let db_signal: Signal<Option<Rc<Db>>> = use_signal(|| None);

    // Blob URL for the current page image (revoked on drop — handled manually).
    let blob_url: Signal<Option<String>> = use_signal(|| None);

    // All chapters for this manga, sorted by chapter_number.
    let chapters_signal: Signal<Vec<ChapterMeta>> = use_signal(Vec::new);

    // The resolved ChapterMeta for the current chapter.
    let chapter_meta_signal: Signal<Option<ChapterMeta>> = use_signal(|| None);

    // Manga title (loaded alongside chapters).
    let manga_title_signal: Signal<String> = use_signal(String::new);

    // ----- Gamepad -----
    let gamepad_config = use_signal(|| GamepadConfig::load());

    // We need copies of signals/values for the gamepad closure.
    let gp_manga_id = manga_id.clone();
    let gp_chapter_id = chapter_id.clone();

    use_gamepad(gamepad_config, {
        let mut overlay_visible = overlay_visible;
        let db_signal = db_signal;
        let chapters_signal = chapters_signal;
        let chapter_meta_signal = chapter_meta_signal;
        let gp_manga_id = gp_manga_id.clone();
        let gp_chapter_id = gp_chapter_id.clone();

        move |action| {
            let chapter_pages = chapter_meta_signal
                .read()
                .as_ref()
                .map(|c| c.page_count)
                .unwrap_or(1);
            let all_chapters = chapters_signal.read().clone();
            let current_idx = all_chapters
                .iter()
                .position(|c| c.id.0 == gp_chapter_id)
                .unwrap_or(0);
            let db = db_signal.read().clone();

            match action {
                Action::NextPage => {
                    go_to_page(
                        page as isize + 1,
                        &gp_manga_id,
                        &gp_chapter_id,
                        chapter_pages,
                        &all_chapters,
                        current_idx,
                        db,
                    );
                }
                Action::PreviousPage => {
                    go_to_page(
                        page as isize - 1,
                        &gp_manga_id,
                        &gp_chapter_id,
                        chapter_pages,
                        &all_chapters,
                        current_idx,
                        db,
                    );
                }
                Action::ToggleOverlay => {
                    overlay_visible.set(!overlay_visible());
                }
                Action::GoBack => {
                    if overlay_visible() {
                        navigator().push(Route::Library {
                            manga_id: gp_manga_id.clone(),
                        });
                    }
                }
                Action::Confirm => {}
            }
        }
    });

    // ----- Resource: open DB -----
    {
        let mut db_signal = db_signal;
        use_resource(move || async move {
            match Db::open().await {
                Ok(db) => {
                    *db_signal.write() = Some(Rc::new(db));
                }
                Err(e) => {
                    web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
                        "DB open error: {e}"
                    )));
                }
            }
        });
    }

    // ----- Resource: sync page with saved progress -----
    // After the DB opens, load the saved progress for this chapter. If the
    // saved page differs from the route param (e.g. the URL is stale because
    // the user navigated back and then forward), redirect to the correct page.
    {
        let db_signal = db_signal;
        let chapter_id_for_progress = chapter_id.clone();
        let manga_id_for_progress = manga_id.clone();

        use_resource(move || {
            let chapter_id_for_progress = chapter_id_for_progress.clone();
            let manga_id_for_progress = manga_id_for_progress.clone();
            async move {
                let db = {
                    let guard = db_signal.read();
                    guard.clone()
                };
                let Some(db) = db else { return };

                match db
                    .load_progress(&ChapterId(chapter_id_for_progress.clone()))
                    .await
                {
                    Ok(Some(saved)) if saved.page != page => {
                        // Saved progress differs from the route — redirect silently.
                        navigator().replace(Route::Reader {
                            manga_id: manga_id_for_progress,
                            chapter_id: chapter_id_for_progress,
                            page: saved.page,
                        });
                    }
                    _ => {}
                }
            }
        });
    }

    // ----- Resource: load chapters + manga meta -----
    {
        let db_signal = db_signal;
        let manga_id_clone = manga_id.clone();
        let chapter_id_clone = chapter_id.clone();
        let mut chapters_signal = chapters_signal;
        let mut chapter_meta_signal = chapter_meta_signal;
        let mut manga_title_signal = manga_title_signal;

        use_resource(move || {
            let manga_id_clone = manga_id_clone.clone();
            let chapter_id_clone = chapter_id_clone.clone();
            async move {
                let db = {
                    let guard = db_signal.read();
                    guard.clone()
                };
                let Some(db) = db else { return };

                // Load manga title.
                if let Ok(mangas) = db.load_all_mangas().await {
                    if let Some(m) = mangas.into_iter().find(|m| m.id.0 == manga_id_clone) {
                        *manga_title_signal.write() = m.title;
                    }
                }

                // Load and sort chapters.
                match db
                    .load_chapters_for_manga(&MangaId(manga_id_clone.clone()))
                    .await
                {
                    Ok(mut chs) => {
                        chs.sort_by(|a, b| a.chapter_number.total_cmp(&b.chapter_number));
                        let current = chs.iter().find(|c| c.id.0 == chapter_id_clone).cloned();
                        *chapters_signal.write() = chs;
                        *chapter_meta_signal.write() = current;
                    }
                    Err(e) => {
                        web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
                            "load_chapters error: {e}"
                        )));
                    }
                }
            }
        });
    }

    // ----- Resource: load page blob -----
    {
        let db_signal = db_signal;
        let mut blob_url = blob_url;

        use_resource(move || {
            async move {
                // Read the reactive signals to subscribe to page/chapter changes.
                // This ensures the resource re-runs when navigating between pages
                // or crossing chapter boundaries.
                let current_page = page_signal();
                let current_chapter_id = chapter_id_signal();

                let db = {
                    let guard = db_signal.read();
                    guard.clone()
                };
                let Some(db) = db else { return };

                // Revoke the previous object URL to avoid memory leaks.
                // Use peek() instead of read() to avoid creating a reactive
                // subscription — otherwise writing the new URL would re-trigger
                // this resource, causing an infinite loop.
                {
                    let old = blob_url.peek().clone();
                    if let Some(url) = old {
                        let _ = web_sys::Url::revoke_object_url(&url);
                    }
                }

                match db
                    .load_page(&ChapterId(current_chapter_id), current_page as u32)
                    .await
                {
                    Ok(Some(blob)) => match blob_to_object_url(&blob) {
                        Ok(url) => {
                            *blob_url.write() = Some(url);
                        }
                        Err(e) => {
                            web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&e));
                            *blob_url.write() = None;
                        }
                    },
                    Ok(None) => {
                        *blob_url.write() = None;
                    }
                    Err(e) => {
                        web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
                            "load_page error: {e}"
                        )));
                        *blob_url.write() = None;
                    }
                }
            }
        });
    }

    // ----- Derived data for render -----
    let db_ready = db_signal.read().is_some();
    let current_blob_url = blob_url.read().clone();
    let all_chapters = chapters_signal.read().clone();
    let chapter_meta = chapter_meta_signal.read().clone();
    let manga_title = manga_title_signal.read().clone();

    let chapter_pages = chapter_meta.as_ref().map(|c| c.page_count).unwrap_or(1);
    let current_chapter_idx = all_chapters
        .iter()
        .position(|c| c.id.0 == chapter_id)
        .unwrap_or(0);

    // ----- Closures for tap zones (capture everything needed) -----
    let manga_id_prev = manga_id.clone();
    let chapter_id_prev = chapter_id.clone();
    let all_chapters_prev = all_chapters.clone();
    let db_prev = db_signal.read().clone();

    let manga_id_next = manga_id.clone();
    let chapter_id_next = chapter_id.clone();
    let all_chapters_next = all_chapters.clone();
    let db_next = db_signal.read().clone();

    let manga_id_back = manga_id.clone();

    // ----- Render -----
    rsx! {
        div {
            class: "fixed inset-0 bg-black flex items-center justify-center",

            // ---- Image area ----
            div {
                class: "w-full h-full flex items-center justify-center",
                if !db_ready || (db_ready && current_blob_url.is_none() && chapter_meta.is_none()) {
                    div {
                        class: "text-[#555] text-base",
                        "Loading..."
                    }
                } else if current_blob_url.is_none() {
                    div {
                        class: "text-[#555] text-base",
                        "Page not available"
                    }
                } else {
                    img {
                        class: "max-w-full max-h-screen object-contain block select-none",
                        src: current_blob_url.clone().unwrap_or_default(),
                        alt: "Manga page {page}",
                    }
                }
            }

            // ---- Tap zones ----
            div {
                class: "reader-tap-zones",

                // Left third → previous page
                div {
                    class: "tap-zone tap-zone-left",
                    onclick: move |_| {
                        go_to_page(
                            page as isize - 1,
                            &manga_id_prev,
                            &chapter_id_prev,
                            chapter_pages,
                            &all_chapters_prev,
                            current_chapter_idx,
                            db_prev.clone(),
                        );
                    }
                }

                // Top strip → toggle overlay (higher z-index via CSS)
                div {
                    class: "tap-zone tap-zone-top",
                    onclick: move |_| {
                        overlay_visible.set(!overlay_visible());
                    }
                }

                // Right third → next page
                div {
                    class: "tap-zone tap-zone-right",
                    onclick: move |_| {
                        go_to_page(
                            page as isize + 1,
                            &manga_id_next,
                            &chapter_id_next,
                            chapter_pages,
                            &all_chapters_next,
                            current_chapter_idx,
                            db_next.clone(),
                        );
                    }
                }
            }

            // ---- Info overlay ----
            if overlay_visible() {
                div {
                    class: "fixed top-0 left-0 right-0 z-20 bg-black/85 backdrop-blur-sm",
                    div {
                        class: "flex flex-col gap-1 px-4 py-3",

                        button {
                            class: "border-0 cursor-pointer text-sm px-2 py-1.5 rounded bg-transparent text-[#888] active:text-[#f0f0f0]",
                            onclick: move |_| {
                                navigator().push(Route::Library {
                                    manga_id: manga_id_back.clone(),
                                });
                            },
                            "← Library"
                        }

                        div {
                            class: "flex flex-wrap gap-1 text-sm text-[#ccc]",
                            span {
                                span { "{manga_title}" }
                                if let Some(ref meta) = chapter_meta {
                                    if let Some(vol) = meta.tankobon_number {
                                        span { class: "text-[#555]", " · " }
                                        span { "Vol. {vol}" }
                                    }
                                    span { class: "text-[#555]", " · " }
                                    span { "Ch. {meta.chapter_number}" }
                                    span { class: "text-[#555]", " · " }
                                    span {
                                        "p. {page + 1} / {meta.page_count}"
                                    }
                                }
                            }
                        }

                        if let Some(ref meta) = chapter_meta {
                            div {
                                class: "text-xs text-[#666] mt-0.5",
                                span { "{meta.filename}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
