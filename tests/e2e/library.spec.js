// @ts-check
/**
 * E2E tests — Library page (entries list).
 *
 * The Library page (`/library/:manga_id`) shows the interleaved list of
 * tankobon volumes and lone chapters for one manga series.  It also provides
 * entry-point navigation to the Reader and entry deletion (with a confirmation
 * dialog).
 */
const { test, expect } = require('@playwright/test');
const { seedDb, seedWcDb, setProxyUrl, WC_CH1, CH1, CH2, CH3_LONE } = require('./helpers/seed');

// ---------------------------------------------------------------------------
// Proxy URL used throughout sync-related tests.
// ---------------------------------------------------------------------------
const TEST_PROXY_URL = 'http://127.0.0.1:7332';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Seed the DB, then navigate directly to the Library page for "m1".
 *
 * @param {import('@playwright/test').Page} page
 * @param {Parameters<typeof seedDb>[1]} [seedOpts]
 */
async function gotoLibrary(page, seedOpts = {}) {
  await page.goto('/');
  await seedDb(page, seedOpts);
  await page.goto('/#/library/m1');
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test('shows tankobon volumes and lone chapters interleaved', async ({ page }) => {
  // CH1 + CH2 share tankobon_number=1 → "Vol. 1"; CH3_LONE has none → "Ch. 5.0"
  await gotoLibrary(page, { chapters: [CH1, CH2, CH3_LONE] });

  await expect(page.getByText('Vol. 1')).toBeVisible();
  await expect(page.getByText('Ch. 5.0')).toBeVisible();
});

test('clicking a library entry navigates to the reader', async ({ page }) => {
  await gotoLibrary(page, { chapters: [CH1, CH2] });

  // Click the cover / body of the first entry (Vol. 1).
  await page.getByText('Vol. 1').click();

  // The reader URL encodes manga_id, chapter_id, and page.
  await expect(page).toHaveURL(/\/read\/m1\/ch1\/\d+/);
});

test('deleting an entry: confirmation dialog → confirm removes the entry', async ({ page }) => {
  await gotoLibrary(page, { chapters: [CH1, CH2, CH3_LONE] });

  // The lone chapter card is simpler to target (unique label).
  await expect(page.getByText('Ch. 5.0')).toBeVisible();

  // Open the delete dialog for the lone chapter.
  // The delete button (🗑) is in the card's action area.
  const loneCard = page.getByText('Ch. 5.0').locator('..').locator('..');
  await loneCard.getByTitle('Delete').click();

  // Confirm dialog appears with "Confirm" and "Cancel" buttons.
  await expect(page.getByRole('button', { name: 'Confirm' })).toBeVisible();

  // Click "Confirm" to delete.
  await page.getByRole('button', { name: 'Confirm' }).click();

  // The lone chapter should disappear; Vol. 1 should remain.
  await expect(page.getByText('Ch. 5.0')).not.toBeVisible();
  await expect(page.getByText('Vol. 1')).toBeVisible();
});

test('deleting an entry: confirmation dialog → cancel keeps the entry', async ({ page }) => {
  await gotoLibrary(page, { chapters: [CH1, CH2, CH3_LONE] });

  await expect(page.getByText('Ch. 5.0')).toBeVisible();

  const loneCard = page.getByText('Ch. 5.0').locator('..').locator('..');
  await loneCard.getByTitle('Delete').click();

  await expect(page.getByRole('button', { name: 'Cancel' })).toBeVisible();

  // Click "Cancel" to dismiss without deleting.
  await page.getByRole('button', { name: 'Cancel' }).click();

  // Both entries should still be visible.
  await expect(page.getByText('Ch. 5.0')).toBeVisible();
  await expect(page.getByText('Vol. 1')).toBeVisible();
});

// ---------------------------------------------------------------------------
// Proxy page button on network error
// ---------------------------------------------------------------------------

test('sync error banner shows an "Open proxy page" button when the network request fails', async ({
  page,
}) => {
  // Seed a WeebCentral manga with one chapter so the Sync button is shown.
  await page.goto('/');
  await seedWcDb(page, { chapters: [WC_CH1] });
  await setProxyUrl(page, TEST_PROXY_URL);

  // Abort all requests to the proxy to simulate a network / certificate error.
  await page.route(`${TEST_PROXY_URL}/**`, (route) => route.abort('failed'));

  await page.goto('/#/library/wc1');
  await expect(page.getByText('Ch. 10.0')).toBeVisible();

  // Open the sync range panel.
  await page.getByRole('button', { name: '↻ Sync' }).click();

  // Start the sync (leave From/To empty to sync everything).
  await page.getByRole('button', { name: 'Go' }).click();

  // The error banner should appear with a network-error message.
  await expect(page.getByText(/network error|failed to fetch/i)).toBeVisible();

  // A button that opens the proxy page must be present so the user can
  // re-approve the certificate without leaving the app flow.
  await expect(page.getByTitle('Open proxy page')).toBeVisible();
});
