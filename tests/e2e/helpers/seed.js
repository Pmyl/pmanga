// @ts-check
/**
 * IndexedDB + localStorage seeding helpers for PManga e2e tests.
 *
 * The PManga WASM app stores all state in:
 *   - IndexedDB "pmanga" v1  (manga metadata, chapters, page blobs, progress)
 *   - localStorage           (last-opened position, reader config)
 *
 * Seeding strategy
 * ----------------
 * 1. `page.goto('/')` — puts us on the correct origin (localhost:8080) so we
 *    can reach the same IndexedDB that the WASM app will use.
 * 2. `page.evaluate(seedDb)` — opens (or creates) the "pmanga" IDB, writes
 *    test fixtures, and resolves the returned Promise once all writes have
 *    committed.  Playwright awaits the Promise before continuing.
 * 3. `page.goto(targetUrl)` — hard-navigates so the WASM app starts fresh and
 *    reads the pre-seeded data on mount.
 *
 * The seed script itself handles the case where the schema already exists
 * (Dioxus opened the DB first) as well as the case where it does not yet
 * exist (we create it via `onupgradeneeded`).
 */

// ---------------------------------------------------------------------------
// Test fixture data
// ---------------------------------------------------------------------------

/**
 * A single manga series used by all tests.
 * Serialised to match the serde_json output of `MangaMeta` exactly:
 *   - `id`    → newtype MangaId(String) → serialised as plain string
 *   - `source`→ enum variant "Local"    → serialised as string "Local"
 */
const MANGA = {
  id: 'm1',
  title: 'Test Manga',
  mangadex_id: null,
  source: 'Local',
  latest_downloaded_chapter: null,
  cover_url_fallback: null,
};

/**
 * A WeebCentral manga series used by sync-related tests.
 * `source` is the serde_json representation of `MangaSource::WeebCentral { series_url }`.
 * `latest_downloaded_chapter` is set so the manga is considered "caught up"
 * when there are no chapters in the DB (total_pages == 0 && last_downloaded_chapter.is_some()).
 */
const WC_MANGA = {
  id: 'wc1',
  title: 'WC Manga',
  mangadex_id: null,
  source: { WeebCentral: { series_url: 'https://weebcentral.com/series/TEST_SERIES_ID/wc-manga' } },
  latest_downloaded_chapter: 10.0,
  cover_url_fallback: null,
};

/**
 * A WeebCentral chapter that belongs to wc1.
 * `source` is the serde_json representation of `ChapterSource::WeebCentral { chapter_id }`.
 * `page_urls` is non-empty to identify this as a WeebCentral chapter.
 */
const WC_CH1 = {
  id: 'wc-ch1',
  manga_id: 'wc1',
  chapter_number: 10.0,
  tankobon_number: null,
  filename: 'Chapter 10',
  page_count: 2,
  source: { WeebCentral: { chapter_id: 'WC_CH1_ID' } },
  page_urls: ['https://weebcentral.com/page1.jpg', 'https://weebcentral.com/page2.jpg'],
};

/**
 * Chapter 1: belongs to Vol. 1 (tankobon_number = 1), 3 pages.
 * Serialised to match serde_json output of `ChapterMeta`.
 * Note: the Rust DB layer patches `manga_id` to be a plain string (not a
 * newtype wrapper object), but serde already serialises MangaId as a plain
 * string, so no special handling is needed here.
 */
const CH1 = {
  id: 'ch1',
  manga_id: 'm1',
  chapter_number: 1.0,
  tankobon_number: 1,
  filename: 'chapter1.cbz',
  page_count: 3,
  source: 'Local',
  page_urls: [],
};

/** Chapter 2: belongs to Vol. 1 (tankobon_number = 1), 2 pages. */
const CH2 = {
  id: 'ch2',
  manga_id: 'm1',
  chapter_number: 2.0,
  tankobon_number: 1,
  filename: 'chapter2.cbz',
  page_count: 2,
  source: 'Local',
  page_urls: [],
};

/** Chapter 3: a lone chapter (tankobon_number = null), 2 pages. */
const CH3_LONE = {
  id: 'ch3',
  manga_id: 'm1',
  chapter_number: 5.0,
  tankobon_number: null,
  filename: 'chapter5.cbz',
  page_count: 2,
  source: 'Local',
  page_urls: [],
};

// ---------------------------------------------------------------------------
// Tiny valid 1×1 pixel PNG (base-64 encoded).
// Used as placeholder page images so the reader can create blob URLs and
// render <img> elements without needing real manga pages.
// ---------------------------------------------------------------------------
const TINY_PNG_B64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhf' +
  'DwAChwGA60e6kgAAAABJRU5ErkJggg==';

// ---------------------------------------------------------------------------
// Core seeding function (runs inside the browser via page.evaluate)
// ---------------------------------------------------------------------------

/**
 * Opens (or creates) the "pmanga" IndexedDB and writes the provided fixtures.
 * Returns a Promise that resolves once all writes are committed.
 *
 * @param {{ manga: object, chapters: object[], pageCountMap: Record<string,number> }} fixtures
 */
async function _seedDbInBrowser({ manga, chapters, pageCountMap, progress }) {
  // Decode the tiny PNG once and reuse it for every page.
  const b64 =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhf' +
    'DwAChwGA60e6kgAAAABJRU5ErkJggg==';
  const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
  const pageBlob = new Blob([bytes], { type: 'image/png' });

  await new Promise((resolve, reject) => {
    const openReq = indexedDB.open('pmanga', 1);

    openReq.onupgradeneeded = (event) => {
      const db = event.target.result;
      // Mirror the schema created by Rust's Db::open().
      db.createObjectStore('mangas');
      const chStore = db.createObjectStore('chapters');
      chStore.createIndex('by_manga', 'manga_id', { unique: false });
      db.createObjectStore('pages');
      db.createObjectStore('progress');
    };

    openReq.onerror = () => reject(openReq.error);

    openReq.onsuccess = () => {
      const db = openReq.result;

      // All writes go through a single read-write transaction across all four
      // stores so they either all commit or all abort.
      const stores = ['mangas', 'chapters', 'pages', 'progress'];
      const tx = db.transaction(stores, 'readwrite');
      tx.oncomplete = resolve;
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(new Error('IDB transaction aborted'));

      const mangas = tx.objectStore('mangas');
      const chapterStore = tx.objectStore('chapters');
      const pagesStore = tx.objectStore('pages');
      const progressStore = tx.objectStore('progress');

      // Write manga metadata.
      mangas.put(manga, manga.id);

      // Write chapters + their page blobs.
      for (const ch of chapters) {
        chapterStore.put(ch, ch.id);

        const count = pageCountMap[ch.id] ?? ch.page_count;
        for (let i = 0; i < count; i++) {
          // Key: [chapter_id, page_number] — matches Rust's page_key().
          pagesStore.put(pageBlob, [ch.id, i]);
        }
      }

      // Write optional reading-progress records.
      if (progress) {
        for (const p of progress) {
          progressStore.put(p, p.chapter_id);
        }
      }
    };
  });
}

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

/**
 * Seed the browser's IndexedDB with the standard test fixtures.
 *
 * Call this AFTER `page.goto('/')` (so the origin is correct) and BEFORE
 * navigating to the URL under test.
 *
 * @param {import('@playwright/test').Page} page
 * @param {{ chapters?: object[], progress?: object[] }} [opts]
 *   - `chapters` defaults to [CH1, CH2, CH3_LONE]
 *   - `progress` optional reading-progress records to seed
 */
async function seedDb(page, { chapters = [CH1, CH2, CH3_LONE], progress } = {}) {
  // Build a pageCountMap so the in-browser function knows how many page blobs
  // to create per chapter without depending on the default `page_count`.
  /** @type {Record<string, number>} */
  const pageCountMap = {};
  for (const ch of chapters) {
    pageCountMap[ch.id] = ch.page_count;
  }

  await page.evaluate(_seedDbInBrowser, {
    manga: MANGA,
    chapters,
    pageCountMap,
    progress: progress ?? null,
  });
}

/**
 * Set `pmanga_reader_config` in localStorage to enable vertical-scroll mode.
 * Call this BEFORE the final `page.goto()` to the URL under test.
 *
 * @param {import('@playwright/test').Page} page
 */
async function enableScrollMode(page) {
  await page.evaluate(() => {
    localStorage.setItem(
      'pmanga_reader_config',
      JSON.stringify({ rtl_taps: false, vertical_scroll: true }),
    );
  });
}

/**
 * Set `pmanga_last_opened` in localStorage to simulate a returning user.
 *
 * @param {import('@playwright/test').Page} page
 * @param {object} lastOpened  serde-serialised LastOpened value
 *   e.g. `{ Reader: { manga_id: 'm1', chapter_id: 'ch1', page: 0 } }`
 *   e.g. `{ Library: { manga_id: 'm1' } }`
 *   e.g. `'Shelf'`
 */
async function setLastOpened(page, lastOpened) {
  await page.evaluate((value) => {
    localStorage.setItem('pmanga_last_opened', JSON.stringify(value));
  }, lastOpened);
}

/**
 * Set `pmanga_proxy_url` in localStorage so the WASM app uses a test-controlled
 * proxy address that can be intercepted or aborted by `page.route()`.
 *
 * @param {import('@playwright/test').Page} page
 * @param {string} url  e.g. `'http://127.0.0.1:7332'`
 */
async function setProxyUrl(page, url) {
  await page.evaluate((value) => {
    localStorage.setItem('pmanga_proxy_url', value);
  }, url);
}

/**
 * Seed the browser's IndexedDB with a WeebCentral manga fixture.
 *
 * The seeded manga (`WC_MANGA`) has `latest_downloaded_chapter` set, so even
 * when no chapters are provided the manga is treated as "caught up" by the
 * shelf's sync-all logic (total_pages == 0 && last_downloaded_chapter.is_some()).
 *
 * Call this AFTER `page.goto('/')` and BEFORE navigating to the URL under test.
 *
 * @param {import('@playwright/test').Page} page
 * @param {{ chapters?: object[], progress?: object[] }} [opts]
 *   - `chapters` defaults to [] (no local chapters → "caught up" from the start)
 *   - `progress` optional reading-progress records to seed
 */
async function seedWcDb(page, { chapters = [], progress } = {}) {
  /** @type {Record<string, number>} */
  const pageCountMap = {};
  for (const ch of chapters) {
    pageCountMap[ch.id] = ch.page_count;
  }

  await page.evaluate(_seedDbInBrowser, {
    manga: WC_MANGA,
    chapters,
    pageCountMap,
    progress: progress ?? null,
  });
}

module.exports = {
  MANGA,
  WC_MANGA,
  WC_CH1,
  CH1,
  CH2,
  CH3_LONE,
  seedDb,
  seedWcDb,
  enableScrollMode,
  setLastOpened,
  setProxyUrl,
};
