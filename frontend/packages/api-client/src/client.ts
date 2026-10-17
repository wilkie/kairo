// Imperative typed client. Each method maps to one daemon
// endpoint, sends a `ky` request, and unwraps the envelope into
// the inner `T` declared by the OpenAPI annotations.
//
// Slice 6 ships all 12 endpoints. The blob endpoint is the only
// non-JSON path: callers receive a `Blob` (browser) or
// `ReadableStream` for streaming consumers.

import { HTTPError, type KyInstance, type SearchParamsOption } from 'ky';
import type { components } from './generated/schema';
import { EnvelopeError, unwrapEnvelope } from './envelope';
import { KairoApiClientError } from './error';
import { createTransport, type TransportOptions } from './transport';

// Re-export schema types under stable, ergonomic names. Public
// callers (web-client, object-model) reach for these instead of
// digging into `components['schemas']`.
export type VersionInfo = components['schemas']['VersionInfo'];
export type StatusInfo = components['schemas']['StatusInfo'];
export type ActorGenesisJson = components['schemas']['ActorGenesisJson'];
export type KeystoreActorDto = components['schemas']['KeystoreActorDto'];
export type KeystoreActorsResponse = components['schemas']['KeystoreActorsResponse'];
export type OpenSessionRequest = components['schemas']['OpenSessionRequest'];
export type SessionInfo = components['schemas']['SessionInfo'];
export type SessionWriteRequest = components['schemas']['SessionWriteRequest'];
export type PutStatementResult = components['schemas']['PutStatementResult'];
export type ObjectGenesisStatementJson = components['schemas']['ObjectGenesisStatementJson'];
export type ObjectBranchStatementJson = components['schemas']['ObjectBranchStatementJson'];
export type ObjectVersionTagStatementJson = components['schemas']['ObjectVersionTagStatementJson'];
export type ActorTrustStatementJson = components['schemas']['ActorTrustStatementJson'];
export type BranchTipDto = components['schemas']['BranchTipDto'];
export type CapabilityHeadDto = components['schemas']['CapabilityHeadDto'];
export type VersionTagHeadDto = components['schemas']['VersionTagHeadDto'];
export type RevisionHeadDto = components['schemas']['RevisionHeadDto'];
export type RevisionLogPage = components['schemas']['RevisionLogPage'];
export type TrustHeadDto = components['schemas']['TrustHeadDto'];
export type StatementByActorDto = components['schemas']['StatementByActorDto'];
export type ProvenanceDto = components['schemas']['ProvenanceDto'];
export type ProvenanceEntryDto = components['schemas']['ProvenanceEntryDto'];
export type ObjectByActorDto = components['schemas']['ObjectByActorDto'];
export type ValidationResult = components['schemas']['ValidationResult'];
export type ValidationStatus = components['schemas']['ValidationStatus'];
export type ValidationIssue = components['schemas']['ValidationIssue'];
export type ValidationIssueSeverity = components['schemas']['ValidationIssueSeverity'];

/**
 * Polymorphic statement-by-id payload. The daemon serves any
 * signed statement variant under `/api/v1/statements/{id}`; the
 * kind discriminator lives in the body itself (`type` field on
 * each `*StatementJson` shape). Callers either match on `type`
 * or feed the value through a typed JSON guard.
 */
export type StatementValue = unknown;

/** Optional `?actor=<id>` for branch / version-tag lookups.
 * Defaults to the object's `created_by` when omitted. */
export interface BranchActorOption {
  actor?: string;
}

export interface KairoApiClient {
  /** `GET /api/v1/version`. */
  getVersion(): Promise<VersionInfo>;
  /** `GET /api/v1/status`. */
  getStatus(): Promise<StatusInfo>;
  /** `GET /api/v1/actors/{id}`. */
  getActor(actorId: string): Promise<ActorGenesisJson>;
  /** `GET /api/v1/actors/{id}/statements` — every signed statement
   * authored by the actor (ObjectGenesis excluded server-side). */
  listStatementsByActor(actorId: string): Promise<StatementByActorDto[]>;
  /** `GET /api/v1/actors/{id}/objects` — every object whose
   * `ObjectGenesis.created_by` is this actor. */
  listObjectsByActor(actorId: string): Promise<ObjectByActorDto[]>;
  /** `GET /api/v1/objects/{id}`. */
  getObject(objectId: string): Promise<ObjectGenesisStatementJson>;
  /** `GET /api/v1/provenance/{record}` — which federation peers served
   * a record. `record` is a typed reference (`statement:<id>`,
   * `object:<id>`, `actor:<id>`, `blob:<id>`); `entries` is empty
   * for local records. */
  getProvenance(record: string): Promise<ProvenanceDto>;
  /** `GET /api/v1/statements/{id}` — polymorphic by kind. */
  getStatement(statementId: string): Promise<StatementValue>;
  /** `GET /api/v1/branches/{object}` — list of `(actor, name)` heads. */
  listBranches(objectId: string): Promise<BranchTipDto[]>;
  /** `GET /api/v1/branches/{object}/{name}/latest`. */
  getLatestBranch(
    objectId: string,
    name: string,
    opts?: BranchActorOption,
  ): Promise<ObjectBranchStatementJson>;
  /** `GET /api/v1/version-tags/{object}` — list of `(actor, version)` heads. */
  listVersionTags(objectId: string): Promise<VersionTagHeadDto[]>;
  /** `GET /api/v1/version-tags/{object}/{version}`. */
  getLatestVersionTag(
    objectId: string,
    version: string,
    opts?: BranchActorOption,
  ): Promise<ObjectVersionTagStatementJson>;
  /** `GET /api/v1/objects/{id}/log` — one page of the object's
   * revision history, newest first, walked along parent links from
   * every revision this node holds or from `from`. */
  getRevisionLog(objectId: string, options?: RevisionLogOptions): Promise<RevisionLogPage>;
  /** `GET /api/v1/trust/{by}/{of}`. */
  getTrust(byActor: string, ofActor: string): Promise<ActorTrustStatementJson>;
  /** `GET /api/v1/trust/about/{of}` — list of opinions expressed about an actor. */
  listTrustAbout(ofActor: string): Promise<TrustHeadDto[]>;
  /** `GET /api/v1/capabilities/{grantor}`. */
  listCapabilitiesFromGrantor(grantorId: string): Promise<CapabilityHeadDto[]>;
  /** `GET /api/v1/capabilities/for-object/{object}` — capability heads scoped to an object, hydrated with each head's scope. */
  listCapabilitiesForObject(objectId: string): Promise<CapabilityHeadDto[]>;
  /**
   * `GET /api/v1/blobs/{id}` — raw `application/octet-stream`.
   * Returns a `Blob` to keep the browser-side surface tractable;
   * streaming consumers can call `.stream()` on the result.
   */
  getBlob(blobId: string): Promise<Blob>;
  /** `GET /api/v1/verify-object/{id}`. */
  verifyObject(objectId: string): Promise<ValidationResult>;

  /** `GET /api/v1/keystore/actors` — local keystore listing. */
  listKeystoreActors(): Promise<KeystoreActorDto[]>;
  /**
   * `POST /api/v1/sessions` — open a daemon-side session bound
   * to `actorId`. The browser cookie jar persists the
   * `kairo_session` cookie automatically; subsequent
   * `getCurrentSession` / `closeSession` calls ride that cookie
   * via the standard `same-origin` fetch behaviour.
   */
  openSession(actorId: string): Promise<SessionInfo>;
  /**
   * `GET /api/v1/sessions/current` — returns the active
   * session, or `null` when the cookie is missing / expired.
   * Wrapping 401 as `null` keeps consuming hooks /
   * components branch-free.
   */
  getCurrentSession(): Promise<SessionInfo | null>;
  /** `DELETE /api/v1/sessions/current` — terminate the active
   * session. Idempotent: succeeds even when no session is live. */
  closeSession(): Promise<void>;

  /**
   * `POST /api/v1/statements` (cookie-mode) — daemon-side body
   * signing per `PHASE_2_WEB_CLIENT_AUTH.md` §2.3.
   *
   * The browser supplies the unsigned body fields; the daemon
   * fetches the session's signing key, stamps `created_at`,
   * builds and signs the envelope, then dispatches through the
   * same per-kind capability check + persistence path used by
   * the `Authorization: KairoActor1` mode.
   *
   * Returns the persisted statement_id plus the echoed
   * statement body the daemon stored.
   */
  writeViaSession(bodyKind: string, body: unknown): Promise<PutStatementResult>;
}

/** Options for [`KairoApiClient.getRevisionLog`]. */
export interface RevisionLogOptions {
  /** Revision statement ids to walk from; defaults to every revision
   * this node holds for the object. */
  from?: string[];
  /** Page size, 1–500 (daemon default 50). */
  limit?: number;
  /** `next_cursor` from the previous page. */
  cursor?: string;
}

export interface CreateKairoClientOptions extends Partial<TransportOptions> {
  /**
   * Pre-built `ky` instance to use instead of constructing one
   * from `baseUrl`. Useful for tests that swap in a fetch mock,
   * or for hosts that need custom hooks (auth, tracing).
   */
  http?: KyInstance;
}

export function createKairoClient(opts: CreateKairoClientOptions = {}): KairoApiClient {
  const http: KyInstance =
    opts.http ??
    createTransport({
      baseUrl: opts.baseUrl ?? '',
      ...(opts.timeoutMs !== undefined ? { timeoutMs: opts.timeoutMs } : {}),
      ...(opts.hooks !== undefined ? { hooks: opts.hooks } : {}),
    });

  return {
    async getVersion() {
      return getJson<VersionInfo>(http, 'api/v1/version');
    },
    async getStatus() {
      return getJson<StatusInfo>(http, 'api/v1/status');
    },
    async getActor(actorId) {
      return getJson<ActorGenesisJson>(http, `api/v1/actors/${encodeURIComponent(actorId)}`);
    },
    async getProvenance(record) {
      return getJson<ProvenanceDto>(http, `api/v1/provenance/${encodeURIComponent(record)}`);
    },
    async listStatementsByActor(actorId) {
      return getJson<StatementByActorDto[]>(
        http,
        `api/v1/actors/${encodeURIComponent(actorId)}/statements`,
      );
    },
    async listObjectsByActor(actorId) {
      return getJson<ObjectByActorDto[]>(
        http,
        `api/v1/actors/${encodeURIComponent(actorId)}/objects`,
      );
    },
    async getObject(objectId) {
      return getJson<ObjectGenesisStatementJson>(
        http,
        `api/v1/objects/${encodeURIComponent(objectId)}`,
      );
    },
    async getStatement(statementId) {
      return getJson<StatementValue>(http, `api/v1/statements/${encodeURIComponent(statementId)}`);
    },
    async listBranches(objectId) {
      return getJson<BranchTipDto[]>(http, `api/v1/branches/${encodeURIComponent(objectId)}`);
    },
    async getLatestBranch(objectId, name, opts) {
      return getJson<ObjectBranchStatementJson>(
        http,
        `api/v1/branches/${encodeURIComponent(objectId)}/${encodeURIComponent(name)}/latest`,
        actorQuery(opts),
      );
    },
    async listVersionTags(objectId) {
      return getJson<VersionTagHeadDto[]>(
        http,
        `api/v1/version-tags/${encodeURIComponent(objectId)}`,
      );
    },
    async getLatestVersionTag(objectId, version, opts) {
      return getJson<ObjectVersionTagStatementJson>(
        http,
        `api/v1/version-tags/${encodeURIComponent(objectId)}/${encodeURIComponent(version)}`,
        actorQuery(opts),
      );
    },
    async getRevisionLog(objectId, options = {}) {
      const searchParams: Record<string, string> = {};
      if (options.from !== undefined && options.from.length > 0) {
        searchParams['from'] = options.from.join(',');
      }
      if (options.limit !== undefined) searchParams['limit'] = String(options.limit);
      if (options.cursor !== undefined) searchParams['cursor'] = options.cursor;
      return getJson<RevisionLogPage>(
        http,
        `api/v1/objects/${encodeURIComponent(objectId)}/log`,
        searchParams,
      );
    },
    async getTrust(byActor, ofActor) {
      return getJson<ActorTrustStatementJson>(
        http,
        `api/v1/trust/${encodeURIComponent(byActor)}/${encodeURIComponent(ofActor)}`,
      );
    },
    async listTrustAbout(ofActor) {
      return getJson<TrustHeadDto[]>(http, `api/v1/trust/about/${encodeURIComponent(ofActor)}`);
    },
    async listCapabilitiesFromGrantor(grantorId) {
      return getJson<CapabilityHeadDto[]>(
        http,
        `api/v1/capabilities/${encodeURIComponent(grantorId)}`,
      );
    },
    async listCapabilitiesForObject(objectId) {
      return getJson<CapabilityHeadDto[]>(
        http,
        `api/v1/capabilities/for-object/${encodeURIComponent(objectId)}`,
      );
    },
    async getBlob(blobId) {
      try {
        return await http.get(`api/v1/blobs/${encodeURIComponent(blobId)}`).blob();
      } catch (error) {
        throw await mapTransportError(error);
      }
    },
    async verifyObject(objectId) {
      return getJson<ValidationResult>(
        http,
        `api/v1/verify-object/${encodeURIComponent(objectId)}`,
      );
    },
    async listKeystoreActors() {
      const response = await getJson<KeystoreActorsResponse>(http, 'api/v1/keystore/actors');
      return response.actors;
    },
    async openSession(actorId) {
      return postJson<SessionInfo>(http, 'api/v1/sessions', { actor_id: actorId });
    },
    async getCurrentSession() {
      try {
        return await getJson<SessionInfo>(http, 'api/v1/sessions/current');
      } catch (error) {
        if (
          error instanceof KairoApiClientError &&
          error.detail.kind === 'daemon' &&
          error.detail.code === 'no_session'
        ) {
          return null;
        }
        throw error;
      }
    },
    async closeSession() {
      try {
        await http.delete('api/v1/sessions/current').text();
      } catch (error) {
        throw await mapTransportError(error);
      }
    },
    async writeViaSession(bodyKind, body) {
      // The OpenAPI-generated `SessionWriteRequest.body` types
      // as `Record<string, never>` because the daemon's schema
      // marks the field with `value_type = Object` (polymorphic
      // body). We bypass the generated shape here and post the
      // payload directly — the daemon's per-kind decoder is the
      // source of truth.
      return postJson<PutStatementResult>(http, 'api/v1/statements', {
        body_kind: bodyKind,
        body,
      });
    },
  };
}

function actorQuery(opts: BranchActorOption | undefined): SearchParamsOption | undefined {
  if (opts?.actor === undefined) {
    return undefined;
  }
  return { actor: opts.actor };
}

async function getJson<T>(
  http: KyInstance,
  path: string,
  searchParams?: SearchParamsOption,
): Promise<T> {
  try {
    const body = await http
      .get(path, searchParams !== undefined ? { searchParams } : {})
      .json<unknown>();
    return unwrapEnvelope<T>(body);
  } catch (error) {
    throw await mapTransportError(error);
  }
}

async function postJson<T>(http: KyInstance, path: string, json: unknown): Promise<T> {
  try {
    const body = await http.post(path, { json }).json<unknown>();
    return unwrapEnvelope<T>(body);
  } catch (error) {
    throw await mapTransportError(error);
  }
}

async function mapTransportError(error: unknown): Promise<KairoApiClientError> {
  if (error instanceof KairoApiClientError) {
    return error;
  }
  if (error instanceof EnvelopeError) {
    if (error.code === 'decode_error') {
      return new KairoApiClientError({
        kind: 'decode',
        message: error.message,
      });
    }
    return new KairoApiClientError({
      kind: 'daemon',
      code: error.code,
      message: error.message,
      status: error.httpStatus,
    });
  }
  if (error instanceof HTTPError) {
    // Try to read the daemon's error envelope off the response.
    // If the body isn't well-formed, fall back to a generic
    // network-level message.
    try {
      const body = (await error.response.clone().json()) as unknown;
      unwrapEnvelope<unknown>(body, error.response.status);
      // unwrapEnvelope only returns when ok=true; throwing the
      // failure envelope is the contract above. If we land here
      // the response was 2xx but ky still threw — bail to the
      // network branch.
    } catch (decoded) {
      if (decoded instanceof EnvelopeError) {
        return new KairoApiClientError({
          kind: 'daemon',
          code: decoded.code,
          message: decoded.message,
          status: error.response.status,
        });
      }
    }
    return new KairoApiClientError({
      kind: 'network',
      message: `unexpected non-2xx response (HTTP ${error.response.status})`,
    });
  }
  if (error instanceof Error) {
    return new KairoApiClientError({ kind: 'network', message: error.message });
  }
  return new KairoApiClientError({
    kind: 'network',
    message: `unknown transport error: ${String(error)}`,
  });
}
