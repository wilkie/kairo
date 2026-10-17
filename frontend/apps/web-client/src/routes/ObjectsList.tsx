// `/objects` placeholder. Slice 8 lands the real object list
// (search + recently-viewed + a `/api/v1/objects` listing once
// the daemon ships one). Signed-in users can already create an
// object here; it also appears under their actor page's Created
// objects.

import { useState } from 'react';
import { useSession } from '@kairo/api-client';
import { Button, EmptyState, Panel } from '@kairo/ui';
import Typography from '@mui/material/Typography';
import { CreateObjectDialogContainer } from '../components/CreateObjectDialogContainer';

export function ObjectsList() {
  const sessionQ = useSession();
  const [createDialogOpen, setCreateDialogOpen] = useState(false);
  const signedIn = (sessionQ.data ?? null) !== null;

  return (
    <>
      <Typography variant="h2" component="h2">
        Objects
      </Typography>
      <Panel
        title="Object browser"
        {...(signedIn
          ? {
              actions: (
                <Button
                  variant="primary"
                  size="small"
                  onClick={() => setCreateDialogOpen(true)}
                  data-testid="create-object"
                >
                  New object
                </Button>
              ),
            }
          : {})}
      >
        <EmptyState
          title="Object listing is a slice-8 follow-up"
          description="Once a /api/v1/objects listing endpoint exists on the daemon, the inspector will surface it here. For now, navigate directly to /objects/{id}, or find an actor's objects on their actor page."
        />
      </Panel>
      <CreateObjectDialogContainer
        open={createDialogOpen}
        onClose={() => setCreateDialogOpen(false)}
      />
    </>
  );
}
