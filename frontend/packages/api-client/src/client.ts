import createClient, { type Client } from 'openapi-fetch';
import type { paths, components } from './generated/schema';

/**
 * Stripped envelope shape: every non-streaming daemon response
 * is `{ ok: true, schema, result } | { ok: false, schema, error }`.
 * The OpenAPI annotations (slice 1) declare the response `body`
 * as the *inner* result type, so the generated `paths` types
 * describe `result` shape directly. We just have to unwrap the
 * envelope at the fetch boundary.
 */
type ApiSuccess<T> = {
  ok: true;
  schema: string;
  result: T;
};

type ApiFailure = {
  ok: false;
  schema: string;
  error: {
    code: string;
    message: string;
  };
};

type ApiEnvelope<T> = ApiSuccess<T> | ApiFailure;

export type VersionInfo = components['schemas']['VersionInfo'];

/**
 * Errors surfaced by `KairoApiClient`. Mirrors `WEB_CLIENT.md`
 * §6 `ApiClientError`. Slice 6 adds the `validation` /
 * `unauthorized` variants when the relevant endpoints are wired.
 */
export type ApiClientError =
  | { kind: 'network'; message: string }
  | { kind: 'daemon'; code: string; message: string; status: number }
  | { kind: 'decode'; message: string };

export class KairoApiClientError extends Error {
  readonly detail: ApiClientError;

  constructor(detail: ApiClientError) {
    super(formatMessage(detail));
    this.name = 'KairoApiClientError';
    this.detail = detail;
  }
}

function formatMessage(detail: ApiClientError): string {
  switch (detail.kind) {
    case 'network':
      return `network error: ${detail.message}`;
    case 'daemon':
      return `daemon error (${detail.code}, HTTP ${detail.status}): ${detail.message}`;
    case 'decode':
      return `decode error: ${detail.message}`;
  }
}

/**
 * Public surface of the typed client. Slice 5 ships only
 * `getVersion`; slice 6 fills in the rest of the read endpoints
 * with the same envelope-unwrap pattern.
 */
export interface KairoApiClient {
  getVersion(): Promise<VersionInfo>;
}

export function createKairoClient(baseUrl: string): KairoApiClient {
  const fetchClient: Client<paths> = createClient<paths>({ baseUrl });

  return {
    async getVersion(): Promise<VersionInfo> {
      const { data, error, response } = await fetchClient.GET('/api/v1/version');
      if (error !== undefined) {
        throw decodeFailure(response, error);
      }
      return unwrap<VersionInfo>(response, data);
    },
  };
}

/**
 * Unwrap the success envelope. `data` is the parsed JSON body;
 * we re-parse the `ok`/`schema`/`result` shape ourselves rather
 * than trust the generated path schema, because the OpenAPI
 * annotations describe the *inner* result type (see slice 1's
 * doc-level note on the envelope).
 */
function unwrap<T>(response: Response, data: unknown): T {
  const envelope = data as ApiEnvelope<T>;
  if (envelope == null || typeof envelope !== 'object') {
    throw new KairoApiClientError({
      kind: 'decode',
      message: `expected JSON envelope; got ${typeof envelope}`,
    });
  }
  if (envelope.ok === true) {
    return envelope.result;
  }
  if (envelope.ok === false) {
    throw new KairoApiClientError({
      kind: 'daemon',
      code: envelope.error.code,
      message: envelope.error.message,
      status: response.status,
    });
  }
  throw new KairoApiClientError({
    kind: 'decode',
    message: `envelope missing required \`ok\` discriminator`,
  });
}

/**
 * `openapi-fetch` returns `{ data, error, response }` for non-2xx
 * responses; `error` is the parsed body. Map that to the typed
 * `ApiClientError` shape.
 */
function decodeFailure(response: Response, error: unknown): KairoApiClientError {
  const envelope = error as ApiFailure | undefined;
  if (envelope && envelope.ok === false && envelope.error) {
    return new KairoApiClientError({
      kind: 'daemon',
      code: envelope.error.code,
      message: envelope.error.message,
      status: response.status,
    });
  }
  return new KairoApiClientError({
    kind: 'network',
    message: `unexpected non-2xx response (HTTP ${response.status})`,
  });
}
