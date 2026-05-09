// Identifier formatters for the inspector. Pure presentation
// helpers; never used to derive semantic meaning.
//
// Kairo identifiers (`kairo:actor:zXyZ...`, `kairo:object:zXyZ...`,
// `kairo:stmt:zXyZ...`, etc.) are 60+ characters because the
// payload is a multibase-encoded SHA-256 digest. They're not
// human-friendly to read at full length; the inspector
// truncates them everywhere except detail headers.

const DEFAULT_PREFIX_CHARS = 8;
const DEFAULT_SUFFIX_CHARS = 6;

export interface TruncateIdOptions {
  /** How many characters to keep after the `:z` payload start. */
  prefixChars?: number;
  /** How many characters to keep at the end. */
  suffixChars?: number;
  /** Replacement for the omitted middle. Defaults to `…`. */
  ellipsis?: string;
}

/**
 * Truncate a Kairo identifier for display. Keeps the
 * `kairo:<kind>:` prefix intact so the type stays obvious;
 * shrinks the multibase payload to a head + tail with an
 * ellipsis between them.
 *
 * Examples (defaults: 8 prefix, 6 suffix):
 *
 * - `kairo:object:zMockObject0000000000000000000000000000000` →
 *   `kairo:object:zMockObj…000000`
 * - `not-a-kairo-id` → `not-a-kairo-id` (unchanged when shorter
 *   than the threshold).
 */
export function truncateId(id: string, opts: TruncateIdOptions = {}): string {
  const prefixChars = opts.prefixChars ?? DEFAULT_PREFIX_CHARS;
  const suffixChars = opts.suffixChars ?? DEFAULT_SUFFIX_CHARS;
  const ellipsis = opts.ellipsis ?? '…';

  const parts = id.split(':');
  // Recognized form: `kairo:<kind>:<payload>`. Payloads typically
  // start with a multibase prefix character (`z` for base58btc)
  // followed by the digest. We truncate only the payload.
  if (parts.length >= 3 && parts[0] === 'kairo') {
    const head = parts.slice(0, parts.length - 1).join(':');
    const payload = parts[parts.length - 1] ?? '';
    if (payload.length <= prefixChars + suffixChars + ellipsis.length) {
      return id;
    }
    const start = payload.slice(0, prefixChars);
    const end = payload.slice(payload.length - suffixChars);
    return `${head}:${start}${ellipsis}${end}`;
  }

  // Non-Kairo identifier (e.g., a `git:sha256:...` reference,
  // or a free-form string). Truncate the whole value.
  if (id.length <= prefixChars + suffixChars + ellipsis.length) {
    return id;
  }
  return `${id.slice(0, prefixChars)}${ellipsis}${id.slice(id.length - suffixChars)}`;
}

/** Identifier kinds the inspector navigates to. The associated
 * `kairo:<prefix>:` token is implied by the surrounding route
 * (`/objects/`, `/actors/`, etc.), so URLs can carry the bare
 * multibase payload to keep them readable. */
export type IdKind = 'object' | 'actor' | 'statement' | 'blob';

const KIND_TO_PREFIX: Record<IdKind, string> = {
  object: 'kairo:object:',
  actor: 'kairo:actor:',
  // Statement ids use `kairo:stmt:` even though the route
  // segment is `/statements/` — the wire prefix is shorter.
  statement: 'kairo:stmt:',
  blob: 'kairo:blob:',
};

/** Returns the `kairo:<kind>:` prefix string for a kind. */
export function idPrefix(kind: IdKind): string {
  return KIND_TO_PREFIX[kind];
}

/** True when `value` already starts with the canonical prefix
 * for `kind`. */
export function isCanonicalId(kind: IdKind, value: string): boolean {
  return value.startsWith(KIND_TO_PREFIX[kind]);
}

/** Add the `kairo:<kind>:` prefix to `raw` unless it's already
 * canonical. The route layer uses this to accept either bare
 * payloads (`/objects/zXyz`) or full canonical ids
 * (`/objects/kairo:object:zXyz`) and hand a single shape to
 * the api-client hooks. */
export function canonicalizeId(kind: IdKind, raw: string): string {
  return isCanonicalId(kind, raw) ? raw : `${KIND_TO_PREFIX[kind]}${raw}`;
}

/** Strip the `kairo:<kind>:` prefix when present so the value
 * fits in a clean URL slug. Pairs with `canonicalizeId` for
 * round-tripping between API calls and route URLs. */
export function bareId(kind: IdKind, canonical: string): string {
  const prefix = KIND_TO_PREFIX[kind];
  return canonical.startsWith(prefix) ? canonical.slice(prefix.length) : canonical;
}

/**
 * Copy a value to the clipboard. Returns `true` on success,
 * `false` if the browser blocked the request (insecure origin,
 * permissions denied, etc.). Used by the inspector's "copy id"
 * affordances.
 */
export async function copyToClipboard(value: string): Promise<boolean> {
  if (typeof navigator === 'undefined' || navigator.clipboard?.writeText === undefined) {
    return false;
  }
  try {
    await navigator.clipboard.writeText(value);
    return true;
  } catch {
    return false;
  }
}
