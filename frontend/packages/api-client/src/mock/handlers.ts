// MSW handlers that mirror the daemon's `/api/v1/*` surface.
//
// Every handler does a lookup against `mockRegistry` (see
// `./registry.ts`); a missing entry returns a daemon-style 404
// envelope so the inspector exercises the not-found path
// instead of confusingly rendering the same fixture for any
// id. To extend the dev surface, add entries to the registry
// — handlers pick them up automatically.
//
// Tests that need ad-hoc behavior (force a 500, swap in a
// crafted response, etc.) layer on `server.use(http.get(...))`
// per case; MSW prepends those, so they take precedence over
// these defaults.
//
// Path patterns use `*/` prefixes so the handlers match
// regardless of whether the SPA hits a same-origin URL
// (browser default) or an absolute URL (Node tests with a
// configured base).

import { http, HttpResponse } from 'msw';
import { errorEnvelope, successEnvelope } from './envelope';
import { mockRegistry, type MockRegistry } from './registry';

function notFound(message: string) {
  return HttpResponse.json(errorEnvelope('not_found', message), { status: 404 });
}

function badRequest(message: string) {
  return HttpResponse.json(errorEnvelope('bad_request', message), { status: 400 });
}

function pathParam(value: string | readonly string[] | undefined): string | null {
  if (typeof value === 'string') {
    return value;
  }
  if (Array.isArray(value) && typeof value[0] === 'string') {
    return value[0];
  }
  return null;
}

export function createHandlers(registry: MockRegistry = mockRegistry) {
  return [
    http.get('*/api/v1/version', () => HttpResponse.json(successEnvelope(registry.version))),
    http.get('*/api/v1/status', () => HttpResponse.json(successEnvelope(registry.status))),

    // Two-segment paths are registered first so the one-segment
    // actor handler doesn't shadow them.
    http.get('*/api/v1/actors/:id/statements', ({ params }) => {
      const id = pathParam(params['id']);
      if (id === null) return badRequest('missing actor id');
      if (registry.actors[id] === undefined) {
        return notFound(`actor not found: ${id}`);
      }
      return HttpResponse.json(successEnvelope(registry.statementsByActor[id] ?? []));
    }),

    http.get('*/api/v1/actors/:id/objects', ({ params }) => {
      const id = pathParam(params['id']);
      if (id === null) return badRequest('missing actor id');
      if (registry.actors[id] === undefined) {
        return notFound(`actor not found: ${id}`);
      }
      return HttpResponse.json(successEnvelope(registry.objectsByActor[id] ?? []));
    }),

    http.get('*/api/v1/actors/:id', ({ params }) => {
      const id = pathParam(params['id']);
      if (id === null) return badRequest('missing actor id');
      const actor = registry.actors[id];
      return actor === undefined
        ? notFound(`actor not found: ${id}`)
        : HttpResponse.json(successEnvelope(actor));
    }),

    http.get('*/api/v1/objects/:id', ({ params }) => {
      const id = pathParam(params['id']);
      if (id === null) return badRequest('missing object id');
      const object = registry.objects[id];
      return object === undefined
        ? notFound(`object not found: ${id}`)
        : HttpResponse.json(successEnvelope(object));
    }),

    http.get('*/api/v1/statements/:id', ({ params }) => {
      const id = pathParam(params['id']);
      if (id === null) return badRequest('missing statement id');
      const statement = registry.statements[id];
      return statement === undefined
        ? notFound(`statement not found: ${id}`)
        : HttpResponse.json(successEnvelope(statement));
    }),

    http.get('*/api/v1/branches/:object', ({ params }) => {
      const object = pathParam(params['object']);
      if (object === null) return badRequest('missing object id');
      // Listing endpoints return [] for known-but-empty
      // objects (matches the daemon). 404 when the object
      // itself is unknown.
      if (registry.objects[object] === undefined) {
        return notFound(`object not found: ${object}`);
      }
      return HttpResponse.json(successEnvelope(registry.branchTips[object] ?? []));
    }),

    http.get('*/api/v1/branches/:object/:name/latest', ({ params, request }) => {
      const object = pathParam(params['object']);
      const name = pathParam(params['name']);
      if (object === null || name === null) {
        return badRequest('missing object id or branch name');
      }
      const objectGenesis = registry.objects[object];
      if (objectGenesis === undefined) {
        return notFound(`object not found: ${object}`);
      }
      const url = new URL(request.url);
      const actor = url.searchParams.get('actor') ?? objectGenesis.body.created_by;
      const key = `${object}:${name}:${actor}`;
      const stmt = registry.branchLatest[key];
      return stmt === undefined
        ? notFound(`branch head not found: ${name} on ${object} by ${actor}`)
        : HttpResponse.json(successEnvelope(stmt));
    }),

    // Two-segment + version-specific path is registered first
    // so the one-segment list path doesn't shadow it.
    http.get('*/api/v1/version-tags/:object/:version', ({ params, request }) => {
      const object = pathParam(params['object']);
      const version = pathParam(params['version']);
      if (object === null || version === null) {
        return badRequest('missing object id or version');
      }
      const objectGenesis = registry.objects[object];
      if (objectGenesis === undefined) {
        return notFound(`object not found: ${object}`);
      }
      const url = new URL(request.url);
      const actor = url.searchParams.get('actor') ?? objectGenesis.body.created_by;
      const key = `${object}:${version}:${actor}`;
      const stmt = registry.versionTagLatest[key];
      return stmt === undefined
        ? notFound(`version tag not found: ${version} on ${object} by ${actor}`)
        : HttpResponse.json(successEnvelope(stmt));
    }),
    http.get('*/api/v1/version-tags/:object', ({ params }) => {
      const object = pathParam(params['object']);
      if (object === null) return badRequest('missing object id');
      if (registry.objects[object] === undefined) {
        return notFound(`object not found: ${object}`);
      }
      return HttpResponse.json(successEnvelope(registry.versionTagHeads[object] ?? []));
    }),

    http.get('*/api/v1/revisions/:object', ({ params }) => {
      const object = pathParam(params['object']);
      if (object === null) return badRequest('missing object id');
      if (registry.objects[object] === undefined) {
        return notFound(`object not found: ${object}`);
      }
      return HttpResponse.json(successEnvelope(registry.revisionHeads[object] ?? []));
    }),

    // Literal-prefix routes (`/trust/about/...`,
    // `/capabilities/for-object/...`) come first so the
    // single-capture routes don't swallow them.
    http.get('*/api/v1/trust/about/:of', ({ params }) => {
      const of = pathParam(params['of']);
      if (of === null) return badRequest('missing actor id');
      if (registry.actors[of] === undefined) {
        return notFound(`actor not found: ${of}`);
      }
      return HttpResponse.json(successEnvelope(registry.trustAbout[of] ?? []));
    }),
    http.get('*/api/v1/trust/:by/:of', ({ params }) => {
      const by = pathParam(params['by']);
      const of = pathParam(params['of']);
      if (by === null || of === null) return badRequest('missing actor id');
      const stmt = registry.trustPair[`${by}:${of}`];
      return stmt === undefined
        ? notFound(`no trust opinion from ${by} about ${of}`)
        : HttpResponse.json(successEnvelope(stmt));
    }),

    http.get('*/api/v1/capabilities/for-object/:object', ({ params }) => {
      const object = pathParam(params['object']);
      if (object === null) return badRequest('missing object id');
      if (registry.objects[object] === undefined) {
        return notFound(`object not found: ${object}`);
      }
      return HttpResponse.json(successEnvelope(registry.capabilitiesForObject[object] ?? []));
    }),
    http.get('*/api/v1/capabilities/:grantor', ({ params }) => {
      const grantor = pathParam(params['grantor']);
      if (grantor === null) return badRequest('missing grantor id');
      if (registry.actors[grantor] === undefined) {
        return notFound(`actor not found: ${grantor}`);
      }
      return HttpResponse.json(successEnvelope(registry.capabilitiesFromGrantor[grantor] ?? []));
    }),

    http.get('*/api/v1/blobs/:id', ({ params }) => {
      const id = pathParam(params['id']);
      if (id === null) return badRequest('missing blob id');
      const blob = registry.blobs[id];
      if (blob === undefined) {
        return notFound(`blob not found: ${id}`);
      }
      // Use ArrayBuffer so the body has a stable byte view; MSW
      // and the browser's fetch both accept it as the body
      // parameter.
      const bytes = new Uint8Array(blob);
      return new HttpResponse(bytes, {
        headers: {
          'Content-Type': 'application/octet-stream',
          'Content-Length': bytes.byteLength.toString(),
        },
      });
    }),

    http.get('*/api/v1/verify-object/:id', ({ params }) => {
      const id = pathParam(params['id']);
      if (id === null) return badRequest('missing object id');
      const result = registry.verifyObject[id];
      return result === undefined
        ? notFound(`object not found: ${id}`)
        : HttpResponse.json(successEnvelope(result));
    }),
  ];
}

/** Default handler set — what `setupBrowserMock` /
 * `setupNodeMock` use when callers don't pass an override. */
export const handlers = createHandlers();
