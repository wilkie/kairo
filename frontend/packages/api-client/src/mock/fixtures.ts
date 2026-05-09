// Canned response bodies the MSW handlers serve. Field shapes
// match the OpenAPI schema exactly — kept in `components`
// types so a daemon-side schema change surfaces as a TS error
// here, not a runtime mismatch in the inspector.
//
// Every fixture is a complete response body (the inner
// `result`); the handlers wrap it in the success envelope so
// the wire format matches the real daemon byte-for-byte.

import type { components } from '../generated/schema';

export const versionFixture: components['schemas']['VersionInfo'] = {
  daemon_version: 'mock-0.1.0',
  api_version: 'v1',
  core_version: 'mock-0.1.0',
  store_version: 'mock-0.1.0',
};

export const statusFixture: components['schemas']['StatusInfo'] = {
  daemon_running: true,
  store_path: '/mock/store',
  store_schema_version: '1',
  pid: 1,
  daemon_version: 'mock-0.1.0',
};

/**
 * Default fixture set. Slice 6+ extends this with object /
 * actor / branch / capability fixtures as the relevant hooks
 * land. Keep one fixture per response shape so the mock surface
 * grows linearly with the typed surface.
 */
export const fixtures = {
  version: versionFixture,
  status: statusFixture,
} as const;
