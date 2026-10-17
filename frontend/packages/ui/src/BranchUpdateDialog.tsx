// `<BranchUpdateDialog />` — modal form for writing an
// `ObjectBranch` statement via cookie-mode write.
//
// Unified create + update flow: the user picks a branch name
// and a target revision. The parent supplies
// `priorTipFromActor` whenever it knows the signed-in actor
// already published a tip with the typed name — in which case
// the dialog labels itself "Update" and the container
// auto-derives `supersedes`. When no prior tip matches,
// the label says "Create".
//
// Dumb component: the parent owns the revision list, prior
// tip lookup, mutation lifecycle, and error mapping.

import * as React from 'react';
import { useEffect, useState } from 'react';
import Alert from '@mui/material/Alert';
import Autocomplete from '@mui/material/Autocomplete';
import Box from '@mui/material/Box';
import CircularProgress from '@mui/material/CircularProgress';
import MenuItem from '@mui/material/MenuItem';
import Stack from '@mui/material/Stack';
import TextField from '@mui/material/TextField';
import Typography from '@mui/material/Typography';
import { Button } from './Button';
import { Dialog } from './Dialog';
import { truncateActorId } from './ActorPicker';

export interface BranchUpdateDialogError {
  code: string;
  message: string;
}

export interface BranchUpdateDialogRevision {
  /** Statement id of the revision; what `ObjectBranch.revision`
   * resolves to on the daemon side. Used as the field's submit
   * value. */
  statementId: string;
  /** Human-readable revision id (e.g. `git:sha256:…`) — what
   * the dropdown actually shows. */
  revisionId: string;
  /** RFC 3339 timestamp shown in the dropdown row for context. */
  createdAt: string;
}

export interface BranchUpdateDialogProps {
  open: boolean;
  onClose: () => void;
  /** Object id the branch belongs to. */
  objectId: string;
  /** Signed-in actor publishing the branch. */
  byActor: string;
  /** Existing branch names this actor has already published on
   * the object. Renders as autocomplete suggestions. */
  existingNames: string[];
  /** Revisions available on the object, newest first. */
  revisions: BranchUpdateDialogRevision[];
  /** `true` while the mutation is in flight. */
  submitting?: boolean;
  /** Surface a submission error inline above the buttons. */
  submitError?: BranchUpdateDialogError;
  /** Fires when the user clicks **Submit**. The container
   * decides whether the result is an update (prior tip exists)
   * or a fresh branch. */
  onSubmit: (vars: { name: string; revisionStatementId: string }) => void;
}

export function BranchUpdateDialog(props: BranchUpdateDialogProps) {
  const {
    open,
    onClose,
    objectId,
    byActor,
    existingNames,
    revisions,
    submitting = false,
    submitError,
    onSubmit,
  } = props;

  const [name, setName] = useState('');
  const [revisionStatementId, setRevisionStatementId] = useState('');

  // Reset the form whenever the dialog reopens.
  useEffect(() => {
    if (open) {
      setName('');
      // Pre-select the newest revision if any — almost always
      // the right default when updating a branch tip.
      setRevisionStatementId(revisions[0]?.statementId ?? '');
    }
  }, [open, revisions]);

  const trimmedName = name.trim();
  const isUpdate = trimmedName.length > 0 && existingNames.includes(trimmedName);
  const canSubmit =
    trimmedName.length > 0 && revisionStatementId.length > 0 && !submitting;

  const handleSubmit = () => {
    onSubmit({ name: trimmedName, revisionStatementId });
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={
        trimmedName.length === 0
          ? 'Update branch'
          : isUpdate
            ? `Update ${trimmedName}`
            : `Create ${trimmedName}`
      }
      footer={
        <>
          <Button onClick={onClose} disabled={submitting}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={!canSubmit}
            onClick={handleSubmit}
            data-testid="branch-update-submit"
          >
            {submitting ? (
              <>
                <CircularProgress size={14} color="inherit" sx={{ mr: 1 }} />
                Submitting…
              </>
            ) : isUpdate ? (
              'Update'
            ) : (
              'Create'
            )}
          </Button>
        </>
      }
    >
      <Box data-testid="branch-update-form">
        <Stack spacing={2}>
          <Box>
            <Typography variant="body2" sx={{ color: 'text.secondary' }}>
              Object
            </Typography>
            <Typography variant="body1" sx={{ fontFamily: 'monospace' }} title={objectId}>
              {truncateActorId(objectId)}
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
          <Autocomplete
            freeSolo
            options={existingNames}
            value={name}
            onInputChange={(_event, newValue) => setName(newValue)}
            renderInput={(params) => {
              // MUI's Autocomplete `renderInput` params spread
              // includes optional `size`/`hiddenLabel` fields
              // that clash with `exactOptionalPropertyTypes`
              // when passed to `TextField`. The cast is the
              // documented MUI-with-strict-TS workaround.
              const textFieldProps = params as React.ComponentProps<typeof TextField>;
              return (
                <TextField
                  {...textFieldProps}
                  label="Branch name"
                  placeholder="head, experimental, release-1.x …"
                  inputProps={{
                    ...params.inputProps,
                    'data-testid': 'branch-name-input',
                  }}
                />
              );
            }}
          />
          <TextField
            select
            label="Revision"
            value={revisionStatementId}
            onChange={(event) => setRevisionStatementId(event.target.value)}
            inputProps={{ 'data-testid': 'branch-revision-select' }}
            SelectProps={{ native: false }}
            disabled={revisions.length === 0}
            helperText={
              revisions.length === 0
                ? 'No revisions available for this object.'
                : 'Newest revision selected by default.'
            }
          >
            {revisions.map((revision) => (
              <MenuItem
                key={revision.statementId}
                value={revision.statementId}
                data-testid={`branch-revision-option-${revision.statementId}`}
              >
                <Stack sx={{ minWidth: 0 }}>
                  <Box
                    component="span"
                    sx={{
                      fontFamily: 'monospace',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                    }}
                    title={revision.revisionId}
                  >
                    {revision.revisionId}
                  </Box>
                  <Typography variant="caption" sx={{ color: 'text.secondary' }}>
                    {revision.createdAt}
                  </Typography>
                </Stack>
              </MenuItem>
            ))}
          </TextField>
          {trimmedName.length > 0 && (
            <Typography
              variant="caption"
              sx={{ color: 'text.secondary' }}
              data-testid="branch-mode-hint"
            >
              {isUpdate
                ? `Replaces your prior tip for ${trimmedName}.`
                : `Creates a new branch named ${trimmedName}.`}
            </Typography>
          )}
          {submitError !== undefined && (
            <Alert severity="error" role="alert" data-testid="branch-update-error">
              {submitError.message}
            </Alert>
          )}
        </Stack>
      </Box>
    </Dialog>
  );
}
