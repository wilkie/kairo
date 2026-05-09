// `@kairo/api-client/mock` — browser-side default surface. The
// browser worker never pulls `msw/node`, so this entry is safe
// to import from production (or dev) bundles without leaking
// Node-only modules.
//
// Tests that need the request-interception server import
// `@kairo/api-client/mock/node` instead.

export { fixtures, mockIds, versionFixture, statusFixture } from './fixtures';
export { mockRegistry, type MockRegistry } from './registry';
export { successEnvelope, errorEnvelope } from './envelope';
export { handlers, createHandlers } from './handlers';
export { startMockWorker, stopMockWorker, type StartMockWorkerOptions } from './browser';
