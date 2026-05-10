// `/blobs/$id` — blob preview backed by the artifact-viewers
// registry (`@kairo/artifact-viewers`). The registry sniffs
// the bytes server-fetched from `/api/v1/blobs/{id}` and
// picks a viewer (text / JSON / binary fallback) per
// `WEB_CLIENT.md` §12.3.
//
// Two badges sit alongside the panel header:
//
//   - LocalityBadge — every blob is `local` in v1.
//   - StatusBadge   — the chosen viewer's safety class.
//                     v1's set is all `inline-safe`; sandboxed
//                     and runtime viewers will surface as
//                     distinct tones once they ship.

import { BlobPreview, useChosenArtifactViewer } from '@kairo/artifact-viewers';
import { canonicalizeId } from '@kairo/object-model';
import { LocalityBadge, Panel, StatusBadge } from '@kairo/ui';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';

export interface BlobPreviewRouteProps {
  id: string;
}

export function BlobPreviewRoute({ id }: BlobPreviewRouteProps) {
  // Accept either canonical `kairo:blob:<payload>` or bare.
  const blobId = canonicalizeId('blob', id);
  const viewer = useChosenArtifactViewer(blobId);

  return (
    <>
      <Typography variant="h2" component="h2">
        Blob
      </Typography>
      <Box sx={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', wordBreak: 'break-all' }}>
        {blobId}
      </Box>

      <Panel
        title={viewer === null ? 'Preview' : `Viewing as ${viewer.label}`}
        description="Inline preview chosen by content sniff (no MIME guessing)."
        actions={
          <Stack direction="row" spacing={1}>
            {viewer !== null && <StatusBadge tone="success">{viewer.safety}</StatusBadge>}
            <LocalityBadge state="local" />
          </Stack>
        }
      >
        <BlobPreview blobId={blobId} />
      </Panel>
    </>
  );
}
