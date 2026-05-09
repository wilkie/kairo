// Catch-all 404 page. TanStack Router invokes it whenever no
// route matches.

import { EmptyState } from '@kairo/ui';
import Typography from '@mui/material/Typography';
import { Link } from '@tanstack/react-router';

export function NotFound() {
  return (
    <>
      <Typography variant="h2" component="h2">Not found</Typography>
      <EmptyState
        title="Page not found"
        description="That path doesn't match any route in the inspector."
        actions={<Link to="/">Back to dashboard</Link>}
      />
    </>
  );
}
