// Smart wrapper around `@kairo/ui`'s `<CapabilityRevokeDialog />`.
// The parent picks which head to revoke (only heads the signed-in
// actor issued and that carry no revocation are offered); the
// daemon still enforces that only the grantor may revoke.

import { useState } from 'react';
import { useRevokeCapability, useSession, type CapabilityHeadDto } from '@kairo/api-client';
import { CapabilityRevokeDialog } from '@kairo/ui';
import { toDialogError, type DialogError } from './dialogError';

export interface CapabilityRevokeDialogContainerProps {
  /** The head to revoke; `null` keeps the dialog closed. */
  head: CapabilityHeadDto | null;
  onClose: () => void;
  objectId: string;
}

export function CapabilityRevokeDialogContainer(props: CapabilityRevokeDialogContainerProps) {
  const { head, onClose, objectId } = props;
  const sessionQ = useSession();
  const mutation = useRevokeCapability();
  const [stickyError, setStickyError] = useState<DialogError | undefined>(undefined);

  if (sessionQ.data === null || sessionQ.data === undefined) {
    return null;
  }
  const byActor = sessionQ.data.actor_id;

  const effectiveError =
    (mutation.error !== null ? toDialogError(mutation.error) : undefined) ?? stickyError;

  const handleClose = () => {
    setStickyError(undefined);
    mutation.reset();
    onClose();
  };

  const handleSubmit = ({ retroactive, reason }: { retroactive: boolean; reason?: string }) => {
    if (head === null) return;
    setStickyError(undefined);
    mutation.mutate(
      {
        byActor,
        objectId,
        revokedGrant: head.statement_id,
        retroactive,
        ...(reason !== undefined ? { reason } : {}),
      },
      {
        onSuccess: handleClose,
        onError: (error) => setStickyError(toDialogError(error)),
      },
    );
  };

  return (
    <CapabilityRevokeDialog
      open={head !== null}
      onClose={handleClose}
      grant={
        head === null
          ? null
          : {
              statementId: head.statement_id,
              grantee: head.grantee,
              statementKinds: head.statement_kinds ?? [],
            }
      }
      submitting={mutation.isPending}
      {...(effectiveError !== undefined ? { submitError: effectiveError } : {})}
      onSubmit={handleSubmit}
    />
  );
}
