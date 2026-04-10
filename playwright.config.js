// @ts-check
const { defineConfig, devices } = require('@playwright/test');

module.exports = defineConfig({
  testDir: './tests/e2e',

  // Each test has 30 s to complete; the expect timeout gives assertions 10 s to
  // succeed so the WASM app has time to fetch data from IndexedDB and render.
  timeout: 30_000,
  expect: { timeout: 10_000 },

  // Run all tests in a single worker so the shared `dx run` process is not
  // overwhelmed and tests don't interfere through the browser's same-origin
  // storage (each Playwright test gets its own BrowserContext, but a single
  // worker keeps the start-up cost low).
  workers: 1,
  fullyParallel: false,

  // Start the app before running tests.  10-minute timeout covers the first
  // Rust/WASM compilation from scratch.  In CI, a fresh server is always
  // started; locally the existing server is reused if one is already running.
  webServer: {
    command: 'dx run --addr 127.0.0.1',
    url: 'http://localhost:8080',
    timeout: 10 * 60 * 1000,
    reuseExistingServer: !process.env.CI,
    stdout: 'pipe',
  },

  use: {
    baseURL: 'http://localhost:8080',
    // Chromium is the only browser that ships with Playwright by default and
    // is representative of the WebKit/Blink engine most users will use.
    browserName: 'chromium',
  },

  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
});
