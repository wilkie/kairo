// Smart wrapper around `@kairo/ui`'s `<TrustOpinionDialog />`.
// Owns the mutation, bridges the session + mutation state into
// the dumb component's props, and clears any sticky error on
// dialog close.
//
// The container reads `useSession()` to fill in the `byActor`
// field automatically — callers don't need to thread it
// through. When the session is absent, the dialog stays closed
// (the trigger should be hidden anyway, but this is a
// defensive guard for the race where the session expires
// between trigger click and dialog mount).

import { useState } from 'react';
import { useCreateTrustOpinion, useSession } from '@kairo/api-client';
import { TrustOpinionDialog, type TrustOpinionDialogError } from '@kairo/ui';
import { toDialogError } from './dialogError';

export interface TrustOpinionDialogContainerProps {
  open: boolean;
  onClose: () => void;
  /** Actor the opinion is *about* (typically the actor whose
   * page is being viewed). */
  trustedActor: string;
  /** Optional: fires after a successful submission, before the
   * mutation cache invalidation completes. The container also
   * calls `onClose()` on success internally. */
  onSubmitted?: () => void;
}

export function TrustOpinionDialogContainer(props: TrustOpinionDialogContainerProps) {
  const { open, onClose, trustedActor, onSubmitted } = props;
  const sessionQ = useSession();
  const mutation = useCreateTrustOpinion();
  const [stickyError, setStickyError] = useState<TrustOpinionDialogError | undefined>(undefined);

  if (sessionQ.data === null || sessionQ.data === undefined) {
    // Defensive: trigger should already be hidden when
    // anonymous; bail rather than render the form.
    return null;
  }

  const byActor = sessionQ.data.actor_id;
  const errorFromMutation = mutation.error !== null ? toDialogError(mutation.error) : undefined;
  const effectiveError = errorFromMutation ?? stickyError;

  const handleClose = () => {
    setStickyError(undefined);
    mutation.reset();
    onClose();
  };

  const handleSubmit = ({
    decision,
    reason,
  }: {
    decision: 'trusted' | 'untrusted';
    reason: string | undefined;
  }) => {
    setStickyError(undefined);
    const vars: import('@kairo/api-client').CreateTrustOpinionVars = {
      byActor,
      trustedActor,
      decision,
      ...(reason !== undefined ? { reason } : {}),
    };
    mutation.mutate(vars, {
      onSuccess: () => {
        onSubmitted?.();
        handleClose();
      },
      onError: (error) => setStickyError(toDialogError(error)),
    });
  };

  return (
    <TrustOpinionDialog
      open={open}
      onClose={handleClose}
      trustedActor={trustedActor}
      byActor={byActor}
      submitting={mutation.isPending}
      {...(effectiveError !== undefined ? { submitError: effectiveError } : {})}
      onSubmit={handleSubmit}
    />
  );
}
