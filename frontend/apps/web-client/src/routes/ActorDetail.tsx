// `/actors/$id` — actor genesis inspection plus the per-actor
// owned-objects and signed-statement audit lists. Per
// `WEB_CLIENT.md` §15 every panel renders a locality state
// distinct from validity; v1 always renders `local`.
//
// Two complementary tables sit below the genesis panel:
//
//   - **Created objects** — backed by `objects_by_actor`. Lists
//     every `ObjectGenesis` whose `created_by` is this actor.
//   - **Signed statements** — backed by `statements_by_actor`.
//     Lists every signed envelope this actor authored.
//     `ObjectGenesis` is excluded server-side (it carries
//     `created_by`, not the envelope `actor` field every other
//     statement type uses), so this table is "what this actor
//     *signed*" — the Created Objects table is the complement.
//
// Together the two answer "what is this actor responsible for in
// the store?".

import {
  useActor,
  useObjectsByActor,
  useSession,
  useStatementsByActor,
  useTrustAbout,
  type ActorGenesisJson,
  type ObjectByActorDto,
  type StatementByActorDto,
  type TrustHeadDto,
} from '@kairo/api-client';
import { idPrefix, statementKindLabel } from '@kairo/object-model';
import {
  Button,
  EmptyState,
  LocalityBadge,
  Panel,
  StatusBadge,
  Table,
  type TableColumn,
} from '@kairo/ui';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import { useState, type ReactNode } from 'react';
import { IdLink, IdText } from '../components/IdLink';
import { QueryStatusBoundary } from '../components/QueryStatusBoundary';
import { TrustOpinionDialogContainer } from '../components/TrustOpinionDialogContainer';

export interface ActorDetailProps {
  id: string;
}

export function ActorDetail({ id }: ActorDetailProps) {
  // Wire form is the bare payload (the daemon emits and accepts
  // bare ids); `kairo:actor:` is presentational and composed
  // for the page header below.
  const actorQ = useActor(id);
  const objectsQ = useObjectsByActor(id);
  const statementsQ = useStatementsByActor(id);
  const trustQ = useTrustAbout(id);
  const sessionQ = useSession();
  const [trustDialogOpen, setTrustDialogOpen] = useState(false);

  const session = sessionQ.data ?? null;

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
        title="Trust opinions"
        description={`What other actors say about ${id.slice(0, 8)}…${id.slice(-4)}. Latest opinion per actor wins.`}
        actions={
          <Stack direction="row" spacing={1} alignItems="center">
            {session !== null && (
              <Button
                variant="primary"
                size="small"
                onClick={() => setTrustDialogOpen(true)}
                data-testid="express-trust-opinion"
              >
                Express trust opinion
              </Button>
            )}
            <LocalityBadge state="local" />
          </Stack>
        }
      >
        <QueryStatusBoundary query={trustQ}>
          {(rows) => <TrustTable rows={rows} subjectActor={id} />}
        </QueryStatusBoundary>
      </Panel>

      <Panel
        title="Created objects"
        description="Every object whose ObjectGenesis names this actor as `created_by`. Sorted oldest first."
        actions={<LocalityBadge state="local" />}
      >
        <QueryStatusBoundary query={objectsQ}>
          {(rows) => <CreatedObjectsTable rows={rows} />}
        </QueryStatusBoundary>
      </Panel>

      <Panel
        title="Signed statements"
        description="Every signed envelope this actor authored, sorted oldest first. ObjectGenesis is excluded; the Created Objects table above covers it."
        actions={<LocalityBadge state="local" />}
      >
        <QueryStatusBoundary query={statementsQ}>
          {(rows) => <StatementsTable rows={rows} />}
        </QueryStatusBoundary>
      </Panel>

      <TrustOpinionDialogContainer
        open={trustDialogOpen}
        onClose={() => setTrustDialogOpen(false)}
        trustedActor={id}
      />
    </>
  );
}

function TrustTable({
  rows,
  subjectActor,
}: {
  rows: ReadonlyArray<TrustHeadDto>;
  subjectActor: string;
}) {
  if (rows.length === 0) {
    return (
      <EmptyState
        title="No opinions recorded"
        description={`No actor has expressed trust about ${subjectActor.slice(0, 8)}…${subjectActor.slice(-4)}.`}
      />
    );
  }
  const columns: ReadonlyArray<TableColumn<TrustHeadDto>> = [
    { key: 'by', header: 'By actor', cell: (r) => <IdLink kind="actor" id={r.by_actor} /> },
    {
      key: 'decision',
      header: 'Decision',
      cell: (r) => <DecisionBadge decision={r.decision} />,
    },
    {
      key: 'statement',
      header: 'Statement',
      cell: (r) => <IdLink kind="statement" id={r.statement_id} />,
    },
    { key: 'created_at', header: 'Created', cell: (r) => r.created_at },
  ];
  return <Table columns={columns} rows={rows} rowKey={(r) => r.by_actor} />;
}

function DecisionBadge({ decision }: { decision: TrustHeadDto['decision'] }) {
  if (decision === null || decision === undefined) {
    return <StatusBadge tone="neutral">Withdrawn</StatusBadge>;
  }
  if (decision === 'trusted') {
    return <StatusBadge tone="success">Trusted</StatusBadge>;
  }
  if (decision === 'untrusted') {
    return <StatusBadge tone="error">Untrusted</StatusBadge>;
  }
  return <StatusBadge tone="neutral">{decision}</StatusBadge>;
}

function CreatedObjectsTable({ rows }: { rows: ReadonlyArray<ObjectByActorDto> }) {
  const columns: ReadonlyArray<TableColumn<ObjectByActorDto>> = [
    {
      key: 'object',
      header: 'Object',
      cell: (r) => <IdLink kind="object" id={r.object_id} />,
    },
    { key: 'kind', header: 'Kind', cell: (r) => <code>{r.object_kind}</code> },
    { key: 'created_at', header: 'Created', cell: (r) => r.created_at },
  ];
  return (
    <Table
      columns={columns}
      rows={rows}
      rowKey={(r) => r.object_id}
      emptyState={
        <EmptyState
          title="No created objects"
          description="This actor has not authored any ObjectGenesis statements in the local store."
        />
      }
    />
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
          description="This actor has not signed any envelopes yet (excluding ObjectGenesis, which is shown in the Created Objects table)."
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
