// Fallback viewer for blobs the sniffer flags as binary. We
// don't try to render the bytes inline — they'd be gibberish
// in any text renderer and a hex dump is rarely what users
// want first. Instead we summarize and offer a download.
//
// Tagged `inline-safe` because nothing happens until the user
// clicks download (and even then the browser handles the
// transfer; the inspector never executes the payload).

import Button from '@mui/material/Button';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import { useEffect, useMemo } from 'react';
import type { ArtifactViewer, ArtifactViewerProps } from '../types';

export function BinaryArtifactViewer({ record }: ArtifactViewerProps) {
  // Build the object URL on mount and revoke it on unmount so
  // we don't leak a handle to the browser's URL store every
  // time the user navigates away.
  const url = useMemo(() => {
    const blob = new Blob([new Uint8Array(record.bytes)], {
      type: 'application/octet-stream',
    });
    return URL.createObjectURL(blob);
  }, [record.bytes]);

  useEffect(() => {
    return () => {
      URL.revokeObjectURL(url);
    };
  }, [url]);

  const filename = filenameFor(record.blobId);

  return (
    <Stack spacing={2}>
      <Typography variant="body2">
        This blob does not appear to be UTF-8 text. The inspector does not render binary
        payloads inline; download the bytes to inspect them locally.
      </Typography>
      <Stack direction="row" spacing={2} alignItems="center">
        <Typography variant="body2" sx={{ color: 'text.secondary' }}>
          {formatByteLength(record.sniff.byteLength)}
        </Typography>
        <Button component="a" href={url} download={filename} variant="outlined" size="small">
          Download
        </Button>
      </Stack>
    </Stack>
  );
}

export const binaryViewer: ArtifactViewer = {
  id: 'binary',
  label: 'Binary',
  // Catch-all — registered last in the default registry so
  // text and JSON take precedence.
  canView: () => true,
  Viewer: BinaryArtifactViewer,
  safety: 'inline-safe',
};

// ---------------------------------------------------------------------------

function formatByteLength(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
}

/** Build a download filename from the multibase payload tail
 * of the blob id so the saved file is identifiable. */
function filenameFor(blobId: string): string {
  const colonParts = blobId.split(':');
  const payload = colonParts[colonParts.length - 1] ?? blobId;
  const tail = payload.length > 12 ? payload.slice(-12) : payload;
  return `kairo-blob-${tail}.bin`;
}
