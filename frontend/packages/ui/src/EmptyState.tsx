// Empty-state placeholder. MUI doesn't ship one out of the
// box; we compose Box + Typography to land at the same
// affordance.

import type { ReactNode } from 'react';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';

export interface EmptyStateProps {
  title: ReactNode;
  description?: ReactNode;
  /** Action affordances rendered below the description. */
  actions?: ReactNode;
}

/**
 * Centered, low-weight placeholder used when a list / page has
 * no data to show. Title-first so the absence is named, not
 * implied — `WEB_CLIENT.md` §17 wants errors to be actionable;
 * the same goes for empty surfaces.
 */
export function EmptyState({ title, description, actions }: EmptyStateProps) {
  return (
    <Box
      role="status"
      sx={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        textAlign: 'center',
        py: 4,
        px: 3,
        gap: 1,
        color: 'text.secondary',
      }}
    >
      <Typography variant="h3" component="h3" sx={{ color: 'text.primary' }}>
        {title}
      </Typography>
      {description !== undefined && (
        <Typography variant="body2" sx={{ maxWidth: '24rem' }}>
          {description}
        </Typography>
      )}
      {actions !== undefined && <Stack direction="row" spacing={1} sx={{ pt: 1 }}>{actions}</Stack>}
    </Box>
  );
}
