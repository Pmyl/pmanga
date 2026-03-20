//! Settings page — gamepad button remapping UI.

use dioxus::prelude::*;
use js_sys::Promise;
use wasm_bindgen_futures::JsFuture;

use crate::{
    input::{Action, config::GamepadConfig},
    routes::Route,
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

    // Build the display rows from the current config snapshot.
    let rows = config.read().display_rows();

    rsx! {
        div {
            class: "page settings-page",

            // ── Header ────────────────────────────────────────────────────
            div {
                class: "settings-header",
                button {
                    class: "btn btn-back",
                    onclick: move |_| {
                        navigator().push(Route::Shelf {});
                    },
                    "← Back"
                }
                h1 { "Settings" }
            }

            // ── Section ───────────────────────────────────────────────────
            div {
                class: "settings-section",

                h2 { "Gamepad Bindings" }
                p {
                    class: "settings-hint",
                    "Press \"Remap\" next to an action, then press the desired button on your gamepad."
                }

                // ── Binding table ─────────────────────────────────────────
                div {
                    class: "settings-table",

                    for (action, button) in rows {
                        {
                            let is_remapping_this = remapping() == Some(action);
                            let button_label = match &button {
                                Some(b) => b.label(),
                                None => "— unbound —".to_string(),
                            };

                            rsx! {
                                div {
                                    class: "settings-row",
                                    key: "{action:?}",

                                    // Action name
                                    span {
                                        class: "settings-row-action",
                                        "{action.label()}"
                                    }

                                    // Current binding
                                    span {
                                        class: "settings-row-button",
                                        "{button_label}"
                                    }

                                    // Remap controls
                                    div {
                                        class: "settings-row-remap",

                                        if is_remapping_this {
                                            span {
                                                class: "settings-listening",
                                                "Listening…"
                                            }
                                            button {
                                                class: "btn btn-small btn-ghost",
                                                onclick: move |_| {
                                                    remapping.set(None);
                                                },
                                                "Cancel"
                                            }
                                        } else {
                                            button {
                                                class: "btn btn-small",
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
                                                    class: "btn btn-small btn-ghost",
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
                    class: "settings-reset",
                    button {
                        class: "btn btn-danger",
                        onclick: move |_| {
                            config.with_mut(|c| c.reset_to_defaults());
                            config.read().save();
                            remapping.set(None);
                        },
                        "Reset to Defaults"
                    }
                }
            }
        }
    }
}
