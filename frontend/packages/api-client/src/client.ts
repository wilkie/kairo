// Imperative typed client. Each method maps to one daemon
// endpoint, sends a `ky` request, and unwraps the envelope into
// the inner `T` declared by the OpenAPI annotations.
//
// Slice 5 ships the single `getVersion` method. Slice 6 will
// fill in the remaining 11 endpoints (`getStatus`, `getActor`,
// `getObject`, `getStatement`, `listBranches`, `getLatestBranch`,
// `getLatestVersionTag`, `getTrust`, `listCapabilitiesFromGrantor`,
// `getBlob`, `verifyObject`).

import { HTTPError, type KyInstance } from 'ky';
import type { components } from './generated/schema';
import { EnvelopeError, unwrapEnvelope } from './envelope';
import { KairoApiClientError } from './error';
import { createTransport, type TransportOptions } from './transport';

export type VersionInfo = components['schemas']['VersionInfo'];
export type StatusInfo = components['schemas']['StatusInfo'];
export type ValidationResult = components['schemas']['ValidationResult'];

export interface KairoApiClient {
  /** `GET /api/v1/version`. */
  getVersion(): Promise<VersionInfo>;
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
  };
}

async function getJson<T>(http: KyInstance, path: string): Promise<T> {
  try {
    const body = await http.get(path).json<unknown>();
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
    return new KairoApiClientError({
      kind: 'decode',
      message: error.message,
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
