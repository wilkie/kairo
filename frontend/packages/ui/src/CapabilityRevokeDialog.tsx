// `<CapabilityRevokeDialog />` — confirmation for writing an
// `ActorCapabilityRevocation` against one grant via cookie-mode
// write.
//
// Defaults to a forward-only revocation: the grant stops
// authorizing from the revocation on. `retroactive` invalidates
// the grant from inception, along with every statement signed
// under it (`CAPABILITIES.md` §6.3) — meant for grant fraud or a
// compromised grantee key — so it sits behind an explicit
// checkbox with a warning and requires a reason.
//
// Dumb component: the parent owns the mutation and error mapping.

import * as React from 'react';
import { useEffect, useState } from 'react';
import Alert from '@mui/material/Alert';
import Box from '@mui/material/Box';
import Checkbox from '@mui/material/Checkbox';
import CircularProgress from '@mui/material/CircularProgress';
import FormControlLabel from '@mui/material/FormControlLabel';
import Stack from '@mui/material/Stack';
import TextField from '@mui/material/TextField';
import Typography from '@mui/material/Typography';
import { Button } from './Button';
import { Dialog } from './Dialog';
import { truncateActorId } from './ActorPicker';

export interface CapabilityRevokeDialogError {
  code: string;
  message: string;
}

export interface CapabilityRevokeDialogGrant {
  statementId: string;
  grantee: string;
  statementKinds: string[];
}

export interface CapabilityRevokeDialogProps {
  open: boolean;
  onClose: () => void;
  /** The grant being revoked; `null` renders nothing. */
  grant: CapabilityRevokeDialogGrant | null;
  /** `true` while the mutation is in flight. */
  submitting?: boolean;
  /** Surface a submission error inline above the buttons. */
  submitError?: CapabilityRevokeDialogError;
  onSubmit: (vars: { retroactive: boolean; reason?: string }) => void;
}

export function CapabilityRevokeDialog(props: CapabilityRevokeDialogProps) {
  const { open, onClose, grant, submitting = false, submitError, onSubmit } = props;
  const [retroactive, setRetroactive] = useState(false);
  const [reason, setReason] = useState('');

  useEffect(() => {
    if (open) {
      setRetroactive(false);
      setReason('');
    }
  }, [open]);

  if (grant === null) return null;

  const trimmed = reason.trim();
  const canSubmit = !submitting && (!retroactive || trimmed.length > 0);

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Revoke grant"
      footer={
        <>
          <Button onClick={onClose} disabled={submitting}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={!canSubmit}
            onClick={() =>
              onSubmit({ retroactive, ...(trimmed.length > 0 ? { reason: trimmed } : {}) })
            }
            data-testid="capability-revoke-submit"
          >
            {submitting ? (
              <>
                <CircularProgress size={14} color="inherit" sx={{ mr: 1 }} />
                Submitting…
              </>
            ) : retroactive ? (
              'Revoke retroactively'
            ) : (
              'Revoke'
            )}
          </Button>
        </>
      }
    >
      <Box data-testid="capability-revoke-form">
        <Stack spacing={2}>
          <Typography variant="body1">
            {'Revoke the grant to '}
            <Box component="span" sx={{ fontFamily: 'monospace' }} title={grant.grantee}>
              {truncateActorId(grant.grantee)}
            </Box>
            {` for ${grant.statementKinds.join(', ')}?`}
          </Typography>
          <TextField
            label={retroactive ? 'Reason (required)' : 'Reason (optional)'}
            value={reason}
            onChange={(event) => setReason(event.target.value)}
            multiline
            minRows={2}
            inputProps={{ 'data-testid': 'capability-revoke-reason' }}
          />
          <FormControlLabel
            label="Retroactive — invalidate from inception"
            control={
              <Checkbox
                checked={retroactive}
                onChange={(event) => setRetroactive(event.target.checked)}
                inputProps={
                  {
                    'data-testid': 'capability-revoke-retroactive',
                  } as React.InputHTMLAttributes<HTMLInputElement>
                }
              />
            }
          />
          {retroactive && (
            <Alert severity="warning" data-testid="capability-revoke-retroactive-warning">
              Every statement the grantee signed under this grant becomes unauthorized, not just
              future ones. Use this only for grant fraud or a compromised grantee key.
            </Alert>
          )}
          {submitError !== undefined && (
            <Alert severity="error" role="alert" data-testid="capability-revoke-error">
              {submitError.message}
            </Alert>
          )}
        </Stack>
      </Box>
    </Dialog>
  );
}
