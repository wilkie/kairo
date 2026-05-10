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
   * Base URL the client prefixes onto every request path. Must
   * be an **absolute** URL (e.g. `window.location.origin` in
   * the browser; `http://daemon` in tests). Bare paths the
   * api-client passes (e.g. `api/v1/version`) are resolved
   * against this base.
   *
   * An empty string is accepted but discouraged — ky then
   * treats every path as relative to the *current document*,
   * which works for the dashboard at `/` but breaks for any
   * route below the root (the request resolves to
   * `/<route>/api/v1/...`, hits the SPA fallback, and the
   * caller gets HTML where it expected JSON).
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
