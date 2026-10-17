// Contract tests for the typed api-client.
//
// Each test drives one method on `KairoApiClient` against an
// MSW node-side server and asserts that the envelope unwrap +
// error mapping land at the documented public shape:
//
// - 2xx success envelope → method returns the inner result.
// - non-2xx failure envelope → method throws
//   `KairoApiClientError` with `kind: 'daemon'` and the
//   wire-stable error code.
// - malformed body → `KairoApiClientError` with `kind: 'decode'`.
//
// All paths in these tests use the `http://daemon` base because
// MSW node intercepts on absolute URLs, and the api-client
// builds absolute URLs once `baseUrl` is set.

import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { http, HttpResponse } from 'msw';
import { setupMockServer } from '../src/mock/node';
import { fixtures, mockIds } from '../src/mock/fixtures';
import { errorEnvelope, successEnvelope } from '../src/mock/envelope';
import { resetMockSessions } from '../src/mock/handlers';
import { createKairoClient, KairoApiClientError } from '../src/index';

const BASE_URL = 'http://daemon';

const server = setupMockServer();

beforeAll(() => {
  server.listen({ onUnhandledRequest: 'error' });
});
afterEach(() => {
  server.resetHandlers();
});
afterAll(() => {
  server.close();
});

const client = createKairoClient({ baseUrl: BASE_URL });

describe('typed read methods unwrap the success envelope', () => {
  it('getVersion returns VersionInfo', async () => {
    const result = await client.getVersion();
    expect(result.api_version).toBe('v1');
    expect(result.daemon_version).toBe(fixtures.version.daemon_version);
  });

  it('getStatus returns StatusInfo', async () => {
    const result = await client.getStatus();
    expect(result.daemon_running).toBe(true);
    expect(result.pid).toBe(fixtures.status.pid);
  });

  it('getActor returns ActorGenesisJson', async () => {
    const result = await client.getActor(mockIds.actor);
    expect(result.type).toBe('ActorGenesis');
    expect(result.actor_kind).toBe(fixtures.actor.actor_kind);
  });

  it('getObject returns ObjectGenesisStatementJson', async () => {
    const result = await client.getObject(mockIds.object);
    expect(result.type).toBe('ObjectGenesis');
  });

  it('getStatement returns the polymorphic body verbatim', async () => {
    const result = await client.getStatement(mockIds.revisionStmt);
    expect(typeof result).toBe('object');
  });

  it('listBranches returns the branch-tip array', async () => {
    const result = await client.listBranches(mockIds.object);
    expect(result.length).toBeGreaterThan(0);
    expect(result.map((tip) => tip.name)).toContain('head');
  });

  it('getLatestBranch returns the chain-leaf branch statement', async () => {
    const result = await client.getLatestBranch(mockIds.object, 'head');
    expect(result.body.name).toBe('head');
  });

  it('getLatestVersionTag returns the chain-leaf tag statement', async () => {
    const result = await client.getLatestVersionTag(mockIds.object, '1.0.0');
    expect(result.body.version).toBe('1.0.0');
  });

  it('getTrust returns the trust statement', async () => {
    const result = await client.getTrust(mockIds.actor, mockIds.actor);
    expect(result.body.decision).toBe('trusted');
  });

  it('listCapabilitiesFromGrantor returns capability heads', async () => {
    const result = await client.listCapabilitiesFromGrantor(mockIds.actor);
    expect(result).toHaveLength(1);
    expect(result[0]?.grantor).toBe(mockIds.actor);
  });

  it('listVersionTags returns the version-tag head array', async () => {
    const result = await client.listVersionTags(mockIds.object);
    expect(result.length).toBeGreaterThan(0);
    expect(result.map((tag) => tag.version)).toContain('1.0.0');
  });

  it('listRevisions returns the revision head array', async () => {
    const result = await client.listRevisions(mockIds.object);
    expect(result.length).toBeGreaterThan(0);
    expect(result.map((rev) => rev.statement_id)).toContain(mockIds.revisionStmt);
    // The initial (no-parents) revision must always be present.
    expect(result.some((rev) => rev.parents.length === 0)).toBe(true);
  });

  it('listStatementsByActor returns the per-actor signed statement list', async () => {
    const result = await client.listStatementsByActor(mockIds.actor);
    expect(result.length).toBeGreaterThan(0);
    for (const entry of result) {
      expect(entry.actor).toBe(mockIds.actor);
    }
    // The seeded fixture covers multiple kinds — assert the big
    // ones (revision, branch, version-tag) are all present.
    const kinds = new Set(result.map((entry) => entry.kind));
    expect(kinds).toContain('ObjectRevision');
    expect(kinds).toContain('ObjectBranch');
    expect(kinds).toContain('ObjectVersionTag');
  });

  it('listObjectsByActor returns the per-actor created-objects list', async () => {
    const result = await client.listObjectsByActor(mockIds.actor);
    expect(result.length).toBeGreaterThan(0);
    for (const entry of result) {
      expect(entry.actor).toBe(mockIds.actor);
      expect(entry.object_kind).toBe('kairo/object');
    }
    // The seeded Alice fixture creates Alpha at minimum.
    expect(result.map((entry) => entry.object_id)).toContain(mockIds.object);
  });

  it('listTrustAbout returns trust heads expressed about an actor', async () => {
    const result = await client.listTrustAbout(mockIds.actor);
    expect(result.length).toBeGreaterThan(0);
    for (const head of result) {
      expect(head.trusted_actor).toBe(mockIds.actor);
    }
    // The seeded "trusted" opinion (Bob → Alice) is the
    // canonical happy-path entry.
    expect(result.map((head) => head.decision)).toContain('trusted');
  });

  it('listCapabilitiesForObject returns capability heads scoped to the object', async () => {
    const result = await client.listCapabilitiesForObject(mockIds.object);
    // The seeded fixture exercises a two-hop delegation chain
    // (Alice → Bob → Carol on Alpha), so the object's head list
    // carries both grants. Assert membership rather than a
    // tighter count to keep this test stable as the fixture
    // grows.
    expect(result.length).toBeGreaterThanOrEqual(2);
    for (const head of result) {
      expect(head.scope).toMatchObject({ object: mockIds.object });
    }
    const grantors = new Set(result.map((h) => h.grantor));
    expect(grantors).toContain(mockIds.alice);
    expect(grantors).toContain(mockIds.bob);
  });

  it('verifyObject returns ValidationResult', async () => {
    const result = await client.verifyObject(mockIds.object);
    expect(result.status).toBe('indeterminate');
    expect(result.issues.length).toBeGreaterThan(0);
  });

  it('getBlob returns a Blob with the fixture bytes', async () => {
    const result = await client.getBlob(mockIds.blob);
    const arrayBuffer = await result.arrayBuffer();
    expect(new Uint8Array(arrayBuffer)).toEqual(new Uint8Array([0xde, 0xad, 0xbe, 0xef]));
  });
});

describe('error envelope mapping', () => {
  it('maps a 404 not_found body to KairoApiClientError kind:daemon', async () => {
    server.use(
      http.get('*/api/v1/objects/:id', () =>
        HttpResponse.json(errorEnvelope('not_found', 'object not found'), { status: 404 }),
      ),
    );

    await expect(client.getObject('kairo:object:zMissing')).rejects.toMatchObject({
      detail: { kind: 'daemon', code: 'not_found', status: 404 },
    });
  });

  it('maps a 400 bad_request body to KairoApiClientError', async () => {
    server.use(
      http.get('*/api/v1/objects/:id', () =>
        HttpResponse.json(errorEnvelope('bad_request', 'malformed object id'), { status: 400 }),
      ),
    );

    const error = await client.getObject('not-an-id').catch((e: unknown) => e);
    expect(error).toBeInstanceOf(KairoApiClientError);
    expect((error as KairoApiClientError).detail).toMatchObject({
      kind: 'daemon',
      code: 'bad_request',
      status: 400,
    });
  });

  it('maps malformed JSON to KairoApiClientError kind:decode', async () => {
    server.use(
      http.get('*/api/v1/version', () =>
        HttpResponse.json({ ok: 'maybe', schema: 'kairo.api.result.v1', result: {} }),
      ),
    );

    await expect(client.getVersion()).rejects.toMatchObject({
      detail: { kind: 'decode' },
    });
  });

  it('maps a non-envelope JSON body to kind:decode', async () => {
    server.use(http.get('*/api/v1/version', () => HttpResponse.json([1, 2, 3])));

    await expect(client.getVersion()).rejects.toMatchObject({
      detail: { kind: 'decode' },
    });
  });
});

describe('session endpoints', () => {
  afterEach(() => {
    resetMockSessions();
  });

  it('listKeystoreActors returns the seeded actors', async () => {
    const actors = await client.listKeystoreActors();
    expect(actors.length).toBeGreaterThan(0);
    expect(actors.map((a) => a.actor_id)).toContain(mockIds.actor);
  });

  it('openSession returns SessionInfo for a known actor', async () => {
    const info = await client.openSession(mockIds.actor);
    expect(info.actor_id).toBe(mockIds.actor);
    expect(typeof info.created_at).toBe('string');
    expect(typeof info.expires_at).toBe('string');
    expect(typeof info.idle_expires_at).toBe('string');
  });

  it('openSession rejects an actor missing from the keystore', async () => {
    const unknown = 'zUnknownActorXxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx';
    const error = await client.openSession(unknown).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(KairoApiClientError);
    expect((error as KairoApiClientError).detail).toMatchObject({
      kind: 'daemon',
      code: 'actor_not_in_keystore',
      status: 404,
    });
  });

  it('openSession rejects a malformed actor id', async () => {
    const error = await client.openSession('not-a-valid-id').catch((e: unknown) => e);
    expect(error).toBeInstanceOf(KairoApiClientError);
    expect((error as KairoApiClientError).detail).toMatchObject({
      kind: 'daemon',
      code: 'bad_request',
      status: 400,
    });
  });

  it('getCurrentSession returns null when no session cookie is present', async () => {
    const info = await client.getCurrentSession();
    expect(info).toBeNull();
  });

  it('closeSession succeeds even with no active session', async () => {
    await expect(client.closeSession()).resolves.toBeUndefined();
  });
});

describe('session writes: ObjectVersionTag', () => {
  afterEach(() => {
    resetMockSessions();
  });

  // Non-mutating paths only: a successful write would mutate the
  // shared mock registry other tests read.

  it('rejects an actor with neither root authority nor a grant', async () => {
    // Beta is Alice-created with no capability heads.
    await client.openSession(mockIds.bob);
    const error = await client
      .writeViaSession('ObjectVersionTag', {
        object: mockIds.beta,
        version: '1.0.0',
        target: mockIds.alphaRev3Stmt,
      })
      .catch((e: unknown) => e);
    expect(error).toBeInstanceOf(KairoApiClientError);
    expect((error as KairoApiClientError).detail).toMatchObject({
      kind: 'daemon',
      code: 'not_authorized',
      status: 403,
    });
  });

  it('rejects a non-strict semver version', async () => {
    await client.openSession(mockIds.alice);
    const error = await client
      .writeViaSession('ObjectVersionTag', {
        object: mockIds.alpha,
        version: 'v1.0.0',
        target: mockIds.alphaRev3Stmt,
      })
      .catch((e: unknown) => e);
    expect(error).toBeInstanceOf(KairoApiClientError);
    expect((error as KairoApiClientError).detail).toMatchObject({
      kind: 'daemon',
      code: 'bad_request',
      status: 400,
    });
  });
});

describe('envelope round-trip', () => {
  it('successEnvelope wraps a value the client can unwrap', async () => {
    server.use(
      http.get('*/api/v1/version', () =>
        HttpResponse.json(
          successEnvelope({
            daemon_version: 'custom',
            api_version: 'v1',
            core_version: 'custom',
            store_version: 'custom',
          }),
        ),
      ),
    );

    const result = await client.getVersion();
    expect(result.daemon_version).toBe('custom');
  });
});
