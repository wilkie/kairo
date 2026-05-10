// Sniffer behavior tests. The sniffer is the part of slice 10
// that decides whether a blob renders as text, JSON, or
// binary; misclassification is the worst case (gibberish or
// missing data) so the boundary cases are nailed down here.

import { describe, expect, it } from 'vitest';
import { sniffArtifact } from '../src/sniffer';

function utf8(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

describe('sniffArtifact', () => {
  it('classifies an empty blob as text', () => {
    const r = sniffArtifact(new Uint8Array());
    expect(r.kind).toBe('text');
    expect(r.byteLength).toBe(0);
    expect(r.text).toBe('');
  });

  it('classifies plain ASCII as text and decodes it', () => {
    const r = sniffArtifact(utf8('hello, world\n'));
    expect(r.kind).toBe('text');
    expect(r.text).toBe('hello, world\n');
  });

  it('classifies UTF-8 multibyte content as text', () => {
    const r = sniffArtifact(utf8('héllo 🌍'));
    expect(r.kind).toBe('text');
    expect(r.text).toBe('héllo 🌍');
  });

  it('preserves the standard ASCII whitespace control bytes', () => {
    const r = sniffArtifact(utf8('a\tb\nc\rd'));
    expect(r.kind).toBe('text');
    expect(r.text).toBe('a\tb\nc\rd');
  });

  it('classifies a NUL-bearing payload as binary', () => {
    const bytes = new Uint8Array([0x68, 0x69, 0x00, 0x21]);
    expect(sniffArtifact(bytes).kind).toBe('binary');
  });

  it('classifies a payload with a non-whitespace control byte as binary', () => {
    // 0x07 (BEL) is a control character that doesn't belong in
    // plain text.
    const bytes = new Uint8Array([0x68, 0x07, 0x69]);
    expect(sniffArtifact(bytes).kind).toBe('binary');
  });

  it('classifies invalid UTF-8 as binary', () => {
    // Lone continuation byte — never valid UTF-8.
    const bytes = new Uint8Array([0xc3, 0x28]);
    expect(sniffArtifact(bytes).kind).toBe('binary');
  });

  it('reports a byte length even for binary payloads', () => {
    const bytes = new Uint8Array([0x00, 0x01, 0x02, 0x03]);
    const r = sniffArtifact(bytes);
    expect(r.kind).toBe('binary');
    expect(r.byteLength).toBe(4);
    expect(r.text).toBeUndefined();
  });
});
