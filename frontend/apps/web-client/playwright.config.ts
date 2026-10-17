// Playwright configuration for the slice-10 e2e suite.
//
// All tests run against the SPA in dev mode with the MSW
// browser worker enabled (`VITE_USE_MOCK_API=true`). The mock
// registry seeds Alpha (rich) / Beta (valid) / Gamma (invalid)
// / Delta (conflicted) objects so each critical workflow has
// a deterministic fixture without spinning up a real daemon.
//
// `webServer` starts `pnpm dev` automatically. Locally, if a
// dev server is already running on :5173, Playwright reuses
// it; in CI we always spin up a fresh one.
//
// Browsers are not downloaded automatically (pnpm 10's
// `onlyBuiltDependencies` policy blocks Playwright's
// post-install). Run `pnpm e2e:install` once after a clean
// `pnpm install` to fetch the chromium binary.

import { defineConfig, devices } from '@playwright/test';

const PORT = 5173;
const BASE_URL = `http://localhost:${PORT}`;

// Headless Chromium under WSL2 never fires `requestAnimationFrame`
// with its default GPU path, so every actionability check (and so
// every `click()`) waits forever. Forcing software compositing
// fixes it; all three flags are needed. Applied only under WSL so
// CI's browser setup is unchanged.
export const chromiumLaunchOptions = process.env['WSL_DISTRO_NAME']
  ? { args: ['--disable-gpu', '--disable-gpu-compositing', '--disable-software-rasterizer'] }
  : {};

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  fullyParallel: true,
  forbidOnly: !!process.env['CI'],
  retries: process.env['CI'] ? 2 : 0,
  reporter: [['list']],
  use: {
    baseURL: BASE_URL,
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'], launchOptions: chromiumLaunchOptions },
    },
  ],
  webServer: {
    command: 'pnpm dev',
    url: BASE_URL,
    env: {
      VITE_USE_MOCK_API: 'true',
    },
    reuseExistingServer: !process.env['CI'],
    timeout: 120_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
});
