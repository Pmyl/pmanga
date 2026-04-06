//! Import source selection dialog.

use dioxus::prelude::*;

#[derive(Props, Clone, PartialEq)]
pub struct ImportSourceDialogProps {
    pub on_local: EventHandler<()>,
    pub on_weebcentral: EventHandler<()>,
    pub on_cancel: EventHandler<()>,
}

#[component]
pub fn ImportSourceDialog(props: ImportSourceDialogProps) -> Element {
    rsx! {
        div {
            class: "fixed inset-0 bg-black/70 flex items-center justify-center z-50 px-4",
            onclick: move |_| props.on_cancel.call(()),
            div {
                class: "bg-[#1a1a1a] rounded-xl p-5 w-full max-w-xs flex flex-col gap-4",
                onclick: move |e| e.stop_propagation(),

                h2 { class: "text-base font-semibold text-center", "Import Manga" }

                div { class: "flex flex-col gap-3",
                    button {
                        class: "border-0 cursor-pointer w-full px-4 py-3 rounded bg-[#e8b44a] text-black font-semibold active:bg-[#d4a03c] text-sm",
                        onclick: move |_| props.on_local.call(()),
                        "📁 Local Import"
                    }
                    button {
                        class: "border-0 cursor-pointer w-full px-4 py-3 rounded bg-[#5a9fd4] text-black font-semibold active:bg-[#4a8fc4] text-sm",
                        onclick: move |_| props.on_weebcentral.call(()),
                        "🌐 WeebCentral"
                    }
                }

                button {
                    class: "border-0 cursor-pointer text-sm text-[#666] bg-transparent py-1",
                    onclick: move |_| props.on_cancel.call(()),
                    "Cancel"
                }
            }
        }
    }
}
