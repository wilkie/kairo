// `<VersionTagDialog />` — modal form for writing an
// `ObjectVersionTag` statement via cookie-mode write.
//
// Unified create + retarget flow, parallel to
// `<BranchUpdateDialog />`: the user types a semver version and
// picks the revision it should point at. When the signed-in
// actor already published a tag for that version the dialog
// labels itself "Retarget" and the container auto-derives
// `supersedes`; otherwise it says "Create".
//
// Version strings are checked against strict semver 2.0.0 here
// for UX only — the daemon's `SemverVersion::parse` is
// authoritative and rejects the same inputs (including a leading
// `v`).
//
// Dumb component: the parent owns the revision list, prior tag
// lookup, mutation lifecycle, and error mapping.

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
import type { BranchUpdateDialogError, BranchUpdateDialogRevision } from './BranchUpdateDialog';

export type VersionTagDialogError = BranchUpdateDialogError;

/** A revision the tag can target. Same shape as the branch
 * dialog's rows: `statementId` is what `ObjectVersionTag.target`
 * resolves to on the daemon side. */
export type VersionTagDialogRevision = BranchUpdateDialogRevision;

export interface VersionTagDialogProps {
  open: boolean;
  onClose: () => void;
  /** Object id the tag belongs to. */
  objectId: string;
  /** Signed-in actor publishing the tag. */
  byActor: string;
  /** Versions this actor has already tagged on the object.
   * Renders as autocomplete suggestions. */
  existingVersions: string[];
  /** Revisions available on the object, newest first. */
  revisions: VersionTagDialogRevision[];
  /** `true` while the mutation is in flight. */
  submitting?: boolean;
  /** Surface a submission error inline above the buttons. */
  submitError?: VersionTagDialogError;
  /** Fires when the user clicks **Submit**. */
  onSubmit: (vars: { version: string; revisionStatementId: string }) => void;
}

// semver.org's recommended strict 2.0.0 pattern: no leading
// `v`, no leading zeros in numeric identifiers.
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;

export function isStrictSemver(value: string): boolean {
  return SEMVER.test(value);
}

export function VersionTagDialog(props: VersionTagDialogProps) {
  const {
    open,
    onClose,
    objectId,
    byActor,
    existingVersions,
    revisions,
    submitting = false,
    submitError,
    onSubmit,
  } = props;

  const [version, setVersion] = useState('');
  const [revisionStatementId, setRevisionStatementId] = useState('');

  // Reset the form whenever the dialog reopens.
  useEffect(() => {
    if (open) {
      setVersion('');
      setRevisionStatementId(revisions[0]?.statementId ?? '');
    }
  }, [open, revisions]);

  const trimmed = version.trim();
  const valid = isStrictSemver(trimmed);
  const isRetarget = valid && existingVersions.includes(trimmed);
  const canSubmit = valid && revisionStatementId.length > 0 && !submitting;

  const handleSubmit = () => {
    onSubmit({ version: trimmed, revisionStatementId });
  };

  const versionHelper =
    trimmed.length === 0
      ? 'Strict semver, e.g. 1.2.0 or 2.0.0-rc.1.'
      : valid
        ? undefined
        : trimmed.startsWith('v') && isStrictSemver(trimmed.slice(1))
          ? `Drop the leading "v": ${trimmed.slice(1)}.`
          : 'Not a strict semver 2.0.0 version.';

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={!valid ? 'Tag version' : isRetarget ? `Retarget ${trimmed}` : `Tag ${trimmed}`}
      footer={
        <>
          <Button onClick={onClose} disabled={submitting}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={!canSubmit}
            onClick={handleSubmit}
            data-testid="version-tag-submit"
          >
            {submitting ? (
              <>
                <CircularProgress size={14} color="inherit" sx={{ mr: 1 }} />
                Submitting…
              </>
            ) : isRetarget ? (
              'Retarget'
            ) : (
              'Create'
            )}
          </Button>
        </>
      }
    >
      <Box data-testid="version-tag-form">
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
            options={existingVersions}
            value={version}
            onInputChange={(_event, newValue) => setVersion(newValue)}
            renderInput={(params) => {
              // Same strict-TS cast as `<BranchUpdateDialog />`.
              const textFieldProps = params as React.ComponentProps<typeof TextField>;
              return (
                <TextField
                  {...textFieldProps}
                  label="Version"
                  placeholder="1.0.0"
                  error={trimmed.length > 0 && !valid}
                  helperText={versionHelper}
                  inputProps={{
                    ...params.inputProps,
                    'data-testid': 'version-tag-input',
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
            inputProps={{ 'data-testid': 'version-tag-revision-select' }}
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
                data-testid={`version-tag-revision-option-${revision.statementId}`}
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
          {valid && (
            <Typography
              variant="caption"
              sx={{ color: 'text.secondary' }}
              data-testid="version-tag-mode-hint"
            >
              {isRetarget ? `Replaces your prior ${trimmed} tag.` : `Creates a new ${trimmed} tag.`}
            </Typography>
          )}
          {submitError !== undefined && (
            <Alert severity="error" role="alert" data-testid="version-tag-error">
              {submitError.message}
            </Alert>
          )}
        </Stack>
      </Box>
    </Dialog>
  );
}
