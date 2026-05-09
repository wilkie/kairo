// Inline, monospace, click-to-navigate ID rendering for table
// cells. Truncates the id for display while keeping the full
// value as a tooltip + accessible label so screen readers and
// copy-paste both still work.
//
// Centralized here so route pages don't repeat the
// `Link + truncateId + tooltip + monospace` recipe per cell.

import { Link } from '@tanstack/react-router';
import { bareId, type IdKind, truncateId } from '@kairo/object-model';
import Tooltip from '@mui/material/Tooltip';

const MONOSPACE = 'ui-monospace, SFMono-Regular, Menlo, monospace';

const KIND_TO_PATH = {
  object: '/objects/$id',
  actor: '/actors/$id',
  statement: '/statements/$id',
  blob: '/blobs/$id',
} as const;

export interface IdLinkProps {
  /** The destination kind drives the route. */
  kind: IdKind;
  /** The full id. Always preserved as the accessible label;
   * stripped of its `kairo:<kind>:` prefix in the URL slug
   * (the kind is implied by the route) and truncated for
   * display. */
  id: string;
}

export function IdLink({ kind, id }: IdLinkProps) {
  return (
    <Tooltip title={id} arrow>
      <Link
        to={KIND_TO_PATH[kind]}
        params={{ id: bareId(kind, id) }}
        aria-label={id}
        style={{
          fontFamily: MONOSPACE,
          fontSize: '0.8125rem',
          wordBreak: 'break-all',
          color: 'inherit',
          textDecoration: 'underline',
          textDecorationColor: 'currentColor',
        }}
      >
        {truncateId(id)}
      </Link>
    </Tooltip>
  );
}

/** Read-only monospace span for ids/values that aren't
 * navigable (revision ids, manifest hashes, raw values). */
export function IdText({ id }: { id: string }) {
  return (
    <Tooltip title={id} arrow>
      <span
        aria-label={id}
        style={{
          fontFamily: MONOSPACE,
          fontSize: '0.8125rem',
          wordBreak: 'break-all',
        }}
      >
        {truncateId(id)}
      </span>
    </Tooltip>
  );
}
