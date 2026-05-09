// `/statements/$id` placeholder. Slice 8 wires `useStatement(id)`
// + the type guards from `@kairo/object-model` to render a
// kind-specific summary plus the raw JSON envelope.

import { EmptyState, Panel } from '@kairo/ui';
import Typography from '@mui/material/Typography';
import { truncateId } from '@kairo/object-model';

export interface StatementDetailProps {
  id: string;
}

export function StatementDetail({ id }: StatementDetailProps) {
  return (
    <>
      <Typography variant="h2" component="h2">Statement</Typography>
      <Panel title={truncateId(id)} description={<code>{id}</code>}>
        <EmptyState
          title="Statement detail is a slice-8 follow-up"
          description="Slice 8 will render the typed summary (per kind) + the raw JSON envelope for this statement."
        />
      </Panel>
    </>
  );
}
