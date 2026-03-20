//! Tankobon volume lookup from the bundled CSV database.
//!
//! The CSV (`/tankobon_db.csv`) has three columns:
//!   manga_title, chapter_number, tankobon_number
//!
//! Both the PDF importer and the WeebCentral importer use this module so the
//! mapping logic lives in exactly one place.

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

use crate::assets::TANKOBON_DB;

#[derive(Debug, Clone, PartialEq)]
pub struct TankobonRow {
    pub manga_title: String,
    pub chapter: String,
    pub tankobon: u32,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse the raw CSV text into a `Vec<TankobonRow>`.
///
/// Lines that are empty, start with `#`, or have fewer than three
/// comma-separated fields are silently skipped.
pub fn parse_tankobon_csv(csv: &str) -> Vec<TankobonRow> {
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

// ---------------------------------------------------------------------------
// Lookup
// ---------------------------------------------------------------------------

/// Return the tankobon volume number for a given manga title + chapter number,
/// or `None` if no match is found.
///
/// Matching is case-insensitive on the title and uses a tolerance of ±0.01 on
/// the chapter number to handle floating-point representation noise.
pub fn lookup_tankobon(manga_name: &str, chapter_number: f32, rows: &[TankobonRow]) -> Option<u32> {
    let manga_lower = manga_name.to_lowercase();

    // Count how many rows exist for this title so we can tell apart
    // "title not in CSV at all" from "title found but chapter not matched".
    let title_matches: Vec<&TankobonRow> = rows
        .iter()
        .filter(|r| r.manga_title.to_lowercase() == manga_lower)
        .collect();

    if title_matches.is_empty() {
        return None;
    }

    for row in &title_matches {
        let row_ch: f32 = match row.chapter.parse() {
            Ok(n) => n,
            Err(_) => {
                continue;
            }
        };
        if (row_ch - chapter_number).abs() < 0.01 {
            return Some(row.tankobon);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Fetching (browser only)
// ---------------------------------------------------------------------------

/// Fetch `/tankobon_db.csv` from the app's own origin and parse it.
///
/// Returns an empty `Vec` on any error so callers degrade gracefully.
pub async fn fetch_tankobon_csv() -> Vec<TankobonRow> {
    use js_sys::Promise;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let window = match web_sys::window() {
        Some(w) => w,
        None => return vec![],
    };

    let promise: Promise = match window.fetch_with_str(&TANKOBON_DB.to_string()).dyn_into() {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let resp_val = match JsFuture::from(promise).await {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    let response: web_sys::Response = match resp_val.dyn_into() {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    if !response.ok() {
        return vec![];
    }

    let text_promise: Promise = match response.text() {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let text_val = match JsFuture::from(text_promise).await {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    match text_val.as_string() {
        Some(text) => {
            let rows = parse_tankobon_csv(&text);
            rows
        }
        None => {
            vec![]
        }
    }
}
