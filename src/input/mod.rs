pub mod config;
pub mod gamepad;

use serde::{Deserialize, Serialize};

/// Abstract input actions used throughout the app.
/// All input sources (touch, mouse, gamepad) map into these actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    NextPage,
    PreviousPage,
    ToggleOverlay,
    GoBack,
    Confirm,
}

impl Action {
    pub fn all() -> &'static [Action] {
        &[
            Action::NextPage,
            Action::PreviousPage,
            Action::ToggleOverlay,
            Action::GoBack,
            Action::Confirm,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Action::NextPage => "Next Page",
            Action::PreviousPage => "Previous Page",
            Action::ToggleOverlay => "Toggle Info Overlay",
            Action::GoBack => "Go Back",
            Action::Confirm => "Confirm",
        }
    }
}
