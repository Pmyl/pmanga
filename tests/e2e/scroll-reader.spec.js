// @ts-check
/**
 * E2E tests — Scroll reader (vertical-scroll / webtoon mode).
 *
 * The scroll reader (`/read/:manga_id/:chapter_id/:page`) renders all pages
 * top-to-bottom in a single scrollable column.  Navigation is driven by
 * position-based click routing on the container:
 *   - Top 15 % of viewport height → toggle info overlay
 *   - Left third                  → scroll up / previous chapter at top
 *   - Right third                 → scroll down / next chapter at bottom
 *   - Middle third                → pass-through (no action)
 *
 * Race condition — initial scroll:
 *   The reader must not scroll to the target page before all images ABOVE
 *   that page have loaded, otherwise offsetTop values are wrong (the "few
 *   pages back" bug).  This is covered by the unit tests in
 *   `scroll_reader.rs`; the e2e tests verify the high-level outcome.
 */
const { test, expect } = require('@playwright/test');
const { seedDb, enableScrollMode, CH1, CH2 } = require('./helpers/seed');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Seed DB, enable vertical-scroll mode, then navigate to the reader.
 *
 * @param {import('@playwright/test').Page} page
 * @param {{ chapterId?: string, pageNum?: number, chapters?: object[] }} [opts]
 */
async function gotoScrollReader(
  page,
  { chapterId = 'ch1', pageNum = 0, chapters = [CH1, CH2] } = {},
) {
  await page.goto('/');
  await seedDb(page, { chapters });
  await enableScrollMode(page);
  await page.goto(`/#/read/m1/${chapterId}/${pageNum}`);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test('renders all pages of the chapter as a vertically stacked strip', async ({ page }) => {
  await gotoScrollReader(page, { chapterId: 'ch1' });

  // CH1 has 3 pages (page_count = 3). Each renders as <img alt="Manga page N">.
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();
  await expect(page.locator('img[alt="Manga page 1"]')).toBeVisible();
  await expect(page.locator('img[alt="Manga page 2"]')).toBeVisible();
});

test('tapping the top strip shows the overlay', async ({ page }) => {
  await gotoScrollReader(page, { chapterId: 'ch1' });

  // Wait for the reader to be fully loaded before interacting with it; without
  // this the tap zones are not yet mounted and the click has no effect.
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  // Overlay not visible initially.
  await expect(page.getByText('Test Manga')).not.toBeVisible();

  // Click inside the top 15 % of the viewport height.
  const viewport = page.viewportSize();
  const topStripY = Math.floor((viewport?.height ?? 800) * 0.05);
  await page.mouse.click(200, topStripY);

  // Overlay shows the manga title and chapter info.
  await expect(page.getByText('Test Manga')).toBeVisible();
});

test('tapping the right zone when at the bottom of a chapter navigates to the next chapter', async ({
  page,
}) => {
  await gotoScrollReader(page, { chapterId: 'ch1' });

  // Wait for all pages to be visible so the scroll container is fully rendered.
  await expect(page.locator('img[alt="Manga page 2"]')).toBeVisible();

  // Scroll the container to its very bottom so at_bottom_signal becomes true.
  // The double-rAF yields back to the browser event loop twice, ensuring
  // Dioxus (WASM) processes the scroll event and updates at_bottom_signal
  // before we attempt the click — without this, the click only scrolls
  // down a step instead of navigating to the next chapter.
  await page.evaluate(() => new Promise((resolve) => {
    const container = document.getElementById('pmanga-scroll-container');
    if (container) container.scrollTop = container.scrollHeight;
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));

  // Click the right third of the screen (outside the top 15 % strip).
  const viewport = page.viewportSize();
  const rightZoneX = Math.floor((viewport?.width ?? 1280) * 0.85);
  const midY = Math.floor((viewport?.height ?? 800) * 0.5);
  await page.mouse.click(rightZoneX, midY);

  // The reader should navigate to ch2, page 0.
  await expect(page).toHaveURL(/\/read\/m1\/ch2\/0/);
});

test('navigating away while pages are loading does not crash the app', async ({ page }) => {
  // This exercises the component_alive guard in the async page-URL-loading
  // resource.  The race is best-effort at e2e level since IndexedDB is fast,
  // but we can verify that a rapid navigate-away + navigate-back leaves the
  // app in a healthy state.
  await page.goto('/');
  await seedDb(page, { chapters: [CH1, CH2] });
  await enableScrollMode(page);

  // Set up the error listener only after initial setup so that any JS module-
  // loading quirks from the initial app boot are not captured.  We filter
  // "Unexpected token 'export'" explicitly because it can occur when the
  // browser interrupts a mid-flight ES-module script during rapid navigation;
  // this is a browser-level artefact, not a Dioxus component crash.
  const errors = [];
  page.on('pageerror', (err) => errors.push(err.message));

  // Navigate to the reader…
  await page.goto('/#/read/m1/ch1/0');

  // …and immediately navigate away before page loads settle.
  await page.goto('/');

  // The shelf should render cleanly.
  await expect(page.getByRole('heading', { name: 'PManga' })).toBeVisible();
  const unexpectedErrors = errors.filter((e) => !e.includes("Unexpected token 'export'"));
  expect(unexpectedErrors).toHaveLength(0);

  // The scroll reader should still work after the cancelled navigation.
  await page.goto('/#/read/m1/ch1/0');
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();
});

