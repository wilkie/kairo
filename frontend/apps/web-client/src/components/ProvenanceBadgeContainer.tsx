// Fetches provenance for one record and renders `<ProvenanceBadge />`
// (`PHASE_2_FEDERATION.md` slice 5). Provenance is advisory: while
// loading, on error, or for a local record, this renders nothing
// rather than an error state.

import { useProvenance } from '@kairo/api-client';
import { ProvenanceBadge } from '@kairo/ui';

export interface ProvenanceBadgeContainerProps {
  /** Typed reference: `statement:<id>`, `object:<id>`, `actor:<id>`. */
  record: string;
}

export function ProvenanceBadgeContainer({ record }: ProvenanceBadgeContainerProps) {
  const query = useProvenance(record);
  if (query.data === undefined) return null;
  return (
    <ProvenanceBadge
      entries={query.data.entries.map((entry) => ({
        peerUrl: entry.peer_url,
        peerActor: entry.peer_actor,
        fetchedAt: entry.fetched_at,
      }))}
    />
  );
}
