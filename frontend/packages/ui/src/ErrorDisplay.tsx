// Wrapper around MUI Alert. The structured detail field renders
// as a monospace code block underneath the alert message, since
// that's typically where the wire-stable error code or stack
// trace lands.

import type { ReactNode } from 'react';
import Alert from '@mui/material/Alert';
import AlertTitle from '@mui/material/AlertTitle';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';

export interface ErrorDisplayProps {
  /** Short headline (e.g., "Could not reach the daemon"). */
  title: ReactNode;
  /** Human-readable explanation. */
  message: ReactNode;
  /** Optional structured detail rendered as monospace; safe
   * to drop in stack traces, error.detail.code, etc. */
  detail?: ReactNode;
  /** Action affordances (e.g., a "Retry" button). */
  actions?: ReactNode;
}

/**
 * Structured error display. Pairs naturally with
 * `KairoApiClientError.detail`: `title` carries the kind;
 * `message` carries the explanation; `detail` can carry the
 * wire-stable code or HTTP status. Title and message are always
 * present; the rest is optional so the component scales from
 * lightweight inline errors to full-page failures.
 */
export function ErrorDisplay({ title, message, detail, actions }: ErrorDisplayProps) {
  return (
    <Alert severity="error" variant="outlined">
      <AlertTitle>{title}</AlertTitle>
      <Typography variant="body2" sx={{ mb: detail !== undefined ? 1 : 0 }}>
        {message}
      </Typography>
      {detail !== undefined && (
        <Box
          component="pre"
          sx={{
            fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
            fontSize: '0.8125rem',
            color: 'text.secondary',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            m: 0,
          }}
        >
          {detail}
        </Box>
      )}
      {actions !== undefined && (
        <Stack direction="row" spacing={1} sx={{ pt: 1 }}>
          {actions}
        </Stack>
      )}
    </Alert>
  );
}
