//! localStorage-based reading progress helpers.

use super::models::LastOpened;
use serde_json;
use web_sys;

const LAST_OPENED_KEY: &str = "pmanga_last_opened";

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
