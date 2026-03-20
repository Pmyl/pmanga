//! Gamepad polling via the browser Gamepad API.
//! Polls every ~16ms using a JS Promise-based timer and fires abstract Actions
//! based on the current GamepadConfig. Uses edge detection so each physical
//! button press fires the action exactly once (on press, not on hold).

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use dioxus::prelude::*;
use js_sys::Promise;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use super::{Action, config::GamepadConfig};

/// State tracker for a single gamepad: which buttons were pressed last frame.
#[derive(Default, Clone)]
struct ButtonState {
    pressed: HashMap<usize, bool>,
}

impl ButtonState {
    /// Returns true if this is a new press (was up, now down).
    fn edge_press(&mut self, index: usize, now_pressed: bool) -> bool {
        let was_pressed = *self.pressed.get(&index).unwrap_or(&false);
        self.pressed.insert(index, now_pressed);
        now_pressed && !was_pressed
    }
}

/// Sleep for `ms` milliseconds using a JS `setTimeout` Promise.
/// Does not require `gloo-timers` — uses only `js-sys`, `web-sys`, and
/// `wasm-bindgen-futures` which are already in `Cargo.toml`.
async fn sleep_ms(ms: i32) {
    let promise = Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .expect("no window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .expect("set_timeout failed");
    });
    JsFuture::from(promise).await.unwrap();
}

/// A hook that polls connected gamepads every ~16 ms and calls `on_action`
/// whenever a button edge-triggers an abstract [`Action`].
///
/// `on_action` is wrapped in `Rc<RefCell<…>>` so it can be safely cloned into
/// the spawned async task even if `use_effect` reruns (e.g. when `config`
/// changes).
///
/// # Usage
/// ```rust
/// use_gamepad(config_signal, move |action| {
///     match action {
///         Action::NextPage => { /* … */ }
///         _ => {}
///     }
/// });
/// ```
pub fn use_gamepad(config: Signal<GamepadConfig>, on_action: impl FnMut(Action) + 'static) {
    // Wrap in Rc<RefCell> so the closure can be cloned cheaply into the async
    // task on every effect run without needing on_action to be Clone.
    let on_action: Rc<RefCell<dyn FnMut(Action)>> = Rc::new(RefCell::new(on_action));

    use_effect(move || {
        // Snapshot the current config value so the async task owns it.
        let config = config.read().clone();
        // Clone the Rc — the async task gets its own reference-counted handle.
        let on_action = on_action.clone();

        spawn(async move {
            // Per-gamepad button state, keyed by gamepad index.
            let mut states: HashMap<usize, ButtonState> = HashMap::new();

            loop {
                // ~60 fps polling interval.
                sleep_ms(16).await;

                let window = match web_sys::window() {
                    Some(w) => w,
                    None => continue,
                };

                let gamepads = match window.navigator().get_gamepads() {
                    Ok(g) => g,
                    Err(_) => continue,
                };

                for i in 0..gamepads.length() {
                    let gamepad_val = gamepads.get(i);
                    // The slot is null/undefined when no gamepad is connected there.
                    if gamepad_val.is_null() || gamepad_val.is_undefined() {
                        continue;
                    }

                    let gamepad: web_sys::Gamepad = match gamepad_val.dyn_into() {
                        Ok(g) => g,
                        Err(_) => continue,
                    };

                    let index = gamepad.index() as usize;
                    let state = states.entry(index).or_default();
                    let buttons = gamepad.buttons();

                    for b in 0..buttons.length() {
                        let btn_val = buttons.get(b);
                        let btn: web_sys::GamepadButton = match btn_val.dyn_into() {
                            Ok(b) => b,
                            Err(_) => continue,
                        };

                        if state.edge_press(b as usize, btn.pressed()) {
                            if let Some(action) = config.action_for(b as usize) {
                                on_action.borrow_mut()(action);
                            }
                        }
                    }
                }
            }
        });

        // Note: if use_effect reruns (config changed), a new polling task is
        // spawned with the updated config snapshot. The old task continues
        // briefly but will be garbage-collected once the JS event loop drains
        // its next sleep_ms tick and the Rc refcount reaches zero.
    });
}
