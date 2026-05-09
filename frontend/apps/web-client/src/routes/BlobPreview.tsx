// `/blobs/$id` placeholder. Slice 10 lands the
// artifact-viewers registry; the real preview lives there.

import { EmptyState, Panel } from '@kairo/ui';
import Typography from '@mui/material/Typography';
import { truncateId } from '@kairo/object-model';

export interface BlobPreviewRouteProps {
  id: string;
}

export function BlobPreviewRoute({ id }: BlobPreviewRouteProps) {
  return (
    <>
      <Typography variant="h2" component="h2">Blob</Typography>
      <Panel title={truncateId(id)} description={<code>{id}</code>}>
        <EmptyState
          title="Blob preview is a slice-10 follow-up"
          description="Slice 10 will land a content-sniff-driven viewer registry (text / JSON / binary placeholder + download)."
        />
      </Panel>
    </>
  );
}
