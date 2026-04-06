//! Settings page — gamepad button remapping UI.

use dioxus::prelude::*;
use js_sys::Promise;
use wasm_bindgen_futures::JsFuture;

use crate::{
    bridge::weebcentral::test_proxy_connection,
    input::{Action, config::GamepadConfig},
    routes::Route,
    storage::progress::{load_proxy_url, save_proxy_url},
};

// ---------------------------------------------------------------------------
// Helper: sleep via JS setTimeout (mirrors gamepad.rs approach)
// ---------------------------------------------------------------------------

async fn sleep_ms(ms: i32) {
    let promise = Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .expect("no window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .expect("set_timeout failed");
    });
    JsFuture::from(promise).await.unwrap();
}

// ---------------------------------------------------------------------------
// listen_for_next_button
// ---------------------------------------------------------------------------

/// Spawns a one-shot polling loop that detects the next raw gamepad button
/// press (edge: was-up → now-down) and calls `on_button(button_index)` once,
/// then stops.
///
/// Polls every 16 ms, same cadence as the action-mapping loop in gamepad.rs.
fn listen_for_next_button(mut on_button: impl FnMut(usize) + 'static) {
    wasm_bindgen_futures::spawn_local(async move {
        // Per-slot: which buttons were pressed on the *previous* frame.
        // We use a flat vec of (gamepad_index, button_index, was_pressed).
        // Simpler: keep a HashMap<(gamepad_slot, btn_idx), bool>.
        use std::collections::HashMap;
        let mut prev: HashMap<(u32, usize), bool> = HashMap::new();

        loop {
            sleep_ms(16).await;

            let window = match web_sys::window() {
                Some(w) => w,
                None => continue,
            };
            let gamepads = match window.navigator().get_gamepads() {
                Ok(g) => g,
                Err(_) => continue,
            };

            let mut found: Option<usize> = None;

            'outer: for i in 0..gamepads.length() {
                let val = gamepads.get(i);
                if val.is_null() || val.is_undefined() {
                    continue;
                }
                use wasm_bindgen::JsCast;
                let gamepad: web_sys::Gamepad = match val.dyn_into() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                let slot = gamepad.index();
                let buttons = gamepad.buttons();

                for b in 0..buttons.length() {
                    let btn_val = buttons.get(b);
                    let btn: web_sys::GamepadButton = match btn_val.dyn_into() {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    let now = btn.pressed();
                    let was = *prev.get(&(slot, b as usize)).unwrap_or(&false);
                    prev.insert((slot, b as usize), now);

                    if now && !was {
                        found = Some(b as usize);
                        break 'outer;
                    }
                }
            }

            if let Some(btn_idx) = found {
                on_button(btn_idx);
                return; // one-shot: stop the loop
            }
        }
    });
}

// ---------------------------------------------------------------------------
// SettingsPage
// ---------------------------------------------------------------------------

#[component]
pub fn SettingsPage() -> Element {
    // Loaded once from localStorage (or defaults).
    let mut config: Signal<GamepadConfig> = use_signal(GamepadConfig::load);

    // Which action is currently waiting for a button press; None = idle.
    let mut remapping: Signal<Option<Action>> = use_signal(|| None);

    // WeebCentral proxy URL (persisted in localStorage).
    let mut proxy_url_input: Signal<String> = use_signal(|| load_proxy_url().unwrap_or_default());
    let mut proxy_saved: Signal<bool> = use_signal(|| false);

    /// Connection test state for the "Test Connection" button.
    #[derive(Clone, PartialEq)]
    enum TestState {
        Idle,
        Testing,
        Ok,
        Failed(String),
    }

    let mut test_state: Signal<TestState> = use_signal(|| TestState::Idle);

    // Build the display rows from the current config snapshot.
    let rows = config.read().display_rows();

    rsx! {
        div {
            class: "h-screen flex flex-col overflow-hidden",

            // ── Header ────────────────────────────────────────────────────
            div {
                class: "flex items-center gap-2 px-4 py-3 border-b border-[#222] shrink-0",
                button {
                    class: "border-0 cursor-pointer text-sm px-2 py-1.5 rounded bg-transparent text-[#888] active:text-[#f0f0f0]",
                    onclick: move |_| {
                        navigator().push(Route::Shelf {});
                    },
                    "← Back"
                }
                h1 { class: "text-lg font-semibold", "Settings" }
            }

            // ── Section ───────────────────────────────────────────────────
            div {
                class: "p-4 flex flex-col gap-3 overflow-y-auto flex-1",

                h2 { class: "text-base font-semibold text-[#ccc]", "Gamepad Bindings" }
                p {
                    class: "text-sm text-[#666]",
                    "Press \"Remap\" next to an action, then press the desired button on your gamepad."
                }

                // ── Binding table ─────────────────────────────────────────
                div {
                    class: "flex flex-col gap-px bg-[#222] rounded-lg overflow-hidden",

                    for (action, button) in rows {
                        {
                            let is_remapping_this = remapping() == Some(action);
                            let button_label = match &button {
                                Some(b) => b.label(),
                                None => "— unbound —".to_string(),
                            };

                            rsx! {
                                div {
                                    class: "flex items-center bg-[#1a1a1a] px-3.5 py-2.5 gap-3",
                                    key: "{action:?}",

                                    // Action name
                                    span {
                                        class: "flex-1 text-sm",
                                        "{action.label()}"
                                    }

                                    // Current binding
                                    span {
                                        class: "text-xs text-[#888] flex-1 text-center",
                                        "{button_label}"
                                    }

                                    // Remap controls
                                    div {
                                        class: "shrink-0 flex gap-1",

                                        if is_remapping_this {
                                            span {
                                                class: "text-xs text-[#e8b44a] italic",
                                                "Listening…"
                                            }
                                            button {
                                                class: "border-0 cursor-pointer text-xs px-2 py-0.5 rounded bg-transparent border border-[#333] text-[#ccc] active:bg-[#1f1f1f]",
                                                onclick: move |_| {
                                                    remapping.set(None);
                                                },
                                                "Cancel"
                                            }
                                        } else {
                                            button {
                                                class: "border-0 cursor-pointer text-xs px-2 py-0.5 rounded bg-[#252525] text-[#f0f0f0] active:bg-[#333]",
                                                onclick: move |_| {
                                                    remapping.set(Some(action));
                                                    listen_for_next_button(move |btn_idx| {
                                                        config.with_mut(|c| c.set_binding(btn_idx, action));
                                                        config.read().save();
                                                        remapping.set(None);
                                                    });
                                                },
                                                "Remap"
                                            }
                                            // Only show Clear when the action is currently bound.
                                            if button.is_some() {
                                                button {
                                                    class: "border-0 cursor-pointer text-xs px-2 py-0.5 rounded bg-transparent border border-[#333] text-[#ccc] active:bg-[#1f1f1f]",
                                                    onclick: move |_| {
                                                        config.with_mut(|c| {
                                                            if let Some(btn) = c.button_for(action) {
                                                                c.clear_binding(btn.0);
                                                            }
                                                        });
                                                        config.read().save();
                                                    },
                                                    "✕ Clear"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // ── Reset to Defaults ─────────────────────────────────────
                div {
                    class: "mt-2",
                    button {
                        class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#8b1a1a] text-[#f0f0f0] active:bg-[#a82020]",
                        onclick: move |_| {
                            config.with_mut(|c| c.reset_to_defaults());
                            config.read().save();
                            remapping.set(None);
                        },
                        "Reset to Defaults"
                    }
                }

                // ── WeebCentral Proxy ──────────────────────────────────────
                div { class: "flex flex-col gap-2 pt-2 border-t border-[#222]",
                    h2 { class: "text-base font-semibold text-[#ccc]", "WeebCentral Proxy URL" }
                    p {
                        class: "text-sm text-[#666]",
                        "Address of the pmanga-proxy server on your local network."
                    }
                    div { class: "flex gap-2",
                        input {
                            class: "flex-1 bg-[#111] border border-[#333] rounded px-3 py-2 text-sm focus:outline-none focus:border-[#555]",
                            r#type: "text",
                            placeholder: "https://192.168.1.x:7331",
                            value: "{proxy_url_input}",
                            oninput: move |e| {
                                *proxy_url_input.write() = e.value();
                                proxy_saved.set(false);
                                test_state.set(TestState::Idle);
                            },
                        }
                        button {
                            class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#e8b44a] text-black font-semibold active:bg-[#d4a03c] shrink-0",
                            onclick: move |_| {
                                save_proxy_url(&proxy_url_input.read());
                                proxy_saved.set(true);
                            },
                            "Save"
                        }
                        button {
                            class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#252525] text-[#f0f0f0] active:bg-[#333] shrink-0",
                            title: "Open in browser",
                            onclick: move |_| {
                                let url = proxy_url_input.read().trim().to_string();
                                if !url.is_empty() {
                                    if let Some(window) = web_sys::window() {
                                        let _ = window.open_with_url_and_target(&url, "_blank");
                                    }
                                }
                            },
                            "↗"
                        }

                    }
                    // ── Test Connection ───────────────────────────────────
                    div { class: "flex items-center gap-2",
                        button {
                            class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#252525] text-[#f0f0f0] active:bg-[#333] shrink-0",
                            disabled: matches!(*test_state.read(), TestState::Testing),
                            onclick: move |_| {
                                let url = proxy_url_input.read().trim().to_string();
                                if url.is_empty() {
                                    test_state.set(TestState::Failed(
                                        "Enter the proxy URL first.".to_string(),
                                    ));
                                    return;
                                }
                                test_state.set(TestState::Testing);
                                spawn(async move {
                                    match test_proxy_connection(&url).await {
                                        Ok(()) => test_state.set(TestState::Ok),
                                        Err(e) => test_state.set(TestState::Failed(e)),
                                    }
                                });
                            },
                            if matches!(*test_state.read(), TestState::Testing) {
                                "Testing…"
                            } else {
                                "Test Connection"
                            }
                        }
                        match &*test_state.read() {
                            TestState::Idle => rsx! {},
                            TestState::Testing => rsx! {
                                span { class: "text-xs text-[#888] italic", "Connecting…" }
                            },
                            TestState::Ok => rsx! {
                                span { class: "text-xs text-[#4caf50]", "✓ Connected" }
                            },
                            TestState::Failed(msg) => rsx! {
                                span { class: "text-xs text-[#cf6679]", "✗ {msg}" }
                            },
                        }
                    }
                    if *proxy_saved.read() {
                        p {
                            class: "text-xs text-[#4caf50]",
                            "✓ Saved"
                        }
                    }
                }

                // ── Version ───────────────────────────────────────────────
                p {
                    class: "text-xs text-[#666] pt-2",
                    "Version: {env!(\"APP_VERSION\")}"
                }
            }
        }
    }
}
