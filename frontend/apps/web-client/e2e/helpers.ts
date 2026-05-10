// Shared helpers for the slice-10 Playwright suite.
//
// Importing from `@kairo/api-client/mock/registry` directly
// (not through the umbrella `@kairo/api-client/mock` entry)
// keeps these test modules out of the `msw/browser` import
// chain — the SPA's own bootstrap brings up the worker in
// the page; the test process only needs the canonical IDs
// to build URLs.

import { mockIds } from '@kairo/api-client/mock/registry';
import { bareId, type IdKind } from '@kairo/object-model';

export { mockIds };

/** Build a route path for the inspector — accepts the full
 * `kairo:<kind>:<payload>` form and emits the bare-payload
 * URL the SPA's `IdLink` uses, so tests navigate to the same
 * URLs links produce. */
export function bareRoute(kind: IdKind, id: string): string {
  const segment = kind === 'statement' ? 'statements' : `${kind}s`;
  return `/${segment}/${bareId(kind, id)}`;
}
