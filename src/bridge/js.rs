//! JavaScript interop bridge.
//! Wraps PDF.js, JSZip, and MangaDex calls via Dioxus eval.

use dioxus::document::eval;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChapterVolumeEntry {
    pub chapter: String,
    pub volume: Option<String>,
}

/// Returns the number of pages in a PDF.
/// `pdf_bytes` is the raw PDF data.
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

    let count: u32 = ev
        .recv()
        .await
        .map_err(|e| format!("eval recv error: {e:?}"))?;

    Ok(count)
}

/// Extracts PDF files from a ZIP archive.
/// Returns `(filename, pdf_bytes)` pairs sorted by name.
pub async fn extract_zip(zip_bytes: Vec<u8>) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut ev = eval(
        r#"
        const bytes = new Uint8Array(await dioxus.recv());
        const entries = await window.pmanga_extract_zip(bytes);
        // Serialize each entry as [name, Array.from(data)] so it's JSON-safe.
        const result = entries.map(e => [e.name, Array.from(e.data)]);
        dioxus.send(result);
        "#,
    );

    ev.send(serde_json::json!(zip_bytes))
        .map_err(|e| format!("eval send error: {e:?}"))?;

    let raw: Vec<(String, Vec<u8>)> = ev
        .recv()
        .await
        .map_err(|e| format!("eval recv error: {e:?}"))?;

    Ok(raw)
}

/// Search MangaDex by title. Returns `(mangadex_id, display_title)` pairs.
pub async fn mangadex_search(query: &str) -> Result<Vec<(String, String)>, String> {
    let mut ev = eval(
        r#"
        const query = await dioxus.recv();
        const results = await window.pmanga_mangadex_search(query);
        dioxus.send(results.map(r => [r.id, r.title]));
        "#,
    );

    ev.send(serde_json::json!(query))
        .map_err(|e| format!("eval send error: {e:?}"))?;

    let pairs: Vec<(String, String)> = ev
        .recv()
        .await
        .map_err(|e| format!("eval recv error: {e:?}"))?;

    Ok(pairs)
}

/// Renders one page of a PDF and returns the raw image bytes (JPEG) as a
/// `Vec<u8>`.  Callers can wrap this in a `web_sys::Blob` for storage.
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

    let bytes: Vec<u8> = ev
        .recv()
        .await
        .map_err(|e| format!("eval recv error: {e:?}"))?;

    Ok(bytes)
}

/// Convert a `Vec<u8>` of image data into a `web_sys::Blob` with the given
/// MIME type (e.g. `"image/jpeg"`).
pub fn bytes_to_blob(bytes: &[u8], mime: &str) -> Result<web_sys::Blob, String> {
    use js_sys::{Array, Uint8Array};

    let uint8 = Uint8Array::from(bytes);
    let arr = Array::new();
    arr.push(&uint8);

    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);

    web_sys::Blob::new_with_u8_array_sequence_and_options(&arr, &opts)
        .map_err(|e| format!("Blob construction failed: {:?}", e))
}

/// Get chapter→volume mapping from MangaDex (English chapters only).
pub async fn mangadex_chapters(mangadex_id: &str) -> Result<Vec<ChapterVolumeEntry>, String> {
    let mut ev = eval(
        r#"
        const id = await dioxus.recv();
        const entries = await window.pmanga_mangadex_chapters(id);
        dioxus.send(entries);
        "#,
    );

    ev.send(serde_json::json!(mangadex_id))
        .map_err(|e| format!("eval send error: {e:?}"))?;

    let entries: Vec<ChapterVolumeEntry> = ev
        .recv()
        .await
        .map_err(|e| format!("eval recv error: {e:?}"))?;

    Ok(entries)
}
