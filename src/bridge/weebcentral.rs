//! WeebCentral proxy client.
//!
//! Thin async wrappers around the three pmanga-proxy endpoints.  All network
//! calls go through a shared `reqwest::Client` (WASM-compatible) exactly like
//! the MangaDex helpers in `bridge/js.rs`.

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Shared client
// ---------------------------------------------------------------------------

static PROXY_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

fn proxy_client() -> &'static reqwest::Client {
    PROXY_CLIENT.get_or_init(reqwest::Client::new)
}

// ---------------------------------------------------------------------------
// Response types (mirror the proxy's JSON shapes)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WcSeriesMeta {
    pub title: String,
    pub series_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WcChapter {
    pub number: f32,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WcPage {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Perform a GET request against the proxy and deserialize the JSON body.
async fn proxy_get<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
    let resp = proxy_client()
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("proxy request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("proxy returned HTTP {}", resp.status()));
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("proxy JSON deserialize failed: {e}"))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fetch the series title and canonical series_id from the proxy.
///
/// `proxy_url`  — e.g. `"https://192.168.1.10:7331"`
/// `series_url` — full WeebCentral URL, e.g.
///                `"https://weebcentral.com/series/01J76XY.../one-piece"`
pub async fn fetch_series_meta(proxy_url: &str, series_url: &str) -> Result<WcSeriesMeta, String> {
    let encoded = urlencoding::encode(series_url);
    let url = format!("{proxy_url}/api/series?url={encoded}");
    proxy_get::<WcSeriesMeta>(&url).await
}

/// Fetch the full chapter list for a series from the proxy.
///
/// Returns chapters in the order the proxy provides them (typically
/// descending from the site); callers are responsible for sorting.
pub async fn fetch_chapter_list(
    proxy_url: &str,
    series_id: &str,
) -> Result<Vec<WcChapter>, String> {
    let url = format!("{proxy_url}/api/chapters/{series_id}");
    proxy_get::<Vec<WcChapter>>(&url).await
}

/// Fetch the ordered list of page image URLs for a single chapter.
pub async fn fetch_chapter_pages(proxy_url: &str, chapter_id: &str) -> Result<Vec<WcPage>, String> {
    let url = format!("{proxy_url}/api/pages/{chapter_id}");
    proxy_get::<Vec<WcPage>>(&url).await
}
