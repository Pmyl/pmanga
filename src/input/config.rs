//! Gamepad button binding configuration.
//! Mappings are persisted to localStorage.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::Action;

/// A gamepad button identified by its index in the Gamepad API `buttons` array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GamepadButton(pub usize);

impl GamepadButton {
    /// Human-readable label for common standard gamepad button indices.
    /// Based on the W3C Standard Gamepad mapping.
    pub fn label(&self) -> String {
        match self.0 {
            0 => "A / Cross".to_string(),
            1 => "B / Circle".to_string(),
            2 => "X / Square".to_string(),
            3 => "Y / Triangle".to_string(),
            4 => "L1 / LB".to_string(),
            5 => "R1 / RB".to_string(),
            6 => "L2 / LT".to_string(),
            7 => "R2 / RT".to_string(),
            8 => "Select / View".to_string(),
            9 => "Start / Menu".to_string(),
            10 => "L3 (Left Stick)".to_string(),
            11 => "R3 (Right Stick)".to_string(),
            12 => "D-Pad Up".to_string(),
            13 => "D-Pad Down".to_string(),
            14 => "D-Pad Left".to_string(),
            15 => "D-Pad Right".to_string(),
            n => format!("Button {n}"),
        }
    }
}

/// The full set of gamepad→action bindings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GamepadConfig {
    pub bindings: HashMap<usize, Action>,
}

impl Default for GamepadConfig {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        // Standard gamepad defaults:
        // R1 / RB  → NextPage
        bindings.insert(5, Action::NextPage);
        // L1 / LB  → PreviousPage
        bindings.insert(4, Action::PreviousPage);
        // Select / View → ToggleOverlay
        bindings.insert(8, Action::ToggleOverlay);
        // B / Circle → GoBack
        bindings.insert(1, Action::GoBack);
        // A / Cross → Confirm
        bindings.insert(0, Action::Confirm);
        Self { bindings }
    }
}

impl GamepadConfig {
    const STORAGE_KEY: &'static str = "pmanga_gamepad_config";

    /// Load from localStorage, falling back to defaults if absent or corrupt.
    pub fn load() -> Self {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            if let Ok(Some(raw)) = storage.get_item(Self::STORAGE_KEY) {
                if let Ok(cfg) = serde_json::from_str::<Self>(&raw) {
                    return cfg;
                }
            }
        }
        Self::default()
    }

    /// Persist to localStorage.
    pub fn save(&self) {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            if let Ok(json) = serde_json::to_string(self) {
                let _ = storage.set_item(Self::STORAGE_KEY, &json);
            }
        }
    }

    /// Set a binding: button index → action. Removes any previous binding for
    /// the same button, and also clears any previous button bound to this action
    /// so each action has at most one button at a time.
    pub fn set_binding(&mut self, button: usize, action: Action) {
        // Remove any existing button that was mapped to this action.
        self.bindings.retain(|_, a| *a != action);
        self.bindings.insert(button, action);
    }

    /// Remove the binding for a button index.
    pub fn clear_binding(&mut self, button: usize) {
        self.bindings.remove(&button);
    }

    /// Look up the action for a pressed button index, if any.
    pub fn action_for(&self, button: usize) -> Option<Action> {
        self.bindings.get(&button).copied()
    }

    /// Return the button currently bound to an action, if any.
    pub fn button_for(&self, action: Action) -> Option<GamepadButton> {
        self.bindings
            .iter()
            .find(|(_, a)| **a == action)
            .map(|(b, _)| GamepadButton(*b))
    }

    /// Reset all bindings to built-in defaults.
    pub fn reset_to_defaults(&mut self) {
        *self = Self::default();
    }

    /// Ordered list of (action, bound button) pairs for display in settings UI.
    pub fn display_rows(&self) -> Vec<(Action, Option<GamepadButton>)> {
        Action::all()
            .iter()
            .map(|a| (*a, self.button_for(*a)))
            .collect()
    }
}
