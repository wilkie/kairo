// HTTP transport. We use `ky` instead of raw fetch for a few
// concrete reasons:
//
// - Built-in JSON convenience, query string handling, and a
//   small surface for `prefixUrl`.
// - `HTTPError` carries the original `Response`, which lets us
//   read the daemon's error envelope on a non-2xx without
//   re-issuing the request.
// - Hooks make it trivial to add a request id / log line per
//   call as the inspector grows.
//
// Retry policy stays at 0 here. TanStack Query owns retry
// behavior at the React layer, so adding it inside ky would
// double-retry.

import ky, { type KyInstance, type Options as KyOptions } from 'ky';

export interface TransportOptions {
  /**
   * Base URL the client prefixes onto every request path.
   * Empty string keeps requests relative to the document
   * origin — what the production deployment uses (the SPA and
   * the API are served by the same `kairo-web` process).
   */
  baseUrl: string;
  /** Per-request timeout in milliseconds. */
  timeoutMs?: number;
  /** Hooks to apply on top of the defaults (logging, tracing, etc). */
  hooks?: KyOptions['hooks'];
}

export function createTransport(opts: TransportOptions): KyInstance {
  const kyOpts: KyOptions = {
    timeout: opts.timeoutMs ?? 10_000,
    retry: 0,
  };
  if (opts.baseUrl !== '') {
    kyOpts.prefixUrl = opts.baseUrl;
  }
  if (opts.hooks !== undefined) {
    kyOpts.hooks = opts.hooks;
  }
  return ky.create(kyOpts);
}
