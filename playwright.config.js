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
  // In CI the WASM is pre-built by a prior workflow step into
  // ./target/dx/pmanga/release/web/public (the path dx build --release
  // --platform web uses).  We use `serve --single` as the static server:
  //   • `--single` is the explicitly-documented SPA flag in serve v14: all
  //     non-file paths (e.g. /library/m1, /read/m1/ch1/0) are served as
  //     index.html so Dioxus's client-side router handles routing.
  //   • We serve the RELEASE build (not debug) because the Dioxus debug
  //     build injects a hot-reload client whose JS uses ES module `export`
  //     syntax in a non-module context, causing an uncaught page error that
  //     would fail the "no console errors" assertion.
  //
  // Locally we keep `dx serve` for the normal hot-reload development flow.
  webServer: {
    command: process.env.CI
      ? 'npx --yes serve ./target/dx/pmanga/release/web/public -l 8080 --single'
      : 'dx serve --platform web --addr 127.0.0.1',
    url: 'http://localhost:8080',
    timeout: 30_000,
    reuseExistingServer: !process.env.CI,
    stdout: 'ignore',
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
