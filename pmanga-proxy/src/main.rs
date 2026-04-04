use axum::{
    Router,
    extract::{Path, Query},
    http::StatusCode,
    response::Json,
    routing::get,
};
use axum_server::tls_rustls::RustlsConfig;
use rcgen::generate_simple_self_signed;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::path::Path as FsPath;
use tower_http::cors::{Any, CorsLayer};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36";

#[derive(Deserialize)]
struct SeriesQuery {
    url: String,
}

#[derive(Serialize)]
struct SeriesMeta {
    title: String,
    series_id: String,
}

#[derive(Serialize)]
struct ChapterItem {
    number: f32,
    id: String,
}

#[derive(Serialize)]
struct PageItem {
    url: String,
    width: u32,
    height: u32,
}

/// Parse a chapter number from a span text such as "Chapter 1", "Act 42", or
/// "1.5".  The strategy is: split by whitespace, take the last token, and
/// attempt to parse it as an `f32`.  Returns `None` if the last token is not
/// a valid floating-point number.
fn parse_chapter_number(text: &str) -> Option<f32> {
    text.trim()
        .split_whitespace()
        .last()
        .and_then(|last| last.parse::<f32>().ok())
}

fn extract_series_id(url: &str) -> Option<String> {
    // e.g. https://weebcentral.com/series/01J76XY7E9FNDZ1DBBM6PBJPFK/one-piece
    // We want the segment immediately after "/series/"
    let path = url.split("weebcentral.com").nth(1)?;
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    // first segment should be "series"
    let first = segments.next()?;
    if first != "series" {
        return None;
    }
    let id = segments.next()?;
    Some(id.to_string())
}

async fn get_series(
    Query(params): Query<SeriesQuery>,
) -> Result<Json<SeriesMeta>, (StatusCode, String)> {
    eprintln!("[GET /api/series] url={}", params.url);

    let series_id = extract_series_id(&params.url).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "Could not extract series_id from url".to_string(),
        )
    })?;

    let client = Client::new();
    let response = client
        .get(&params.url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let html = response
        .text()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let document = Html::parse_document(&html);
    let title_selector = Selector::parse("title").unwrap();

    let raw_title = document
        .select(&title_selector)
        .next()
        .map(|el| el.inner_html())
        .unwrap_or_default();

    eprintln!("[GET /api/series] raw_title={:?}", raw_title);

    let title = raw_title
        .trim()
        .trim_end_matches(" | Weeb Central")
        .trim_end_matches(" - Weeb Central")
        .trim()
        .to_string();

    eprintln!(
        "[GET /api/series] series_id={} title={:?}",
        series_id, title
    );

    Ok(Json(SeriesMeta { title, series_id }))
}

async fn get_chapters(
    Path(series_id): Path<String>,
) -> Result<Json<Vec<ChapterItem>>, (StatusCode, String)> {
    eprintln!("[GET /api/chapters/{}]", series_id);

    let url = format!(
        "https://weebcentral.com/series/{}/full-chapter-list",
        series_id
    );

    let client = Client::new();
    let response = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let html = response
        .text()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let document = Html::parse_document(&html);
    let anchor_selector = Selector::parse("a[href*=\"/chapters/\"]").unwrap();
    let span_selector = Selector::parse("span").unwrap();

    let mut chapters: Vec<ChapterItem> = Vec::new();

    for anchor in document.select(&anchor_selector) {
        let href = match anchor.value().attr("href") {
            Some(h) => h,
            None => continue,
        };

        // chapter_id is the last non-empty path segment
        let chapter_id = match href.split('/').filter(|s| !s.is_empty()).last() {
            Some(id) => id.to_string(),
            None => continue,
        };

        // Find a span whose text ends with a number (e.g. "Chapter 1", "Act 42")
        let mut chapter_number: Option<f32> = None;
        for span in anchor.select(&span_selector) {
            let text: String = span.text().collect::<Vec<_>>().join("");
            if let Some(n) = parse_chapter_number(&text) {
                chapter_number = Some(n);
                break;
            }
        }

        let number = match chapter_number {
            Some(n) => n,
            None => continue,
        };

        chapters.push(ChapterItem {
            number,
            id: chapter_id,
        });
    }

    eprintln!(
        "[GET /api/chapters/{}] found {} chapters",
        series_id,
        chapters.len()
    );

    Ok(Json(chapters))
}

async fn get_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn get_pages(
    Path(chapter_id): Path<String>,
) -> Result<Json<Vec<PageItem>>, (StatusCode, String)> {
    eprintln!("[GET /api/pages/{}]", chapter_id);

    let url = format!(
        "https://weebcentral.com/chapters/{}/images?is_prev=False&current_page=1&reading_style=long_strip",
        chapter_id
    );

    let client = Client::new();
    let response = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let html = response
        .text()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let document = Html::parse_document(&html);
    let img_selector = Selector::parse("img[alt^=\"Page \"]").unwrap();

    let mut pages: Vec<PageItem> = Vec::new();

    for img in document.select(&img_selector) {
        let src = match img.value().attr("src") {
            Some(s) => s.to_string(),
            None => continue,
        };

        let width: u32 = img
            .value()
            .attr("width")
            .and_then(|w| w.parse().ok())
            .unwrap_or(0);

        let height: u32 = img
            .value()
            .attr("height")
            .and_then(|h| h.parse().ok())
            .unwrap_or(0);

        pages.push(PageItem {
            url: src,
            width,
            height,
        });
    }

    eprintln!(
        "[GET /api/pages/{}] found {} pages",
        chapter_id,
        pages.len()
    );

    Ok(Json(pages))
}

#[tokio::main]
async fn main() {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/health", get(get_health))
        .route("/api/series", get(get_series))
        .route("/api/chapters/:series_id", get(get_chapters))
        .route("/api/pages/:chapter_id", get(get_pages))
        .layer(cors);

    // Load or generate a self-signed certificate so the proxy can be reached
    // over HTTPS from GitHub Pages without mixed-content blocks.
    //
    // The cert is persisted to disk so it stays the same across restarts —
    // this means Firefox's one-time "Accept the Risk" exception stays valid
    // and doesn't need to be re-accepted every time the proxy is restarted.
    let cert_path = FsPath::new("cert.pem");
    let key_path = FsPath::new("key.pem");

    let (cert_pem, key_pem) = if cert_path.exists() && key_path.exists() {
        eprintln!("pmanga-proxy: loading existing TLS cert from cert.pem / key.pem");
        let cert = std::fs::read_to_string(cert_path).expect("failed to read cert.pem");
        let key = std::fs::read_to_string(key_path).expect("failed to read key.pem");
        (cert, key)
    } else {
        eprintln!(
            "pmanga-proxy: generating new self-signed TLS cert (saved to cert.pem / key.pem)"
        );
        let subject_alt_names = vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "192.168.1.79".to_string(),
        ];
        let cert = generate_simple_self_signed(subject_alt_names)
            .expect("failed to generate self-signed certificate");
        let cert_pem = cert.cert.pem();
        let key_pem = cert.key_pair.serialize_pem();
        std::fs::write(cert_path, &cert_pem).expect("failed to write cert.pem");
        std::fs::write(key_path, &key_pem).expect("failed to write key.pem");
        (cert_pem, key_pem)
    };

    let tls_config = RustlsConfig::from_pem(cert_pem.into_bytes(), key_pem.into_bytes())
        .await
        .expect("failed to build TLS config");

    let addr = "0.0.0.0:7331".parse().unwrap();
    eprintln!("pmanga-proxy listening on https://{}", addr);
    eprintln!("First time? Visit https://192.168.1.79:7331 in Firefox and accept the certificate.");
    eprintln!("The cert is saved to disk — you only need to do this once.");

    axum_server::bind_rustls(addr, tls_config)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chapter_number_standard_prefix() {
        assert_eq!(parse_chapter_number("Chapter 1"), Some(1.0));
        assert_eq!(parse_chapter_number("Chapter 42"), Some(42.0));
        assert_eq!(parse_chapter_number("Chapter 1.5"), Some(1.5));
    }

    #[test]
    fn parse_chapter_number_non_standard_prefix() {
        assert_eq!(parse_chapter_number("Act 1"), Some(1.0));
        assert_eq!(parse_chapter_number("Episode 12"), Some(12.0));
        assert_eq!(parse_chapter_number("Part 3.5"), Some(3.5));
    }

    #[test]
    fn parse_chapter_number_bare_number() {
        assert_eq!(parse_chapter_number("7"), Some(7.0));
        assert_eq!(parse_chapter_number("  100  "), Some(100.0));
    }

    #[test]
    fn parse_chapter_number_no_number() {
        assert_eq!(parse_chapter_number("Chapter"), None);
        assert_eq!(parse_chapter_number(""), None);
        assert_eq!(parse_chapter_number("   "), None);
        assert_eq!(parse_chapter_number("Act One"), None);
    }
}
