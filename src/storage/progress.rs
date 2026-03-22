//! localStorage-based reading progress helpers.

use super::models::LastOpened;
use serde_json;
use web_sys;

use std::cell::Cell;

const LAST_OPENED_KEY: &str = "pmanga_last_opened";
const PROXY_URL_KEY: &str = "pmanga_proxy_url";

// In-memory flag: set to `true` the first time the startup redirect fires.
// Using `thread_local!` instead of sessionStorage means it can never fail
// silently (no storage API, no iOS quirks) and resets automatically when the
// WASM module is reloaded (i.e. on a real page reload), which is exactly the
// desired "once per browser session" semantics.
thread_local! {
    static STARTUP_REDIRECT_DONE: Cell<bool> = const { Cell::new(false) };
}

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
    STARTUP_REDIRECT_DONE.with(|v| v.get())
}

/// Marks the startup redirect as done for this session.
pub fn mark_startup_redirect_done() {
    STARTUP_REDIRECT_DONE.with(|v| v.set(true));
}

pub fn save_proxy_url(url: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(storage)) = window.local_storage() else {
        return;
    };
    let _ = storage.set_item(PROXY_URL_KEY, url);
}

pub fn load_proxy_url() -> Option<String> {
    let window = web_sys::window()?;
    let storage = window.local_storage().ok()??;
    if let Ok(Some(url)) = storage.get_item(PROXY_URL_KEY) {
        if !url.trim().is_empty() {
            return Some(url);
        }
    }
    // Fall back to the current browser hostname so the app works correctly
    // whether accessed via localhost or a LAN IP like 192.168.1.79.
    let hostname = window.location().hostname().ok()?;
    Some(format!("https://{}:7331", hostname))
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
