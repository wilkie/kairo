// `/actors/$id` — actor genesis inspection plus the per-actor
// signed-statement audit list. Per `WEB_CLIENT.md` §15 every
// panel renders a locality state distinct from validity; v1
// always renders `local`.
//
// The Statements panel is backed by the daemon's
// `statements_by_actor` materialized index — `ObjectGenesis` is
// excluded server-side because it carries `created_by` rather
// than the envelope `actor` field every other statement type
// uses, so this panel is "what this actor *signed*", not "what
// this actor caused to exist". The owned-objects view is a
// separate follow-up.

import {
  useActor,
  useStatementsByActor,
  type ActorGenesisJson,
  type StatementByActorDto,
} from '@kairo/api-client';
import { idPrefix, statementKindLabel } from '@kairo/object-model';
import {
  EmptyState,
  LocalityBadge,
  Panel,
  Table,
  type TableColumn,
} from '@kairo/ui';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import type { ReactNode } from 'react';
import { IdLink, IdText } from '../components/IdLink';
import { QueryStatusBoundary } from '../components/QueryStatusBoundary';

export interface ActorDetailProps {
  id: string;
}

export function ActorDetail({ id }: ActorDetailProps) {
  // Wire form is the bare payload (the daemon emits and accepts
  // bare ids); `kairo:actor:` is presentational and composed
  // for the page header below.
  const actorQ = useActor(id);
  const statementsQ = useStatementsByActor(id);

  return (
    <>
      <Typography variant="h2" component="h2">
        Actor
      </Typography>
      <Box sx={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', wordBreak: 'break-all' }}>
        {idPrefix('actor')}
        {id}
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
        description="Every signed envelope this actor authored, sorted oldest first. ObjectGenesis is excluded; the daemon's per-actor index keys off the envelope signer."
        actions={<LocalityBadge state="local" />}
      >
        <QueryStatusBoundary query={statementsQ}>
          {(rows) => <StatementsTable rows={rows} />}
        </QueryStatusBoundary>
      </Panel>
    </>
  );
}

function StatementsTable({ rows }: { rows: ReadonlyArray<StatementByActorDto> }) {
  const columns: ReadonlyArray<TableColumn<StatementByActorDto>> = [
    { key: 'kind', header: 'Kind', cell: (r) => <strong>{statementKindLabel(r.kind)}</strong> },
    {
      key: 'statement',
      header: 'Statement',
      cell: (r) => <IdLink kind="statement" id={r.statement_id} />,
    },
    { key: 'created_at', header: 'Created', cell: (r) => r.created_at },
  ];
  return (
    <Table
      columns={columns}
      rows={rows}
      rowKey={(r) => r.statement_id}
      emptyState={
        <EmptyState
          title="No signed statements"
          description="This actor has not signed any envelopes yet (excluding ObjectGenesis, which is tracked separately)."
        />
      }
    />
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
