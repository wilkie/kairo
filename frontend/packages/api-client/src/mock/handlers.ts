// MSW handlers that mirror the daemon's `/api/v1/*` surface.
//
// `createHandlers(...)` lets callers (tests, dev mode) pass
// their own fixture set when they want to override defaults —
// the test suite for the object browser, for instance, drives
// validation status by swapping in a `verifyObject` fixture
// per case.
//
// Path patterns use `*/` prefixes so the handlers match
// regardless of whether the SPA hits a same-origin URL
// (browser default) or an absolute URL (Node tests with a
// configured base).

import { http, HttpResponse } from 'msw';
import { fixtures as defaultFixtures } from './fixtures';
import { successEnvelope } from './envelope';
import type { components } from '../generated/schema';

export interface MockFixtures {
  version: components['schemas']['VersionInfo'];
  status: components['schemas']['StatusInfo'];
  actor: components['schemas']['ActorGenesisJson'];
  object: components['schemas']['ObjectGenesisStatementJson'];
  statement: unknown;
  branchTips: components['schemas']['BranchTipDto'][];
  branchLatest: components['schemas']['ObjectBranchStatementJson'];
  versionTagLatest: components['schemas']['ObjectVersionTagStatementJson'];
  trust: components['schemas']['ActorTrustStatementJson'];
  capabilityHeads: components['schemas']['CapabilityHeadDto'][];
  blob: Uint8Array;
  verifyObject: components['schemas']['ValidationResult'];
}

export function createHandlers(fixtures: MockFixtures = defaultFixtures) {
  return [
    http.get('*/api/v1/version', () => HttpResponse.json(successEnvelope(fixtures.version))),
    http.get('*/api/v1/status', () => HttpResponse.json(successEnvelope(fixtures.status))),
    http.get('*/api/v1/actors/:id', () => HttpResponse.json(successEnvelope(fixtures.actor))),
    http.get('*/api/v1/objects/:id', () => HttpResponse.json(successEnvelope(fixtures.object))),
    http.get('*/api/v1/statements/:id', () =>
      HttpResponse.json(successEnvelope(fixtures.statement)),
    ),
    http.get('*/api/v1/branches/:object', () =>
      HttpResponse.json(successEnvelope(fixtures.branchTips)),
    ),
    http.get('*/api/v1/branches/:object/:name/latest', () =>
      HttpResponse.json(successEnvelope(fixtures.branchLatest)),
    ),
    http.get('*/api/v1/version-tags/:object/:version', () =>
      HttpResponse.json(successEnvelope(fixtures.versionTagLatest)),
    ),
    http.get('*/api/v1/trust/:by/:of', () => HttpResponse.json(successEnvelope(fixtures.trust))),
    http.get('*/api/v1/capabilities/:grantor', () =>
      HttpResponse.json(successEnvelope(fixtures.capabilityHeads)),
    ),
    http.get('*/api/v1/blobs/:id', () => {
      // Use ArrayBuffer so the body has a stable byte view; MSW
      // and the browser's fetch both accept it as the body
      // parameter.
      const bytes = new Uint8Array(fixtures.blob);
      return new HttpResponse(bytes, {
        headers: {
          'Content-Type': 'application/octet-stream',
          'Content-Length': bytes.byteLength.toString(),
        },
      });
    }),
    http.get('*/api/v1/verify-object/:id', () =>
      HttpResponse.json(successEnvelope(fixtures.verifyObject)),
    ),
  ];
}

/** Default handler set — what `setupBrowserMock` /
 * `setupNodeMock` use when callers don't pass an override. */
export const handlers = createHandlers();
