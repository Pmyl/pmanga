// @ts-check
const { defineConfig, devices } = require('@playwright/test');

module.exports = defineConfig({
  testDir: './tests/e2e',

  // Each test has 60 s to complete; the expect timeout gives assertions 20 s to
  // succeed so the WASM app has time to fetch data from IndexedDB and render.
  timeout: 60_000,
  expect: { timeout: 20_000 },

  // Run all tests in a single worker so the shared `dx run` process is not
  // overwhelmed and tests don't interfere through the browser's same-origin
  // storage (each Playwright test gets its own BrowserContext, but a single
  // worker keeps the start-up cost low).
  workers: 1,
  fullyParallel: false,

  // Start the app before running tests.
  //
  // `dx serve --platform web` builds the WASM app and starts the Dioxus dev
  // server, which correctly handles client-side routing (unlike a plain static
  // file server that returns 404 for the SPA root).
  //
  // The 10-minute timeout covers first-time compilation from scratch in CI.
  webServer: {
    command: 'dx serve --platform web --addr 127.0.0.1',
    url: 'http://127.0.0.1:8080',
    timeout: 10 * 60 * 1000,
    reuseExistingServer: !process.env.CI,
    stdout: 'pipe',
  },

  use: {
    baseURL: 'http://127.0.0.1:8080',
    // Chromium is the only browser that ships with Playwright by default and
    // is representative of the WebKit/Blink engine most users will use.
    browserName: 'chromium',
  },

  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
});
