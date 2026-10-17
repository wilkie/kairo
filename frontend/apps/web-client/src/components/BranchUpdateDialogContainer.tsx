// Smart wrapper around `@kairo/ui`'s `<BranchUpdateDialog />`.
// Owns the mutation + lookups against the existing branch tips
// and revisions, then maps results into the dumb component's
// props.
//
// Auto-derived `supersedes`: when the signed-in actor already
// published a tip for the typed name, the container looks up
// its `statement_id` and threads it through as `supersedes`
// on submit. The dialog itself renders the "Replaces your
// prior tip" hint based on the same lookup.

import { useState } from 'react';
import {
  useBranches,
  useRevisions,
  useSession,
  useUpdateBranch,
  type KairoApiClientError,
} from '@kairo/api-client';
import {
  BranchUpdateDialog,
  type BranchUpdateDialogError,
  type BranchUpdateDialogRevision,
} from '@kairo/ui';

export interface BranchUpdateDialogContainerProps {
  open: boolean;
  onClose: () => void;
  objectId: string;
  /** Optional: fires after a successful submission. The
   * container clears its sticky error and closes the dialog
   * automatically on success. */
  onSubmitted?: () => void;
}

export function BranchUpdateDialogContainer(props: BranchUpdateDialogContainerProps) {
  const { open, onClose, objectId, onSubmitted } = props;
  const sessionQ = useSession();
  const branchesQ = useBranches(objectId);
  const revisionsQ = useRevisions(objectId);
  const mutation = useUpdateBranch();
  const [stickyError, setStickyError] = useState<BranchUpdateDialogError | undefined>(undefined);

  if (sessionQ.data === null || sessionQ.data === undefined) {
    return null;
  }

  const byActor = sessionQ.data.actor_id;
  const tips = branchesQ.data ?? [];
  const ownTips = tips.filter((tip) => tip.actor === byActor);
  const existingNames = ownTips.map((tip) => tip.name);

  // Revisions are sorted oldest-first server-side; reverse so
  // the dialog shows newest at the top and pre-selects it.
  const revisions: BranchUpdateDialogRevision[] = [...(revisionsQ.data ?? [])]
    .reverse()
    .map((row) => ({
      statementId: row.statement_id,
      revisionId: row.revision_id,
      createdAt: row.created_at,
    }));

  const errorFromMutation = mutation.error !== null ? toDialogError(mutation.error) : undefined;
  const effectiveError = errorFromMutation ?? stickyError;

  const handleClose = () => {
    setStickyError(undefined);
    mutation.reset();
    onClose();
  };

  const handleSubmit = ({
    name,
    revisionStatementId,
  }: {
    name: string;
    revisionStatementId: string;
  }) => {
    setStickyError(undefined);
    const supersedes = ownTips.find((tip) => tip.name === name)?.statement_id;
    const vars: import('@kairo/api-client').UpdateBranchVars = {
      byActor,
      objectId,
      name,
      revisionStatementId,
      ...(supersedes !== undefined ? { supersedes } : {}),
    };
    mutation.mutate(vars, {
      onSuccess: () => {
        onSubmitted?.();
        handleClose();
      },
      // Keep the dialog open on failure so the user can see the
      // inline error and adjust inputs (a `not_authorized`
      // surface or a typo, for instance).
      onError: (error) => setStickyError(toDialogError(error)),
    });
  };

  return (
    <BranchUpdateDialog
      open={open}
      onClose={handleClose}
      objectId={objectId}
      byActor={byActor}
      existingNames={existingNames}
      revisions={revisions}
      submitting={mutation.isPending}
      {...(effectiveError !== undefined ? { submitError: effectiveError } : {})}
      onSubmit={handleSubmit}
    />
  );
}

function toDialogError(error: KairoApiClientError): BranchUpdateDialogError {
  const detail = error.detail;
  if (detail.kind === 'daemon') {
    return { code: detail.code, message: detail.message };
  }
  if (detail.kind === 'network') {
    return { code: 'network', message: detail.message };
  }
  return { code: 'decode', message: detail.message };
}
