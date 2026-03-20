// pmanga_bridge.js — loaded as type="module"
// Exposes window.pmanga_* globals for use via Dioxus eval.
// JSZip is expected to be loaded as a UMD script before this module runs.

import * as pdfjsLib from "https://cdnjs.cloudflare.com/ajax/libs/pdf.js/4.4.168/pdf.min.mjs";

// ---------------------------------------------------------------------------
// PDF.js worker setup
// ---------------------------------------------------------------------------

pdfjsLib.GlobalWorkerOptions.workerSrc = "https://cdnjs.cloudflare.com/ajax/libs/pdf.js/4.4.168/pdf.worker.min.mjs";

window.pmanga_init_pdf = function () {
  pdfjsLib.GlobalWorkerOptions.workerSrc = "https://cdnjs.cloudflare.com/ajax/libs/pdf.js/4.4.168/pdf.worker.min.mjs";
};

// ---------------------------------------------------------------------------
// PDF functions
// ---------------------------------------------------------------------------

/**
 * Returns the number of pages in a PDF.
 * @param {Uint8Array} pdfBytes
 * @returns {Promise<number>}
 */
window.pmanga_pdf_page_count = async function (pdfBytes) {
  const loadingTask = pdfjsLib.getDocument({ data: pdfBytes });
  const pdf = await loadingTask.promise;
  const count = pdf.numPages;
  pdf.destroy();
  return count;
};

/**
 * Renders a single PDF page to a blob URL (PNG).
 * @param {Uint8Array} pdfBytes
 * @param {number} pageNum  1-based page number
 * @param {number} scale    render scale factor (default 2.0)
 * @returns {Promise<string>} blob: URL usable as <img src>
 */
/**
 * Renders a single PDF page and returns the image bytes as a plain JS Array
 * (JSON-serialisable) so Rust can receive them as Vec<u8> and build a Blob.
 * @param {Uint8Array} pdfBytes
 * @param {number} pageNum  1-based page number
 * @param {number} scale    render scale factor (default 2.0)
 * @returns {Promise<number[]>} JPEG bytes as a plain Array
 */
window.pmanga_render_page_to_uint8array = async function (pdfBytes, pageNum, scale = 2.0) {
  const loadingTask = pdfjsLib.getDocument({ data: pdfBytes });
  const pdf = await loadingTask.promise;

  const page = await pdf.getPage(pageNum);
  const viewport = page.getViewport({ scale });

  const canvas = document.createElement("canvas");
  canvas.width = viewport.width;
  canvas.height = viewport.height;

  const ctx = canvas.getContext("2d");
  await page.render({ canvasContext: ctx, viewport }).promise;

  page.cleanup();
  pdf.destroy();

  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) => {
        if (!blob) {
          reject(new Error("canvas.toBlob returned null"));
          return;
        }
        blob
          .arrayBuffer()
          .then((buf) => {
            resolve(Array.from(new Uint8Array(buf)));
          })
          .catch(reject);
      },
      "image/jpeg",
      0.85,
    );
  });
};

window.pmanga_render_pdf_page = async function (pdfBytes, pageNum, scale = 2.0) {
  const loadingTask = pdfjsLib.getDocument({ data: pdfBytes });
  const pdf = await loadingTask.promise;

  const page = await pdf.getPage(pageNum);
  const viewport = page.getViewport({ scale });

  const canvas = document.createElement("canvas");
  canvas.width = viewport.width;
  canvas.height = viewport.height;

  const ctx = canvas.getContext("2d");
  await page.render({ canvasContext: ctx, viewport }).promise;

  page.cleanup();
  pdf.destroy();

  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (!blob) {
        reject(new Error("canvas.toBlob returned null"));
        return;
      }
      resolve(URL.createObjectURL(blob));
    }, "image/png");
  });
};

// ---------------------------------------------------------------------------
// ZIP function
// ---------------------------------------------------------------------------

/**
 * Extracts PDF files from a ZIP archive.
 * @param {Uint8Array} zipBytes
 * @returns {Promise<Array<{name: string, data: Uint8Array}>>}
 *   Sorted by name (numerically aware). Only non-directory, .pdf entries.
 */
window.pmanga_extract_zip = async function (zipBytes) {
  const JSZip = window.JSZip;
  if (!JSZip) {
    throw new Error("JSZip is not loaded");
  }

  const zip = await JSZip.loadAsync(zipBytes);

  const entries = [];
  const promises = [];

  zip.forEach((relativePath, zipEntry) => {
    if (zipEntry.dir) return;
    if (!relativePath.toLowerCase().endsWith(".pdf")) return;

    const p = zipEntry.async("uint8array").then((data) => {
      entries.push({ name: relativePath, data });
    });
    promises.push(p);
  });

  await Promise.all(promises);

  // Numerically-aware sort (natural sort).
  entries.sort((a, b) => {
    return a.name.localeCompare(b.name, undefined, {
      numeric: true,
      sensitivity: "base",
    });
  });

  return entries;
};

// ---------------------------------------------------------------------------
// MangaDex functions
// ---------------------------------------------------------------------------

const MANGADEX_BASE = "https://api.mangadex.org";

/**
 * Search manga by title on MangaDex. Returns up to 10 results.
 * @param {string} query
 * @returns {Promise<Array<{id: string, title: string}>>}
 */
window.pmanga_mangadex_search = async function (query) {
  try {
    const url = new URL(`${MANGADEX_BASE}/manga`);
    url.searchParams.set("title", query);
    url.searchParams.set("limit", "10");
    url.searchParams.set("contentRating[]", "safe");
    url.searchParams.append("contentRating[]", "suggestive");
    url.searchParams.append("contentRating[]", "erotica");
    url.searchParams.append("contentRating[]", "pornographic");

    const resp = await fetch(url.toString());
    if (!resp.ok) return [];

    const json = await resp.json();
    const data = json.data ?? [];

    return data.map((manga) => {
      const titles = manga.attributes?.title ?? {};
      // Prefer English, fall back to first available language.
      const title = titles["en"] ?? Object.values(titles)[0] ?? "(no title)";
      return { id: manga.id, title };
    });
  } catch (_err) {
    return [];
  }
};

/**
 * Get chapter-to-volume mapping for a manga (English chapters only).
 * Paginates automatically up to 2000 chapters.
 * @param {string} mangadex_id
 * @returns {Promise<Array<{chapter: string, volume: string|null}>>}
 */
window.pmanga_mangadex_chapters = async function (mangadex_id) {
  const MAX_CHAPTERS = 2000;
  const PAGE_SIZE = 100;

  const results = [];

  try {
    let offset = 0;

    while (results.length < MAX_CHAPTERS) {
      const url = new URL(`${MANGADEX_BASE}/manga/${mangadex_id}/feed`);
      url.searchParams.set("limit", String(PAGE_SIZE));
      url.searchParams.set("offset", String(offset));
      url.searchParams.set("translatedLanguage[]", "en");
      url.searchParams.set("order[chapter]", "asc");
      url.searchParams.set("contentRating[]", "safe");
      url.searchParams.append("contentRating[]", "suggestive");
      url.searchParams.append("contentRating[]", "erotica");
      url.searchParams.append("contentRating[]", "pornographic");

      const resp = await fetch(url.toString());
      if (!resp.ok) break;

      const json = await resp.json();
      const data = json.data ?? [];

      if (data.length === 0) break;

      for (const ch of data) {
        const attrs = ch.attributes ?? {};
        const chapter = attrs.chapter ?? null;
        if (chapter === null) continue; // skip unnumbered chapters
        results.push({
          chapter: String(chapter),
          volume: attrs.volume != null ? String(attrs.volume) : null,
        });
      }

      offset += data.length;

      // If we got fewer results than requested, we're at the end.
      if (data.length < PAGE_SIZE) break;
    }
  } catch (_err) {
    return [];
  }

  return results;
};
