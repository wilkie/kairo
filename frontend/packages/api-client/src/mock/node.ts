// Node-side MSW server for Vitest / unit / component tests.
// Tests call `setupMockServer()` from a test setup file;
// individual tests can swap fixtures with
// `server.use(...createHandlers({ ...customFixtures }))`.

import { setupServer, type SetupServer } from 'msw/node';
import { createHandlers, handlers as defaultHandlers, type MockFixtures } from './handlers';

export interface SetupMockServerOptions {
  /** Override the default fixture set. */
  fixtures?: MockFixtures;
}

export function setupMockServer(opts: SetupMockServerOptions = {}): SetupServer {
  const handlers = opts.fixtures ? createHandlers(opts.fixtures) : defaultHandlers;
  return setupServer(...handlers);
}
