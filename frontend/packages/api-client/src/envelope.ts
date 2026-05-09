// Daemon API envelope shapes (`specs/API.md` §7).
//
// `body = T` annotations on the daemon side describe the *inner*
// result type, so this module is the one place that knows about
// the wrapper. Every typed client method passes its `unknown`
// JSON body through `unwrapEnvelope` to land back at `T`.
//
// Zod runtime validation (`WEB_CLIENT.md` §5.2) catches malformed
// daemon responses at the network boundary, before they reach
// the typed surface. The success body's `result` shape is *not*
// re-validated against the OpenAPI types — that's a deeper
// runtime check best handled per-endpoint when (if) callers
// genuinely need it. Slice 6 stops at the envelope.

import { z } from 'zod';

/** Stable wire-stable error codes (`specs/API.md` §8). */
export const apiErrorCodeSchema = z.enum([
  'bad_request',
  'not_found',
  'store_error',
  'internal_error',
  'web_proxy_error',
]);

export type ApiErrorCode = z.infer<typeof apiErrorCodeSchema>;

const apiErrorBodySchema = z.object({
  code: apiErrorCodeSchema.or(z.string()),
  message: z.string(),
});

const apiSuccessSchema = z.object({
  ok: z.literal(true),
  schema: z.string(),
  result: z.unknown(),
});

const apiFailureSchema = z.object({
  ok: z.literal(false),
  schema: z.string(),
  error: apiErrorBodySchema,
});

const apiEnvelopeSchema = z.discriminatedUnion('ok', [apiSuccessSchema, apiFailureSchema]);

export type ApiSuccess<T> = {
  ok: true;
  schema: string;
  result: T;
};

export type ApiFailure = {
  ok: false;
  schema: string;
  error: {
    code: string;
    message: string;
  };
};

export type ApiEnvelope<T> = ApiSuccess<T> | ApiFailure;

export class EnvelopeError extends Error {
  readonly code: string;
  readonly httpStatus: number | undefined;

  constructor(code: string, message: string, httpStatus?: number) {
    super(message);
    this.name = 'EnvelopeError';
    this.code = code;
    this.httpStatus = httpStatus;
  }
}

/**
 * Parse an `unknown` JSON body into `T`. Validates the envelope
 * shape with Zod before unwrapping; throws `EnvelopeError` for
 * the failure envelope or for shape mismatches.
 *
 * The inner `result` is *not* re-validated against the OpenAPI
 * type — the daemon's drift test guarantees the wire shape
 * matches the schema. Per-endpoint runtime validation (if it
 * ever becomes load-bearing) lands as a follow-up.
 */
export function unwrapEnvelope<T>(body: unknown, httpStatus?: number): T {
  const parsed = apiEnvelopeSchema.safeParse(body);
  if (!parsed.success) {
    throw new EnvelopeError(
      'decode_error',
      `expected JSON envelope; ${parsed.error.message}`,
      httpStatus,
    );
  }
  if (parsed.data.ok === true) {
    return parsed.data.result as T;
  }
  throw new EnvelopeError(parsed.data.error.code, parsed.data.error.message, httpStatus);
}
