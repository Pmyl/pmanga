// pmanga_bridge.js — loaded as a plain script (not type="module")
// Exposes window.pmanga_* globals for use via Dioxus eval.
// JSZip is expected to be loaded as a UMD script before this file runs.
// PDF.js is loaded lazily via dynamic import on first use.

// ---------------------------------------------------------------------------
// PDF.js lazy loader
// ---------------------------------------------------------------------------

const PDFJS_URL = "https://cdnjs.cloudflare.com/ajax/libs/pdf.js/4.4.168/pdf.min.mjs";
const PDFJS_WORKER_URL = "https://cdnjs.cloudflare.com/ajax/libs/pdf.js/4.4.168/pdf.worker.min.mjs";

let _pdfjsLib = null;

async function getPdfJs() {
  if (_pdfjsLib) return _pdfjsLib;
  _pdfjsLib = await import(PDFJS_URL);
  _pdfjsLib.GlobalWorkerOptions.workerSrc = PDFJS_WORKER_URL;
  return _pdfjsLib;
}

// ---------------------------------------------------------------------------
// PDF functions
// ---------------------------------------------------------------------------

/**
 * Returns the number of pages in a PDF.
 * @param {Uint8Array} pdfBytes
 * @returns {Promise<number>}
 */
window.pmanga_pdf_page_count = async function (pdfBytes) {
  const pdfjsLib = await getPdfJs();
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
  const pdfjsLib = await getPdfJs();
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
