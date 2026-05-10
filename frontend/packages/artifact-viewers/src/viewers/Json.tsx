// JSON viewer. Read-only. We parse + re-serialize at 2-space
// indent so the rendered form is canonical regardless of how
// the source was minified. Tokens are walked with a small
// hand-rolled tokenizer (no regex backtracking, no third-party
// highlighter dep) and rendered as colored spans.
//
// `canView` only matches when the payload genuinely parses as
// JSON — the registry lists this viewer ahead of the plain
// text viewer so JSON wins over generic text, but a malformed
// JSON file still falls through to text rendering.
//
// Tagged `inline-safe` per `WEB_CLIENT.md` §12.3 — JSON has no
// script-execution surface; we render only its tokens, never
// any HTML the source might contain inside a string literal.

import Box from '@mui/material/Box';
import type { ReactNode } from 'react';
import type { ArtifactViewer, ArtifactViewerProps } from '../types';

const MONOSPACE = 'ui-monospace, SFMono-Regular, Menlo, monospace';

const TOKEN_COLORS = {
  // Picked to read on both light and dark MUI surfaces — these
  // are tuned to harmonize with the deep purple primary.
  key: '#7c3aed',
  string: '#0e7490',
  number: '#b45309',
  literal: '#475569',
  punct: 'inherit',
} as const;

interface JsonToken {
  text: string;
  color: keyof typeof TOKEN_COLORS;
}

export function JsonArtifactViewer({ record }: ArtifactViewerProps) {
  const text = record.sniff.text ?? '';
  let canonical: string;
  try {
    canonical = JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    // canView guarded against this; if we land here someone
    // bypassed the registry. Render the raw text rather than
    // crash.
    canonical = text;
  }
  const tokens = tokenize(canonical);
  return (
    <Box
      component="pre"
      sx={{
        fontFamily: MONOSPACE,
        fontSize: '0.8125rem',
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
        m: 0,
      }}
    >
      {renderTokens(tokens)}
    </Box>
  );
}

export const jsonViewer: ArtifactViewer = {
  id: 'json',
  label: 'JSON',
  canView: (record) => {
    if (record.sniff.kind !== 'text' || record.sniff.text === undefined) {
      return false;
    }
    const trimmed = record.sniff.text.trimStart();
    // Cheap pre-filter so we don't try to JSON.parse a 10 MB
    // novel just to fail.
    if (
      !trimmed.startsWith('{') &&
      !trimmed.startsWith('[') &&
      !trimmed.startsWith('"') &&
      !trimmed.startsWith('-') &&
      !/^[0-9]/.test(trimmed) &&
      !trimmed.startsWith('true') &&
      !trimmed.startsWith('false') &&
      !trimmed.startsWith('null')
    ) {
      return false;
    }
    try {
      JSON.parse(record.sniff.text);
      return true;
    } catch {
      return false;
    }
  },
  Viewer: JsonArtifactViewer,
  safety: 'inline-safe',
};

// ---------------------------------------------------------------------------
// Tokenizer

function tokenize(input: string): JsonToken[] {
  const tokens: JsonToken[] = [];
  let i = 0;
  while (i < input.length) {
    const ch = input[i] ?? '';
    if (ch === '"') {
      // Walk to the matching unescaped quote.
      let j = i + 1;
      while (j < input.length) {
        const c = input[j] ?? '';
        if (c === '\\') {
          j += 2;
          continue;
        }
        if (c === '"') {
          j += 1;
          break;
        }
        j += 1;
      }
      const literal = input.slice(i, j);
      // Lookahead — a string immediately followed by `:`
      // (optionally through whitespace) is a JSON key.
      let k = j;
      while (k < input.length && /\s/.test(input[k] ?? '')) {
        k += 1;
      }
      const isKey = input[k] === ':';
      tokens.push({ text: literal, color: isKey ? 'key' : 'string' });
      i = j;
    } else if (ch === '-' || (ch >= '0' && ch <= '9')) {
      let j = i + 1;
      while (j < input.length && /[0-9.eE+-]/.test(input[j] ?? '')) {
        j += 1;
      }
      tokens.push({ text: input.slice(i, j), color: 'number' });
      i = j;
    } else if (ch === 't' && input.startsWith('true', i)) {
      tokens.push({ text: 'true', color: 'literal' });
      i += 4;
    } else if (ch === 'f' && input.startsWith('false', i)) {
      tokens.push({ text: 'false', color: 'literal' });
      i += 5;
    } else if (ch === 'n' && input.startsWith('null', i)) {
      tokens.push({ text: 'null', color: 'literal' });
      i += 4;
    } else {
      tokens.push({ text: ch, color: 'punct' });
      i += 1;
    }
  }
  return tokens;
}

function renderTokens(tokens: ReadonlyArray<JsonToken>): ReactNode {
  return tokens.map((token, idx) =>
    token.color === 'punct' ? (
      // eslint-disable-next-line react/jsx-key
      <span key={idx}>{token.text}</span>
    ) : (
      <span key={idx} style={{ color: TOKEN_COLORS[token.color] }}>
        {token.text}
      </span>
    ),
  );
}
