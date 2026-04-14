// @ts-check
/**
 * E2E tests — Paged reader zoom (portrait mode).
 *
 * Portrait zoom is activated by tapping the centre tap zone (.tap-zone-middle)
 * when the viewport is portrait (height > width).  Zoom in landscape is
 * intentionally disabled by design.
 *
 * In portrait zoom mode a portrait (or square) image is displayed at 2×
 * viewport width and the user steps through 9 nonet positions (3 cols × 3
 * rows): top-right → top-middle → top-left → middle-right → … → bottom-left.
 * The left/right tap zones navigate **nonets** rather than pages while zoomed.
 * A second middle-tap exits zoom.
 *
 * All tests explicitly set a portrait viewport (390 × 844) so that
 * `is_portrait()` inside the WASM returns `true` and the zoom code path is
 * reachable.  The last test (decode() fallback) specifically exposes the
 * iOS Safari bug where a detached HtmlImageElement returns 0×0 natural
 * dimensions after decode(), leaving img_natural_size = None and blocking zoom.
 */
const { test, expect } = require('@playwright/test');
const { seedDb, CH1, CH2 } = require('./helpers/seed');

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/** A portrait viewport so `is_portrait()` returns true in WASM. */
const PORTRAIT_VIEWPORT = { width: 390, height: 844 };

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Set portrait viewport, seed DB, force paged (non-scroll) mode, and navigate
 * to the reader.
 *
 * The viewport is set before every navigation so the WASM sees the portrait
 * dimensions when `is_portrait()` is called during the middle-zone tap.
 *
 * @param {import('@playwright/test').Page} page
 * @param {{ chapterId?: string, pageNum?: number, chapters?: object[] }} [opts]
 */
async function gotoPagedReader(page, { chapterId = 'ch1', pageNum = 0, chapters = [CH1, CH2] } = {}) {
  await page.setViewportSize(PORTRAIT_VIEWPORT);
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test('tapping the middle zone in portrait activates zoom', async ({ page }) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });

  // Wait for the page image to be rendered (DB + pre-decode have completed).
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  // Before zooming the image must NOT carry a position:absolute inline style.
  await expect(page.locator('img[alt="Manga page 0"]')).not.toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );

  // Tap the centre zone to activate portrait zoom.
  await clickMiddleZone(page);

  // The image must now carry the portrait-zoom inline style which sets
  // `position: absolute` and `width: <2×vw>px`.
  await expect(page.locator('img[alt="Manga page 0"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );
});

test('tapping the middle zone a second time deactivates zoom', async ({ page }) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  // First tap: enter zoom.
  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 0"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );

  // Second tap: exit zoom.  The inline style disappears and the image reverts
  // to its default class-only presentation.
  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 0"]')).not.toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );
});

test('while portrait-zoomed, tapping the right zone advances the nonet quadrant without changing page', async ({
  page,
}) => {
  // CH1 has 3 pages.  Without zoom, right tap navigates to page 1.
  // While zoomed it must advance the nonet and stay on page 0.
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 0"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );

  // Capture the translate offset of nonet 0 (top-right position).
  const styleBefore = await page.locator('img[alt="Manga page 0"]').getAttribute('style');

  // Advance nonet: quadrant 0 → 1.
  await clickRightZone(page);

  // URL must still be page 0.
  await expect(page).toHaveURL(/\/read\/m1\/ch1\/0/);

  // The translate offset in the style must have changed.
  const styleAfter = await page.locator('img[alt="Manga page 0"]').getAttribute('style');
  expect(styleAfter).not.toEqual(styleBefore);
});

test('while portrait-zoomed, tapping the left zone retreats the nonet quadrant without changing page', async ({
  page,
}) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 1 });
  await expect(page.locator('img[alt="Manga page 1"]')).toBeVisible();

  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 1"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );

  // Advance to nonet 1 first so there is a nonet to retreat to.
  await clickRightZone(page);
  const styleBefore = await page.locator('img[alt="Manga page 1"]').getAttribute('style');

  // Retreat: quadrant 1 → 0.
  await clickLeftZone(page);

  // URL must still be page 1.
  await expect(page).toHaveURL(/\/read\/m1\/ch1\/1/);

  // The translate offset must have changed back.
  const styleAfter = await page.locator('img[alt="Manga page 1"]').getAttribute('style');
  expect(styleAfter).not.toEqual(styleBefore);
});

test('portrait zoom clamps at the last nonet — right tap at quadrant 8 does not navigate pages', async ({
  page,
}) => {
  // There are 9 nonets (indices 0–8, PORTRAIT_QUADRANT_COUNT = 9).  After advancing 8 times from
  // quadrant 0 we land on quadrant 8.  One more right tap must clamp (no
  // change) and must NOT navigate to the next page.
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

  const styleAtLast = await page.locator('img[alt="Manga page 0"]').getAttribute('style');

  // One extra right tap: clamps at quadrant 8 — style unchanged, page unchanged.
  await clickRightZone(page);

  await expect(page).toHaveURL(/\/read\/m1\/ch1\/0/);
  expect(await page.locator('img[alt="Manga page 0"]').getAttribute('style')).toEqual(styleAtLast);
});

test('zoom state resets when navigating to a different page', async ({ page }) => {
  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  // Activate zoom.
  await clickMiddleZone(page);
  await expect(page.locator('img[alt="Manga page 0"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );

  // Exit zoom so the right-zone tap navigates rather than advancing quadrant.
  await clickMiddleZone(page);

  // Navigate to page 1.
  await clickRightZone(page);
  await expect(page).toHaveURL(/\/read\/m1\/ch1\/1/);

  // Page 1 must render without any zoom style.
  await expect(page.locator('img[alt="Manga page 1"]')).toBeVisible();
  await expect(page.locator('img[alt="Manga page 1"]')).not.toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );
});

test('portrait zoom activates via onload fallback when HtmlImageElement.decode() fails', async ({
  page,
}) => {
  // Override decode() to always reject before any navigation.  This simulates
  // the failure mode where the pre-decode path cannot populate img_natural_size,
  // matching the outcome on iOS Safari where a detached HtmlImageElement reports
  // naturalWidth = 0 even after a successful decode().  (iOS Safari resolves the
  // decode() promise but the detached element does not update its natural
  // dimensions; the effect is the same: img_natural_size stays None.)
  //
  // Without the onload fallback: zoom never activates (this test fails, exposing
  // the bug).  With the fallback: the visible <img>'s onload event captures the
  // natural dimensions, so zoom activates correctly (this test passes).
  await page.addInitScript(() => {
    Object.defineProperty(HTMLImageElement.prototype, 'decode', {
      value() {
        return Promise.reject(new DOMException('Simulated decode failure', 'EncodingError'));
      },
      writable: true,
      configurable: true,
    });
  });

  await gotoPagedReader(page, { chapterId: 'ch1', pageNum: 0 });

  // The image still renders because blob_url is set even when decode() rejects.
  await expect(page.locator('img[alt="Manga page 0"]')).toBeVisible();

  // Tap the centre zone to attempt zoom.
  await clickMiddleZone(page);

  // With the onload fallback the natural dimensions are captured from the
  // visible <img> element and zoom must activate normally.
  await expect(page.locator('img[alt="Manga page 0"]')).toHaveAttribute(
    'style',
    /position:\s*absolute/,
  );
});
