// `<CapabilityGrantDialog />` — modal form for writing an
// object-scoped `ActorCapabilityGrant` via cookie-mode write.
//
// Unified create + update flow, parallel to the branch and tag
// dialogs. Each `(grantor, grantee, scope)` triple has a single
// chain whose leaf's kind list is the complete authorized set
// (`CAPABILITIES.md` §5.1.1). So when the typed grantee already
// holds a grant from the signed-in actor, the dialog switches to
// "Update", pre-fills that grant's kinds and delegable flag, and
// the container threads its head through as `supersedes`.
//
// All three object-scoped kinds are always offered. Whether the
// signer may grant a kind onward is the daemon's call
// (`evaluate_capability_for_regrant`); a refusal comes back as
// `submitError`. Re-deriving it here would duplicate capability
// semantics in the client.
//
// Dumb component: the parent owns lookups, the mutation, and
// error mapping.

import * as React from 'react';
import { useEffect, useState } from 'react';
import Alert from '@mui/material/Alert';
import Autocomplete from '@mui/material/Autocomplete';
import Box from '@mui/material/Box';
import Checkbox from '@mui/material/Checkbox';
import CircularProgress from '@mui/material/CircularProgress';
import FormControl from '@mui/material/FormControl';
import FormControlLabel from '@mui/material/FormControlLabel';
import FormGroup from '@mui/material/FormGroup';
import FormHelperText from '@mui/material/FormHelperText';
import FormLabel from '@mui/material/FormLabel';
import Stack from '@mui/material/Stack';
import TextField from '@mui/material/TextField';
import Typography from '@mui/material/Typography';
import { Button } from './Button';
import { Dialog } from './Dialog';
import { truncateActorId } from './ActorPicker';

export type CapabilityGrantKind = 'ObjectRevision' | 'ObjectBranch' | 'ObjectVersionTag';

export const CAPABILITY_GRANT_KINDS: readonly CapabilityGrantKind[] = [
  'ObjectRevision',
  'ObjectBranch',
  'ObjectVersionTag',
];

export interface CapabilityGrantDialogError {
  code: string;
  message: string;
}

/** A grant the signed-in actor already issued on this object. */
export interface CapabilityGrantDialogExisting {
  grantee: string;
  statementKinds: CapabilityGrantKind[];
  delegable: boolean;
}

export interface CapabilityGrantDialogSubmit {
  grantee: string;
  statementKinds: CapabilityGrantKind[];
  delegable: boolean;
  /** RFC 3339 UTC seconds (`YYYY-MM-DDT00:00:00Z`), or absent. */
  expiresAt?: string;
}

export interface CapabilityGrantDialogProps {
  open: boolean;
  onClose: () => void;
  /** Object the grant is scoped to. */
  objectId: string;
  /** Signed-in actor issuing the grant. */
  byActor: string;
  /** Actor ids offered as grantee suggestions (free text is
   * still accepted). */
  granteeSuggestions: string[];
  /** Grants `byActor` already issued on this object. */
  existing: CapabilityGrantDialogExisting[];
  /** `true` while the mutation is in flight. */
  submitting?: boolean;
  /** Surface a submission error inline above the buttons. */
  submitError?: CapabilityGrantDialogError;
  onSubmit: (vars: CapabilityGrantDialogSubmit) => void;
}

export function CapabilityGrantDialog(props: CapabilityGrantDialogProps) {
  const {
    open,
    onClose,
    objectId,
    byActor,
    granteeSuggestions,
    existing,
    submitting = false,
    submitError,
    onSubmit,
  } = props;

  const [grantee, setGrantee] = useState('');
  const [kinds, setKinds] = useState<CapabilityGrantKind[]>([]);
  const [delegable, setDelegable] = useState(false);
  const [expiresOn, setExpiresOn] = useState('');

  useEffect(() => {
    if (open) {
      setGrantee('');
      setKinds([]);
      setDelegable(false);
      setExpiresOn('');
    }
  }, [open]);

  const trimmed = grantee.trim();
  const prior = existing.find((grant) => grant.grantee === trimmed);

  // Typing (or picking) a grantee that already holds a grant
  // from this actor loads that grant, so the submitted list
  // starts from the complete current set. Done on change rather
  // than in an effect so later checkbox edits aren't overwritten.
  const handleGranteeChange = (value: string) => {
    setGrantee(value);
    const match = existing.find((grant) => grant.grantee === value.trim());
    if (match !== undefined) {
      setKinds(match.statementKinds);
      setDelegable(match.delegable);
    }
  };

  const toggleKind = (kind: CapabilityGrantKind) =>
    setKinds((current) =>
      current.includes(kind)
        ? current.filter((k) => k !== kind)
        : CAPABILITY_GRANT_KINDS.filter((k) => k === kind || current.includes(k)),
    );

  const canSubmit = trimmed.length > 0 && kinds.length > 0 && !submitting;

  const handleSubmit = () => {
    onSubmit({
      grantee: trimmed,
      statementKinds: kinds,
      delegable,
      ...(expiresOn.length > 0 ? { expiresAt: `${expiresOn}T00:00:00Z` } : {}),
    });
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={prior !== undefined ? 'Update grant' : 'Grant capability'}
      footer={
        <>
          <Button onClick={onClose} disabled={submitting}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={!canSubmit}
            onClick={handleSubmit}
            data-testid="capability-grant-submit"
          >
            {submitting ? (
              <>
                <CircularProgress size={14} color="inherit" sx={{ mr: 1 }} />
                Submitting…
              </>
            ) : prior !== undefined ? (
              'Update'
            ) : (
              'Grant'
            )}
          </Button>
        </>
      }
    >
      <Box data-testid="capability-grant-form">
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
              Granting as
            </Typography>
            <Typography variant="body1" sx={{ fontFamily: 'monospace' }} title={byActor}>
              {truncateActorId(byActor)}
            </Typography>
          </Box>
          <Autocomplete
            freeSolo
            options={granteeSuggestions}
            value={grantee}
            onInputChange={(_event, newValue) => handleGranteeChange(newValue)}
            renderInput={(params) => {
              // Same strict-TS cast as `<BranchUpdateDialog />`.
              const textFieldProps = params as React.ComponentProps<typeof TextField>;
              return (
                <TextField
                  {...textFieldProps}
                  label="Grantee actor"
                  placeholder="zQm…"
                  inputProps={{
                    ...params.inputProps,
                    'data-testid': 'capability-grantee-input',
                  }}
                />
              );
            }}
          />
          <FormControl component="fieldset">
            <FormLabel component="legend">Statement kinds</FormLabel>
            <FormGroup>
              {CAPABILITY_GRANT_KINDS.map((kind) => (
                <FormControlLabel
                  key={kind}
                  label={kind}
                  control={
                    <Checkbox
                      checked={kinds.includes(kind)}
                      onChange={() => toggleKind(kind)}
                      inputProps={
                        {
                          'data-testid': `capability-kind-${kind}`,
                        } as React.InputHTMLAttributes<HTMLInputElement>
                      }
                    />
                  }
                />
              ))}
            </FormGroup>
            <FormHelperText>
              The complete set the grantee will hold; unchecking a kind removes it.
            </FormHelperText>
          </FormControl>
          <FormControlLabel
            label="Delegable (grantee may grant these kinds onward)"
            control={
              <Checkbox
                checked={delegable}
                onChange={(event) => setDelegable(event.target.checked)}
                inputProps={
                  {
                    'data-testid': 'capability-delegable',
                  } as React.InputHTMLAttributes<HTMLInputElement>
                }
              />
            }
          />
          <TextField
            type="date"
            label="Expires (optional)"
            value={expiresOn}
            onChange={(event) => setExpiresOn(event.target.value)}
            InputLabelProps={{ shrink: true }}
            inputProps={{ 'data-testid': 'capability-expires-input' }}
            helperText="Expires at 00:00 UTC on this date. Leave empty for no expiry."
          />
          {prior !== undefined && (
            <Typography
              variant="caption"
              sx={{ color: 'text.secondary' }}
              data-testid="capability-grant-mode-hint"
            >
              {`Replaces your current grant to ${truncateActorId(trimmed)}.`}
            </Typography>
          )}
          {submitError !== undefined && (
            <Alert severity="error" role="alert" data-testid="capability-grant-error">
              {submitError.message}
            </Alert>
          )}
        </Stack>
      </Box>
    </Dialog>
  );
}
