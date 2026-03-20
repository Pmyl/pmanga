//! Modal confirmation dialog component.

use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct ConfirmDialogProps {
    pub message: String,
    pub on_confirm: EventHandler<()>,
    pub on_cancel: EventHandler<()>,
}

#[component]
pub fn ConfirmDialog(props: ConfirmDialogProps) -> Element {
    rsx! {
        div {
            class: "modal-overlay",
            onclick: move |_| props.on_cancel.call(()),
            div {
                class: "modal-dialog",
                // Stop clicks inside the dialog from closing it via the overlay handler.
                onclick: move |e| e.stop_propagation(),
                p {
                    class: "modal-message",
                    "{props.message}"
                }
                div {
                    class: "modal-actions",
                    button {
                        class: "btn btn-danger",
                        onclick: move |_| props.on_confirm.call(()),
                        "Confirm"
                    }
                    button {
                        class: "btn btn-secondary",
                        onclick: move |_| props.on_cancel.call(()),
                        "Cancel"
                    }
                }
            }
        }
    }
}
