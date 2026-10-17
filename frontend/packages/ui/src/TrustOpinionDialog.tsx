// `<TrustOpinionDialog />` — modal form for expressing a
// trust opinion via cookie-mode write.
//
// Dumb component: callbacks and current state come in as
// props. The smart container in the web-client app owns the
// mutation, fills in the implicit fields (signed-in actor +
// viewed actor), and handles the success / error surfaces.
//
// `decision` is restricted to `trusted` / `untrusted` in v1;
// withdrawal needs the `supersedes` chain (picking a prior
// opinion to retract), which is its own slice. Withdrawn
// reads existing entries — the picker doesn't need to write
// them yet.

import { useEffect, useState } from 'react';
import Alert from '@mui/material/Alert';
import Box from '@mui/material/Box';
import CircularProgress from '@mui/material/CircularProgress';
import FormControl from '@mui/material/FormControl';
import FormControlLabel from '@mui/material/FormControlLabel';
import Radio from '@mui/material/Radio';
import RadioGroup from '@mui/material/RadioGroup';
import Stack from '@mui/material/Stack';
import TextField from '@mui/material/TextField';
import Typography from '@mui/material/Typography';
import { Button } from './Button';
import { Dialog } from './Dialog';
import { truncateActorId } from './ActorPicker';

export type TrustDecision = 'trusted' | 'untrusted';

export interface TrustOpinionDialogError {
  code: string;
  message: string;
}

export interface TrustOpinionDialogProps {
  open: boolean;
  onClose: () => void;
  /** The actor the opinion is *about* (typically the actor
   * whose page is being viewed). */
  trustedActor: string;
  /** The actor expressing the opinion (the signed-in user). */
  byActor: string;
  /** `true` while the mutation is in flight. */
  submitting?: boolean;
  /** Surface a submission error inline above the buttons. */
  submitError?: TrustOpinionDialogError;
  /** Fires when the user clicks **Submit**. */
  onSubmit: (vars: { decision: TrustDecision; reason: string | undefined }) => void;
}

export function TrustOpinionDialog(props: TrustOpinionDialogProps) {
  const { open, onClose, trustedActor, byActor, submitting = false, submitError, onSubmit } = props;

  const [decision, setDecision] = useState<TrustDecision>('trusted');
  const [reason, setReason] = useState('');

  // Reset the form whenever the dialog reopens — otherwise a
  // prior session's fields would persist across closes.
  useEffect(() => {
    if (open) {
      setDecision('trusted');
      setReason('');
    }
  }, [open]);

  const handleSubmit = () => {
    const trimmed = reason.trim();
    onSubmit({ decision, reason: trimmed.length > 0 ? trimmed : undefined });
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Express trust opinion"
      footer={
        <>
          <Button onClick={onClose} disabled={submitting}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={submitting}
            onClick={handleSubmit}
            data-testid="trust-opinion-submit"
          >
            {submitting ? (
              <>
                <CircularProgress size={14} color="inherit" sx={{ mr: 1 }} />
                Submitting…
              </>
            ) : (
              'Submit'
            )}
          </Button>
        </>
      }
    >
      <Box data-testid="trust-opinion-form">
        <Stack spacing={2}>
          <Box>
            <Typography variant="body2" sx={{ color: 'text.secondary' }}>
              About
            </Typography>
            <Typography variant="body1" sx={{ fontFamily: 'monospace' }} title={trustedActor}>
              {truncateActorId(trustedActor)}
            </Typography>
          </Box>
          <Box>
            <Typography variant="body2" sx={{ color: 'text.secondary' }}>
              Signing as
            </Typography>
            <Typography variant="body1" sx={{ fontFamily: 'monospace' }} title={byActor}>
              {truncateActorId(byActor)}
            </Typography>
          </Box>
          <FormControl>
            <Typography variant="body2" sx={{ color: 'text.secondary', mb: 0.5 }}>
              Decision
            </Typography>
            <RadioGroup
              row
              value={decision}
              onChange={(event) => setDecision(event.target.value as TrustDecision)}
            >
              <FormControlLabel
                value="trusted"
                control={<Radio data-testid="trust-decision-trusted" />}
                label="Trusted"
              />
              <FormControlLabel
                value="untrusted"
                control={<Radio data-testid="trust-decision-untrusted" />}
                label="Untrusted"
              />
            </RadioGroup>
          </FormControl>
          <TextField
            label="Reason (optional)"
            multiline
            minRows={2}
            value={reason}
            onChange={(event) => setReason(event.target.value)}
            inputProps={{ 'data-testid': 'trust-reason-input' }}
          />
          {submitError !== undefined && (
            <Alert severity="error" role="alert">
              {submitError.message}
            </Alert>
          )}
        </Stack>
      </Box>
    </Dialog>
  );
}
