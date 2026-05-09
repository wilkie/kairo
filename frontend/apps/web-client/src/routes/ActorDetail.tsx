// `/actors/$id` placeholder. Slice 8 wires `useActor(id)` and
// renders genesis fields + a list of statements observable in
// the local store.

import { EmptyState, Panel } from '@kairo/ui';
import Typography from '@mui/material/Typography';
import { truncateId } from '@kairo/object-model';

export interface ActorDetailProps {
  id: string;
}

export function ActorDetail({ id }: ActorDetailProps) {
  return (
    <>
      <Typography variant="h2" component="h2">Actor</Typography>
      <Panel title={truncateId(id)} description={<code>{id}</code>}>
        <EmptyState
          title="Actor detail is a slice-8 follow-up"
          description="Slice 8 will render genesis fields + observable signed statements for this actor."
        />
      </Panel>
    </>
  );
}
