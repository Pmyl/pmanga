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
            class: "fixed inset-0 bg-black/75 flex items-center justify-center z-[100]",
            onclick: move |_| props.on_cancel.call(()),
            div {
                class: "bg-[#1a1a1a] rounded-xl p-6 max-w-xs w-[90%] flex flex-col gap-4",
                // Stop clicks inside the dialog from closing it via the overlay handler.
                onclick: move |e| e.stop_propagation(),
                p {
                    class: "text-sm text-[#ccc]",
                    "{props.message}"
                }
                div {
                    class: "flex gap-2 justify-end",
                    button {
                        class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#8b1a1a] text-[#f0f0f0] active:bg-[#a82020]",
                        onclick: move |_| props.on_confirm.call(()),
                        "Confirm"
                    }
                    button {
                        class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#252525] text-[#f0f0f0] active:bg-[#333]",
                        onclick: move |_| props.on_cancel.call(()),
                        "Cancel"
                    }
                }
            }
        }
    }
}
