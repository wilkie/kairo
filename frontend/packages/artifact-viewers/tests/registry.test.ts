// Registry behavior tests. The default registry's order
// matters — JSON beats text beats binary — so the tests are
// structured around the priority decision rather than the
// per-viewer rendering output.

import { describe, expect, it } from 'vitest';
import { defaultArtifactViewers, pickViewer } from '../src/registry';
import { sniffArtifact } from '../src/sniffer';
import type { ArtifactRecord } from '../src/types';

function recordOf(text: string): ArtifactRecord {
  const bytes = new TextEncoder().encode(text);
  return { blobId: 'kairo:blob:zTest', bytes, sniff: sniffArtifact(bytes) };
}

function binaryRecordOf(...bytes: number[]): ArtifactRecord {
  const buf = new Uint8Array(bytes);
  return { blobId: 'kairo:blob:zTest', bytes: buf, sniff: sniffArtifact(buf) };
}

describe('pickViewer (default registry)', () => {
  it('routes a JSON object to the json viewer', () => {
    const v = pickViewer(recordOf('{"a":1}'));
    expect(v?.id).toBe('json');
  });

  it('routes a JSON array to the json viewer', () => {
    const v = pickViewer(recordOf('[1,2,3]'));
    expect(v?.id).toBe('json');
  });

  it('routes plain text to the text viewer', () => {
    const v = pickViewer(recordOf('hello world\n'));
    expect(v?.id).toBe('text');
  });

  it('routes a JSON-looking-but-malformed payload to the text viewer', () => {
    // Starts with `{` but isn't actually parseable.
    const v = pickViewer(recordOf('{ not really json'));
    expect(v?.id).toBe('text');
  });

  it('routes binary to the binary viewer', () => {
    const v = pickViewer(binaryRecordOf(0x00, 0xff, 0xfe, 0xfd));
    expect(v?.id).toBe('binary');
  });

  it('routes invalid UTF-8 to the binary viewer (not a JSON or text false-positive)', () => {
    const v = pickViewer(binaryRecordOf(0xc3, 0x28));
    expect(v?.id).toBe('binary');
  });

  it('every default viewer is inline-safe', () => {
    for (const viewer of defaultArtifactViewers) {
      expect(viewer.safety).toBe('inline-safe');
    }
  });

  it('returns null when a custom registry has no fallback and nothing matches', () => {
    const onlyJson = defaultArtifactViewers.filter((v) => v.id === 'json');
    expect(pickViewer(binaryRecordOf(0xff, 0xfe), onlyJson)).toBeNull();
  });
});
