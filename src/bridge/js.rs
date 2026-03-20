//! JavaScript interop bridge.
//!
//! Only contains things that genuinely require JS APIs unavailable in Rust/WASM:
//! - PDF rendering (PDF.js + Canvas)
//! - ZIP extraction (JSZip)
//!
//! Everything else (HTTP calls, JSON parsing) is done in pure Rust.
//!
//! PDF and ZIP calls go directly through js_sys (calling window.pmanga_* globals)
//! rather than dioxus::document::eval. eval() requires the Dioxus runtime to be
//! active on the current task — it panics when called from spawn_local during an
//! import loop because the runtime RefCell is already borrowed by the render cycle.
//! Direct js_sys calls have zero dependency on the Dioxus runtime.

use js_sys::{Array, Promise, Uint8Array};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

// reqwest is used for all MangaDex HTTP calls — it handles CORS correctly on WASM
// by compiling to the browser's fetch API with the right mode automatically.
static MANGADEX_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn mangadex_client() -> &'static reqwest::Client {
    MANGADEX_CLIENT.get_or_init(reqwest::Client::new)
}

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

/// Get a named function from `window`, returning a clear error if absent.
fn window_fn(name: &str) -> Result<js_sys::Function, String> {
    let window = web_sys::window().ok_or("no window")?;
    let val = js_sys::Reflect::get(&window, &wasm_bindgen::JsValue::from_str(name))
        .map_err(|_| format!("window.{name} not found"))?;
    val.dyn_into::<js_sys::Function>()
        .map_err(|_| format!("window.{name} is not a function"))
}

/// Returns the number of pages in a PDF.
/// Calls window.pmanga_pdf_page_count(Uint8Array) directly — no eval/runtime needed.
pub async fn pdf_page_count(pdf_bytes: Vec<u8>) -> Result<u32, String> {
    let func = window_fn("pmanga_pdf_page_count")?;
    let uint8 = Uint8Array::from(pdf_bytes.as_slice());
    let promise: Promise = func
        .call1(&wasm_bindgen::JsValue::NULL, &uint8)
        .map_err(|e| format!("pmanga_pdf_page_count call failed: {e:?}"))?
        .dyn_into()
        .map_err(|_| "pmanga_pdf_page_count did not return a Promise".to_string())?;
    let result = JsFuture::from(promise)
        .await
        .map_err(|e| format!("pmanga_pdf_page_count rejected: {e:?}"))?;
    result
        .as_f64()
        .map(|n| n as u32)
        .ok_or_else(|| "pmanga_pdf_page_count returned non-number".to_string())
}

/// Renders one PDF page and returns raw JPEG bytes.
/// `page_num` is 1-based.
/// Calls window.pmanga_render_page_to_uint8array(Uint8Array, number) directly.
pub async fn render_page_to_uint8array(
    pdf_bytes: Vec<u8>,
    page_num: u32,
) -> Result<Vec<u8>, String> {
    let func = window_fn("pmanga_render_page_to_uint8array")?;
    let uint8 = Uint8Array::from(pdf_bytes.as_slice());
    let page_js = wasm_bindgen::JsValue::from_f64(page_num as f64);
    let promise: Promise = func
        .call2(&wasm_bindgen::JsValue::NULL, &uint8, &page_js)
        .map_err(|e| format!("pmanga_render_page_to_uint8array call failed: {e:?}"))?
        .dyn_into()
        .map_err(|_| "pmanga_render_page_to_uint8array did not return a Promise".to_string())?;
    let result = JsFuture::from(promise)
        .await
        .map_err(|e| format!("pmanga_render_page_to_uint8array rejected: {e:?}"))?;
    // The JS function returns a plain JS Array of numbers.
    let arr: Array = result
        .dyn_into()
        .map_err(|_| "pmanga_render_page_to_uint8array result is not an Array".to_string())?;
    Ok((0..arr.length())
        .map(|i| arr.get(i).as_f64().unwrap_or(0.0) as u8)
        .collect())
}

// ---------------------------------------------------------------------------
// ZIP (requires JSZip — must stay in JS)
// ---------------------------------------------------------------------------

/// Extracts PDF files from a ZIP archive.
/// Returns `(filename, pdf_bytes)` pairs sorted by name.
/// Calls window.pmanga_extract_zip(Uint8Array) directly — no eval/runtime needed.
pub async fn extract_zip(zip_bytes: Vec<u8>) -> Result<Vec<(String, Vec<u8>)>, String> {
    let func = window_fn("pmanga_extract_zip")?;
    let uint8 = Uint8Array::from(zip_bytes.as_slice());
    let promise: Promise = func
        .call1(&wasm_bindgen::JsValue::NULL, &uint8)
        .map_err(|e| format!("pmanga_extract_zip call failed: {e:?}"))?
        .dyn_into()
        .map_err(|_| "pmanga_extract_zip did not return a Promise".to_string())?;
    let result = JsFuture::from(promise)
        .await
        .map_err(|e| format!("pmanga_extract_zip rejected: {e:?}"))?;
    // Result is Array<[name: string, data: Array<number>]>
    // The JS bridge returns entries as {name, data} objects.
    // We read name via Reflect and data as a Uint8Array.
    let entries: Array = result
        .dyn_into()
        .map_err(|_| "pmanga_extract_zip result is not an Array".to_string())?;
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..entries.length() {
        let entry = entries.get(i);
        let name_val = js_sys::Reflect::get(&entry, &wasm_bindgen::JsValue::from_str("name"))
            .map_err(|_| "entry missing 'name'".to_string())?;
        let name = name_val
            .as_string()
            .ok_or_else(|| "entry 'name' is not a string".to_string())?;
        let data_val = js_sys::Reflect::get(&entry, &wasm_bindgen::JsValue::from_str("data"))
            .map_err(|_| "entry missing 'data'".to_string())?;
        let data_arr: Uint8Array = data_val
            .dyn_into()
            .map_err(|_| "entry 'data' is not a Uint8Array".to_string())?;
        out.push((name, data_arr.to_vec()));
    }
    Ok(out)
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
// MangaDex API (reqwest — handles CORS on WASM automatically)
// ---------------------------------------------------------------------------

const MANGADEX_BASE: &str = "https://api.mangadex.org";

/// All content ratings we want to include in MangaDex queries.
const CONTENT_RATINGS: &[&str] = &["safe", "suggestive", "erotica", "pornographic"];

/// Perform a GET request and deserialize the JSON body via reqwest.
/// Returns `Err` on network failure or non-2xx status.
async fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    let response = mangadex_client()
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("JSON deserialize failed: {e}"))
}

/// Build a MangaDex query URL, appending all content rating params.
fn mangadex_url(path: &str, params: &[(&str, &str)]) -> String {
    let mut query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect::<Vec<_>>();
    for rating in CONTENT_RATINGS {
        query.push(format!("contentRating%5B%5D={}", rating));
    }
    format!("{MANGADEX_BASE}{path}?{}", query.join("&"))
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

/// Get chapter→volume mapping for a manga using MangaDex's aggregate endpoint.
/// A single request returns the full mapping across all languages and scanlations,
/// which is far more complete than the English-only feed (most manga have very few
/// English translations on MangaDex, so the feed would return almost nothing).
/// Volume "none" means MangaDex hasn't assigned those chapters to a tankobon yet.
/// Silently returns empty vec on any error.
pub async fn mangadex_chapters(mangadex_id: &str) -> Result<Vec<ChapterVolumeEntry>, String> {
    let url = format!("{MANGADEX_BASE}/manga/{mangadex_id}/aggregate");

    let json = match fetch_json(&url).await {
        Ok(j) => j,
        Err(_) => return Ok(vec![]),
    };

    let mut results: Vec<ChapterVolumeEntry> = Vec::new();

    let Some(volumes) = json["volumes"].as_object() else {
        return Ok(results);
    };

    for (vol_key, vol_data) in volumes {
        // "none" means MangaDex hasn't assigned these chapters to a tankobon yet.
        let volume = if vol_key == "none" {
            None
        } else {
            Some(vol_key.clone())
        };

        let Some(chapters) = vol_data["chapters"].as_object() else {
            continue;
        };

        for (ch_key, _) in chapters {
            results.push(ChapterVolumeEntry {
                chapter: ch_key.clone(),
                volume: volume.clone(),
            });
        }
    }

    Ok(results)
}
