// `<ProvenanceBadge />` — "Fetched from <peer>" for a record a
// federation pull brought in (`specs/DECISIONS.md` §16
// sub-decision 8, `PHASE_2_FEDERATION.md` slice 5).
//
// Provenance names only the immediate peer, and the peer's node
// actor, which the pull authenticated by its signed node document
// (`DECISIONS.md` §25). Authorship is the signature's job, so this
// badge sits beside the locality badge rather than replacing anything
// about validity. Renders nothing for a local record (no entries).
//
// Dumb component: the parent fetches provenance and passes entries.

import Box from '@mui/material/Box';
import Tooltip from '@mui/material/Tooltip';
import { StatusBadge } from './StatusBadge';
import { truncateActorId } from './ActorPicker';

export interface ProvenanceBadgeEntry {
  peerUrl: string;
  /** Authenticated by the peer's signed node document. */
  peerActor: string;
  /** RFC 3339. */
  fetchedAt: string;
}

export interface ProvenanceBadgeProps {
  /** First-seen first, as the daemon returns them. */
  entries: ProvenanceBadgeEntry[];
}

/** `https://archive.example.org:8443` → `archive.example.org:8443`. */
export function peerLabel(peerUrl: string): string {
  return peerUrl.replace(/^https?:\/\//, '');
}

export function ProvenanceBadge({ entries }: ProvenanceBadgeProps) {
  const [first, ...rest] = entries;
  if (first === undefined) return null;
  const label =
    rest.length === 0
      ? `Fetched from ${peerLabel(first.peerUrl)}`
      : `Fetched from ${peerLabel(first.peerUrl)} +${rest.length}`;
  const details = entries
    .map(
      (entry) =>
        `${entry.peerUrl} at ${entry.fetchedAt} (node actor ${truncateActorId(entry.peerActor)}, authenticated)`,
    )
    .join('\n');
  return (
    <Tooltip
      title={<Box sx={{ whiteSpace: 'pre-line' }}>{details}</Box>}
      describeChild
      data-testid="provenance-badge-tooltip"
    >
      <Box component="span" data-testid="provenance-badge" aria-label={`${label}. ${details}`}>
        <StatusBadge tone="info">{label}</StatusBadge>
      </Box>
    </Tooltip>
  );
}
