// Content sniffer — looks at the raw bytes and decides
// whether a record is text or binary. The spec is explicit
// (`PHASE_2_WEB_CLIENT.md` slice 10): "no MIME guessing." The
// daemon serves every blob as `application/octet-stream`, and
// MIME types from filename extensions are unreliable across
// the federation surface anyway, so we look at bytes only.
//
// Heuristic:
//
// 1. Try to decode the bytes as UTF-8 with `fatal: true`.
// 2. If decoding succeeds, scan the first chunk for control
//    characters that don't belong in plain text (NUL through
//    0x1F, except the usual whitespace: tab, LF, CR).
// 3. If no disallowed control bytes are seen, the record is
//    text and we cache the decoded string.
//
// The sniffer is deliberately conservative: a single NUL or
// non-whitespace control character flips a record to `binary`
// even if the rest decodes cleanly. Inspector workloads care
// more about not misclassifying binary blobs as text (which
// would render gibberish) than about the inverse.

import type { ArtifactSniff } from './types';

/** Bytes scanned in the control-character pass. 4 KiB matches
 * what `git`'s binary detection uses. */
const SNIFF_LIMIT = 4096;

export function sniffArtifact(bytes: Uint8Array): ArtifactSniff {
  const byteLength = bytes.byteLength;

  // Empty blobs are text by convention — nothing to render
  // either way and "binary" implies an unviewable payload.
  if (byteLength === 0) {
    return { kind: 'text', byteLength, text: '' };
  }

  // Quick reject: any disallowed control byte in the first
  // window means binary. Cheaper than a full UTF-8 decode and
  // catches most binary formats by their NUL bytes.
  const window = bytes.subarray(0, Math.min(byteLength, SNIFF_LIMIT));
  for (let i = 0; i < window.length; i += 1) {
    const b = window[i] ?? 0;
    if (b < 0x20 && b !== 0x09 && b !== 0x0a && b !== 0x0d) {
      return { kind: 'binary', byteLength };
    }
  }

  // Strict UTF-8 decode of the *full* payload. We don't decode
  // only the prefix because a multibyte sequence split across
  // the prefix boundary would falsely fail.
  let text: string;
  try {
    text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    return { kind: 'binary', byteLength };
  }
  return { kind: 'text', byteLength, text };
}
