// Plain-text viewer. Renders the sniffed UTF-8 string in a
// monospace `<pre>` with line wrapping so long lines reflow
// instead of triggering horizontal scroll. Tagged
// `inline-safe` per `WEB_CLIENT.md` §12.3 — no script tags,
// no HTML interpretation, no link auto-detection (anything
// the inspector turns into a clickable affordance must come
// from a typed payload, not text scraping).

import Box from '@mui/material/Box';
import type { ArtifactViewer, ArtifactViewerProps } from '../types';

const MONOSPACE = 'ui-monospace, SFMono-Regular, Menlo, monospace';

export function TextArtifactViewer({ record }: ArtifactViewerProps) {
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
      {record.sniff.text ?? ''}
    </Box>
  );
}

export const textViewer: ArtifactViewer = {
  id: 'text',
  label: 'Text',
  canView: (record) => record.sniff.kind === 'text',
  Viewer: TextArtifactViewer,
  safety: 'inline-safe',
};
