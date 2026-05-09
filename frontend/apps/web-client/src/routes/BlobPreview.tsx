// `/blobs/$id` placeholder. Slice 10 lands the
// artifact-viewers registry; the real preview lives there.

import { EmptyState, Panel } from '@kairo/ui';
import Typography from '@mui/material/Typography';
import { canonicalizeId, truncateId } from '@kairo/object-model';

export interface BlobPreviewRouteProps {
  id: string;
}

export function BlobPreviewRoute({ id }: BlobPreviewRouteProps) {
  // Accept either canonical `kairo:blob:<payload>` or bare.
  const blobId = canonicalizeId('blob', id);
  return (
    <>
      <Typography variant="h2" component="h2">Blob</Typography>
      <Panel title={truncateId(blobId)} description={<code>{blobId}</code>}>
        <EmptyState
          title="Blob preview is a slice-10 follow-up"
          description="Slice 10 will land a content-sniff-driven viewer registry (text / JSON / binary placeholder + download)."
        />
      </Panel>
    </>
  );
}
