//! JavaScript interop bridge.
//!
//! Only contains things that genuinely require JS APIs unavailable in Rust/WASM:
//! - PDF rendering (PDF.js + Canvas)
//! - ZIP extraction (JSZip)
//!
//! Everything else (HTTP calls, JSON parsing) is done in pure Rust.

use dioxus::document::eval;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChapterVolumeEntry {
    pub chapter: String,
    pub volume: Option<String>,
}

// ---------------------------------------------------------------------------
// PDF (requires PDF.js + Canvas — must stay in JS)
// ---------------------------------------------------------------------------

/// Returns the number of pages in a PDF.
pub async fn pdf_page_count(pdf_bytes: Vec<u8>) -> Result<u32, String> {
    let mut ev = eval(
        r#"
        const bytes = new Uint8Array(await dioxus.recv());
        const count = await window.pmanga_pdf_page_count(bytes);
        dioxus.send(count);
        "#,
    );
    ev.send(serde_json::json!(pdf_bytes))
        .map_err(|e| format!("eval send error: {e:?}"))?;
    ev.recv::<u32>()
        .await
        .map_err(|e| format!("eval recv error: {e:?}"))
}

/// Renders one PDF page and returns raw JPEG bytes.
/// `page_num` is 1-based.
pub async fn render_page_to_uint8array(
    pdf_bytes: Vec<u8>,
    page_num: u32,
) -> Result<Vec<u8>, String> {
    let mut ev = eval(
        r#"
        const [bytes, pageNum] = await dioxus.recv();
        const arr = await window.pmanga_render_page_to_uint8array(new Uint8Array(bytes), pageNum);
        dioxus.send(arr);
        "#,
    );
    ev.send(serde_json::json!([pdf_bytes, page_num]))
        .map_err(|e| format!("eval send error: {e:?}"))?;
    ev.recv::<Vec<u8>>()
        .await
        .map_err(|e| format!("eval recv error: {e:?}"))
}

// ---------------------------------------------------------------------------
// ZIP (requires JSZip — must stay in JS)
// ---------------------------------------------------------------------------

/// Extracts PDF files from a ZIP archive.
/// Returns `(filename, pdf_bytes)` pairs sorted by name.
pub async fn extract_zip(zip_bytes: Vec<u8>) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut ev = eval(
        r#"
        const bytes = new Uint8Array(await dioxus.recv());
        const entries = await window.pmanga_extract_zip(bytes);
        const result = entries.map(e => [e.name, Array.from(e.data)]);
        dioxus.send(result);
        "#,
    );
    ev.send(serde_json::json!(zip_bytes))
        .map_err(|e| format!("eval send error: {e:?}"))?;
    ev.recv::<Vec<(String, Vec<u8>)>>()
        .await
        .map_err(|e| format!("eval recv error: {e:?}"))
}

// ---------------------------------------------------------------------------
// Blob helpers (pure web_sys — no JS eval needed)
// ---------------------------------------------------------------------------

/// Convert raw image bytes into a `web_sys::Blob` with the given MIME type.
pub fn bytes_to_blob(bytes: &[u8], mime: &str) -> Result<web_sys::Blob, String> {
    use js_sys::{Array, Uint8Array};
    let uint8 = Uint8Array::from(bytes);
    let arr = Array::new();
    arr.push(&uint8);
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    web_sys::Blob::new_with_u8_array_sequence_and_options(&arr, &opts)
        .map_err(|e| format!("Blob construction failed: {e:?}"))
}

// ---------------------------------------------------------------------------
// MangaDex API (pure Rust web_sys fetch — no JS eval needed)
// ---------------------------------------------------------------------------

const MANGADEX_BASE: &str = "https://api.mangadex.org";

/// All content ratings we want to include in MangaDex queries.
const CONTENT_RATINGS: &[&str] = &["safe", "suggestive", "erotica", "pornographic"];

/// Perform a GET request and deserialize the JSON body.
/// Returns `Err` on network failure or non-2xx status.
async fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response_val = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("fetch failed: {e:?}"))?;
    let response: web_sys::Response = response_val
        .dyn_into()
        .map_err(|_| "fetch result was not a Response".to_string())?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    let text_val = JsFuture::from(
        response
            .text()
            .map_err(|e| format!("response.text() failed: {e:?}"))?,
    )
    .await
    .map_err(|e| format!("text promise failed: {e:?}"))?;

    let text = text_val
        .as_string()
        .ok_or("response body was not a string")?;
    serde_json::from_str(&text).map_err(|e| format!("JSON deserialize failed: {e}"))
}

/// Build a MangaDex query URL, appending all content rating params.
fn mangadex_url(path: &str, params: &[(&str, &str)]) -> String {
    let mut parts: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, js_encode(v)))
        .collect();
    for rating in CONTENT_RATINGS {
        parts.push(format!("contentRating%5B%5D={}", rating));
    }
    format!("{MANGADEX_BASE}{path}?{}", parts.join("&"))
}

/// Percent-encode a string using JS `encodeURIComponent`.
fn js_encode(s: &str) -> String {
    js_sys::encode_uri_component(s)
        .as_string()
        .unwrap_or_else(|| s.to_string())
}

/// Search manga by title on MangaDex. Returns up to 10 `(id, title)` pairs.
/// Silently returns an empty vec on any error so import flow degrades gracefully.
pub async fn mangadex_search(query: &str) -> Result<Vec<(String, String)>, String> {
    let url = mangadex_url("/manga", &[("title", query), ("limit", "10")]);
    let json = match fetch_json(&url).await {
        Ok(j) => j,
        Err(_) => return Ok(vec![]),
    };

    let results = json["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|manga| {
                    let id = manga["id"].as_str()?.to_string();
                    let titles = manga["attributes"]["title"].as_object()?;
                    // Prefer English, fall back to first available language.
                    let title = titles
                        .get("en")
                        .or_else(|| titles.values().next())
                        .and_then(|v| v.as_str())
                        .unwrap_or("(no title)")
                        .to_string();
                    Some((id, title))
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(results)
}

/// Get chapter→volume mapping for a manga from MangaDex (English only).
/// Paginates automatically. Silently returns empty vec on any error.
pub async fn mangadex_chapters(mangadex_id: &str) -> Result<Vec<ChapterVolumeEntry>, String> {
    const PAGE_SIZE: usize = 500;
    const MAX_CHAPTERS: usize = 2000;

    let mut results: Vec<ChapterVolumeEntry> = Vec::new();
    let mut offset = 0usize;

    loop {
        let url = mangadex_url(
            &format!("/manga/{mangadex_id}/feed"),
            &[
                ("limit", &PAGE_SIZE.to_string()),
                ("offset", &offset.to_string()),
                ("translatedLanguage%5B%5D", "en"),
                ("order%5Bchapter%5D", "asc"),
            ],
        );

        let json = match fetch_json(&url).await {
            Ok(j) => j,
            Err(_) => break,
        };

        let data = match json["data"].as_array() {
            Some(d) if !d.is_empty() => d,
            _ => break,
        };

        for ch in data {
            let attrs = &ch["attributes"];
            let chapter = match attrs["chapter"].as_str() {
                Some(c) => c.to_string(),
                None => continue, // skip unnumbered chapters
            };
            let volume = attrs["volume"].as_str().map(|s| s.to_string());
            results.push(ChapterVolumeEntry { chapter, volume });
        }

        let fetched = data.len();
        offset += fetched;

        if fetched < PAGE_SIZE || results.len() >= MAX_CHAPTERS {
            break;
        }
    }

    Ok(results)
}
