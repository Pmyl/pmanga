// @ts-check
/**
 * E2E tests — Paged reader (non-scroll mode).
 *
 * The paged reader (`/read/:manga_id/:chapter_id/:page`) shows one page at a
 * time.  Three tap zones drive navigation:
 *   - Left third  → previous page (or last page of previous chapter at ch start)
 *   - Right third → next page (or first page of next chapter at ch end)
 *   - Top strip   → toggle info overlay
 *
 * Progress is saved to localStorage (and fire-and-forget to IndexedDB) on
 * every navigation.
 */
const { test, expect } = require('@playwright/test');
const { seedDb, CH1, CH2 } = require('./helpers/seed');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Seed DB, ensure paged mode (vertical_scroll = false, which is the default),
 * then navigate directly to the reader at the given page.
 *
 * @param {import('@playwright/test').Page} page
 * @param {{ chapterId?: string, pageNum?: number, chapters?: object[] }} [opts]
 */
async function gotoPagedReader(page, { chapterId = 'ch1', pageNum = 0, chapters = [CH1, CH2] } = {}) {
  await page.goto('/');
  await seedDb(page, { chapters });
  // Explicitly set paged mode so the test is not affected by a leftover
  // vertical_scroll=true from a previous test run's localStorage.
  await page.evaluate(() => {
    localStorage.setItem(
      'pmanga_reader_config',
      JSON.stringify({ rtl_taps: false, vertical_scroll: false }),
    );
  });
  await page.goto(`/#/read/m1/${chapterId}/${pageNum}`);
}

/**
 * Click the right tap-zone (right third of screen) in the paged reader.
 * @param {import('@playwright/test').Page} page
 */
async function clickRightZone(page) {
  await page.locator('.tap-zone-right').click();
}

/**
 * Click the left tap-zone (left third of screen) in the paged reader.
 * @param {import('@playwright/test').Page} page
 */
async function clickLeftZone(page) {
  await page.locator('.tap-zone-left').click();
}

/**
 * Click the top tap-zone (top strip) in the paged reader.
 * @param {import('@playwright/test').Page} page
 */
async function clickTopZone(page) {
  await page.locator('.tap-zone-top').click();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test('shows the page image once data is loaded from IndexedDB', async ({ page }) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });

  // The reader renders <img alt="Manga page 0"> when the blob is loaded.
  // The image src is a blob: URL, so we just check the element is visible.
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();
});

test('tapping the right zone advances to the next page', async ({ page }) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });

  // Wait for the reader to be ready (page image visible).
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  await clickRightZone(page);

  // URL updates via navigator().replace() — page index increments.
  await expect(page).toHaveURL(/\/read\/m1\/ch1\/1/);
});

test('tapping the right zone on the last page navigates to the first page of the next chapter', async ({
  page,
}) => {
  // CH1 has 3 pages (0, 1, 2). Navigate to the last one.
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 2 });

  await expect(page.locator('img[alt="Manga page 2"]')).toBeVisible();

  await clickRightZone(page);

  // Should jump to ch2 page 0.
  await expect(page).toHaveURL(/\/read\/m1\/ch2\/0/);
});

test('tapping the left zone goes to the previous page', async ({ page }) => {
  // Start at page 1 so there is a page to go back to within the same chapter.
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 1 });

  await expect(page.locator('img[alt="Manga page 1"]')).toBeVisible();

  await clickLeftZone(page);

  await expect(page).toHaveURL(/\/read\/m1\/ch1\/0/);
});

test('tapping the left zone on the first page of a chapter navigates to the last page of the previous chapter', async ({
  page,
}) => {
  // CH2 has 2 pages (0, 1). Navigate to page 0 of ch2.
  await gotoPagedReader(page, { chapterId: 'ch2', pageNum: 0 });

  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  await clickLeftZone(page);

  // Should jump to ch1 last page (page_count - 1 = 2).
  await expect(page).toHaveURL(/\/read\/m1\/ch1\/2/);
});

test('tapping the top zone shows the overlay; tapping again hides it', async ({ page }) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });

  // Overlay should not be visible initially.
  await expect(page.getByRole('button', { name: '←' }).first()).not.toBeVisible();

  // First tap: show overlay.
  await clickTopZone(page);
  // The overlay shows a back-to-library button (SVG arrow), manga title, and
  // page info. The easiest stable target is the manga title text.
  await expect(page.getByText('Test Manga')).toBeVisible();

  // Second tap: hide overlay. The overlay is the top-zone div itself after it
  // becomes visible; clicking it calls on_close which sets overlay_visible=false.
  await clickTopZone(page);
  await expect(page.getByText('Test Manga')).not.toBeVisible();
});

test('navigating away immediately after opening the reader does not crash the app', async ({
  page,
}) => {
  // This exercises the component_alive guard that prevents async DB callbacks
  // from writing to signals of an already-unmounted component.
  await page.goto('/');
  await seedDb(page, { chapters: [CH1, CH2] });
  await page.evaluate(() => {
    localStorage.setItem(
      'pmanga_reader_config',
      JSON.stringify({ rtl_taps: false, vertical_scroll: false }),
    );
  });

  // Set up the error listener only after initial setup so that any JS module-
  // loading quirks from the initial app boot are not captured.  We filter
  // "Unexpected token 'export'" explicitly because it can occur when the
  // browser interrupts a mid-flight ES-module script during rapid navigation;
  // this is a browser-level artefact, not a Dioxus component crash.
  const errors = [];
  page.on('pageerror', (err) => errors.push(err.message));

  // Navigate to the reader.
  await page.goto('/#/read/m1/ch1/0');

  // Immediately navigate away before the WASM async DB work can finish.
  await page.goto('/');

  // The shelf should load cleanly — no JS errors, no blank screen.
  await expect(page.getByRole('heading', { name: 'PManga' })).toBeVisible();

  // No uncaught errors should have been captured during the race.
  const unexpectedErrors = errors.filter((e) => !e.includes("Unexpected token 'export'"));
  expect(unexpectedErrors).toHaveLength(0);

  // The reader should still be usable after the cancelled navigation — the
  // component_alive guard must not leave the app in a broken state.
  await page.goto('/#/read/m1/ch1/0');
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();
});

