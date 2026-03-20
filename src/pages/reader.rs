//! Reader page — displays a single manga page from IndexedDB with tap-zone
//! navigation, an info overlay, gamepad support, and double-spread zoom.

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

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
    pages::padding::{ChapterPadding, Padding, load_chapter_padding, save_chapter_padding},
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
// Spread zoom helpers
// ---------------------------------------------------------------------------

/// Returns true if the device is currently in portrait orientation.
fn is_portrait() -> bool {
    let Some(window) = web_sys::window() else {
        return true;
    };
    let w = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let h = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    h > w
}

/// Returns the viewport width in pixels.
fn viewport_width() -> f64 {
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(400.0)
}

/// Returns the viewport height in pixels.
fn viewport_height() -> f64 {
    web_sys::window()
        .and_then(|w| w.inner_height().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(800.0)
}

/// Given the image's natural dimensions, compute its rendered width when
/// height-fitted (height = viewport height, width = auto).
fn rendered_width_when_height_fitted(natural_w: u32, natural_h: u32) -> f64 {
    if natural_h == 0 {
        return 0.0;
    }
    let vh = viewport_height();
    (natural_w as f64) * (vh / natural_h as f64)
}

/// Pan step: how many pixels one left/right tap moves the view.
/// ~40% of the viewport width so 3 taps cover a double-spread.
fn pan_step() -> f64 {
    viewport_width() * 0.4
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
        save_progress_fire_and_forget(db, manga_id.to_string(), chapter_id.to_string(), target);
        nav.replace(Route::Reader {
            manga_id: manga_id.to_string(),
            chapter_id: chapter_id.to_string(),
            page: target,
        });
    }
}

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

    // Signals to track props reactively.
    let mut page_signal = use_signal(|| page);
    let mut chapter_id_signal = use_signal(|| chapter_id.clone());

    // Sync props to signals during render.
    if *page_signal.peek() != page {
        page_signal.set(page);
    }
    if *chapter_id_signal.peek() != chapter_id {
        chapter_id_signal.set(chapter_id.clone());
    }

    // Holds the open Db once the async open completes.
    let db_signal: Signal<Option<Rc<Db>>> = use_signal(|| None);

    // Blob URL for the current page image.
    let blob_url: Signal<Option<String>> = use_signal(|| None);

    // All chapters for this manga, sorted by chapter_number.
    let chapters_signal: Signal<Vec<ChapterMeta>> = use_signal(Vec::new);

    // The resolved ChapterMeta for the current chapter.
    let chapter_meta_signal: Signal<Option<ChapterMeta>> = use_signal(|| None);

    // Manga title.
    let manga_title_signal: Signal<String> = use_signal(String::new);

    // Padding adjustment state.
    let mut padding_signal: Signal<ChapterPadding> =
        use_signal(|| load_chapter_padding(&chapter_id));
    let mut padding_modal_open = use_signal(|| false);

    // Sync padding from session storage when chapter changes.
    if *chapter_id_signal.peek() != chapter_id {
        let loaded = load_chapter_padding(&chapter_id);
        if *padding_signal.peek() != loaded {
            padding_signal.set(loaded);
        }
    }

    // ----- Spread zoom state -----
    // Natural dimensions of the current page image (width, height).
    let img_natural_size: Signal<Option<(u32, u32)>> = use_signal(|| None);
    // Whether we are currently in spread-zoom mode.
    let mut spread_zoomed: Signal<bool> = use_signal(|| false);
    // Pan offset in pixels. 0 = showing rightmost edge (manga default).
    // Positive values = panned toward the left side of the image.
    let mut pan_x: Signal<f64> = use_signal(|| 0.0);

    // Reset zoom when page/chapter changes.
    {
        let mut prev_page = use_signal(|| page);
        let mut prev_chapter = use_signal(|| chapter_id.clone());
        if *prev_page.peek() != page || *prev_chapter.peek() != chapter_id {
            if spread_zoomed() {
                spread_zoomed.set(false);
                pan_x.set(0.0);
            }
            prev_page.set(page);
            prev_chapter.set(chapter_id.clone());
        }
    }

    // Detect image natural dimensions when blob URL changes.
    {
        let mut img_natural_size = img_natural_size;
        use_effect(move || {
            let url = blob_url.read().clone();
            img_natural_size.set(None);

            if let Some(url) = url {
                wasm_bindgen_futures::spawn_local(async move {
                    let Ok(img) = web_sys::HtmlImageElement::new() else {
                        return;
                    };

                    let (tx, rx) = futures_channel::oneshot::channel::<()>();
                    let tx = RefCell::new(Some(tx));
                    let onload = Closure::<dyn FnMut()>::new(move || {
                        if let Some(tx) = tx.borrow_mut().take() {
                            let _ = tx.send(());
                        }
                    });
                    img.set_onload(Some(onload.as_ref().unchecked_ref()));
                    img.set_src(&url);
                    let _ = rx.await;
                    drop(onload);

                    let w = img.natural_width();
                    let h = img.natural_height();
                    if w > 0 && h > 0 {
                        img_natural_size.set(Some((w, h)));
                    }
                });
            }
        });
    }

    // ----- Spread zoom action handler -----
    let try_toggle_spread_zoom = {
        let img_natural_size = img_natural_size;
        let mut spread_zoomed = spread_zoomed;
        let mut pan_x = pan_x;
        move || {
            // If currently zoomed, always allow de-zoom.
            if spread_zoomed() {
                spread_zoomed.set(false);
                pan_x.set(0.0);
                return;
            }

            // Guard: don't zoom if in landscape orientation.
            if !is_portrait() {
                return;
            }

            // Guard: don't zoom if image is taller than wide (not a spread).
            if let Some((w, h)) = img_natural_size() {
                if h >= w {
                    return; // Portrait page — not a double spread.
                }
                // Activate zoom: fit height, show right side.
                spread_zoomed.set(true);
                pan_x.set(0.0); // 0 = rightmost edge
            }
        }
    };

    // ----- Pan handlers -----
    let handle_pan_left = {
        let mut pan_x = pan_x;
        let img_natural_size = img_natural_size;
        move || {
            if let Some((nw, nh)) = img_natural_size() {
                let rendered_w = rendered_width_when_height_fitted(nw, nh);
                let vw = viewport_width();
                let max_pan = (rendered_w - vw).max(0.0);
                let step = pan_step();
                let new_val = (pan_x() + step).min(max_pan);
                pan_x.set(new_val);
            }
        }
    };

    let handle_pan_right = {
        let mut pan_x = pan_x;
        move || {
            let step = pan_step();
            let new_val = (pan_x() - step).max(0.0);
            pan_x.set(new_val);
        }
    };

    // ----- Gamepad -----
    let gamepad_config = use_signal(|| GamepadConfig::load());

    let gp_manga_id = manga_id.clone();
    let gp_chapter_id = chapter_id.clone();

    use_gamepad(gamepad_config, {
        let mut overlay_visible = overlay_visible;
        let db_signal = db_signal;
        let chapters_signal = chapters_signal;
        let chapter_meta_signal = chapter_meta_signal;
        let gp_manga_id = gp_manga_id.clone();
        let gp_chapter_id = gp_chapter_id.clone();
        let spread_zoomed = spread_zoomed;
        let mut try_toggle_spread_zoom = try_toggle_spread_zoom.clone();
        let mut handle_pan_left = handle_pan_left.clone();
        let mut handle_pan_right = handle_pan_right.clone();

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
                    if spread_zoomed() {
                        handle_pan_right();
                    } else {
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
                }
                Action::PreviousPage => {
                    if spread_zoomed() {
                        handle_pan_left();
                    } else {
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
                }
                Action::ToggleOverlay => {
                    overlay_visible.set(!overlay_visible());
                }
                Action::ToggleSpreadZoom => {
                    try_toggle_spread_zoom();
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
                let current_page = page_signal();
                let current_chapter_id = chapter_id_signal();

                let db = {
                    let guard = db_signal.read();
                    guard.clone()
                };
                let Some(db) = db else { return };

                // Revoke the previous object URL to avoid memory leaks.
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

    let is_zoomed = spread_zoomed();
    let current_pan_x = pan_x();

    // ----- Closures for tap zones -----
    let manga_id_prev = manga_id.clone();
    let chapter_id_prev = chapter_id.clone();
    let all_chapters_prev = all_chapters.clone();
    let db_prev = db_signal.read().clone();

    let manga_id_next = manga_id.clone();
    let chapter_id_next = chapter_id.clone();
    let all_chapters_next = all_chapters.clone();
    let db_next = db_signal.read().clone();

    let manga_id_back = manga_id.clone();

    // Clone handlers for tap zone use.
    let mut tap_toggle_zoom = try_toggle_spread_zoom.clone();
    let mut tap_pan_left = handle_pan_left.clone();
    let mut tap_pan_right = handle_pan_right.clone();

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
                } else if is_zoomed {
                    // ---- Spread-zoom mode: fit height, overflow width, pan ----
                    {
                        // The image is positioned absolutely with right:0 and
                        // translated by pan_x to show the desired horizontal slice.
                        let img_style = format!(
                            "height: 100vh; width: auto; position: absolute; right: 0; transform: translateX(-{}px); user-select: none; display: block;",
                            current_pan_x
                        );
                        rsx! {
                            div {
                                class: "w-full h-full overflow-hidden relative",
                                img {
                                    style: "{img_style}",
                                    src: current_blob_url.clone().unwrap_or_default(),
                                    alt: "Manga page {page}",
                                }
                            }
                        }
                    }
                } else {
                    // ---- Normal mode (with optional padding crop) ----
                    {
                        let p = padding_signal.read().effective_for_page(page);
                        let has_crop = !p.is_zero();

                        if has_crop {
                            let img_style = format!(
                                "max-width: calc(100% + {}px + {}px); max-height: calc(100vh + {}px + {}px); margin: -{}px -{}px -{}px -{}px; object-fit: contain; display: block; user-select: none;",
                                p.left, p.right, p.up, p.down,
                                p.up, p.right, p.down, p.left
                            );
                            rsx! {
                                div {
                                    class: "w-full h-full flex items-center justify-center overflow-hidden",
                                    img {
                                        style: "{img_style}",
                                        src: current_blob_url.clone().unwrap_or_default(),
                                        alt: "Manga page {page}",
                                    }
                                }
                            }
                        } else {
                            rsx! {
                                img {
                                    class: "max-w-full max-h-screen object-contain block select-none",
                                    src: current_blob_url.clone().unwrap_or_default(),
                                    alt: "Manga page {page}",
                                }
                            }
                        }
                    }
                }
            }

            // ---- Tap zones ----
            div {
                class: "reader-tap-zones",

                // Left third → previous page / pan left when zoomed
                div {
                    class: "tap-zone tap-zone-left",
                    onclick: move |_| {
                        if spread_zoomed() {
                            tap_pan_left();
                        } else {
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
                }

                // Middle third → toggle spread zoom
                div {
                    class: "tap-zone tap-zone-middle",
                    onclick: move |_| {
                        tap_toggle_zoom();
                    }
                }

                // Top strip → toggle overlay
                div {
                    class: "tap-zone tap-zone-top",
                    onclick: move |_| {
                        overlay_visible.set(!overlay_visible());
                    }
                }

                // Right third → next page / pan right when zoomed
                div {
                    class: "tap-zone tap-zone-right",
                    onclick: move |_| {
                        if spread_zoomed() {
                            tap_pan_right();
                        } else {
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
            }

            // ---- Info overlay ----
            if overlay_visible() {
                // Backdrop to capture clicks and dismiss the overlay
                div {
                    class: "fixed inset-0 z-20",
                    onclick: move |_| {
                        overlay_visible.set(false);
                    },
                }
                // Top bar content
                div {
                    class: "fixed top-0 left-0 right-0 z-30 bg-black/85 backdrop-blur-sm cursor-pointer",
                    onclick: move |_| {
                        overlay_visible.set(false);
                    },
                    div {
                        class: "flex items-center gap-3 px-3 py-2",

                        // Back button
                        button {
                            class: "flex-shrink-0 w-8 h-8 flex items-center justify-center border-0 cursor-pointer rounded bg-transparent text-[#888] hover:text-[#ccc] active:text-[#f0f0f0]",
                            onclick: move |e| {
                                e.stop_propagation();
                                navigator().push(Route::Library {
                                    manga_id: manga_id_back.clone(),
                                });
                            },
                            svg {
                                class: "w-5 h-5",
                                fill: "none",
                                stroke: "currentColor",
                                stroke_width: "2",
                                view_box: "0 0 24 24",
                                path {
                                    d: "M15 19l-7-7 7-7",
                                }
                            }
                        }

                        // Info section
                        div {
                            class: "flex flex-col gap-0.5 min-w-0 flex-1",
                            div {
                                class: "text-sm text-[#ccc] truncate",
                                span { "{manga_title}" }
                                if let Some(ref meta) = chapter_meta {
                                    if let Some(vol) = meta.tankobon_number {
                                        span { class: "text-[#555]", " · " }
                                        span { "Vol. {vol}" }
                                    }
                                    span { class: "text-[#555]", " · " }
                                    span { "Ch. {meta.chapter_number}" }
                                }
                            }
                            div {
                                class: "text-xs text-[#666]",
                                if let Some(ref meta) = chapter_meta {
                                    span { "p. {page + 1} / {meta.page_count}" }
                                    span { class: "text-[#555]", " · " }
                                    span { class: "truncate", "{meta.filename}" }
                                }
                            }
                        }

                        // Crop / padding button (inside top bar)
                        button {
                            class: "flex-shrink-0 w-8 h-8 flex items-center justify-center border-0 cursor-pointer rounded bg-transparent text-[#888] hover:text-[#ccc] active:text-[#f0f0f0]",
                            onclick: move |e| {
                                e.stop_propagation();
                                padding_modal_open.set(true);
                            },
                            svg {
                                class: "w-5 h-5",
                                fill: "none",
                                stroke: "currentColor",
                                stroke_width: "2",
                                view_box: "0 0 24 24",
                                path { d: "M6 2v4" }
                                path { d: "M6 14v8" }
                                path { d: "M2 6h4" }
                                path { d: "M14 6h8" }
                                path { d: "M6 6h12v12H6z" }
                            }
                        }
                    }
                }
            }

            // ---- Zoom indicator (visible when zoomed) ----
            if is_zoomed {
                div {
                    class: "fixed bottom-4 left-1/2 -translate-x-1/2 z-10 px-3 py-1.5 rounded-full bg-black/70 text-[#888] text-xs select-none pointer-events-none",
                    "Spread zoom · tap middle to exit"
                }
            }

            // ---- Padding adjustment modal ----
            if padding_modal_open() {
                PaddingModal {
                    chapter_id: chapter_id.clone(),
                    padding: padding_signal,
                    on_close: move || padding_modal_open.set(false),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Padding Modal Component
// ---------------------------------------------------------------------------

#[component]
fn PaddingModal(
    chapter_id: String,
    padding: Signal<ChapterPadding>,
    on_close: EventHandler<()>,
) -> Element {
    let mut padding = padding;
    let chapter_id_for_save = chapter_id.clone();

    rsx! {
        // Backdrop
        div {
            class: "fixed inset-0 z-40 bg-black/70",
            onclick: move |_| on_close.call(()),
        }
        // Modal
        div {
            class: "fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-50 bg-[#1a1a1a] rounded-lg p-4 min-w-[280px] max-w-[90vw]",
            onclick: move |e| e.stop_propagation(),

            // Header
            div {
                class: "flex items-center justify-between mb-4",
                h3 { class: "text-sm font-medium text-[#ccc] m-0", "Padding Adjustment" }
                button {
                    class: "w-6 h-6 flex items-center justify-center rounded bg-transparent text-[#666] hover:text-[#ccc] border-0 cursor-pointer",
                    onclick: move |_| on_close.call(()),
                    "✕"
                }
            }

            // General section
            div {
                class: "mb-4",
                div { class: "text-xs text-[#666] mb-2 uppercase tracking-wide", "General" }
                PaddingControls {
                    padding_value: padding.read().general,
                    on_change: {
                        let chapter_id = chapter_id_for_save.clone();
                        move |p: Padding| {
                            padding.write().general = p;
                            save_chapter_padding(&chapter_id, &padding.read());
                        }
                    },
                }
            }

            // Odd pages section
            div {
                class: "mb-4",
                div { class: "text-xs text-[#666] mb-2 uppercase tracking-wide", "Odd Pages (1, 3, 5...)" }
                PaddingControls {
                    padding_value: padding.read().odd,
                    on_change: {
                        let chapter_id = chapter_id_for_save.clone();
                        move |p: Padding| {
                            padding.write().odd = p;
                            save_chapter_padding(&chapter_id, &padding.read());
                        }
                    },
                }
            }

            // Even pages section
            div {
                class: "mb-4",
                div { class: "text-xs text-[#666] mb-2 uppercase tracking-wide", "Even Pages (2, 4, 6...)" }
                PaddingControls {
                    padding_value: padding.read().even,
                    on_change: {
                        let chapter_id = chapter_id_for_save.clone();
                        move |p: Padding| {
                            padding.write().even = p;
                            save_chapter_padding(&chapter_id, &padding.read());
                        }
                    },
                }
            }

            // Reset button
            div {
                class: "pt-3 border-t border-[#333]",
                button {
                    class: "w-full py-2 rounded bg-[#333] text-[#888] hover:bg-[#444] hover:text-[#ccc] border-0 cursor-pointer text-sm",
                    onclick: {
                        let chapter_id = chapter_id_for_save.clone();
                        move |_| {
                            padding.set(ChapterPadding::default());
                            save_chapter_padding(&chapter_id, &ChapterPadding::default());
                        }
                    },
                    "Reset All"
                }
            }
        }
    }
}

#[component]
fn PaddingControls(padding_value: Padding, on_change: EventHandler<Padding>) -> Element {
    let up = padding_value.up;
    let down = padding_value.down;
    let left = padding_value.left;
    let right = padding_value.right;

    rsx! {
        div {
            class: "grid grid-cols-2 gap-2",

            // UP
            div {
                class: "flex items-center gap-2",
                span { class: "w-14 text-xs text-[#888]", "UP" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up: (up - 1).max(0), down, left, right }),
                    "-"
                }
                span { class: "w-8 text-center text-sm text-[#ccc]", "{up}" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up: up + 1, down, left, right }),
                    "+"
                }
            }

            // DOWN
            div {
                class: "flex items-center gap-2",
                span { class: "w-14 text-xs text-[#888]", "DOWN" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down: (down - 1).max(0), left, right }),
                    "-"
                }
                span { class: "w-8 text-center text-sm text-[#ccc]", "{down}" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down: down + 1, left, right }),
                    "+"
                }
            }

            // LEFT
            div {
                class: "flex items-center gap-2",
                span { class: "w-14 text-xs text-[#888]", "LEFT" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down, left: (left - 1).max(0), right }),
                    "-"
                }
                span { class: "w-8 text-center text-sm text-[#ccc]", "{left}" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down, left: left + 1, right }),
                    "+"
                }
            }

            // RIGHT
            div {
                class: "flex items-center gap-2",
                span { class: "w-14 text-xs text-[#888]", "RIGHT" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down, left, right: (right - 1).max(0) }),
                    "-"
                }
                span { class: "w-8 text-center text-sm text-[#ccc]", "{right}" }
                button {
                    class: "w-7 h-7 rounded bg-[#333] text-[#ccc] hover:bg-[#444] active:bg-[#555] border-0 cursor-pointer text-sm",
                    onclick: move |_| on_change.call(Padding { up, down, left, right: right + 1 }),
                    "+"
                }
            }
        }
    }
}
