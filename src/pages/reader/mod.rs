//! Reader page — entry point that routes to the appropriate reading mode.
//!
//! - **Paged mode** (default): one page at a time with tap-zone navigation
//!   and zoom support.  Lives in [`paged_reader`].
//! - **Vertical scroll mode**: all pages rendered top-to-bottom for
//!   continuous webtoon-style reading.  Lives in [`scroll_reader`].
//!
//! The active mode is read from [`ReaderConfig`] which is persisted to
//! `localStorage`.  The user switches modes via the reader options modal.

mod navigation;
mod options_modal;
mod overlay;
mod paged_reader;
mod reader_config;
mod scroll_reader;
mod viewport;
mod zoom;

use dioxus::prelude::*;

use paged_reader::PagedReaderView;
use reader_config::ReaderConfig;
use scroll_reader::ScrollReaderView;

// ---------------------------------------------------------------------------
// Entry-point component
// ---------------------------------------------------------------------------

/// Top-level reader component.  Reads the persisted [`ReaderConfig`] and
/// renders the appropriate reading mode, passing the mutable config signal
/// down so either child can toggle modes from within the options modal.
///
/// `overlay_visible` and `settings_modal_open` are lifted here so that
/// toggling the reading mode (which swaps the child component) does not
/// dismiss the top bar or the reader-settings dialog.
#[component]
pub fn ReaderPage(manga_id: String, chapter_id: String, page: usize) -> Element {
    let reader_config: Signal<ReaderConfig> = use_signal(|| ReaderConfig::load());
    let overlay_visible: Signal<bool> = use_signal(|| false);
    let settings_modal_open: Signal<bool> = use_signal(|| false);

    if reader_config.read().vertical_scroll {
        rsx! {
            ScrollReaderView {
                manga_id: manga_id.clone(),
                chapter_id: chapter_id.clone(),
                page,
                reader_config,
                overlay_visible,
                settings_modal_open,
            }
        }
    } else {
        rsx! {
            PagedReaderView {
                manga_id: manga_id.clone(),
                chapter_id: chapter_id.clone(),
                page,
                reader_config,
                overlay_visible,
                settings_modal_open,
            }
        }
    }
}
