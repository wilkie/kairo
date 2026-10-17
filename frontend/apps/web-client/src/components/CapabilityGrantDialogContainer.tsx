// Smart wrapper around `@kairo/ui`'s `<CapabilityGrantDialog />`.
// Owns the grant mutation and the lookups the dialog needs:
// grantee suggestions from the local keystore, and the grants the
// signed-in actor already issued on this object.
//
// Auto-derived `supersedes`: a grant to a grantee who already
// holds one from this actor on this object extends that
// `(grantor, grantee, scope)` chain, so the container threads the
// current head's `statement_id` through (`CAPABILITIES.md`
// §5.1.1). The dialog has already pre-filled that head's kinds.

import { useState } from 'react';
import {
  useCapabilitiesForObject,
  useGrantCapability,
  useKeystoreActors,
  useSession,
  type GrantCapabilityVars,
} from '@kairo/api-client';
import {
  CAPABILITY_GRANT_KINDS,
  CapabilityGrantDialog,
  type CapabilityGrantDialogExisting,
  type CapabilityGrantDialogSubmit,
  type CapabilityGrantKind,
} from '@kairo/ui';
import { toDialogError, type DialogError } from './dialogError';

export interface CapabilityGrantDialogContainerProps {
  open: boolean;
  onClose: () => void;
  objectId: string;
}

function isGrantKind(kind: string): kind is CapabilityGrantKind {
  return (CAPABILITY_GRANT_KINDS as readonly string[]).includes(kind);
}

export function CapabilityGrantDialogContainer(props: CapabilityGrantDialogContainerProps) {
  const { open, onClose, objectId } = props;
  const sessionQ = useSession();
  const capsQ = useCapabilitiesForObject(objectId);
  const actorsQ = useKeystoreActors();
  const mutation = useGrantCapability();
  const [stickyError, setStickyError] = useState<DialogError | undefined>(undefined);

  if (sessionQ.data === null || sessionQ.data === undefined) {
    return null;
  }

  const byActor = sessionQ.data.actor_id;
  const ownHeads = (capsQ.data ?? []).filter((head) => head.grantor === byActor);
  const existing: CapabilityGrantDialogExisting[] = ownHeads.map((head) => ({
    grantee: head.grantee,
    statementKinds: (head.statement_kinds ?? []).filter(isGrantKind),
    delegable: head.delegable ?? false,
  }));
  const granteeSuggestions = (actorsQ.data ?? [])
    .map((actor) => actor.actor_id)
    .filter((id) => id !== byActor);

  const effectiveError =
    (mutation.error !== null ? toDialogError(mutation.error) : undefined) ?? stickyError;

  const handleClose = () => {
    setStickyError(undefined);
    mutation.reset();
    onClose();
  };

  const handleSubmit = ({
    grantee,
    statementKinds,
    delegable,
    expiresAt,
  }: CapabilityGrantDialogSubmit) => {
    setStickyError(undefined);
    const supersedes = ownHeads.find((head) => head.grantee === grantee)?.statement_id;
    const vars: GrantCapabilityVars = {
      byActor,
      objectId,
      grantee,
      statementKinds,
      delegable,
      ...(expiresAt !== undefined ? { expiresAt } : {}),
      ...(supersedes !== undefined ? { supersedes } : {}),
    };
    mutation.mutate(vars, {
      onSuccess: handleClose,
      onError: (error) => setStickyError(toDialogError(error)),
    });
  };

  return (
    <CapabilityGrantDialog
      open={open}
      onClose={handleClose}
      objectId={objectId}
      byActor={byActor}
      granteeSuggestions={granteeSuggestions}
      existing={existing}
      submitting={mutation.isPending}
      {...(effectiveError !== undefined ? { submitError: effectiveError } : {})}
      onSubmit={handleSubmit}
    />
  );
}
