// `/objects` placeholder. Slice 8 lands the real object list
// (search + recently-viewed + a `/api/v1/objects` listing once
// the daemon ships one).

import { EmptyState, Panel } from '@kairo/ui';
import Typography from '@mui/material/Typography';

export function ObjectsList() {
  return (
    <>
      <Typography variant="h2" component="h2">Objects</Typography>
      <Panel title="Object browser">
        <EmptyState
          title="Object listing is a slice-8 follow-up"
          description="Once a /api/v1/objects listing endpoint exists on the daemon, the inspector will surface it here. For now, navigate directly to /objects/{id}."
        />
      </Panel>
    </>
  );
}
