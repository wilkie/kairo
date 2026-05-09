// `/actors/$id` — actor genesis inspection. Per `WEB_CLIENT.md`
// §15 the locality state is rendered alongside, distinct from
// validity. v1 always renders `local`.
//
// Slice 8 ships only the genesis fields; the "signed statements
// observable for this actor" listing is deferred behind a
// placeholder per the slice plan (option (a) — defer the
// listing until we either index by-actor or accept the
// statements-dir scan cost).

import { useActor, type ActorGenesisJson } from '@kairo/api-client';
import { canonicalizeId } from '@kairo/object-model';
import { EmptyState, LocalityBadge, Panel } from '@kairo/ui';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import type { ReactNode } from 'react';
import { IdText } from '../components/IdLink';
import { QueryStatusBoundary } from '../components/QueryStatusBoundary';

export interface ActorDetailProps {
  id: string;
}

export function ActorDetail({ id }: ActorDetailProps) {
  // Accept either the canonical `kairo:actor:<payload>` form
  // or the bare payload — the kind is implied by the route.
  const actorId = canonicalizeId('actor', id);
  const actorQ = useActor(actorId);

  return (
    <>
      <Typography variant="h2" component="h2">
        Actor
      </Typography>
      <Box sx={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', wordBreak: 'break-all' }}>
        {actorId}
      </Box>

      <Panel
        title="Genesis"
        description="The signed statement that creates this actor."
        actions={<LocalityBadge state="local" />}
      >
        <QueryStatusBoundary query={actorQ}>
          {(data) => <GenesisFields data={data} />}
        </QueryStatusBoundary>
      </Panel>

      <Panel
        title="Signed statements"
        description="What this actor has signed in the local store."
        actions={<LocalityBadge state="local" />}
      >
        <EmptyState
          title="Statements list — coming soon"
          description="Listing statements by signer requires either a per-actor index or a full statements-dir scan; the daemon endpoint and its cost trade-off are a follow-up to slice 8."
        />
      </Panel>
    </>
  );
}

function GenesisFields({ data }: { data: ActorGenesisJson }) {
  return (
    <Stack spacing={1.5}>
      <KeyValue label="Actor kind" value={<code>{data.actor_kind}</code>} />
      <KeyValue label="Created at" value={data.created_at} />
      <KeyValue label="Nonce" value={<IdText id={data.nonce} />} />
      <KeyValue
        label="Initial key"
        value={
          <Stack spacing={0.5}>
            <span>
              <code>{data.initial_key.algorithm}</code>
            </span>
            <IdText id={data.initial_key.bytes} />
          </Stack>
        }
      />
      <KeyValue
        label="Attestation threshold"
        value={`${data.attestation_threshold} of ${data.attestation_keys.length}`}
      />
      <KeyValue
        label="Attestation keys"
        value={
          <Stack spacing={1}>
            {data.attestation_keys.map((key, idx) => (
              <Box key={`${key.algorithm}:${key.bytes}:${idx}`} sx={{ display: 'grid', rowGap: 0.25 }}>
                <span>
                  <code>{key.algorithm}</code>
                </span>
                <IdText id={key.bytes} />
              </Box>
            ))}
          </Stack>
        }
      />
    </Stack>
  );
}

function KeyValue({ label, value }: { label: string; value: ReactNode }) {
  return (
    <Box sx={{ display: 'grid', gridTemplateColumns: '12rem 1fr', columnGap: 2, alignItems: 'baseline' }}>
      <Typography variant="body2" sx={{ color: 'text.secondary' }}>
        {label}
      </Typography>
      <Box>{value}</Box>
    </Box>
  );
}
