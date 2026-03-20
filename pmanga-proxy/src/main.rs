use axum::{
    Router,
    extract::{Path, Query},
    http::StatusCode,
    response::Json,
    routing::get,
};
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
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

        // Find a span whose text matches "Chapter N"
        let mut chapter_number: Option<f32> = None;
        for span in anchor.select(&span_selector) {
            let text: String = span.text().collect::<Vec<_>>().join("");
            let trimmed = text.trim();
            if let Some(rest) = trimmed.strip_prefix("Chapter ") {
                if let Ok(n) = rest.trim().parse::<f32>() {
                    chapter_number = Some(n);
                    break;
                }
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
        .route("/api/series", get(get_series))
        .route("/api/chapters/:series_id", get(get_chapters))
        .route("/api/pages/:chapter_id", get(get_pages))
        .layer(cors);

    let addr = "0.0.0.0:7331";
    eprintln!("pmanga-proxy listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
