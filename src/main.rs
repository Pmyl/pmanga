mod assets;
mod bridge;
mod components;
mod input;
mod pages;
mod routes;
mod storage;

use std::rc::Rc;

use dioxus_web::{Config, HashHistory};
use routes::App;

fn main() {
    // MangaDex's CORS policy allows http://localhost but not http://127.0.0.1.
    // If the browser opened the app via 127.0.0.1, silently redirect to the
    // equivalent localhost URL so all API calls work without any extra setup.
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(window) = web_sys::window() {
            if let Ok(location) = window.location().href() {
                if location.contains("//127.0.0.1") {
                    let redirected = location.replacen("//127.0.0.1", "//localhost", 1);
                    let _ = window.location().replace(&redirected);
                    return; // stop — the page will reload at localhost
                }
            }
        }
    }

    dioxus::LaunchBuilder::new()
        .with_cfg(Config::new().history(Rc::new(HashHistory::new(true))))
        .launch(App);
}
