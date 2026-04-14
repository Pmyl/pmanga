// @ts-check
/**
 * E2E tests — Paged reader zoom (middle tap zone).
 *
 * The paged reader supports two zoom modes triggered by tapping the centre
 * tap zone (.tap-zone-middle):
 *
 *   - **Portrait zoom**: portrait image displayed on a portrait screen →
 *     rendered at 2× viewport width, stepped through 9 nonet positions
 *     (3 cols × 3 rows).  Reading order: top-right → top-middle → top-left →
 *     middle-right → … → bottom-left.
 *   - **Spread zoom**: landscape (double-spread) image displayed on a portrait
 *     screen → fitted to viewport height so its width overflows; the user
 *     pans left/right.
 *
 * While zoomed the left/right tap zones navigate **quadrants** (portrait zoom)
 * or **pan** the spread (spread zoom) rather than turning pages.  Tapping the
 * middle zone a second time exits zoom.  Zoom is reset whenever the reader
 * navigates to a different page or chapter.
 *
 * NOTE: the tests below intentionally do NOT call page.setViewportSize().
 * They rely on the default Playwright viewport (Desktop Chrome, 1280 × 720,
 * which is landscape).  The viewport helper `is_portrait()` inside the Rust/
 * WASM code returns `false` for that size (height 720 ≤ width 1280), which
 * causes `try_toggle_zoom` to return early before activating any zoom mode.
 * All tests below FAIL until the `!is_portrait()` guard in
 * `src/pages/reader/paged_reader.rs` is relaxed or removed.
 */
const { test, expect } = require('@playwright/test');
const { seedDb, CH1, CH2 } = require('./helpers/seed');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Seed DB, force paged (non-scroll) mode, and navigate to the reader.
 *
 * @param {import('@playwright/test').Page} page
 * @param {{ chapterId?: string, pageNum?: number, chapters?: object[] }} [opts]
 */
async function gotoPagedReader(page, { chapterId = 'ch1', pageNum = 0, chapters = [CH1, CH2] } = {}) {
  await page.goto('/');
  await seedDb(page, { chapters });
  await page.evaluate(() => {
    localStorage.setItem(
      'pmanga_reader_config',
      JSON.stringify({ rtl_taps: false, vertical_scroll: false }),
    );
  });
  await page.goto(`/#/read/m1/${chapterId}/${pageNum}`);
}

/** Click the centre tap zone to toggle zoom. */
async function clickMiddleZone(page) {
  await page.locator('.tap-zone-middle').click();
}

/** Click the right tap zone. */
async function clickRightZone(page) {
  await page.locator('.tap-zone-right').click();
}

/** Click the left tap zone. */
async function clickLeftZone(page) {
  await page.locator('.tap-zone-left').click();
}

/**
 * Returns true if the page image is currently in a zoom mode, identified by
 * the presence of `position: absolute` in its inline style.  In normal mode
 * the image carries no inline style; both portrait-zoom and spread-zoom modes
 * apply an inline style that includes `position: absolute`.
 *
 * @param {import('@playwright/test').Page} page
 * @param {number} pageNum  0-based page index shown in the alt text
 */
async function isZoomedIn(page, pageNum = 0) {
  const style = await page
    .locator(`img[alt="Manga page ${pageNum}"]`)
    .getAttribute('style');
  return style != null && /position:\s*absolute/.test(style);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test('tapping the middle zone activates zoom', async ({ page }) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });

  // Wait for the page image to be rendered.
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  // Before zooming the image must NOT carry a position:absolute inline style.
  expect(await isZoomedIn(page, 0)).toBe(false);

  // Tap the centre zone to activate zoom.
  await clickMiddleZone(page);

  // The image should now have an inline style with position:absolute, which
  // is the hallmark of both portrait-zoom and spread-zoom rendering.
  await expect(page.locator('img[alt="Manga page 0"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );
});

test('tapping the middle zone a second time deactivates zoom', async ({ page }) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });

  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  // First tap: enter zoom mode.
  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 0"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );

  // Second tap: exit zoom mode.  The inline style should be gone and the
  // image should revert to its default `class`-only presentation.
  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 0"]')).not.toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );
});

test('while portrait-zoomed, tapping the right zone advances the quadrant without changing page', async ({
  page,
}) => {
  // CH1 has 3 pages so tapping right on page 0 while NOT zoomed would go to
  // page 1.  While zoomed it must instead advance the nonet quadrant and keep
  // the reader on page 0.
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  // Enter zoom mode.
  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 0"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );

  // Capture the transform before advancing.
  const styleBefore = await page
    .locator('img[alt="Manga page 0"]')
    .getAttribute('style');

  // Tap the right zone: should advance the quadrant, NOT navigate to page 1.
  await clickRightZone(page);

  // The URL must still point to page 0.
  await expect(page).toHaveURL(/\/read\/m1\/ch1\/0/);

  // The inline style (specifically the translate offset) must have changed,
  // proving the quadrant actually advanced.
  const styleAfter = await page
    .locator('img[alt="Manga page 0"]')
    .getAttribute('style');
  expect(styleAfter).not.toEqual(styleBefore);
});

test('while portrait-zoomed, tapping the left zone retreats the quadrant without changing page', async ({
  page,
}) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 1 });
  await expect(page.locator('img[alt="Manga page 1"]')).toBeVisible();

  // Enter zoom mode.
  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 1"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );

  // First advance to quadrant 1 so there is somewhere to retreat to.
  await clickRightZone(page);

  const styleBefore = await page
    .locator('img[alt="Manga page 1"]')
    .getAttribute('style');

  // Retreat: should go back to quadrant 0, NOT navigate to page 0.
  await clickLeftZone(page);

  await expect(page).toHaveURL(/\/read\/m1\/ch1\/1/);

  const styleAfter = await page
    .locator('img[alt="Manga page 1"]')
    .getAttribute('style');
  expect(styleAfter).not.toEqual(styleBefore);
});

test('portrait zoom clamps at the last quadrant — right tap on the final nonet does not navigate pages', async ({
  page,
}) => {
  // There are 9 nonets (PORTRAIT_QUADRANT_COUNT = 9).  After advancing 8
  // times from quadrant 0 we land on quadrant 8 (the last one).  A further
  // right tap must NOT leave zoom mode or navigate to the next page.
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 0"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );

  // Advance through all 8 transitions (quadrants 0→1→…→8).
  for (let i = 0; i < 8; i++) {
    await clickRightZone(page);
  }

  // Capture style at quadrant 8.
  const styleAtLast = await page
    .locator('img[alt="Manga page 0"]')
    .getAttribute('style');

  // One more right tap: clamps at quadrant 8 (no change) and must not
  // navigate away from page 0.
  await clickRightZone(page);

  await expect(page).toHaveURL(/\/read\/m1\/ch1\/0/);
  expect(
    await page.locator('img[alt="Manga page 0"]').getAttribute('style'),
  ).toEqual(styleAtLast);
});

test('zoom is reset when the reader navigates to a different page', async ({ page }) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  // Activate zoom.
  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 0"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );

  // Exit zoom first so that the right-tap navigates to the next page.
  await clickMiddleZone(page);

  // Navigate to page 1.
  await clickRightZone(page);
  await expect(page).toHaveURL(/\/read\/m1\/ch1\/1/);

  // Page 1 should render without zoom (image in normal mode).
  await expect(page.locator('img[alt="Manga page 1"]')).toBeVisible();
  await expect(page.locator('img[alt="Manga page 1"]')).not.toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );
});
