// `<CreateObjectDialog />` — modal form for writing an
// `ObjectGenesis` via cookie-mode write.
//
// The only caller-supplied field is the object kind. The daemon
// fills `created_by` (the session actor), `created_at`, and a
// fresh nonce, which together derive the new `ObjectId`; the
// signer becomes the object's root authority.
//
// Object kinds are free text — the specs fix no vocabulary — so
// the field suggests `software` (the CLI's and the spec's
// example) plus whatever kinds the parent passes in.
//
// Dumb component: the parent owns the mutation, error mapping,
// and what happens after success.

import * as React from 'react';
import { useEffect, useState } from 'react';
import Alert from '@mui/material/Alert';
import Autocomplete from '@mui/material/Autocomplete';
import Box from '@mui/material/Box';
import CircularProgress from '@mui/material/CircularProgress';
import Stack from '@mui/material/Stack';
import TextField from '@mui/material/TextField';
import Typography from '@mui/material/Typography';
import { Button } from './Button';
import { Dialog } from './Dialog';
import { truncateActorId } from './ActorPicker';

export interface CreateObjectDialogError {
  code: string;
  message: string;
}

export interface CreateObjectDialogProps {
  open: boolean;
  onClose: () => void;
  /** Signed-in actor who will own the object. */
  byActor: string;
  /** Extra kind suggestions (e.g. kinds this actor already
   * uses); `software` is always offered. */
  kindSuggestions?: string[];
  /** `true` while the mutation is in flight. */
  submitting?: boolean;
  /** Surface a submission error inline above the buttons. */
  submitError?: CreateObjectDialogError;
  onSubmit: (vars: { objectKind: string }) => void;
}

export function CreateObjectDialog(props: CreateObjectDialogProps) {
  const {
    open,
    onClose,
    byActor,
    kindSuggestions = [],
    submitting = false,
    submitError,
    onSubmit,
  } = props;
  const [kind, setKind] = useState('software');

  useEffect(() => {
    if (open) setKind('software');
  }, [open]);

  const trimmed = kind.trim();
  const canSubmit = trimmed.length > 0 && !submitting;
  const options = [...new Set(['software', ...kindSuggestions])];

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="New object"
      footer={
        <>
          <Button onClick={onClose} disabled={submitting}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={!canSubmit}
            onClick={() => onSubmit({ objectKind: trimmed })}
            data-testid="create-object-submit"
          >
            {submitting ? (
              <>
                <CircularProgress size={14} color="inherit" sx={{ mr: 1 }} />
                Creating…
              </>
            ) : (
              'Create'
            )}
          </Button>
        </>
      }
    >
      <Box data-testid="create-object-form">
        <Stack spacing={2}>
          <Box>
            <Typography variant="body2" sx={{ color: 'text.secondary' }}>
              Owner (root authority)
            </Typography>
            <Typography variant="body1" sx={{ fontFamily: 'monospace' }} title={byActor}>
              {truncateActorId(byActor)}
            </Typography>
          </Box>
          <Autocomplete
            freeSolo
            options={options}
            inputValue={kind}
            onInputChange={(_event, newValue) => setKind(newValue)}
            renderInput={(params) => {
              // Same strict-TS cast as `<BranchUpdateDialog />`.
              const textFieldProps = params as React.ComponentProps<typeof TextField>;
              return (
                <TextField
                  {...textFieldProps}
                  label="Object kind"
                  helperText="What this object archives, e.g. software."
                  inputProps={{
                    ...params.inputProps,
                    'data-testid': 'create-object-kind-input',
                  }}
                />
              );
            }}
          />
          <Typography variant="caption" sx={{ color: 'text.secondary' }}>
            Creates the object with no revisions. Its id is derived from you, the time, and a fresh
            nonce, so it cannot be chosen.
          </Typography>
          {submitError !== undefined && (
            <Alert severity="error" role="alert" data-testid="create-object-error">
              {submitError.message}
            </Alert>
          )}
        </Stack>
      </Box>
    </Dialog>
  );
}
