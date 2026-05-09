// `/settings` placeholder. v1 has no real settings yet (the
// daemon is loopback-only, no auth, no theming choices). This
// page exists so the sidebar nav has somewhere to point and
// later slices have a home for the settings that will land.

import { EmptyState, Panel } from '@kairo/ui';
import Typography from '@mui/material/Typography';

export function Settings() {
  return (
    <>
      <Typography variant="h2" component="h2">Settings</Typography>
      <Panel title="Inspector settings">
        <EmptyState
          title="Settings are intentionally empty in v1"
          description="The v1 inspector has no client-configurable knobs (loopback-only, no auth, no theming overrides). Later slices will populate this page as configurable surfaces emerge."
        />
      </Panel>
    </>
  );
}
