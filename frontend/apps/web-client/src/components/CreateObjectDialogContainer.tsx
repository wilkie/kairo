// Smart wrapper around `@kairo/ui`'s `<CreateObjectDialog />`.
// Owns the genesis mutation, suggests the kinds the signed-in
// actor already uses, and navigates to the new object on success.

import { useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { useCreateObject, useObjectsByActor, useSession } from '@kairo/api-client';
import { bareId } from '@kairo/object-model';
import { CreateObjectDialog } from '@kairo/ui';
import { toDialogError, type DialogError } from './dialogError';

export interface CreateObjectDialogContainerProps {
  open: boolean;
  onClose: () => void;
}

export function CreateObjectDialogContainer({ open, onClose }: CreateObjectDialogContainerProps) {
  const sessionQ = useSession();
  const byActor = sessionQ.data?.actor_id ?? '';
  // Disabled until signed in: the hook takes an id, and '' would 400.
  const objectsQ = useObjectsByActor(byActor, { enabled: byActor.length > 0 });
  const mutation = useCreateObject();
  const navigate = useNavigate();
  const [stickyError, setStickyError] = useState<DialogError | undefined>(undefined);

  if (byActor.length === 0) return null;

  const kindSuggestions = [...new Set((objectsQ.data ?? []).map((row) => row.object_kind))];
  const effectiveError =
    (mutation.error !== null ? toDialogError(mutation.error) : undefined) ?? stickyError;

  const handleClose = () => {
    setStickyError(undefined);
    mutation.reset();
    onClose();
  };

  return (
    <CreateObjectDialog
      open={open}
      onClose={handleClose}
      byActor={byActor}
      kindSuggestions={kindSuggestions}
      submitting={mutation.isPending}
      {...(effectiveError !== undefined ? { submitError: effectiveError } : {})}
      onSubmit={({ objectKind }) => {
        setStickyError(undefined);
        mutation.mutate(
          { byActor, objectKind },
          {
            onSuccess: (result) => {
              handleClose();
              void navigate({
                to: '/objects/$id',
                params: { id: bareId('object', result.statement_id) },
              });
            },
            onError: (error) => setStickyError(toDialogError(error)),
          },
        );
      }}
    />
  );
}
