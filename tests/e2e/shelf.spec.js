// @ts-check
/**
 * E2E tests — Shelf page (manga list).
 *
 * The Shelf is the root page (`/`).  It displays a grid of manga series cards
 * loaded from IndexedDB, handles the one-time startup redirect, and provides
 * an entry point to each manga's Library page.
 */
const { test, expect } = require('@playwright/test');
const { seedDb, seedWcDb, setLastOpened, setProxyUrl, CH1, CH2 } = require('./helpers/seed');

// ---------------------------------------------------------------------------
// Proxy URL used throughout sync-related tests.
// Route this address with page.route() to intercept / abort proxy requests.
// ---------------------------------------------------------------------------
const TEST_PROXY_URL = 'http://127.0.0.1:7332';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Navigate to the app root to put us on the right origin, seed the DB, then
 * hard-navigate to `/` so the WASM app starts fresh and reads seeded data.
 *
 * @param {import('@playwright/test').Page} page
 * @param {Parameters<typeof seedDb>[1]} [seedOpts]
 */
async function gotoShelf(page, seedOpts) {
  await page.goto('/');
  if (seedOpts !== undefined) {
    await seedDb(page, seedOpts);
  }
  await page.goto('/');
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test('shows empty grid when no manga is in the library', async ({ page }) => {
  // No seeding — fresh empty DB.
  await page.goto('/');

  // App header should be visible.
  await expect(page.getByRole('heading', { name: 'PManga' })).toBeVisible();

  // No manga card titles should appear.
  await expect(page.getByText('Test Manga')).not.toBeVisible();
});

test('shows a manga card for each series in the library', async ({ page }) => {
  await gotoShelf(page, { chapters: [CH1, CH2] });

  // The manga card renders the series title.
  await expect(page.getByText('Test Manga')).toBeVisible();
});

test('clicking a manga card uses hash-based routing (url contains /#/library)', async ({ page }) => {
  await gotoShelf(page, { chapters: [CH1, CH2] });

  await expect(page.getByText('Test Manga')).toBeVisible();
  await page.getByText('Test Manga').click();

  // The app is configured with HashHistory: routes live in the URL hash
  // fragment (e.g. http://localhost:8080/#/library/m1), not as a plain path
  // (e.g. http://localhost:8080/library/m1).  This assertion proves that the
  // hash fragment is what drives routing and that deep-route page.goto() calls
  // must use the /#/ prefix.
  await expect(page).toHaveURL(/\/#\/library\/m1/);
});

test('startup redirect: navigates to reader when last session was in reader', async ({ page }) => {
  // Seed DB so the reader can load the chapter, then set last_opened.
  await page.goto('/');
  await seedDb(page, { chapters: [CH1] });
  await setLastOpened(page, { Reader: { manga_id: 'm1', chapter_id: 'ch1', page: 1 } });

  // Hard-navigate to `/` — the app reads localStorage and redirects.
  await page.goto('/');

  await expect(page).toHaveURL(/\/read\/m1\/ch1\/1/);
});

test('startup redirect: navigates to library when last session was in library', async ({ page }) => {
  await page.goto('/');
  await seedDb(page, { chapters: [CH1] });
  await setLastOpened(page, { Library: { manga_id: 'm1' } });

  await page.goto('/');

  await expect(page).toHaveURL(/\/library\/m1/);
});

// ---------------------------------------------------------------------------
// Auto-sync on first shelf load
// ---------------------------------------------------------------------------

/**
 * The shelf should automatically trigger a "sync all caught-up" when it first
 * loads in a browser session, so the user always sees fresh chapters without
 * having to click the ↻ Sync button manually.
 *
 * "Once per refresh" is enforced via the same in-memory WASM flag used for the
 * startup redirect (`is_startup_redirect_done`).  A full page reload resets the
 * WASM module and allows auto-sync to fire again; in-app navigation does not.
 */
test('auto-sync fires on the first shelf load when there are caught-up WC mangas', async ({ page }) => {
  // Seed a WeebCentral manga that is "caught up":
  // no local chapters, but latest_downloaded_chapter is set.
  await page.goto('/');
  await seedWcDb(page, { chapters: [] });
  await setProxyUrl(page, TEST_PROXY_URL);

  // Intercept proxy requests: respond with an empty chapter list so the sync
  // completes quickly and we can assert on the Done banner.
  await page.route(`${TEST_PROXY_URL}/**`, async (route) => {
    await route.fulfill({ contentType: 'application/json', body: '[]' });
  });

  // Fresh load: auto-sync should start automatically (without clicking ↻ Sync).
  await page.goto('/');

  // The Done banner ("✓ All caught-up manga are up to date.") must appear
  // without the user clicking the ↻ Sync button.
  await expect(page.getByText(/all caught-up manga are up to date/i)).toBeVisible({
    timeout: 15_000,
  });
});

test('auto-sync does not fire again when navigating away and back within the same session', async ({ page }) => {
  // Same setup as above.
  await page.goto('/');
  await seedWcDb(page, { chapters: [] });
  await setProxyUrl(page, TEST_PROXY_URL);

  let proxyRequestCount = 0;
  await page.route(`${TEST_PROXY_URL}/**`, async (route) => {
    proxyRequestCount++;
    await route.fulfill({ contentType: 'application/json', body: '[]' });
  });

  // First visit: auto-sync should fire once.
  await page.goto('/');
  await expect(page.getByText(/all caught-up manga are up to date/i)).toBeVisible({
    timeout: 15_000,
  });
  const countAfterFirstVisit = proxyRequestCount;
  expect(countAfterFirstVisit).toBeGreaterThan(0);

  // Navigate to the library (in-app navigation — same WASM session, same module
  // instance, so the in-memory "startup done" flag is still set).
  await page.goto('/#/library/wc1');
  await expect(page).toHaveURL(/\/#\/library\/wc1/);

  // Navigate back to the shelf via a hash change (in-app, no full reload).
  await page.goto('/#/');
  await expect(page.getByText('WC Manga')).toBeVisible();

  // Give the app time to settle; no new proxy request should have been fired.
  await page.waitForTimeout(3_000);
  expect(proxyRequestCount).toBe(countAfterFirstVisit);
});

// ---------------------------------------------------------------------------
// Sync with no eligible mangas
// ---------------------------------------------------------------------------

test('clicking sync shows a positive all-done message when no caught-up WC mangas exist', async ({
  page,
}) => {
  // Only local mangas — none have a WeebCentral series_url, so none are eligible.
  await page.goto('/');
  await seedDb(page, { chapters: [CH1, CH2] });
  await page.goto('/');

  await expect(page.getByText('Test Manga')).toBeVisible();

  // Manually trigger the sync.
  await page.getByRole('button', { name: '↻ Sync' }).click();

  // Expect a positive "all done" message — NOT an error.
  await expect(page.getByText(/all caught-up manga are up to date/i)).toBeVisible();
  await expect(page.getByText(/No caught-up WeebCentral series/i)).not.toBeVisible();
});

// ---------------------------------------------------------------------------
// Proxy page button on network error
// ---------------------------------------------------------------------------

test('sync error banner shows an "Open proxy page" button when the network request fails', async ({
  page,
}) => {
  // Seed a caught-up WC manga and configure a proxy URL we can intercept.
  await page.goto('/');
  await seedWcDb(page, { chapters: [] });
  await setProxyUrl(page, TEST_PROXY_URL);

  // Abort all requests to the proxy to simulate a network / certificate error.
  await page.route(`${TEST_PROXY_URL}/**`, (route) => route.abort('failed'));

  await page.goto('/');
  await expect(page.getByText('WC Manga')).toBeVisible();

  // Trigger sync manually (auto-sync will also do this once implemented, but
  // here we test the error-banner feature independently).
  await page.getByRole('button', { name: '↻ Sync' }).click();

  // The error banner should appear with a network-error message.
  await expect(page.getByText(/network error|failed to fetch/i)).toBeVisible();

  // A button that opens the proxy page must be present so the user can
  // re-approve the certificate without leaving the app flow.
  await expect(page.getByTitle('Open proxy page')).toBeVisible();
});

// ---------------------------------------------------------------------------
// Regression: local-only library must not hit the proxy on load
// ---------------------------------------------------------------------------

test('shelf with local-only mangas makes no proxy requests on load', async ({ page }) => {
  await page.goto('/');
  await seedDb(page, { chapters: [CH1, CH2] });
  await setProxyUrl(page, TEST_PROXY_URL);

  const proxyRequests = /** @type {string[]} */ ([]);
  await page.route(`${TEST_PROXY_URL}/**`, (route) => {
    proxyRequests.push(route.request().url());
    route.abort();
  });

  await page.goto('/');
  await expect(page.getByText('Test Manga')).toBeVisible();

  // Wait long enough for any accidental auto-sync to surface.
  await page.waitForTimeout(3_000);

  expect(proxyRequests).toHaveLength(0);
});
