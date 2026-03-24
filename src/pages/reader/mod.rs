//! Reader page — displays a single manga page with tap-zone navigation, an
//! info overlay, gamepad support, and double-spread zoom.

mod navigation;
mod options_modal;
mod overlay;
mod reader_config;
mod viewport;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

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

use navigation::go_to_page;
use options_modal::ReaderOptionsModal;
use overlay::ReaderOverlay;
use reader_config::ReaderConfig;
use viewport::{
    blob_to_object_url, is_portrait, pan_step, rendered_width_when_height_fitted, viewport_width,
};

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[component]
pub fn ReaderPage(manga_id: String, chapter_id: String, page: usize) -> Element {
    // ----- Alive guard -----
    // Cloned into the progress-sync resource. use_drop sets it to false when
    // the component unmounts, preventing any ghost async task from calling
    // navigator().replace() after the Reader has been removed from the tree.
    let component_alive = Rc::new(Cell::new(true));
    {
        let alive_for_drop = component_alive.clone();
        use_drop(move || alive_for_drop.set(false));
    }

    // ----- Signals -----
    let mut overlay_visible = use_signal(|| false);

    // Track props reactively so resources and effects can depend on them.
    let mut page_signal = use_signal(|| page);
    let mut chapter_id_signal = use_signal(|| chapter_id.clone());

    if *page_signal.peek() != page {
        page_signal.set(page);
    }
    if *chapter_id_signal.peek() != chapter_id {
        chapter_id_signal.set(chapter_id.clone());
    }

    let db_signal: Signal<Option<Rc<Db>>> = use_signal(|| None);
    let blob_url: Signal<Option<String>> = use_signal(|| None);
    let chapters_signal: Signal<Vec<ChapterMeta>> = use_signal(Vec::new);
    let manga_title_signal: Signal<String> = use_signal(String::new);

    // Derived: always reflects the current chapter's meta, even after navigation.
    let chapter_meta_signal = use_memo(move || {
        chapters_signal
            .read()
            .iter()
            .find(|c| c.id.0 == chapter_id_signal())
            .cloned()
    });

    // Per-chapter padding / crop.
    let mut padding_signal: Signal<ChapterPadding> =
        use_signal(|| load_chapter_padding(&chapter_id));
    let mut settings_modal_open = use_signal(|| false);
    let reader_config_signal: Signal<ReaderConfig> = use_signal(|| ReaderConfig::load());

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

    // ----- Spread-zoom state -----
    let img_natural_size: Signal<Option<(u32, u32)>> = use_signal(|| None);
    let mut spread_zoomed: Signal<bool> = use_signal(|| false);
    let mut pan_x: Signal<f64> = use_signal(|| 0.0);

    // Reset zoom when the page or chapter changes.
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

    // Detect image natural dimensions whenever the blob URL changes so we can
    // decide whether the page is a double-spread.
    {
        let mut img_natural_size = img_natural_size;
        use_effect(move || {
            let url = blob_url.read().clone();
            img_natural_size.set(None);

            if let Some(url) = url {
                spawn(async move {
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

    // ----- Spread-zoom / pan action handlers -----
    let try_toggle_spread_zoom = {
        let img_natural_size = img_natural_size;
        let mut spread_zoomed = spread_zoomed;
        let mut pan_x = pan_x;
        move || {
            if spread_zoomed() {
                spread_zoomed.set(false);
                pan_x.set(0.0);
                return;
            }
            if !is_portrait() {
                return;
            }
            if let Some((w, h)) = img_natural_size() {
                if h >= w {
                    return; // Portrait page — not a double spread.
                }
                spread_zoomed.set(true);
                pan_x.set(0.0);
            }
        }
    };

    let handle_pan_left = {
        let mut pan_x = pan_x;
        let img_natural_size = img_natural_size;
        move || {
            if let Some((nw, nh)) = img_natural_size() {
                let rendered_w = rendered_width_when_height_fitted(nw, nh);
                let vw = viewport_width();
                let max_pan = (rendered_w - vw).max(0.0);
                let new_val = (pan_x() + pan_step()).min(max_pan);
                pan_x.set(new_val);
            }
        }
    };

    let handle_pan_right = {
        let mut pan_x = pan_x;
        move || {
            let new_val = (pan_x() - pan_step()).max(0.0);
            pan_x.set(new_val);
        }
    };

    // ----- Gamepad -----
    let gamepad_config = use_signal(|| GamepadConfig::load());
    let gp_manga_id = manga_id.clone();

    use_gamepad(gamepad_config, {
        let mut overlay_visible = overlay_visible;
        let db_signal = db_signal;
        let chapters_signal = chapters_signal;
        let chapter_meta_signal = chapter_meta_signal;
        let spread_zoomed = spread_zoomed;
        let mut try_toggle_spread_zoom = try_toggle_spread_zoom.clone();
        let mut handle_pan_left = handle_pan_left.clone();
        let mut handle_pan_right = handle_pan_right.clone();

        move |action| {
            let current_page = page_signal();
            let current_chapter_id = chapter_id_signal();
            let chapter_pages = chapter_meta_signal
                .read()
                .as_ref()
                .map(|c| c.page_count)
                .unwrap_or(1);
            let all_chapters = chapters_signal.read().clone();
            let current_idx = all_chapters
                .iter()
                .position(|c| c.id.0 == current_chapter_id)
                .unwrap_or(0);
            let db = db_signal.read().clone();

            match action {
                Action::NextPage => {
                    if spread_zoomed() {
                        handle_pan_right();
                    } else {
                        go_to_page(
                            current_page as isize + 1,
                            &gp_manga_id,
                            &current_chapter_id,
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
                            current_page as isize - 1,
                            &gp_manga_id,
                            &current_chapter_id,
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
                    navigator().push(Route::Library {
                        manga_id: gp_manga_id.clone(),
                    });
                }
                Action::Confirm => {}
            }
        }
    });

    // ----- Resource: open the database -----
    {
        let mut db_signal = db_signal;
        use_resource(move || async move {
            match Db::open().await {
                Ok(db) => {
                    *db_signal.write() = Some(Rc::new(db));
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("DB open error: {e}").into());
                }
            }
        });
    }

    // ----- Resource: sync page with saved progress -----
    // Reactive to chapter_id_signal so it re-runs whenever we navigate to a
    // new chapter, not just on the initial mount.
    {
        let db_signal = db_signal;
        let manga_id_for_progress = manga_id.clone();

        let alive = component_alive.clone();
        use_resource(move || {
            // Reading chapter_id_signal() here makes the resource re-run every
            // time the chapter changes.
            let current_chapter_id = chapter_id_signal();
            let db = db_signal.read().clone();
            let manga_id_for_progress = manga_id_for_progress.clone();
            let alive = alive.clone();
            async move {
                let Some(db) = db else { return };
                // Snapshot the page non-reactively so we don't re-run on every
                // page turn, only on chapter changes.
                let current_page = *page_signal.peek();

                if let Ok(Some(saved)) = db
                    .load_progress(&ChapterId(current_chapter_id.clone()))
                    .await
                {
                    // Guard: if the Reader has already unmounted (e.g. the user
                    // navigated away while the DB was still opening), do not
                    // call replace() — that would stomp on whatever page the
                    // user is currently on.
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
    // manga_id never changes while in the reader, so this runs once when the
    // DB is ready.  chapter_meta_signal is a memo derived from chapters_signal
    // + chapter_id_signal and updates automatically on navigation.
    {
        let db_signal = db_signal;
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

    // ----- Resource: load current page image -----
    {
        let db_signal = db_signal;
        let mut blob_url = blob_url;
        let chapter_meta_signal = chapter_meta_signal;

        use_resource(move || async move {
            let current_page = page_signal();
            let current_chapter_id = chapter_id_signal();
            let chapter_meta = chapter_meta_signal.read().clone();

            let db = db_signal.read().clone();
            let Some(db) = db else { return };

            // Revoke the previous blob URL (never revoke CDN URLs).
            {
                let old = blob_url.peek().clone();
                if let Some(url) = old {
                    if url.starts_with("blob:") {
                        let _ = web_sys::Url::revoke_object_url(&url);
                    }
                }
            }

            // WeebCentral: use the stored CDN URL directly.
            if let Some(ref meta) = chapter_meta {
                if !meta.page_urls.is_empty() {
                    *blob_url.write() = meta.page_urls.get(current_page as usize).cloned();
                    return;
                }
            }

            // Local: load blob from IndexedDB.
            match db
                .load_page(&ChapterId(current_chapter_id), current_page as u32)
                .await
            {
                Ok(Some(blob)) => match blob_to_object_url(&blob) {
                    Ok(url) => *blob_url.write() = Some(url),
                    Err(e) => {
                        web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&e));
                        *blob_url.write() = None;
                    }
                },
                Ok(None) => *blob_url.write() = None,
                Err(e) => {
                    web_sys::console::error_1(&format!("load_page error: {e}").into());
                    *blob_url.write() = None;
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

    // Clone handlers and data for tap zones (closures must own their data).
    let manga_id_prev = manga_id.clone();
    let chapter_id_prev = chapter_id.clone();
    let all_chapters_prev = all_chapters.clone();
    let db_prev = db_signal.read().clone();

    let manga_id_next = manga_id.clone();
    let chapter_id_next = chapter_id.clone();
    let all_chapters_next = all_chapters.clone();
    let db_next = db_signal.read().clone();

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
                    // Spread-zoom mode: fit height, overflow width, pan horizontally.
                    // max-width: none overrides Tailwind/browser defaults that would
                    // otherwise constrain width and cause aspect-ratio stretching.
                    {
                        let img_style = format!(
                            "height: 100dvh; width: auto; max-width: none; position: absolute; \
                             right: 0; transform: translateX({}px); user-select: none; display: block;",
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
                    // Normal mode (with optional padding crop).
                    {
                        let p = padding_signal.read().effective_for_page(page);
                        if p.is_zero() {
                            rsx! {
                                img {
                                    class: "max-w-full max-h-dvh object-contain block select-none",
                                    src: current_blob_url.clone().unwrap_or_default(),
                                    alt: "Manga page {page}",
                                }
                            }
                        } else {
                            let img_style = format!(
                                "max-width: calc(100% + {}px + {}px); \
                                 max-height: calc(100dvh + {}px + {}px); \
                                 margin: -{}px -{}px -{}px -{}px; \
                                 object-fit: contain; display: block; user-select: none;",
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
                        }
                    }
                }
            }

            // ---- Tap zones ----
            div {
                class: "reader-tap-zones",

                // Left third → prev page in LTR, next page in RTL/manga style; in zoom, pan right.
                div {
                    class: "tap-zone tap-zone-left",
                    onclick: move |_| {
                        if spread_zoomed() {
                            if reader_config_signal().rtl_taps {
                                tap_pan_left();
                            } else {
                                tap_pan_right();
                            };
                        } else {
                            let delta: isize = if reader_config_signal().rtl_taps { 1 } else { -1 };
                            go_to_page(
                                page as isize + delta,
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

                // Middle third → toggle spread zoom.
                div {
                    class: "tap-zone tap-zone-middle",
                    onclick: move |_| tap_toggle_zoom(),
                }

                // Top strip → toggle overlay.
                div {
                    class: "tap-zone tap-zone-top",
                    onclick: move |_| overlay_visible.set(!overlay_visible()),
                }

                // Right third → next page in LTR, prev page in RTL/manga style; in zoom, pan left.
                div {
                    class: "tap-zone tap-zone-right",
                    onclick: move |_| {
                        if spread_zoomed() {
                            if reader_config_signal().rtl_taps {
                                tap_pan_right();
                            } else {
                                tap_pan_left();
                            };
                        } else {
                            let delta: isize = if reader_config_signal().rtl_taps { -1 } else { 1 };
                            go_to_page(
                                page as isize + delta,
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
                ReaderOverlay {
                    manga_id: manga_id.clone(),
                    manga_title: manga_title.clone(),
                    chapter_meta: chapter_meta.clone(),
                    page,
                    on_close: move |_| overlay_visible.set(false),
                    on_open_settings: move |_| settings_modal_open.set(true),
                }
            }

            // ---- Padding adjustment modal ----
            if settings_modal_open() {
                ReaderOptionsModal {
                    chapter_id: chapter_id.clone(),
                    padding: padding_signal,
                    reader_config: reader_config_signal,
                    on_close: move |_| settings_modal_open.set(false),
                }
            }
        }
    }
}
