// Playwright configuration for the real-stack e2e suite
// (`PHASE_2_WEB_WRITES.md` slice 8).
//
// Unlike `playwright.config.ts` (Vite dev server + MSW mocks),
// this suite runs the built SPA served by a real `kairo-web`,
// proxying to a real `kairo-daemon` over a store seeded by
// `scripts/smoke-seed.sh`. It is the layer that catches what the
// mocks can't: the proxy's request guards, real capability
// evaluation, real signing, and persistence across reloads.
//
// Prerequisites (CI's `e2e-real` job does all three):
//
//   cargo build --bins                       # kairo, kairo-daemon, kairo-web
//   pnpm turbo run build --filter @kairo/web-client...   # SPA dist, no mocks
//   pnpm -F @kairo/web-client e2e:install    # chromium
//
// `e2e-real/global-setup.ts` seeds a temp store, starts both
// processes, and tears them down afterwards. Tests share one
// store and write to it, so the suite runs serially.

import { defineConfig, devices } from '@playwright/test';
import { chromiumLaunchOptions } from './playwright.config';

export const REAL_STACK_PORT = Number(process.env['KAIRO_E2E_REAL_PORT'] ?? 7979);

export default defineConfig({
  testDir: './e2e-real',
  globalSetup: './e2e-real/global-setup.ts',
  timeout: 30_000,
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env['CI'],
  retries: 0,
  reporter: [['list']],
  use: {
    baseURL: `http://127.0.0.1:${REAL_STACK_PORT}`,
    trace: 'retain-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'], launchOptions: chromiumLaunchOptions },
    },
  ],
});
