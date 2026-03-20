//! Multi-step import modal/dialog component.
//!
//! Can be opened from:
//! - The Shelf page: no manga context, name is guessed from filename.
//! - The Library page: manga context provided, name is pre-filled.

use std::rc::Rc;

use dioxus::prelude::*;

use crate::{
    bridge::js::{
        self, ChapterVolumeEntry, bytes_to_blob, mangadex_chapters, mangadex_search,
        render_page_to_uint8array,
    },
    storage::{
        db::Db,
        models::{ChapterId, ChapterMeta, MangaId, MangaMeta},
    },
};

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// A single row in the review table.
#[derive(Clone, PartialEq)]
pub struct ImportRow {
    pub filename: String,
    pub manga_name: String,
    pub chapter_number: String,
    pub tankobon_number: String,
    pub pdf_bytes: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Internal step state
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum ImportStep {
    /// Step 1: file picker
    SelectFiles,
    /// Step 1.5: loading MangaDex results (only when no preset manga)
    FetchingMangaDex,
    /// Step 2: pick from MangaDex results or skip
    SelectMangaDex { results: Vec<(String, String)> },
    /// Step 3: editable review table
    Review { rows: Vec<ImportRow> },
    /// Step 4: importing with progress
    Importing { done: usize, total: usize },
}

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

#[derive(Props, Clone)]
pub struct ImporterProps {
    /// If `Some`, the manga is pre-known (import from Library page).
    /// If `None`, manga name must be guessed (import from Shelf page).
    pub preset_manga: Option<MangaMeta>,
    /// Called when import is complete (with the MangaId that was imported into).
    pub on_complete: EventHandler<MangaId>,
    /// Called when the user cancels.
    pub on_cancel: EventHandler<()>,
    /// The open DB handle.
    pub db: Rc<Db>,
}

// Always re-render — Rc doesn't implement PartialEq and the importer is
// short-lived anyway.
impl PartialEq for ImporterProps {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a simple random hex ID using js_sys::Math::random().
fn random_id() -> String {
    let a = (js_sys::Math::random() * u32::MAX as f64) as u64;
    let b = (js_sys::Math::random() * u32::MAX as f64) as u64;
    format!("{:08x}{:08x}", a, b)
}

/// Async-read every file from a Dioxus `Vec<FileData>`, extracting ZIPs inline.
/// Returns `(filename, bytes)` pairs for all PDFs found.
async fn collect_pdf_entries_from_files(
    files: Vec<dioxus::html::FileData>,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut all: Vec<(String, Vec<u8>)> = Vec::new();

    for file in files {
        let name = file.name();
        let bytes_result = file.read_bytes().await;
        let bytes = bytes_result
            .map_err(|e| format!("file read error: {e}"))?
            .to_vec();

        let lower = name.to_lowercase();
        if lower.ends_with(".zip") {
            let entries = js::extract_zip(bytes).await?;
            for (entry_name, entry_bytes) in entries {
                all.push((entry_name, entry_bytes));
            }
        } else {
            all.push((name, bytes));
        }
    }

    Ok(all)
}

// ---------------------------------------------------------------------------
// Chapter-number heuristic
// ---------------------------------------------------------------------------

/// Extract a chapter number string from a filename stem.
fn guess_chapter_number(stem: &str, fallback_idx: usize) -> String {
    let lower = stem.to_lowercase();

    // 1. "chapter(\d+\.?\d*)" or "chapter_(\d+\.?\d*)"
    if let Some(n) = extract_after_keyword(&lower, "chapter") {
        return n;
    }
    // 2. "ch.(\d+\.?\d*)" or "ch(\d+\.?\d*)"
    if let Some(n) = extract_after_keyword(&lower, "ch") {
        return n;
    }
    // 3. Bare number at end: trailing whitespace/underscore then digits
    if let Some(n) = extract_trailing_number(stem) {
        return n;
    }
    // 4. Fallback: index starting at 1
    (fallback_idx + 1).to_string()
}

/// After finding `keyword` in `s`, skip optional separators and collect a
/// decimal number.
fn extract_after_keyword(s: &str, keyword: &str) -> Option<String> {
    let mut search = s;
    while let Some(pos) = search.find(keyword) {
        let rest = &search[pos + keyword.len()..];
        // skip separators
        let rest = rest.trim_start_matches(|c: char| c == '_' || c == '.' || c == ' ');
        if let Some(num) = collect_number(rest) {
            return Some(num);
        }
        search = &search[pos + 1..];
    }
    None
}

/// Collect leading `\d+(\.\d+)?` from s.
fn collect_number(s: &str) -> Option<String> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = &s[digits.len()..];
    if rest.starts_with('.') {
        let frac: String = rest[1..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !frac.is_empty() {
            return Some(format!("{}.{}", digits, frac));
        }
    }
    Some(digits)
}

/// Extract trailing number from a filename stem.
fn extract_trailing_number(stem: &str) -> Option<String> {
    let trimmed = stem.trim_end_matches(|c: char| c == ' ' || c == '_' || c == '-');
    let chars: Vec<char> = trimmed.chars().collect();
    let end = chars.len();
    let mut dot_seen = false;

    let mut i = end;
    while i > 0 {
        i -= 1;
        let c = chars[i];
        if c.is_ascii_digit() {
            // continue
        } else if c == '.' && !dot_seen {
            dot_seen = true;
        } else {
            i += 1;
            break;
        }
    }

    if i < end && i > 0 {
        let num: String = chars[i..end].iter().collect();
        let trimmed_num = num.trim_matches('.');
        if !trimmed_num.is_empty() {
            return Some(trimmed_num.to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Manga-name guess from filename
// ---------------------------------------------------------------------------

fn guess_manga_name(filename: &str) -> String {
    // Strip extension.
    let stem = if let Some(dot) = filename.rfind('.') {
        &filename[..dot]
    } else {
        filename
    };

    // Remove common chapter/volume suffixes.
    let cleaned = strip_chapter_suffix(stem);

    // Replace underscores and hyphens with spaces.
    let spaced = cleaned.replace('_', " ").replace('-', " ");

    // Collapse multiple spaces and title-case.
    let collapsed = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    title_case(&collapsed)
}

/// Strip trailing chapter/volume designators.
fn strip_chapter_suffix(s: &str) -> &str {
    let lower = s.to_lowercase();
    let patterns: &[&str] = &[" - ", " ch", "_ch", " chapter", "_chapter", " vol", "_vol"];
    let mut best = s.len();

    for &pat in patterns {
        if let Some(idx) = lower.rfind(pat) {
            let after = &lower[idx + pat.len()..];
            let after_trimmed = after.trim_start_matches(|c: char| c == '.' || c == ' ');
            if !after_trimmed.is_empty()
                && after_trimmed
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '.')
            {
                best = best.min(idx);
            }
        }
    }

    // Strip bare " \d+" at the end.
    if let Some(last_space) = s.rfind(' ') {
        let after = &s[last_space + 1..];
        if !after.is_empty() && after.chars().all(|c| c.is_ascii_digit() || c == '.') {
            best = best.min(last_space);
        }
    }

    &s[..best]
}

fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    upper + chars.as_str()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Tankobon lookup from MangaDex chapter data
// ---------------------------------------------------------------------------

fn lookup_tankobon_from_mangadex(chapter_str: &str, entries: &[ChapterVolumeEntry]) -> Option<u32> {
    let target: f32 = chapter_str.parse().ok()?;

    for entry in entries {
        let entry_ch: f32 = entry.chapter.parse().ok()?;
        if (entry_ch - target).abs() < 0.01 {
            if let Some(vol_str) = &entry.volume {
                return vol_str.parse().ok();
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tankobon CSV lookup
// ---------------------------------------------------------------------------

/// Simple CSV row: manga_title, chapter_number, tankobon_number
#[derive(Clone)]
struct TankobonRow {
    manga_title: String,
    chapter: String,
    tankobon: u32,
}

fn parse_tankobon_csv(csv: &str) -> Vec<TankobonRow> {
    let mut rows = Vec::new();
    for line in csv.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, ',').collect();
        if parts.len() < 3 {
            continue;
        }
        let manga_title = parts[0].trim().to_string();
        let chapter = parts[1].trim().to_string();
        let tankobon: u32 = match parts[2].trim().parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        rows.push(TankobonRow {
            manga_title,
            chapter,
            tankobon,
        });
    }
    rows
}

fn lookup_tankobon_from_csv(
    manga_name: &str,
    chapter_str: &str,
    rows: &[TankobonRow],
) -> Option<u32> {
    let target_ch: f32 = chapter_str.parse().ok()?;
    let manga_lower = manga_name.to_lowercase();

    for row in rows {
        if row.manga_title.to_lowercase() != manga_lower {
            continue;
        }
        let row_ch: f32 = row.chapter.parse().ok()?;
        if (row_ch - target_ch).abs() < 0.01 {
            return Some(row.tankobon);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Build ImportRows from collected pdf entries
// ---------------------------------------------------------------------------

fn build_rows(
    entries: Vec<(String, Vec<u8>)>,
    manga_name: &str,
    mangadex_entries: &[ChapterVolumeEntry],
    csv_rows: &[TankobonRow],
) -> Vec<ImportRow> {
    entries
        .into_iter()
        .enumerate()
        .map(|(idx, (filename, pdf_bytes))| {
            let stem = if let Some(dot) = filename.rfind('.') {
                &filename[..dot]
            } else {
                &filename[..]
            };
            let chapter_number = guess_chapter_number(stem, idx);

            // Resolve tankobon: MangaDex takes priority over CSV.
            let tankobon_number = if !mangadex_entries.is_empty() {
                lookup_tankobon_from_mangadex(&chapter_number, mangadex_entries)
            } else {
                lookup_tankobon_from_csv(manga_name, &chapter_number, csv_rows)
            };

            ImportRow {
                filename,
                manga_name: manga_name.to_string(),
                chapter_number,
                tankobon_number: tankobon_number.map(|n| n.to_string()).unwrap_or_default(),
                pdf_bytes,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[component]
pub fn Importer(props: ImporterProps) -> Element {
    let ImporterProps {
        preset_manga,
        on_complete,
        on_cancel,
        db,
    } = props;

    // --- state ---
    let step: Signal<ImportStep> = use_signal(|| ImportStep::SelectFiles);

    // Guessed / edited manga name (used when no preset_manga).
    let manga_name_guess: Signal<String> = use_signal(|| {
        preset_manga
            .as_ref()
            .map(|m| m.title.clone())
            .unwrap_or_default()
    });

    // Collected raw pdf entries from file input, persisted between steps.
    let pdf_entries: Signal<Vec<(String, Vec<u8>)>> = use_signal(Vec::new);

    // MangaDex chapter-to-volume entries collected in step 2.
    let mdex_entries: Signal<Vec<ChapterVolumeEntry>> = use_signal(Vec::new);

    // Parsed rows from tankobon_db.csv fetched on mount (empty if absent/unreachable).
    let csv_rows: Signal<Vec<TankobonRow>> = use_signal(Vec::new);

    // Error message shown to the user.
    let error_msg: Signal<Option<String>> = use_signal(|| None);

    // Fetch tankobon_db.csv once on mount as an offline fallback for tankobon lookup.
    {
        let mut csv_rows = csv_rows;
        use_effect(move || {
            wasm_bindgen_futures::spawn_local(async move {
                let fetched = dioxus::document::eval(
                    r#"
                    try {
                        const resp = await fetch('/tankobon_db.csv');
                        if (resp.ok) {
                            dioxus.send(await resp.text());
                        } else {
                            dioxus.send(null);
                        }
                    } catch (_) {
                        dioxus.send(null);
                    }
                    "#,
                )
                .recv::<Option<String>>()
                .await;

                if let Ok(Some(text)) = fetched {
                    *csv_rows.write() = parse_tankobon_csv(&text);
                }
            });
        });
    }

    let step_read = step.read().clone();

    rsx! {
        // --- overlay backdrop ---
        div {
            class: "fixed inset-0 bg-black/75 z-[100]",
            onclick: move |_| on_cancel.call(()),
        }
        // --- dialog box ---
        div {
            class: "bg-[#1a1a1a] rounded-xl p-5 w-[95%] max-w-2xl max-h-[90vh] flex flex-col gap-4 overflow-y-auto fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-[101]",
            // Stop backdrop click from leaking through.
            onclick: move |e| e.stop_propagation(),

            div {
                class: "flex justify-between items-center shrink-0",
                h2 { class: "text-base font-semibold", "Import Chapters" }
                button {
                    class: "border-0 cursor-pointer text-lg px-2 py-1 rounded bg-transparent text-[#888] active:text-[#f0f0f0]",
                    onclick: move |_| on_cancel.call(()),
                    "✕"
                }
            }

            // Error banner
            if let Some(err) = error_msg.read().clone() {
                div {
                    class: "bg-[#8b1a1a]/30 border border-[#8b1a1a] rounded p-2 text-sm text-[#f0f0f0] flex justify-between items-center",
                    "⚠ {err}"
                    button {
                        class: "border-0 cursor-pointer text-lg px-2 py-1 rounded bg-transparent text-[#888] active:text-[#f0f0f0]",
                        onclick: {
                            let mut error_msg = error_msg.clone();
                            move |_| *error_msg.write() = None
                        },
                        " ✕"
                    }
                }
            }

            div {
                class: "flex-1 overflow-y-auto",
                match step_read {
                    // -------------------------------------------------------
                    // Step 1 — file picker
                    // -------------------------------------------------------
                    ImportStep::SelectFiles => rsx! {
                        StepSelectFiles {
                            preset_manga: preset_manga.clone(),
                            step,
                            manga_name_guess,
                            pdf_entries,
                            mdex_entries,
                            error_msg,
                            csv_rows,
                        }
                    },

                    // -------------------------------------------------------
                    // Step 1.5 — fetching MangaDex
                    // -------------------------------------------------------
                    ImportStep::FetchingMangaDex => rsx! {
                        div {
                            class: "flex flex-col items-center gap-3 py-8",
                            div { class: "w-8 h-8 rounded-full border-2 border-[#333] border-t-[#e8b44a] animate-spin" }
                            p { "Searching MangaDex…" }
                        }
                    },

                    // -------------------------------------------------------
                    // Step 2 — MangaDex result picker
                    // -------------------------------------------------------
                    ImportStep::SelectMangaDex { results } => rsx! {
                        StepSelectMangaDex {
                            results,
                            step,
                            manga_name_guess,
                            pdf_entries,
                            mdex_entries,
                            error_msg,
                            csv_rows,
                        }
                    },

                    // -------------------------------------------------------
                    // Step 3 — review table
                    // -------------------------------------------------------
                    ImportStep::Review { rows } => rsx! {
                        StepReview {
                            rows,
                            step,
                            error_msg,
                            db: db.clone(),
                            preset_manga: preset_manga.clone(),
                            on_complete,
                        }
                    },

                    // -------------------------------------------------------
                    // Step 4 — progress
                    // -------------------------------------------------------
                    ImportStep::Importing { done, total } => rsx! {
                        div {
                            class: "flex flex-col gap-2",
                            p {
                                class: "text-sm text-[#ccc]",
                                "Importing {done} / {total} pages…"
                            }
                            div {
                                class: "h-[3px] bg-[#333] rounded-sm overflow-hidden",
                                div {
                                    class: "h-full bg-[#e8b44a] rounded-sm",
                                    style: {
                                        let pct = if total > 0 {
                                            done * 100 / total
                                        } else {
                                            0
                                        };
                                        format!("width: {}%", pct)
                                    },
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Step 1 sub-component
// ---------------------------------------------------------------------------

#[derive(Props, Clone)]
struct StepSelectFilesProps {
    preset_manga: Option<MangaMeta>,
    step: Signal<ImportStep>,
    manga_name_guess: Signal<String>,
    pdf_entries: Signal<Vec<(String, Vec<u8>)>>,
    mdex_entries: Signal<Vec<ChapterVolumeEntry>>,
    error_msg: Signal<Option<String>>,
    csv_rows: Signal<Vec<TankobonRow>>,
}

impl PartialEq for StepSelectFilesProps {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

#[component]
fn StepSelectFiles(props: StepSelectFilesProps) -> Element {
    let StepSelectFilesProps {
        preset_manga,
        csv_rows,
        mut step,
        mut manga_name_guess,
        pdf_entries,
        mut mdex_entries,
        mut error_msg,
    } = props;

    let has_preset = preset_manga.is_some();
    // Pre-clone so both the onchange and onclick closures can each capture
    // their own copy without conflicting over ownership.
    let preset_manga_for_change = preset_manga.clone();
    let preset_manga_for_click = preset_manga.clone();

    // When files are selected via the input, read them immediately and cache
    // both the file list and a guessed manga name.  We store the FileList as
    // a signal so the "Next" button can drive the async work.
    //
    // Because FileList is not Clone/Send, we store the files as raw bytes
    // right here in the onchange handler (async spawn).

    rsx! {
        div {
            class: "flex flex-col gap-3",

            label {
                class: "text-sm text-[#888]",
                "Select PDF or ZIP files"
            }

            input {
                r#type: "file",
                accept: ".pdf,.zip",
                multiple: true,
                onchange: move |e: Event<FormData>| {
                    // Use the Dioxus FileData API to read files from the event.
                    let files = e.files();

                    let mut pdf_entries = pdf_entries.clone();
                    let mut manga_name_guess = manga_name_guess.clone();
                    let mut error_msg = error_msg.clone();
                    let has_preset = preset_manga_for_change.is_some();
                    let preset_title = preset_manga_for_change.as_ref().map(|m| m.title.clone());

                    wasm_bindgen_futures::spawn_local(async move {
                        match collect_pdf_entries_from_files(files).await {
                            Ok(entries) => {
                                if !entries.is_empty() {
                                    // Guess manga name from first filename only
                                    // when there is no preset and the field is
                                    // still empty.
                                    if !has_preset && manga_name_guess.read().is_empty() {
                                        let guessed = guess_manga_name(&entries[0].0);
                                        *manga_name_guess.write() = guessed;
                                    }
                                    // If we have a preset, ensure the name signal
                                    // reflects the preset title.
                                    if has_preset {
                                        if let Some(title) = preset_title {
                                            *manga_name_guess.write() = title;
                                        }
                                    }
                                }
                                *pdf_entries.write() = entries;
                            }
                            Err(e) => {
                                *error_msg.write() = Some(format!("File read error: {e}"));
                            }
                        }
                    });
                },
            }

            // If no preset manga, show editable name field.
            if !has_preset {
                div {
                    class: "flex flex-col gap-1",
                    label { "Manga name (guessed from filename)" }
                    input {
                        r#type: "text",
                        class: "bg-[#252525] border border-[#333] rounded px-2.5 py-1.5 text-[#f0f0f0] text-sm outline-none focus:border-[#e8b44a]",
                        placeholder: "e.g. One Piece",
                        value: "{manga_name_guess}",
                        oninput: move |e| *manga_name_guess.write() = e.value(),
                    }
                }
            }

            div {
                class: "flex gap-2 justify-end mt-2",
                button {
                    class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#e8b44a] text-black font-semibold active:bg-[#d4a03c]",
                    onclick: move |_| {
                        let entries = pdf_entries.read().clone();

                        if entries.is_empty() {
                            *error_msg.write() = Some(
                                "No PDF files selected. Please choose files first.".into(),
                            );
                            return;
                        }

                        let preset = preset_manga_for_click.clone();
                        let manga_name = manga_name_guess.read().clone();

                        if preset.is_some() {
                            // Skip MangaDex — go straight to review.
                            let name = preset.as_ref().unwrap().title.clone();
                            let csv = csv_rows.read().clone();
                            let rows = build_rows(entries, &name, &[], &csv);
                            *mdex_entries.write() = vec![];
                            *step.write() = ImportStep::Review { rows };
                        } else {
                            // Search MangaDex.
                            let search_name = if manga_name.is_empty() {
                                guess_manga_name(
                                    &pdf_entries.read().first().map(|(n, _)| n.clone()).unwrap_or_default(),
                                )
                            } else {
                                manga_name.clone()
                            };

                            let mut step = step.clone();
                            let mut error_msg = error_msg.clone();
                            let mut step2 = step.clone();

                            *step2.write() = ImportStep::FetchingMangaDex;

                            wasm_bindgen_futures::spawn_local(async move {
                                match mangadex_search(&search_name).await {
                                    Ok(results) => {
                                        *step.write() =
                                            ImportStep::SelectMangaDex { results };
                                    }
                                    Err(e) => {
                                        *error_msg.write() =
                                            Some(format!("MangaDex search error: {e}"));
                                        *step.write() = ImportStep::SelectFiles;
                                    }
                                }
                            });
                        }
                    },
                    "Next →"
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Step 2 sub-component
// ---------------------------------------------------------------------------

#[derive(Props, Clone)]
struct StepSelectMangaDexProps {
    results: Vec<(String, String)>,
    step: Signal<ImportStep>,
    manga_name_guess: Signal<String>,
    pdf_entries: Signal<Vec<(String, Vec<u8>)>>,
    mdex_entries: Signal<Vec<ChapterVolumeEntry>>,
    error_msg: Signal<Option<String>>,
    csv_rows: Signal<Vec<TankobonRow>>,
}

impl PartialEq for StepSelectMangaDexProps {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

#[component]
fn StepSelectMangaDex(props: StepSelectMangaDexProps) -> Element {
    let StepSelectMangaDexProps {
        results,
        step,
        manga_name_guess,
        pdf_entries,
        mdex_entries,
        error_msg,
        csv_rows,
    } = props;

    rsx! {
        div {
            class: "flex flex-col gap-3",
            p {
                class: "text-sm text-[#888]",
                "Select the matching manga on MangaDex to auto-fill tankobon volumes, or skip."
            }

            if results.is_empty() {
                p { class: "text-sm text-[#888]", "No results found on MangaDex." }
            } else {
                ul { class: "flex flex-col gap-1.5 max-h-60 overflow-y-auto",
                    for (id, title) in results.iter() {
                        li {
                            button {
                                class: "bg-[#252525] border border-[#333] rounded px-3 py-2 text-[#f0f0f0] text-left cursor-pointer text-sm active:bg-[#333] w-full",
                                onclick: {
                                    let id = id.clone();
                                    let step = step.clone();
                                    let mdex_entries = mdex_entries.clone();
                                    let error_msg = error_msg.clone();

                                    let manga_name_guess = manga_name_guess.clone();
                                    let pdf_entries = pdf_entries.clone();
                                    move |_| {
                                        let id = id.clone();
                                        let mut step = step.clone();
                                        let mut mdex_entries = mdex_entries.clone();
                                        let mut error_msg = error_msg.clone();
                                        let manga_name_guess = manga_name_guess.clone();
                                        let pdf_entries = pdf_entries.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            *step.write() = ImportStep::FetchingMangaDex;
                                            match mangadex_chapters(&id).await {
                                                Ok(entries) => {
                                                    *mdex_entries.write() = entries;
                                                    let name =
                                                        manga_name_guess.read().clone();
                                                    let pdf_ents =
                                                        pdf_entries.read().clone();
                                                    let mdex =
                                                        mdex_entries.read().clone();
                                                    let csv = csv_rows.read().clone();
                                                    let rows = build_rows(
                                                        pdf_ents, &name, &mdex, &csv,
                                                    );
                                                    *step.write() =
                                                        ImportStep::Review { rows };
                                                }
                                                Err(e) => {
                                                    *error_msg.write() = Some(format!(
                                                        "Failed to load chapters: {e}"
                                                    ));
                                                    *step.write() =
                                                        ImportStep::SelectFiles;
                                                }
                                            }
                                        });
                                    }
                                },
                                "{title}"
                            }
                        }
                    }
                }
            }

            div {
                class: "flex gap-2 justify-end mt-2",
                button {
                    class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#252525] text-[#f0f0f0] active:bg-[#333]",
                    onclick: {
                        let step = step.clone();
                        let manga_name_guess = manga_name_guess.clone();
                        let pdf_entries = pdf_entries.clone();
                        let mdex_entries = mdex_entries.clone();
                        move |_| {
                            let mut step = step.clone();
                            let name = manga_name_guess.read().clone();
                            let entries = pdf_entries.read().clone();
                            let mdex = mdex_entries.read().clone();
                            let csv = csv_rows.read().clone();
                            let rows = build_rows(entries, &name, &mdex, &csv);
                            *step.write() = ImportStep::Review { rows };
                        }
                    },
                    "Skip / Not listed"
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Step 3 sub-component
// ---------------------------------------------------------------------------

#[derive(Props, Clone)]
struct StepReviewProps {
    rows: Vec<ImportRow>,
    step: Signal<ImportStep>,
    error_msg: Signal<Option<String>>,
    db: Rc<Db>,
    preset_manga: Option<MangaMeta>,
    on_complete: EventHandler<MangaId>,
}

impl PartialEq for StepReviewProps {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

#[component]
fn StepReview(props: StepReviewProps) -> Element {
    let StepReviewProps {
        rows,
        mut step,
        mut error_msg,
        db,
        preset_manga,
        on_complete,
    } = props;

    // Local editable copy of the rows.
    let local_rows: Signal<Vec<ImportRow>> = use_signal(|| rows);

    rsx! {
        div {
            class: "flex flex-col gap-3",
            p {
                class: "text-xs text-[#888]",
                "Review and edit the import details before proceeding."
            }

            div {
                class: "overflow-x-auto",
                table {
                    class: "w-full border-collapse text-xs",
                    thead {
                        tr {
                            th { class: "px-2 py-1.5 text-left text-[#888] border-b border-[#333]", "Filename" }
                            th { class: "px-2 py-1.5 text-left text-[#888] border-b border-[#333]", "Manga" }
                            th { class: "px-2 py-1.5 text-left text-[#888] border-b border-[#333]", "Chapter #" }
                            th { class: "px-2 py-1.5 text-left text-[#888] border-b border-[#333]", "Tankobon #" }
                            th { class: "px-2 py-1.5 text-left text-[#888] border-b border-[#333]", "" }
                        }
                    }
                    tbody {
                        for (i, row) in local_rows.read().clone().iter().enumerate() {
                            tr {
                                key: "{i}",
                                td {
                                    class: "px-1.5 py-1 border-b border-[#222]",
                                    span {
                                        class: "text-[#888] text-xs",
                                        title: "{row.filename}",
                                        "{row.filename}"
                                    }
                                }
                                td {
                                    class: "px-1.5 py-1 border-b border-[#222]",
                                    input {
                                        r#type: "text",
                                        class: "bg-[#252525] border border-[#333] rounded-sm px-1.5 py-0.5 text-[#f0f0f0] w-full text-xs",
                                        value: "{row.manga_name}",
                                        oninput: {
                                            let mut local_rows = local_rows.clone();
                                            move |e| {
                                                local_rows.write()[i].manga_name =
                                                    e.value();
                                            }
                                        },
                                    }
                                }
                                td {
                                    class: "px-1.5 py-1 border-b border-[#222]",
                                    input {
                                        r#type: "text",
                                        class: "bg-[#252525] border border-[#333] rounded-sm px-1.5 py-0.5 text-[#f0f0f0] w-16 text-xs",
                                        value: "{row.chapter_number}",
                                        oninput: {
                                            let mut local_rows = local_rows.clone();
                                            move |e| {
                                                local_rows.write()[i].chapter_number =
                                                    e.value();
                                            }
                                        },
                                    }
                                }
                                td {
                                    class: "px-1.5 py-1 border-b border-[#222]",
                                    input {
                                        r#type: "text",
                                        class: "bg-[#252525] border border-[#333] rounded-sm px-1.5 py-0.5 text-[#f0f0f0] w-16 text-xs",
                                        value: "{row.tankobon_number}",
                                        placeholder: "—",
                                        oninput: {
                                            let mut local_rows = local_rows.clone();
                                            move |e| {
                                                local_rows.write()[i].tankobon_number =
                                                    e.value();
                                            }
                                        },
                                    }
                                }
                                td {
                                    class: "px-1.5 py-1 border-b border-[#222]",
                                    button {
                                        class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#8b1a1a] text-[#f0f0f0] active:bg-[#a82020]",
                                        title: "Remove row",
                                        onclick: {
                                            let mut local_rows = local_rows.clone();
                                            move |_| {
                                                local_rows.write().remove(i);
                                            }
                                        },
                                        "✕"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            div {
                class: "flex gap-2 justify-end mt-2",
                button {
                    class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#252525] text-[#f0f0f0] active:bg-[#333]",
                    onclick: move |_| *step.write() = ImportStep::SelectFiles,
                    "← Back"
                }
                button {
                    class: "border-0 cursor-pointer text-sm px-3 py-1.5 rounded bg-[#e8b44a] text-black font-semibold active:bg-[#d4a03c]",
                    onclick: {
                        let db = db.clone();
                        let preset_manga = preset_manga.clone();
                        move |_| {
                            let rows_snapshot = local_rows.read().clone();
                            if rows_snapshot.is_empty() {
                                *error_msg.write() = Some("No rows to import.".into());
                                return;
                            }

                            let total_rows = rows_snapshot.len();
                            *step.write() = ImportStep::Importing {
                                done: 0,
                                total: total_rows,
                            };

                            let db = db.clone();
                            let preset_manga = preset_manga.clone();
                            let mut step = step.clone();
                            let mut error_msg = error_msg.clone();
                            let on_complete = on_complete.clone();

                            wasm_bindgen_futures::spawn_local(async move {
                                let result = run_import(
                                    rows_snapshot,
                                    db,
                                    preset_manga,
                                    &mut step,
                                )
                                .await;

                                match result {
                                    Ok(manga_id) => on_complete.call(manga_id),
                                    Err(e) => {
                                        *error_msg.write() = Some(e);
                                        *step.write() = ImportStep::SelectFiles;
                                    }
                                }
                            });
                        }
                    },
                    "Import"
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Core import logic (Step 4)
// ---------------------------------------------------------------------------

async fn run_import(
    rows: Vec<ImportRow>,
    db: Rc<Db>,
    preset_manga: Option<MangaMeta>,
    step: &mut Signal<ImportStep>,
) -> Result<MangaId, String> {
    let mut done_pages = 0usize;
    let mut total_pages = 0usize;

    // First pass: count total pages across all valid rows.
    for row in &rows {
        if row.chapter_number.parse::<f32>().is_err() {
            continue;
        }
        let count = js::pdf_page_count(row.pdf_bytes.clone()).await.unwrap_or(0);
        total_pages += count as usize;
    }

    *step.write() = ImportStep::Importing {
        done: 0,
        total: total_pages,
    };

    // Load existing mangas to match by title.
    let existing_mangas = db.load_all_mangas().await.unwrap_or_default();

    let mut last_manga_id: Option<MangaId> = preset_manga.as_ref().map(|m| m.id.clone());

    for row in rows {
        let chapter_num: f32 = match row.chapter_number.parse() {
            Ok(n) => n,
            Err(_) => continue, // skip un-parseable rows
        };
        let tankobon_num: Option<u32> = if row.tankobon_number.is_empty() {
            None
        } else {
            row.tankobon_number.parse().ok()
        };

        // Resolve or create the manga.
        let manga_meta: MangaMeta = if let Some(ref preset) = preset_manga {
            preset.clone()
        } else {
            let found = existing_mangas
                .iter()
                .find(|m| m.title.to_lowercase() == row.manga_name.to_lowercase());
            match found {
                Some(m) => m.clone(),
                None => {
                    let new_manga = MangaMeta {
                        id: MangaId(random_id()),
                        title: row.manga_name.clone(),
                        mangadex_id: None,
                    };
                    db.save_manga(&new_manga).await?;
                    new_manga
                }
            }
        };

        last_manga_id = Some(manga_meta.id.clone());

        // Count pages in this PDF.
        let page_count = js::pdf_page_count(row.pdf_bytes.clone()).await.unwrap_or(0);

        // Create and persist the chapter.
        let chapter_meta = ChapterMeta {
            id: ChapterId(random_id()),
            manga_id: manga_meta.id.clone(),
            chapter_number: chapter_num,
            tankobon_number: tankobon_num,
            filename: row.filename.clone(),
            page_count,
        };
        db.save_manga(&manga_meta).await?;
        db.save_chapter(&chapter_meta).await?;

        // Render and store each page.
        for page_num in 1..=page_count {
            let img_bytes = render_page_to_uint8array(row.pdf_bytes.clone(), page_num).await?;
            let blob = bytes_to_blob(&img_bytes, "image/jpeg")?;
            db.save_page(&chapter_meta.id, page_num - 1, blob).await?;

            done_pages += 1;
            *step.write() = ImportStep::Importing {
                done: done_pages,
                total: total_pages,
            };
        }
    }

    last_manga_id.ok_or_else(|| "No rows were imported.".to_string())
}
