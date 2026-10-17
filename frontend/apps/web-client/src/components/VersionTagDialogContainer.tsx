// Smart wrapper around `@kairo/ui`'s `<VersionTagDialog />`.
// Owns the mutation + lookups against the existing tag heads
// and revisions, then maps results into the dumb component's
// props. Mirrors `BranchUpdateDialogContainer`.
//
// Auto-derived `supersedes`: when the signed-in actor already
// published a tag for the typed version, the container threads
// that head's `statement_id` through as `supersedes` on submit,
// so the daemon accepts it as the next link in the actor's
// `(object, version)` chain.

import { useState } from 'react';
import {
  useRevisions,
  useSession,
  useSetVersionTag,
  useVersionTags,
  type KairoApiClientError,
  type SetVersionTagVars,
} from '@kairo/api-client';
import {
  VersionTagDialog,
  type VersionTagDialogError,
  type VersionTagDialogRevision,
} from '@kairo/ui';

export interface VersionTagDialogContainerProps {
  open: boolean;
  onClose: () => void;
  objectId: string;
  /** Optional: fires after a successful submission. The
   * container clears its sticky error and closes the dialog
   * automatically on success. */
  onSubmitted?: () => void;
}

export function VersionTagDialogContainer(props: VersionTagDialogContainerProps) {
  const { open, onClose, objectId, onSubmitted } = props;
  const sessionQ = useSession();
  const tagsQ = useVersionTags(objectId);
  const revisionsQ = useRevisions(objectId);
  const mutation = useSetVersionTag();
  const [stickyError, setStickyError] = useState<VersionTagDialogError | undefined>(undefined);

  if (sessionQ.data === null || sessionQ.data === undefined) {
    return null;
  }

  const byActor = sessionQ.data.actor_id;
  const ownHeads = (tagsQ.data ?? []).filter((head) => head.actor === byActor);
  const existingVersions = ownHeads.map((head) => head.version);

  // Revisions are sorted oldest-first server-side; reverse so
  // the dialog shows newest at the top and pre-selects it.
  const revisions: VersionTagDialogRevision[] = [...(revisionsQ.data ?? [])]
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
    version,
    revisionStatementId,
  }: {
    version: string;
    revisionStatementId: string;
  }) => {
    setStickyError(undefined);
    const supersedes = ownHeads.find((head) => head.version === version)?.statement_id;
    const vars: SetVersionTagVars = {
      byActor,
      objectId,
      version,
      revisionStatementId,
      ...(supersedes !== undefined ? { supersedes } : {}),
    };
    mutation.mutate(vars, {
      onSuccess: () => {
        onSubmitted?.();
        handleClose();
      },
      // Keep the dialog open on failure so the inline error
      // stays visible (`not_authorized`, a rejected version, …).
      onError: (error) => setStickyError(toDialogError(error)),
    });
  };

  return (
    <VersionTagDialog
      open={open}
      onClose={handleClose}
      objectId={objectId}
      byActor={byActor}
      existingVersions={existingVersions}
      revisions={revisions}
      submitting={mutation.isPending}
      {...(effectiveError !== undefined ? { submitError: effectiveError } : {})}
      onSubmit={handleSubmit}
    />
  );
}

function toDialogError(error: KairoApiClientError): VersionTagDialogError {
  const detail = error.detail;
  if (detail.kind === 'daemon') {
    return { code: detail.code, message: detail.message };
  }
  if (detail.kind === 'network') {
    return { code: 'network', message: detail.message };
  }
  return { code: 'decode', message: detail.message };
}
