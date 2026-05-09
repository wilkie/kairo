// MSW handlers that mirror the daemon's `/api/v1/*` surface.
//
// `createHandlers(...)` lets callers (tests, dev mode) pass
// their own fixture set when they want to override the
// defaults — the test suite for the object browser, for
// instance, drives validation status by swapping in a
// `verifyObject` fixture per case.
//
// Slice 5 ships the `version` handler. Slice 6+ adds the rest
// alongside the corresponding hooks.

import { http, HttpResponse } from 'msw';
import { fixtures as defaultFixtures } from './fixtures';
import { successEnvelope } from './envelope';
import type { components } from '../generated/schema';

export interface MockFixtures {
  version: components['schemas']['VersionInfo'];
  status: components['schemas']['StatusInfo'];
}

export function createHandlers(fixtures: MockFixtures = defaultFixtures) {
  return [
    http.get('*/api/v1/version', () => HttpResponse.json(successEnvelope(fixtures.version))),
    http.get('*/api/v1/status', () => HttpResponse.json(successEnvelope(fixtures.status))),
  ];
}

/** Default handler set — what `setupBrowserMock` /
 * `setupNodeMock` use when callers don't pass an override. */
export const handlers = createHandlers();
