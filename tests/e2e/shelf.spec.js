// @ts-check
/**
 * E2E tests — Shelf page (manga list).
 *
 * The Shelf is the root page (`/`).  It displays a grid of manga series cards
 * loaded from IndexedDB, handles the one-time startup redirect, and provides
 * an entry point to each manga's Library page.
 */
const { test, expect } = require('@playwright/test');
const { seedDb, setLastOpened, CH1, CH2 } = require('./helpers/seed');

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

test('clicking a manga card navigates to its library page', async ({ page }) => {
  await gotoShelf(page, { chapters: [CH1, CH2] });

  await expect(page.getByText('Test Manga')).toBeVisible();
  await page.getByText('Test Manga').click();

  await expect(page).toHaveURL(/\/library\/m1/);
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
