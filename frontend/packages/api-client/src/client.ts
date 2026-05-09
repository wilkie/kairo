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
export type ObjectGenesisStatementJson = components['schemas']['ObjectGenesisStatementJson'];
export type ObjectBranchStatementJson = components['schemas']['ObjectBranchStatementJson'];
export type ObjectVersionTagStatementJson = components['schemas']['ObjectVersionTagStatementJson'];
export type ActorTrustStatementJson = components['schemas']['ActorTrustStatementJson'];
export type BranchTipDto = components['schemas']['BranchTipDto'];
export type CapabilityHeadDto = components['schemas']['CapabilityHeadDto'];
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
  /** `GET /api/v1/objects/{id}`. */
  getObject(objectId: string): Promise<ObjectGenesisStatementJson>;
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
  /** `GET /api/v1/version-tags/{object}/{version}`. */
  getLatestVersionTag(
    objectId: string,
    version: string,
    opts?: BranchActorOption,
  ): Promise<ObjectVersionTagStatementJson>;
  /** `GET /api/v1/trust/{by}/{of}`. */
  getTrust(byActor: string, ofActor: string): Promise<ActorTrustStatementJson>;
  /** `GET /api/v1/capabilities/{grantor}`. */
  listCapabilitiesFromGrantor(grantorId: string): Promise<CapabilityHeadDto[]>;
  /**
   * `GET /api/v1/blobs/{id}` — raw `application/octet-stream`.
   * Returns a `Blob` to keep the browser-side surface tractable;
   * streaming consumers can call `.stream()` on the result.
   */
  getBlob(blobId: string): Promise<Blob>;
  /** `GET /api/v1/verify-object/{id}`. */
  verifyObject(objectId: string): Promise<ValidationResult>;
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
    async getObject(objectId) {
      return getJson<ObjectGenesisStatementJson>(
        http,
        `api/v1/objects/${encodeURIComponent(objectId)}`,
      );
    },
    async getStatement(statementId) {
      return getJson<StatementValue>(
        http,
        `api/v1/statements/${encodeURIComponent(statementId)}`,
      );
    },
    async listBranches(objectId) {
      return getJson<BranchTipDto[]>(
        http,
        `api/v1/branches/${encodeURIComponent(objectId)}`,
      );
    },
    async getLatestBranch(objectId, name, opts) {
      return getJson<ObjectBranchStatementJson>(
        http,
        `api/v1/branches/${encodeURIComponent(objectId)}/${encodeURIComponent(name)}/latest`,
        actorQuery(opts),
      );
    },
    async getLatestVersionTag(objectId, version, opts) {
      return getJson<ObjectVersionTagStatementJson>(
        http,
        `api/v1/version-tags/${encodeURIComponent(objectId)}/${encodeURIComponent(version)}`,
        actorQuery(opts),
      );
    },
    async getTrust(byActor, ofActor) {
      return getJson<ActorTrustStatementJson>(
        http,
        `api/v1/trust/${encodeURIComponent(byActor)}/${encodeURIComponent(ofActor)}`,
      );
    },
    async listCapabilitiesFromGrantor(grantorId) {
      return getJson<CapabilityHeadDto[]>(
        http,
        `api/v1/capabilities/${encodeURIComponent(grantorId)}`,
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
