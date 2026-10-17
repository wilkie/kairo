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
import type { components } from '../generated/schema';
import { errorEnvelope, successEnvelope } from './envelope';
import { mockRegistry, type MockRegistry } from './registry';

function notFound(message: string) {
  return HttpResponse.json(errorEnvelope('not_found', message), { status: 404 });
}

function badRequest(message: string) {
  return HttpResponse.json(errorEnvelope('bad_request', message), { status: 400 });
}

function notInKeystore(actorId: string) {
  return HttpResponse.json(
    errorEnvelope('actor_not_in_keystore', `no local signing key for actor ${actorId}`),
    { status: 404 },
  );
}

function noSession() {
  return HttpResponse.json(errorEnvelope('no_session', 'no live session for that cookie'), {
    status: 401,
  });
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

// ---------------------------------------------------------------------------
// Mock session state
//
// MSW handlers run in a single page / Node process; one global
// map is enough for the inspector's needs. Tests that need
// isolation call `resetMockSessions()` in `beforeEach`.
//
// The cookie attributes mirror the daemon's
// `set_cookie_for` exactly: HttpOnly; SameSite=Strict; Path=/;
// Max-Age=<idle>.

const COOKIE_NAME = 'kairo_session';
const IDLE_TIMEOUT_SECS = 30 * 60;
const ABSOLUTE_TIMEOUT_SECS = 24 * 60 * 60;

interface MockSession {
  actorId: string;
  createdAt: Date;
  expiresAt: Date;
}

const sessions = new Map<string, MockSession>();

/**
 * The current "active session" the SPA is signed-in to.
 * MSW + browser mode does not fully round-trip `Set-Cookie`
 * → fetch headers when the cookie is `HttpOnly`, so the
 * handlers can't reliably authenticate session-mode requests
 * by reading `Cookie`. Instead the open/close handlers
 * update this module-level slot, and downstream handlers
 * (notably `POST /api/v1/statements`) fall back to it when
 * the cookie header is missing.
 *
 * The real daemon's auth path uses real cookies; this slot
 * is a mock-only convenience.
 */
let activeMockSessionId: string | null = null;

/**
 * Wipe every active mock session. Vitest test setup hooks
 * should call this so a leftover cookie from a prior test
 * can't bleed across runs.
 */
export function resetMockSessions(): void {
  sessions.clear();
  activeMockSessionId = null;
}

function generateSessionId(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function parseSessionCookie(cookieHeader: string | null): string | null {
  if (cookieHeader === null) return null;
  for (const pair of cookieHeader.split(';')) {
    const trimmed = pair.trim();
    if (trimmed.startsWith(`${COOKIE_NAME}=`)) {
      const value = trimmed.slice(COOKIE_NAME.length + 1);
      return value.length > 0 ? value : null;
    }
  }
  return null;
}

function rfc3339(date: Date): string {
  return date.toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function sessionInfoFor(session: MockSession): components['schemas']['SessionInfo'] {
  const idleExpires = new Date(Date.now() + IDLE_TIMEOUT_SECS * 1000);
  return {
    actor_id: session.actorId,
    created_at: rfc3339(session.createdAt),
    expires_at: rfc3339(session.expiresAt),
    idle_expires_at: rfc3339(idleExpires),
  };
}

function setCookieFor(id: string): string {
  return `${COOKIE_NAME}=${id}; HttpOnly; SameSite=Strict; Path=/; Max-Age=${IDLE_TIMEOUT_SECS}`;
}

function clearCookieValue(): string {
  return `${COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0`;
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

    http.get('*/api/v1/keystore/actors', () =>
      HttpResponse.json(successEnvelope({ actors: registry.keystoreActors })),
    ),

    http.post('*/api/v1/sessions', async ({ request }) => {
      let body: components['schemas']['OpenSessionRequest'];
      try {
        body = (await request.json()) as components['schemas']['OpenSessionRequest'];
      } catch {
        return badRequest('malformed request body');
      }
      const actorId = body?.actor_id;
      if (typeof actorId !== 'string' || actorId.length === 0) {
        return badRequest('invalid actor id');
      }
      // Same lightweight shape check the daemon applies: any
      // non-`z`-prefixed id is rejected as malformed before the
      // keystore lookup. Loose enough for the inspector — the
      // real daemon does a full `ActorId::from_str` parse.
      if (!actorId.startsWith('z')) {
        return badRequest(`invalid actor id ${actorId}`);
      }
      const inKeystore = registry.keystoreActors.some((entry) => entry.actor_id === actorId);
      if (!inKeystore) {
        return notInKeystore(actorId);
      }

      // Replace-on-reauth: if a prior session cookie rides in,
      // drop it before creating the new one.
      const prior = parseSessionCookie(request.headers.get('cookie'));
      if (prior !== null) {
        sessions.delete(prior);
      }

      const id = generateSessionId();
      const now = new Date();
      const session: MockSession = {
        actorId,
        createdAt: now,
        expiresAt: new Date(now.getTime() + ABSOLUTE_TIMEOUT_SECS * 1000),
      };
      sessions.set(id, session);
      activeMockSessionId = id;

      return new HttpResponse(JSON.stringify(successEnvelope(sessionInfoFor(session))), {
        status: 201,
        headers: {
          'Content-Type': 'application/json',
          'Set-Cookie': setCookieFor(id),
        },
      });
    }),

    http.get('*/api/v1/sessions/current', ({ request }) => {
      const id = parseSessionCookie(request.headers.get('cookie')) ?? activeMockSessionId;
      if (id === null) return noSession();
      const session = sessions.get(id);
      if (session === undefined) return noSession();
      return HttpResponse.json(successEnvelope(sessionInfoFor(session)));
    }),

    http.delete('*/api/v1/sessions/current', ({ request }) => {
      const id = parseSessionCookie(request.headers.get('cookie')) ?? activeMockSessionId;
      if (id !== null) sessions.delete(id);
      activeMockSessionId = null;
      return new HttpResponse(null, {
        status: 204,
        headers: { 'Set-Cookie': clearCookieValue() },
      });
    }),

    // `POST /api/v1/statements` — bifurcates on Authorization
    // header presence vs `kairo_session` cookie. The mock only
    // implements the cookie-mode (daemon-side body signing)
    // path; signed-envelope mode is exercised by the Rust
    // integration tests against the real daemon.
    http.post('*/api/v1/statements', async ({ request }) => handleSessionWrite(request, registry)),
  ];
}

async function handleSessionWrite(request: Request, registry: MockRegistry) {
  const sessionId = parseSessionCookie(request.headers.get('cookie')) ?? activeMockSessionId;
  if (sessionId === null) {
    return HttpResponse.json(
      errorEnvelope(
        'missing_authorization',
        'POST /api/v1/statements requires a kairo_session cookie',
      ),
      { status: 401 },
    );
  }
  const session = sessions.get(sessionId);
  if (session === undefined) {
    return HttpResponse.json(errorEnvelope('no_session', 'no live session for that cookie'), {
      status: 401,
    });
  }

  let payload: components['schemas']['SessionWriteRequest'];
  try {
    payload = (await request.json()) as components['schemas']['SessionWriteRequest'];
  } catch {
    return badRequest('request body is not valid JSON');
  }
  if (typeof payload?.body_kind !== 'string') {
    return badRequest('missing required string field `body_kind`');
  }

  switch (payload.body_kind) {
    case 'ActorTrust':
      return handleSessionActorTrust(session.actorId, payload.body, registry);
    case 'ObjectBranch':
      return handleSessionObjectBranch(session.actorId, payload.body, registry);
    case 'ObjectVersionTag':
      return handleSessionObjectVersionTag(session.actorId, payload.body, registry);
    case 'ActorCapabilityGrant':
      return handleSessionActorCapabilityGrant(session.actorId, payload.body, registry);
    case 'ActorCapabilityRevocation':
      return handleSessionActorCapabilityRevocation(session.actorId, payload.body, registry);
    default:
      return HttpResponse.json(
        errorEnvelope(
          'unsupported_statement_kind',
          `mock surface does not yet accept body_kind ${payload.body_kind}`,
        ),
        { status: 415 },
      );
  }
}

function handleSessionActorTrust(byActor: string, body: unknown, registry: MockRegistry) {
  if (typeof body !== 'object' || body === null) {
    return badRequest('invalid ActorTrust body shape');
  }
  const fields = body as {
    trusted_actor?: unknown;
    decision?: unknown;
    reason?: unknown;
  };
  if (typeof fields.trusted_actor !== 'string') {
    return badRequest('invalid ActorTrust body: trusted_actor must be a string');
  }
  if (
    fields.decision !== 'trusted' &&
    fields.decision !== 'untrusted' &&
    fields.decision !== null &&
    fields.decision !== undefined
  ) {
    return badRequest('invalid ActorTrust body: decision must be trusted | untrusted | null');
  }

  const trustedActor = fields.trusted_actor;
  const decision: 'trusted' | 'untrusted' | null =
    fields.decision === 'trusted'
      ? 'trusted'
      : fields.decision === 'untrusted'
        ? 'untrusted'
        : null;
  const createdAt = rfc3339(new Date());
  const statementId = `zMockSessionTrustStmt${Date.now().toString(36)}`;
  const reason = typeof fields.reason === 'string' ? fields.reason : undefined;

  const trustHead: components['schemas']['TrustHeadDto'] = {
    by_actor: byActor,
    trusted_actor: trustedActor,
    statement_id: statementId,
    created_at: createdAt,
    decision,
  };

  // Append into the registry so the trust-about query
  // re-fetches and renders the new opinion. Replace any prior
  // opinion from the same `by_actor` — matches the daemon's
  // latest-wins semantics.
  const prior = registry.trustAbout[trustedActor] ?? [];
  registry.trustAbout[trustedActor] = [
    ...prior.filter((entry) => entry.by_actor !== byActor),
    trustHead,
  ];

  // Also seed the statement under `getStatement` so navigating
  // to the new statement id resolves a body.
  const statementBody: Record<string, unknown> = {
    trusted_actor: trustedActor,
    decision,
  };
  if (reason !== undefined) statementBody.reason = reason;
  registry.statements[statementId] = {
    type: 'ActorTrust',
    version: 1,
    actor: byActor,
    subject: `actor:${trustedActor}`,
    created_at: createdAt,
    body: statementBody,
    signature: {
      actor: byActor,
      key_id: 'mock-key-1',
      algorithm: 'ed25519',
      bytes: 'A'.repeat(88),
    },
  };

  const result: components['schemas']['PutStatementResult'] = {
    statement_id: statementId,
    statement: registry.statements[statementId],
  };
  return HttpResponse.json(successEnvelope(result));
}

function handleSessionObjectBranch(byActor: string, body: unknown, registry: MockRegistry) {
  if (typeof body !== 'object' || body === null) {
    return badRequest('invalid ObjectBranch body shape');
  }
  const fields = body as {
    object?: unknown;
    name?: unknown;
    revision?: unknown;
    supersedes?: unknown;
  };
  if (typeof fields.object !== 'string') {
    return badRequest('invalid ObjectBranch body: object must be a string');
  }
  if (typeof fields.name !== 'string' || fields.name.length === 0) {
    return badRequest('invalid ObjectBranch body: name must be a non-empty string');
  }
  if (typeof fields.revision !== 'string') {
    return badRequest('invalid ObjectBranch body: revision must be a string');
  }
  if (
    fields.supersedes !== undefined &&
    fields.supersedes !== null &&
    typeof fields.supersedes !== 'string'
  ) {
    return badRequest('invalid ObjectBranch body: supersedes must be a string or null');
  }

  const objectId = fields.object;
  const objectGenesis = registry.objects[objectId];
  if (objectGenesis === undefined) {
    return HttpResponse.json(errorEnvelope('not_found', `object not found: ${objectId}`), {
      status: 404,
    });
  }

  // Root-authority check: only the object's creator may write
  // branches in the mock. The real daemon evaluates the full
  // capability chain (root authority for owners, plus
  // delegated grants); the mock approximates the common case.
  if (objectGenesis.body.created_by !== byActor) {
    return HttpResponse.json(
      errorEnvelope(
        'not_authorized',
        `actor ${byActor} lacks an ObjectBranch capability for object ${objectId}`,
      ),
      { status: 403 },
    );
  }

  const name = fields.name;
  const revision = fields.revision;
  const supersedes = typeof fields.supersedes === 'string' ? fields.supersedes : undefined;
  const createdAt = rfc3339(new Date());
  const statementId = `zMockSessionBranchStmt${Date.now().toString(36)}${Math.floor(Math.random() * 1e6).toString(36)}`;

  // Replace the matching `(actor, name)` tip; otherwise
  // append. Latest-wins per `(actor, name)` mirrors the
  // daemon's branch resolver.
  const priorTips = registry.branchTips[objectId] ?? [];
  const tip: components['schemas']['BranchTipDto'] = {
    actor: byActor,
    object: objectId,
    name,
    statement_id: statementId,
    created_at: createdAt,
  };
  const others = priorTips.filter((entry) => !(entry.actor === byActor && entry.name === name));
  registry.branchTips[objectId] = [...others, tip];

  // Seed the statement body so navigating to the new id
  // resolves the same JSON shape the daemon would emit.
  const statementBody: Record<string, unknown> = {
    object: objectId,
    name,
    revision,
  };
  if (supersedes !== undefined) statementBody.supersedes = supersedes;
  registry.statements[statementId] = {
    type: 'ObjectBranch',
    version: 1,
    actor: byActor,
    subject: `object:${objectId}`,
    created_at: createdAt,
    body: statementBody,
    signature: {
      actor: byActor,
      key_id: 'mock-key-1',
      algorithm: 'ed25519',
      bytes: 'A'.repeat(88),
    },
  };

  // Update the latest-branch lookup so `useLatestBranch`
  // resolves to the new tip without a follow-up fetch.
  const latestKey = `${objectId}:${name}:${byActor}`;
  registry.branchLatest[latestKey] = registry.statements[
    statementId
  ] as components['schemas']['ObjectBranchStatementJson'];

  const result: components['schemas']['PutStatementResult'] = {
    statement_id: statementId,
    statement: registry.statements[statementId],
  };
  return HttpResponse.json(successEnvelope(result));
}

// Strict semver 2.0.0 (semver.org's pattern) — mirrors the
// daemon's `SemverVersion::parse`, which rejects a leading `v`.
const STRICT_SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;

type CapabilityHead = components['schemas']['CapabilityHeadDto'];

/** Unrevoked capability heads on `objectId` whose grantee is
 * `actor` and whose grant lists `kind`. */
function mockHeldHeads(
  registry: MockRegistry,
  actor: string,
  objectId: string,
  kind: string,
): CapabilityHead[] {
  return (registry.capabilitiesForObject[objectId] ?? []).filter(
    (head) =>
      head.grantee === actor &&
      (head.revocation ?? null) === null &&
      (head.statement_kinds ?? []).includes(kind),
  );
}

/** Whether `actor` may write `kind` on `objectId` in the mock:
 * the object's creator (root authority), or the grantee of an
 * unrevoked seeded head listing the kind. The real daemon also
 * walks the chain back to the root and checks expiry; the mock
 * trusts the seeds. */
function mockMayWrite(
  registry: MockRegistry,
  actor: string,
  objectId: string,
  kind: string,
): boolean {
  if (registry.objects[objectId]?.body.created_by === actor) return true;
  return mockHeldHeads(registry, actor, objectId, kind).length > 0;
}

/** Whether `actor` may grant `kind` onward on `objectId`: root
 * authority, or a held head with `delegable` set — the mock's
 * approximation of `evaluate_capability_for_regrant`. */
function mockMayRegrant(
  registry: MockRegistry,
  actor: string,
  objectId: string,
  kind: string,
): boolean {
  if (registry.objects[objectId]?.body.created_by === actor) return true;
  return mockHeldHeads(registry, actor, objectId, kind).some((head) => head.delegable === true);
}

function mockStatementId(prefix: string): string {
  return `zMockSession${prefix}${Date.now().toString(36)}${Math.floor(Math.random() * 1e6).toString(36)}`;
}

function mockSignature(actor: string) {
  return { actor, key_id: 'mock-key-1', algorithm: 'ed25519', bytes: 'A'.repeat(88) };
}

const OBJECT_SCOPED_KINDS = ['ObjectRevision', 'ObjectBranch', 'ObjectVersionTag'];

function handleSessionActorCapabilityGrant(byActor: string, body: unknown, registry: MockRegistry) {
  if (typeof body !== 'object' || body === null) {
    return badRequest('invalid ActorCapabilityGrant body shape');
  }
  const fields = body as {
    grantee?: unknown;
    capability?: {
      scope?: { object?: unknown };
      statement_kinds?: unknown;
      delegable?: unknown;
      constraints?: unknown;
    };
    supersedes?: unknown;
  };
  if (typeof fields.grantee !== 'string' || !fields.grantee.startsWith('z')) {
    return badRequest('invalid ActorCapabilityGrant body: grantee must be an actor id');
  }
  const capability = fields.capability;
  const objectId = capability?.scope?.object;
  if (typeof objectId !== 'string') {
    return badRequest('invalid ActorCapabilityGrant body: scope must be { object }');
  }
  const kinds = capability?.statement_kinds;
  if (
    !Array.isArray(kinds) ||
    kinds.length === 0 ||
    !kinds.every((kind) => typeof kind === 'string' && OBJECT_SCOPED_KINDS.includes(kind))
  ) {
    return badRequest(
      'invalid ActorCapabilityGrant body: statement_kinds must be a non-empty list of object-scoped kinds',
    );
  }
  if (typeof capability?.delegable !== 'boolean') {
    return badRequest('invalid ActorCapabilityGrant body: delegable must be a boolean');
  }
  const constraints = Array.isArray(capability.constraints)
    ? (capability.constraints as components['schemas']['CapabilityConstraintJson'][])
    : [];
  if (
    fields.supersedes !== undefined &&
    fields.supersedes !== null &&
    typeof fields.supersedes !== 'string'
  ) {
    return badRequest('invalid ActorCapabilityGrant body: supersedes must be a string or null');
  }
  if (registry.objects[objectId] === undefined) {
    return HttpResponse.json(errorEnvelope('not_found', `object not found: ${objectId}`), {
      status: 404,
    });
  }
  const denied = (kinds as string[]).find(
    (kind) => !mockMayRegrant(registry, byActor, objectId, kind),
  );
  if (denied !== undefined) {
    return HttpResponse.json(
      errorEnvelope(
        'not_authorized',
        `actor ${byActor} may not grant ${denied} on object ${objectId} (not held with delegable=true)`,
      ),
      { status: 403 },
    );
  }

  // One chain per (grantor, grantee, scope): a successor must
  // supersede the current head (`CAPABILITIES.md` §5.1.1).
  const heads = registry.capabilitiesForObject[objectId] ?? [];
  const prior = heads.find((head) => head.grantor === byActor && head.grantee === fields.grantee);
  const supersedes = typeof fields.supersedes === 'string' ? fields.supersedes : undefined;
  if (prior !== undefined && supersedes !== prior.statement_id) {
    return badRequest(
      `grant on an existing (grantor, grantee, scope) chain must supersede ${prior.statement_id}`,
    );
  }

  const grantee = fields.grantee;
  const createdAt = rfc3339(new Date());
  const statementId = mockStatementId('GrantStmt');
  const head: CapabilityHead = {
    grantor: byActor,
    grantee,
    scope: { object: objectId },
    statement_id: statementId,
    created_at: createdAt,
    statement_kinds: kinds as string[],
    delegable: capability.delegable,
    constraints,
    revocation: null,
  };
  const replace = (list: CapabilityHead[] | undefined) => [
    ...(list ?? []).filter((entry) => !(entry.grantor === byActor && entry.grantee === grantee)),
    head,
  ];
  registry.capabilitiesForObject[objectId] = replace(registry.capabilitiesForObject[objectId]);
  registry.capabilitiesFromGrantor[byActor] = replace(registry.capabilitiesFromGrantor[byActor]);

  const grantBody: Record<string, unknown> = {
    grantee,
    capability: {
      scope: { object: objectId },
      statement_kinds: kinds,
      delegable: capability.delegable,
      constraints,
    },
  };
  if (supersedes !== undefined) grantBody.supersedes = supersedes;
  registry.statements[statementId] = {
    type: 'ActorCapabilityGrant',
    version: 1,
    actor: byActor,
    subject: `actor:${grantee}`,
    created_at: createdAt,
    body: grantBody,
    signature: mockSignature(byActor),
  };
  const result: components['schemas']['PutStatementResult'] = {
    statement_id: statementId,
    statement: registry.statements[statementId],
  };
  return HttpResponse.json(successEnvelope(result));
}

function handleSessionActorCapabilityRevocation(
  byActor: string,
  body: unknown,
  registry: MockRegistry,
) {
  if (typeof body !== 'object' || body === null) {
    return badRequest('invalid ActorCapabilityRevocation body shape');
  }
  const fields = body as { revoked_grant?: unknown; retroactive?: unknown; reason?: unknown };
  if (typeof fields.revoked_grant !== 'string') {
    return badRequest('invalid ActorCapabilityRevocation body: revoked_grant must be a string');
  }
  if (typeof fields.retroactive !== 'boolean') {
    return badRequest('invalid ActorCapabilityRevocation body: retroactive must be a boolean');
  }
  const revokedGrant = fields.revoked_grant;
  const heads = Object.values(registry.capabilitiesForObject).flat();
  const target = heads.find((head) => head.statement_id === revokedGrant);
  if (target === undefined) {
    return HttpResponse.json(errorEnvelope('not_found', `grant not found: ${revokedGrant}`), {
      status: 404,
    });
  }
  // Only the original grantor may revoke (`DECISIONS.md` §14).
  if (target.grantor !== byActor) {
    return HttpResponse.json(
      errorEnvelope(
        'not_authorized',
        `actor ${byActor} did not issue grant ${revokedGrant}; only its grantor may revoke it`,
      ),
      { status: 403 },
    );
  }

  const createdAt = rfc3339(new Date());
  const statementId = mockStatementId('RevocationStmt');
  const revocation = {
    statement_id: statementId,
    retroactive: fields.retroactive,
    created_at: createdAt,
  };
  // Heads are shared objects across both indices; most-
  // restrictive wins, so a retroactive revocation is sticky.
  for (const head of heads.concat(Object.values(registry.capabilitiesFromGrantor).flat())) {
    if (head.statement_id === revokedGrant && head.revocation?.retroactive !== true) {
      head.revocation = revocation;
    }
  }

  const revocationBody: Record<string, unknown> = {
    revoked_grant: revokedGrant,
    retroactive: fields.retroactive,
  };
  if (typeof fields.reason === 'string' && fields.reason.length > 0) {
    revocationBody.reason = fields.reason;
  }
  registry.statements[statementId] = {
    type: 'ActorCapabilityRevocation',
    version: 1,
    actor: byActor,
    subject: `statement:${revokedGrant}`,
    created_at: createdAt,
    body: revocationBody,
    signature: mockSignature(byActor),
  };
  const result: components['schemas']['PutStatementResult'] = {
    statement_id: statementId,
    statement: registry.statements[statementId],
  };
  return HttpResponse.json(successEnvelope(result));
}

function handleSessionObjectVersionTag(byActor: string, body: unknown, registry: MockRegistry) {
  if (typeof body !== 'object' || body === null) {
    return badRequest('invalid ObjectVersionTag body shape');
  }
  const fields = body as {
    object?: unknown;
    version?: unknown;
    target?: unknown;
    supersedes?: unknown;
  };
  if (typeof fields.object !== 'string') {
    return badRequest('invalid ObjectVersionTag body: object must be a string');
  }
  if (typeof fields.version !== 'string' || !STRICT_SEMVER.test(fields.version)) {
    return badRequest(
      `invalid ObjectVersionTag body: version ${JSON.stringify(fields.version)} is not strict semver`,
    );
  }
  // The inspector only binds; revocations (no `target`) aren't
  // modelled by the mock yet.
  if (typeof fields.target !== 'string') {
    return badRequest('invalid ObjectVersionTag body: target must be a string');
  }
  if (
    fields.supersedes !== undefined &&
    fields.supersedes !== null &&
    typeof fields.supersedes !== 'string'
  ) {
    return badRequest('invalid ObjectVersionTag body: supersedes must be a string or null');
  }

  const objectId = fields.object;
  if (registry.objects[objectId] === undefined) {
    return HttpResponse.json(errorEnvelope('not_found', `object not found: ${objectId}`), {
      status: 404,
    });
  }
  if (!mockMayWrite(registry, byActor, objectId, 'ObjectVersionTag')) {
    return HttpResponse.json(
      errorEnvelope(
        'not_authorized',
        `actor ${byActor} lacks an ObjectVersionTag capability for object ${objectId}`,
      ),
      { status: 403 },
    );
  }

  const version = fields.version;
  const target = fields.target;
  const supersedes = typeof fields.supersedes === 'string' ? fields.supersedes : undefined;
  const createdAt = rfc3339(new Date());
  const statementId = `zMockSessionTagStmt${Date.now().toString(36)}${Math.floor(Math.random() * 1e6).toString(36)}`;

  // Latest-wins per `(actor, version)`, like the daemon's tag
  // resolver: replace the matching head, otherwise append.
  const priorHeads = registry.versionTagHeads[objectId] ?? [];
  const head: components['schemas']['VersionTagHeadDto'] = {
    actor: byActor,
    object: objectId,
    version,
    statement_id: statementId,
    created_at: createdAt,
  };
  const others = priorHeads.filter(
    (entry) => !(entry.actor === byActor && entry.version === version),
  );
  registry.versionTagHeads[objectId] = [...others, head];

  const statementBody: Record<string, unknown> = { object: objectId, version, target };
  if (supersedes !== undefined) statementBody.supersedes = supersedes;
  registry.statements[statementId] = {
    type: 'ObjectVersionTag',
    version: 1,
    actor: byActor,
    subject: `object:${objectId}`,
    created_at: createdAt,
    body: statementBody,
    signature: {
      actor: byActor,
      key_id: 'mock-key-1',
      algorithm: 'ed25519',
      bytes: 'A'.repeat(88),
    },
  };
  registry.versionTagLatest[`${objectId}:${version}:${byActor}`] = registry.statements[
    statementId
  ] as components['schemas']['ObjectVersionTagStatementJson'];

  const result: components['schemas']['PutStatementResult'] = {
    statement_id: statementId,
    statement: registry.statements[statementId],
  };
  return HttpResponse.json(successEnvelope(result));
}

/** Default handler set — what `setupBrowserMock` /
 * `setupNodeMock` use when callers don't pass an override. */
export const handlers = createHandlers();
