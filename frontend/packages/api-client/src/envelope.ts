// Daemon API envelope shapes (`specs/API.md` §7).
//
// `body = T` annotations on the daemon side describe the *inner*
// result type, so this module is the one place that knows about
// the wrapper. Every typed client method passes its `unknown`
// JSON body through `unwrapEnvelope` to land back at `T`.

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
 * Parse an `unknown` JSON body into `T`. Throws `EnvelopeError`
 * for the failure envelope or for shape mismatches; success
 * unwraps to `result` directly.
 */
export function unwrapEnvelope<T>(body: unknown, httpStatus?: number): T {
  if (body == null || typeof body !== 'object') {
    throw new EnvelopeError('decode_error', `expected JSON envelope; got ${typeof body}`);
  }
  const envelope = body as Partial<ApiEnvelope<T>>;
  if (envelope.ok === true) {
    return (envelope as ApiSuccess<T>).result;
  }
  if (envelope.ok === false) {
    const failure = envelope as ApiFailure;
    throw new EnvelopeError(
      failure.error?.code ?? 'unknown_error',
      failure.error?.message ?? 'daemon returned an error envelope without a message',
      httpStatus,
    );
  }
  throw new EnvelopeError(
    'decode_error',
    'envelope missing required `ok` discriminator',
    httpStatus,
  );
}
