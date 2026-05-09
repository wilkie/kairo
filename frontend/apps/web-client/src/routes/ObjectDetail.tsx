// `/objects/$id` placeholder. Slice 8 lands the real composite
// view (genesis + branches + tags + revisions + capability
// heads + trust opinions). For now it just confirms the route
// param flow and shows the truncated id.

import { EmptyState, Panel } from '@kairo/ui';
import Typography from '@mui/material/Typography';
import { truncateId } from '@kairo/object-model';

export interface ObjectDetailProps {
  id: string;
}

export function ObjectDetail({ id }: ObjectDetailProps) {
  return (
    <>
      <Typography variant="h2" component="h2">Object</Typography>
      <Panel title={truncateId(id)} description={<code>{id}</code>}>
        <EmptyState
          title="Object detail is a slice-8 follow-up"
          description="Slice 8 will render genesis + branches + tags + revisions + capability heads + trust opinions for this object."
        />
      </Panel>
    </>
  );
}
