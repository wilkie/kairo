// `@kairo/api-client/mock` — browser-side default surface. The
// browser worker never pulls `msw/node`, so this entry is safe
// to import from production (or dev) bundles without leaking
// Node-only modules.
//
// Tests that need the request-interception server import
// `@kairo/api-client/mock/node` instead.

export { fixtures, versionFixture, statusFixture } from './fixtures';
export { successEnvelope, errorEnvelope } from './envelope';
export { handlers, createHandlers, type MockFixtures } from './handlers';
export { startMockWorker, stopMockWorker, type StartMockWorkerOptions } from './browser';
