// Browser-side MSW worker. Apps call `startMockWorker()` in
// dev mode (gated by `import.meta.env.VITE_USE_MOCK_API ===
// 'true'`) before mounting React; once it resolves, every
// `fetch` for `/api/v1/*` is answered by the in-process
// handlers without ever touching a real daemon.
//
// `mockServiceWorker.js` must exist at the served root for the
// worker to register. `apps/web-client/public/mockServiceWorker.js`
// is what Vite serves; keep it in sync via `pnpm -F
// @kairo/web-client mock:init` (which runs `msw init`
// transparently).

import { setupWorker, type SetupWorker } from 'msw/browser';
import { handlers as defaultHandlers } from './handlers';
import type { MockRegistry } from './registry';

export interface StartMockWorkerOptions {
  /** Replace the default mock registry wholesale. */
  registry?: MockRegistry;
  /**
   * What to do with requests no handler matches. `'bypass'`
   * lets non-API URLs (the SPA bundle, source maps, dev
   * assets) flow through to the network unchanged — the
   * default we want for a dev-mode app that mocks only the
   * daemon API.
   */
  onUnhandledRequest?: 'bypass' | 'warn' | 'error';
}

let worker: SetupWorker | undefined;

export async function startMockWorker(opts: StartMockWorkerOptions = {}): Promise<SetupWorker> {
  if (worker !== undefined) {
    return worker;
  }
  const { createHandlers } = await import('./handlers');
  const handlers = opts.registry ? createHandlers(opts.registry) : defaultHandlers;
  worker = setupWorker(...handlers);
  await worker.start({
    onUnhandledRequest: opts.onUnhandledRequest ?? 'bypass',
    quiet: false,
  });
  return worker;
}

export function stopMockWorker(): void {
  if (worker !== undefined) {
    worker.stop();
    worker = undefined;
  }
}
