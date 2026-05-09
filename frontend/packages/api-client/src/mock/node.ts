// Node-side MSW server for Vitest / unit / component tests.
// Tests call `setupMockServer()` from a test setup file;
// individual tests can swap behavior with
// `server.use(http.get(...))` per case (MSW prepends), or pass
// a custom `registry` to seed a different default surface.

import { setupServer, type SetupServer } from 'msw/node';
import { createHandlers, handlers as defaultHandlers } from './handlers';
import type { MockRegistry } from './registry';

export interface SetupMockServerOptions {
  /** Replace the default mock registry wholesale. Most tests
   * don't need this — they layer per-case overrides via
   * `server.use(...)`. */
  registry?: MockRegistry;
}

export function setupMockServer(opts: SetupMockServerOptions = {}): SetupServer {
  const handlers = opts.registry ? createHandlers(opts.registry) : defaultHandlers;
  return setupServer(...handlers);
}
