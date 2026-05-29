// `<ActorPicker />` — header widget that drives the inspector's
// sign-in / sign-out flow.
//
// Anonymous-by-default per `PHASE_2_WEB_CLIENT_AUTH.md` §1.2:
// a signed-out picker renders a single "Sign in" button. The
// popover lists the local keystore's actors; selecting one
// shows a confirmation step ("Sign in as <actor>? — Confirm /
// Cancel") before any network call. Once signed in, the
// picker shows the truncated id and a "Sign out" button.
//
// The component is **dumb** — no data fetching. Wire it from a
// smart container that owns the `useSession` / `useKeystoreActors`
// hooks and forwards state + callbacks here. This keeps the
// component trivially testable and reusable in Storybook /
// Playwright outside the inspector shell.

import { useEffect, useState, type MouseEvent } from 'react';
import Alert from '@mui/material/Alert';
import Box from '@mui/material/Box';
import CircularProgress from '@mui/material/CircularProgress';
import List from '@mui/material/List';
import ListItemButton from '@mui/material/ListItemButton';
import ListItemText from '@mui/material/ListItemText';
import Popover from '@mui/material/Popover';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import ArrowDropDownIcon from '@mui/icons-material/ArrowDropDown';
import { Button } from './Button';

/**
 * Minimal shape the picker needs out of each keystore actor.
 * Matches `@kairo/api-client`'s `KeystoreActorDto`, replicated
 * here so the UI package stays free of an api-client dep.
 */
export interface ActorPickerActor {
  actor_id: string;
}

/** Minimal session shape the picker reads. Matches
 * `@kairo/api-client`'s `SessionInfo`. */
export interface ActorPickerSession {
  actor_id: string;
}

/**
 * Structured sign-in error. `code` is the daemon's wire-stable
 * error code (e.g. `actor_not_in_keystore`, `key_revoked`).
 */
export interface ActorPickerError {
  code: string;
  message: string;
}

export interface ActorPickerProps {
  /** Current session, or `null` when the viewer is anonymous. */
  session: ActorPickerSession | null;
  /** Local keystore listing. Empty array → "no local actors" hint. */
  actors: ActorPickerActor[];
  /** `true` while `onSignIn` is in flight. */
  signInPending?: boolean;
  /** `true` while `onSignOut` is in flight. */
  signOutPending?: boolean;
  /** Last sign-in failure to surface inline; `undefined` = no error. */
  signInError?: ActorPickerError;
  /** `true` while the actors list is loading. Shows a small spinner inside the popover. */
  actorsLoading?: boolean;
  /** Fires when the user clicks **Confirm** in the sign-in dialog. */
  onSignIn: (actorId: string) => void;
  /** Fires when the user clicks **Sign out**. */
  onSignOut: () => void;
  /** Optional: clear a stale sign-in error (e.g. when the popover closes). */
  onClearError?: () => void;
}

const TRUNCATE_PREFIX = 8;
const TRUNCATE_SUFFIX = 4;

/** `zMockActo…0000`-style abbreviation for the picker label. */
export function truncateActorId(id: string): string {
  if (id.length <= TRUNCATE_PREFIX + TRUNCATE_SUFFIX + 1) return id;
  return `${id.slice(0, TRUNCATE_PREFIX)}…${id.slice(-TRUNCATE_SUFFIX)}`;
}

export function ActorPicker(props: ActorPickerProps) {
  const {
    session,
    actors,
    signInPending = false,
    signOutPending = false,
    signInError,
    actorsLoading = false,
    onSignIn,
    onSignOut,
    onClearError,
  } = props;

  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const [pendingConfirm, setPendingConfirm] = useState<string | null>(null);

  // When the parent transitions to a signed-in session (or back
  // to anonymous via sign-out), close any open popover and drop
  // any pending confirmation. Otherwise stale popover state
  // persists across the next anonymous render and the backdrop
  // intercepts clicks on the sign-in button.
  useEffect(() => {
    setAnchor(null);
    setPendingConfirm(null);
  }, [session]);

  const closePopover = () => {
    setAnchor(null);
    setPendingConfirm(null);
    onClearError?.();
  };

  if (session !== null) {
    return (
      <Stack
        direction="row"
        alignItems="center"
        spacing={1}
        sx={{ minWidth: 0 }}
        data-testid="actor-picker-signed-in"
      >
        <Typography
          variant="body2"
          sx={{ fontFamily: 'monospace', cursor: 'default' }}
          aria-label={`Signed in as ${session.actor_id}`}
          title={session.actor_id}
        >
          {truncateActorId(session.actor_id)}
        </Typography>
        <Button
          variant="ghost"
          size="small"
          onClick={onSignOut}
          disabled={signOutPending}
          aria-label="Sign out"
        >
          {signOutPending ? 'Signing out…' : 'Sign out'}
        </Button>
      </Stack>
    );
  }

  const openButton = (event: MouseEvent<HTMLButtonElement>) => {
    setAnchor(event.currentTarget);
  };

  return (
    <>
      <Button
        variant="default"
        size="small"
        onClick={openButton}
        endIcon={<ArrowDropDownIcon fontSize="small" />}
        aria-haspopup="dialog"
        aria-expanded={anchor !== null}
        data-testid="actor-picker-sign-in"
      >
        Sign in
      </Button>
      <Popover
        open={anchor !== null}
        anchorEl={anchor}
        onClose={closePopover}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'right' }}
        transformOrigin={{ vertical: 'top', horizontal: 'right' }}
        slotProps={{ paper: { sx: { width: 280, maxWidth: '90vw' } } }}
        data-testid="actor-picker-popover"
      >
        {pendingConfirm === null
          ? renderActorList({
              actors,
              actorsLoading,
              signInError,
              onPick: (id) => setPendingConfirm(id),
            })
          : renderConfirm({
              actorId: pendingConfirm,
              signInPending,
              signInError,
              onCancel: () => {
                setPendingConfirm(null);
                onClearError?.();
              },
              onConfirm: () => onSignIn(pendingConfirm),
            })}
      </Popover>
    </>
  );
}

function renderActorList(args: {
  actors: ActorPickerActor[];
  actorsLoading: boolean;
  signInError: ActorPickerError | undefined;
  onPick: (actorId: string) => void;
}) {
  const { actors, actorsLoading, signInError, onPick } = args;

  if (actorsLoading) {
    return (
      <Box sx={{ p: 2, display: 'flex', justifyContent: 'center' }}>
        <CircularProgress size={20} />
      </Box>
    );
  }

  return (
    <Box>
      <Typography variant="overline" sx={{ display: 'block', px: 2, pt: 1.5, pb: 0.5 }}>
        Sign in as
      </Typography>
      {signInError !== undefined && (
        <Alert severity="error" sx={{ mx: 2, mb: 1 }} role="alert">
          {signInError.message}
        </Alert>
      )}
      {actors.length === 0 ? (
        <Typography variant="body2" sx={{ px: 2, pb: 2, color: 'text.secondary' }}>
          No local actors. Run <code>kairo keygen</code> first.
        </Typography>
      ) : (
        <List dense disablePadding role="menu">
          {actors.map((actor) => (
            <ListItemButton
              key={actor.actor_id}
              onClick={() => onPick(actor.actor_id)}
              data-testid="actor-picker-actor"
              role="menuitem"
              title={actor.actor_id}
            >
              <ListItemText
                primary={
                  <Box component="span" sx={{ fontFamily: 'monospace' }}>
                    {truncateActorId(actor.actor_id)}
                  </Box>
                }
              />
            </ListItemButton>
          ))}
        </List>
      )}
    </Box>
  );
}

function renderConfirm(args: {
  actorId: string;
  signInPending: boolean;
  signInError: ActorPickerError | undefined;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { actorId, signInPending, signInError, onCancel, onConfirm } = args;
  return (
    <Box sx={{ p: 2 }} data-testid="actor-picker-confirm">
      <Typography variant="body2" sx={{ mb: 1 }}>
        Sign in as
      </Typography>
      <Typography
        variant="body1"
        sx={{ fontFamily: 'monospace', mb: 2 }}
        title={actorId}
      >
        {truncateActorId(actorId)}
      </Typography>
      {signInError !== undefined && (
        <Alert severity="error" sx={{ mb: 2 }} role="alert">
          {signInError.message}
        </Alert>
      )}
      <Stack direction="row" spacing={1} justifyContent="flex-end">
        <Button variant="ghost" onClick={onCancel} disabled={signInPending}>
          Cancel
        </Button>
        <Button
          variant="primary"
          onClick={onConfirm}
          disabled={signInPending}
          data-testid="actor-picker-confirm-button"
        >
          {signInPending ? (
            <>
              <CircularProgress size={14} color="inherit" sx={{ mr: 1 }} />
              Signing in…
            </>
          ) : (
            'Confirm'
          )}
        </Button>
      </Stack>
    </Box>
  );
}

