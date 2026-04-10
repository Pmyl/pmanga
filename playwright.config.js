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
  // In CI the WASM is pre-built by a prior workflow step, so we use a simple
  // static-file server (`http-server`) that starts instantly.  `--spa`
  // enables SPA-mode fallback so every non-file path serves `index.html` and
  // client-side routing (Dioxus router) handles the rest.
  //
  // NOTE: `serve` (the vercel package) is NOT used here because its "clean
  // URLs" feature rewrites index.html → /index → / → 404, creating a redirect
  // loop that permanently blocks Playwright's readiness probe.
  //
  // Locally we keep `dx serve` for the normal hot-reload development flow.
  // The 10-minute timeout covers first-time compilation from scratch.
  webServer: {
    command: process.env.CI
      ? 'npx --yes http-server ./dist -p 8080 --spa'
      : 'dx serve --platform web --addr 127.0.0.1',
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
