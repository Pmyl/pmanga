//! WeebCentral import modal component.

use std::rc::Rc;

use dioxus::prelude::*;
use js_sys::Promise;
use wasm_bindgen_futures::JsFuture;

use crate::{
    bridge::weebcentral::{fetch_chapter_list, fetch_chapter_pages, fetch_series_meta},
    storage::{
        db::Db,
        models::{ChapterId, ChapterMeta, ChapterSource, MangaId, MangaMeta, MangaSource},
        progress::load_proxy_url,
        tankobon::{fetch_tankobon_csv, lookup_tankobon},
    },
};

// ---------------------------------------------------------------------------
// sleep_ms — identical pattern to settings.rs, no new dependency needed
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
// State machine
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum WcImportStep {
    InputUrl,
    Importing {
        done: usize,
        total: usize,
        status: String,
    },
    Done {
        manga_id: MangaId,
    },
    Error {
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[derive(Props, Clone)]
pub struct WeebCentralImporterProps {
    pub db: Rc<Db>,
    pub on_complete: EventHandler<MangaId>,
    pub on_cancel: EventHandler<()>,
}

// Always re-render — Rc<Db> doesn't implement PartialEq and the importer is
// short-lived anyway.
impl PartialEq for WeebCentralImporterProps {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

#[component]
pub fn WeebCentralImporter(props: WeebCentralImporterProps) -> Element {
    let mut step: Signal<WcImportStep> = use_signal(|| WcImportStep::InputUrl);
    let mut url_input: Signal<String> = use_signal(String::new);
    // Empty string means "no bound" — all chapters from/to that end.
    let mut from_input: Signal<String> = use_signal(String::new);
    let mut to_input: Signal<String> = use_signal(String::new);

    // Auto-call on_complete when the Done state is reached.
    let on_complete = props.on_complete.clone();
    use_effect(move || {
        if let WcImportStep::Done { ref manga_id } = *step.read() {
            on_complete.call(manga_id.clone());
        }
    });

    let db = props.db.clone();

    // -----------------------------------------------------------------------
    // Import handler
    // -----------------------------------------------------------------------
    let start_import = {
        let db = db.clone();
        move |_| {
            let raw_url = url_input.read().trim().to_string();
            let db = db.clone();

            // Basic validation
            if !raw_url.contains("weebcentral.com/series/") {
                step.set(WcImportStep::Error {
                    message: "URL must be a WeebCentral series URL containing \
                              \"weebcentral.com/series/\"."
                        .to_string(),
                });
                return;
            }

            // Parse optional chapter range bounds.
            let from_raw = from_input.read().trim().to_string();
            let to_raw = to_input.read().trim().to_string();

            let from_ch: Option<f32> = if from_raw.is_empty() {
                None
            } else {
                match from_raw.parse::<f32>() {
                    Ok(n) => Some(n),
                    Err(_) => {
                        step.set(WcImportStep::Error {
                            message: format!(
                                "\"From chapter\" must be a number, got \"{from_raw}\"."
                            ),
                        });
                        return;
                    }
                }
            };

            let to_ch: Option<f32> = if to_raw.is_empty() {
                None
            } else {
                match to_raw.parse::<f32>() {
                    Ok(n) => Some(n),
                    Err(_) => {
                        step.set(WcImportStep::Error {
                            message: format!("\"To chapter\" must be a number, got \"{to_raw}\"."),
                        });
                        return;
                    }
                }
            };

            if let (Some(f), Some(t)) = (from_ch, to_ch) {
                if f > t {
                    step.set(WcImportStep::Error {
                        message: format!("\"From\" ({f}) must not be greater than \"To\" ({t})."),
                    });
                    return;
                }
            }

            // Extract series_id (second path segment after "/series/")
            let series_id = match extract_series_id(&raw_url) {
                Some(id) => id,
                None => {
                    step.set(WcImportStep::Error {
                        message: "Could not extract series ID from URL.".to_string(),
                    });
                    return;
                }
            };

            // Load proxy URL from localStorage
            let proxy_url = match load_proxy_url() {
                Some(url) if !url.trim().is_empty() => url,
                _ => {
                    step.set(WcImportStep::Error {
                        message: "Proxy URL not configured. \
                                  Go to Settings → WeebCentral Proxy URL first."
                            .to_string(),
                    });
                    return;
                }
            };

            let series_url = raw_url.clone();

            step.set(WcImportStep::Importing {
                done: 0,
                total: 0,
                status: "Fetching series info…".to_string(),
            });

            spawn(async move {
                let csv_rows_snapshot = fetch_tankobon_csv().await;
                // 1. Fetch series meta
                let series_meta = match fetch_series_meta(&proxy_url, &series_url).await {
                    Ok(m) => m,
                    Err(e) => {
                        step.set(WcImportStep::Error {
                            message: format!("Failed to fetch series info: {e}"),
                        });
                        return;
                    }
                };

                // 2. Fetch chapter list
                step.set(WcImportStep::Importing {
                    done: 0,
                    total: 0,
                    status: "Fetching chapter list…".to_string(),
                });

                let mut chapters =
                    match fetch_chapter_list(&proxy_url, &series_meta.series_id).await {
                        Ok(chs) => chs,
                        Err(e) => {
                            step.set(WcImportStep::Error {
                                message: format!("Failed to fetch chapter list: {e}"),
                            });
                            return;
                        }
                    };

                // Sort ascending so chapter 1 is imported first.
                chapters.sort_by(|a, b| a.number.total_cmp(&b.number));

                // Apply range filter.
                let chapters: Vec<_> = chapters
                    .into_iter()
                    .filter(|c| {
                        if let Some(f) = from_ch {
                            if c.number < f {
                                return false;
                            }
                        }
                        if let Some(t) = to_ch {
                            if c.number > t {
                                return false;
                            }
                        }
                        true
                    })
                    .collect();

                let total = chapters.len();

                if total == 0 {
                    step.set(WcImportStep::Error {
                        message: "No chapters found in the requested range.".to_string(),
                    });
                    return;
                }

                // 3. Save MangaMeta — use the WeebCentral series_id as the app's MangaId
                let manga_id = MangaId(series_id.clone());
                let manga_meta = MangaMeta {
                    id: manga_id.clone(),
                    title: series_meta.title.clone(),
                    mangadex_id: None,
                    source: MangaSource::WeebCentral {
                        series_url: series_url.clone(),
                    },
                };

                if let Err(e) = db.save_manga(&manga_meta).await {
                    step.set(WcImportStep::Error {
                        message: format!("Failed to save manga: {e}"),
                    });
                    return;
                }

                // 4. For each chapter, fetch pages then save ChapterMeta
                let manga_title = series_meta.title.clone();
                for (i, chapter) in chapters.iter().enumerate() {
                    step.set(WcImportStep::Importing {
                        done: i,
                        total,
                        status: format!(
                            "Fetching pages for chapter {} ({}/{})…",
                            chapter.number,
                            i + 1,
                            total
                        ),
                    });

                    let pages = match fetch_chapter_pages(&proxy_url, &chapter.id).await {
                        Ok(p) => p,
                        Err(e) => {
                            step.set(WcImportStep::Error {
                                message: format!(
                                    "Failed to fetch pages for chapter {}: {e}",
                                    chapter.number
                                ),
                            });
                            return;
                        }
                    };

                    let tankobon_number =
                        lookup_tankobon(&manga_title, chapter.number, &csv_rows_snapshot);

                    let chapter_meta = ChapterMeta {
                        id: ChapterId(chapter.id.clone()),
                        manga_id: manga_id.clone(),
                        chapter_number: chapter.number,
                        tankobon_number,
                        filename: format!("Chapter {}", chapter.number),
                        page_count: pages.len() as u32,
                        source: ChapterSource::WeebCentral {
                            chapter_id: chapter.id.clone(),
                        },
                        page_urls: pages.into_iter().map(|p| p.url).collect(),
                    };

                    if let Err(e) = db.save_chapter(&chapter_meta).await {
                        step.set(WcImportStep::Error {
                            message: format!("Failed to save chapter {}: {e}", chapter.number),
                        });
                        return;
                    }

                    // Stagger requests to avoid rate-limiting
                    sleep_ms(500).await;
                }

                step.set(WcImportStep::Done {
                    manga_id: manga_id.clone(),
                });
            });
        }
    };

    // -----------------------------------------------------------------------
    // Render
    // -----------------------------------------------------------------------
    let current_step = step.read().clone();
    let on_cancel = props.on_cancel.clone();

    rsx! {
        // Modal backdrop
        div {
            class: "fixed inset-0 bg-black/70 flex items-center justify-center z-50 px-4",

            div {
                class: "bg-[#1a1a1a] rounded-xl p-5 w-full max-w-md flex flex-col gap-4",

                h2 { class: "text-base font-semibold", "Import from WeebCentral" }

                // ── InputUrl ─────────────────────────────────────────────
                if matches!(current_step, WcImportStep::InputUrl) {
                    div { class: "flex flex-col gap-3",
                        // Series URL
                        div { class: "flex flex-col gap-1",
                            label { class: "text-xs text-[#888]",
                                "Series URL"
                            }
                            input {
                                class: "bg-[#111] border border-[#333] rounded px-3 py-2 text-sm w-full focus:outline-none focus:border-[#555]",
                                r#type: "text",
                                placeholder: "https://weebcentral.com/series/…",
                                value: "{url_input}",
                                oninput: move |e| *url_input.write() = e.value(),
                            }
                        }

                        // Chapter range
                        div { class: "flex flex-col gap-1",
                            label { class: "text-xs text-[#888]",
                                "Chapter range "
                                span { class: "text-[#555]", "(leave blank to import all)" }
                            }
                            div { class: "flex gap-2 items-center",
                                input {
                                    class: "bg-[#111] border border-[#333] rounded px-3 py-2 text-sm w-full focus:outline-none focus:border-[#555]",
                                    r#type: "number",
                                    min: "1",
                                    step: "0.1",
                                    placeholder: "From",
                                    value: "{from_input}",
                                    oninput: move |e| *from_input.write() = e.value(),
                                }
                                span { class: "text-[#555] shrink-0 text-sm", "–" }
                                input {
                                    class: "bg-[#111] border border-[#333] rounded px-3 py-2 text-sm w-full focus:outline-none focus:border-[#555]",
                                    r#type: "number",
                                    min: "1",
                                    step: "0.1",
                                    placeholder: "To",
                                    value: "{to_input}",
                                    oninput: move |e| *to_input.write() = e.value(),
                                }
                            }
                        }

                        div { class: "flex gap-2 justify-end",
                            button {
                                class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-transparent border border-[#333] text-[#ccc] active:bg-[#222]",
                                onclick: move |_| on_cancel.call(()),
                                "Cancel"
                            }
                            button {
                                class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#e8b44a] text-black font-semibold active:bg-[#d4a03c]",
                                onclick: start_import,
                                "Import"
                            }
                        }
                    }
                }

                // ── Importing ─────────────────────────────────────────────
                if let WcImportStep::Importing { done, total, ref status } = current_step {
                    div { class: "flex flex-col gap-3",
                        p { class: "text-sm text-[#ccc]", "{status}" }
                        if total > 0 {
                            {
                                let pct = ((done as f32 / total as f32) * 100.0) as u32;
                                rsx! {
                                    div { class: "flex flex-col gap-1",
                                        div {
                                            class: "w-full bg-[#333] rounded-full h-2 overflow-hidden",
                                            div {
                                                class: "bg-[#e8b44a] h-2 rounded-full transition-all",
                                                style: "width: {pct}%",
                                            }
                                        }
                                        p {
                                            class: "text-xs text-[#666] text-right",
                                            "{done} / {total} chapters"
                                        }
                                    }
                                }
                            }
                        }
                        p {
                            class: "text-xs text-[#555] italic",
                            "Import running — please wait…"
                        }
                    }
                }

                // ── Done ─────────────────────────────────────────────────
                if matches!(current_step, WcImportStep::Done { .. }) {
                    p {
                        class: "text-sm text-[#4caf50]",
                        "Import complete! Opening library…"
                    }
                }

                // ── Error ─────────────────────────────────────────────────
                if let WcImportStep::Error { ref message } = current_step {
                    div { class: "flex flex-col gap-3",
                        p { class: "text-sm text-[#cf6679]", "{message}" }
                        div { class: "flex justify-end",
                            button {
                                class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-transparent border border-[#333] text-[#ccc] active:bg-[#222]",
                                onclick: move |_| step.set(WcImportStep::InputUrl),
                                "Back"
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the series_id from a WeebCentral URL.
///
/// `https://weebcentral.com/series/01J76XY7E9FNDZ1DBBM6PBJPFK/one-piece`
/// → `Some("01J76XY7E9FNDZ1DBBM6PBJPFK")`
fn extract_series_id(url: &str) -> Option<String> {
    let after = url.split("/series/").nth(1)?;
    let id = after.split('/').next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}
