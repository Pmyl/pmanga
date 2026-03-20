//! localStorage-based reading progress helpers.

use super::models::LastOpened;
use serde_json;
use web_sys;

const LAST_OPENED_KEY: &str = "pmanga_last_opened";

/// sessionStorage key used to ensure the startup redirect to the last-read
/// page fires only once per browser session (on fresh app open), not every
/// time the user navigates back to the shelf.
const STARTUP_REDIRECT_DONE_KEY: &str = "pmanga_startup_redirect_done";

pub fn save_last_opened(pos: &LastOpened) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return;
    };
    if let Ok(json) = serde_json::to_string(pos) {
        let _ = storage.set_item(LAST_OPENED_KEY, &json);
    }
}

pub fn load_last_opened() -> Option<LastOpened> {
    let window = web_sys::window()?;
    let storage = window.local_storage().ok()??;
    let json = storage.get_item(LAST_OPENED_KEY).ok()??;
    serde_json::from_str(&json).ok()
}

/// Returns `true` if the one-time startup redirect has already been
/// performed in this browser session.
pub fn is_startup_redirect_done() -> bool {
    let Some(window) = web_sys::window() else {
        return true; // Fail-safe: don't redirect if we can't check.
    };
    let Ok(Some(storage)) = window.session_storage() else {
        return true;
    };
    matches!(storage.get_item(STARTUP_REDIRECT_DONE_KEY), Ok(Some(_)))
}

/// Marks the startup redirect as done for this browser session.
pub fn mark_startup_redirect_done() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(storage)) = window.session_storage() else {
        return;
    };
    let _ = storage.set_item(STARTUP_REDIRECT_DONE_KEY, "1");
}

#[allow(dead_code)]
pub fn clear_last_opened() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return;
    };
    let _ = storage.remove_item(LAST_OPENED_KEY);
}
