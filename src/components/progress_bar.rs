//! Progress bar component showing reading progress.

use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct ProgressBarProps {
    /// Value 0.0 to 1.0
    pub value: f32,
    pub pages_read: u32,
    pub total_pages: u32,
}

#[component]
pub fn ProgressBar(props: ProgressBarProps) -> Element {
    let percent = (props.value * 100.0).clamp(0.0, 100.0);

    rsx! {
        div {
            class: "progress-bar-container",
            div {
                class: "progress-bar-fill",
                style: "width: {percent:.1}%",
            }
        }
        small {
            class: "progress-bar-label",
            "{props.pages_read} / {props.total_pages} pages"
        }
    }
}
