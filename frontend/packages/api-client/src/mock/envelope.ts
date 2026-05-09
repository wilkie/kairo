// Helpers that wrap fixture bodies in the success / failure
// envelopes the daemon would emit. Used by the MSW handlers so
// each handler is a one-liner.

import type { ApiFailure, ApiSuccess } from '../envelope';

const RESULT_SCHEMA = 'kairo.api.result.v1';
const ERROR_SCHEMA = 'kairo.api.error.v1';

export function successEnvelope<T>(result: T): ApiSuccess<T> {
  return {
    ok: true,
    schema: RESULT_SCHEMA,
    result,
  };
}

export function errorEnvelope(code: string, message: string): ApiFailure {
  return {
    ok: false,
    schema: ERROR_SCHEMA,
    error: { code, message },
  };
}
