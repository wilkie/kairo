// `/statements/$id` — the polymorphic statement envelope view.
// The daemon endpoint returns *any* signed statement variant
// (`ObjectGenesis`, `ObjectBranch`, etc.); we narrow on the
// `type` discriminator and emit a kind-specific summary, then
// always render the full envelope as a pretty-printed block
// so anything the summary doesn't surface is still inspectable.
//
// Per `WEB_CLIENT.md` §15 every panel carries a locality badge
// distinct from validity. v1 single-store stays `local`.

import {
  useStatement,
  type ActorTrustStatementJson,
  type ObjectBranchStatementJson,
  type ObjectGenesisStatementJson,
  type ObjectVersionTagStatementJson,
  type StatementValue,
} from '@kairo/api-client';
import {
  canonicalizeId,
  isActorTrustStatement,
  isObjectBranchStatement,
  isObjectGenesisStatement,
  isObjectVersionTagStatement,
  statementKindLabel,
  statementType,
} from '@kairo/object-model';
import { LocalityBadge, Panel, StatusBadge } from '@kairo/ui';
import { ValidationBadge } from '@kairo/validation-viewer';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import type { ReactNode } from 'react';
import { IdLink, IdText } from '../components/IdLink';
import { QueryStatusBoundary } from '../components/QueryStatusBoundary';

export interface StatementDetailProps {
  id: string;
}

export function StatementDetail({ id }: StatementDetailProps) {
  // Accept either the canonical `kairo:stmt:<payload>` form
  // or the bare payload — the kind is implied by the route.
  const statementId = canonicalizeId('statement', id);
  const stmtQ = useStatement(statementId);

  return (
    <>
      <Typography variant="h2" component="h2">
        Statement
      </Typography>
      <Box sx={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', wordBreak: 'break-all' }}>
        {statementId}
      </Box>

      <QueryStatusBoundary query={stmtQ}>
        {(value) => <StatementBody value={value} />}
      </QueryStatusBoundary>
    </>
  );
}

function StatementBody({ value }: { value: StatementValue }) {
  const kind = statementType(value);
  const kindLabel = kind === null ? 'Unknown' : statementKindLabel(kind);

  return (
    <>
      <Panel
        title={kindLabel}
        description={
          <>
            Typed summary derived from the body&rsquo;s kind discriminator. This page does not
            run verification &mdash; navigate to the owning object to see the daemon&rsquo;s
            validation status (per <code>WEB_CLIENT.md</code> &sect;10, raw statement views must
            never imply validity).
          </>
        }
        actions={
          <Stack direction="row" spacing={1}>
            <StatusBadge tone="info">{kind ?? 'unknown'}</StatusBadge>
            <ValidationBadge status="unverified" />
            <LocalityBadge state="local" />
          </Stack>
        }
      >
        <TypedSummary value={value} />
      </Panel>

      <Panel
        title="Envelope"
        description="The raw signed statement as the daemon returned it."
        actions={<LocalityBadge state="local" />}
      >
        <Box
          component="pre"
          sx={{
            fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
            fontSize: '0.8125rem',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            m: 0,
          }}
        >
          {JSON.stringify(value, null, 2)}
        </Box>
      </Panel>
    </>
  );
}

function TypedSummary({ value }: { value: StatementValue }) {
  if (isObjectGenesisStatement(value)) {
    return <ObjectGenesisSummary stmt={value} />;
  }
  if (isObjectBranchStatement(value)) {
    return <ObjectBranchSummary stmt={value} />;
  }
  if (isObjectVersionTagStatement(value)) {
    return <ObjectVersionTagSummary stmt={value} />;
  }
  if (isActorTrustStatement(value)) {
    return <ActorTrustSummary stmt={value} />;
  }
  // ObjectRevision and the actor-key statement family don't
  // have dedicated guards yet (the `@kairo/object-model` slice
  // 6 surface only covered four kinds). Render a partial
  // summary keyed off the discriminator + actor when we can.
  return <FallbackSummary value={value} />;
}

function ObjectGenesisSummary({ stmt }: { stmt: ObjectGenesisStatementJson }) {
  return (
    <Stack spacing={1.5}>
      <KeyValue label="Object kind" value={<code>{stmt.body.object_kind}</code>} />
      <KeyValue label="Created by" value={<IdLink kind="actor" id={stmt.body.created_by} />} />
      <KeyValue label="Created at" value={stmt.body.created_at} />
      <KeyValue label="Nonce" value={<IdText id={stmt.body.nonce} />} />
    </Stack>
  );
}

function ObjectBranchSummary({ stmt }: { stmt: ObjectBranchStatementJson }) {
  return (
    <Stack spacing={1.5}>
      <KeyValue label="Object" value={<IdLink kind="object" id={stmt.body.object} />} />
      <KeyValue label="Branch name" value={<strong>{stmt.body.name}</strong>} />
      <KeyValue label="Revision" value={<IdLink kind="statement" id={stmt.body.revision} />} />
      <KeyValue label="Signer" value={<IdLink kind="actor" id={stmt.actor} />} />
      <KeyValue label="Created at" value={stmt.created_at} />
    </Stack>
  );
}

function ObjectVersionTagSummary({ stmt }: { stmt: ObjectVersionTagStatementJson }) {
  const target = stmt.body.target;
  const supersedes = stmt.body.supersedes;
  return (
    <Stack spacing={1.5}>
      <KeyValue label="Object" value={<IdLink kind="object" id={stmt.body.object} />} />
      <KeyValue label="Version" value={<strong>{stmt.body.version}</strong>} />
      <KeyValue
        label="Target"
        value={target === null || target === undefined || target === '' ? <em>(none)</em> : <IdText id={target} />}
      />
      <KeyValue
        label="Supersedes"
        value={
          supersedes === null || supersedes === undefined || supersedes === '' ? (
            <em>(none)</em>
          ) : (
            <IdLink kind="statement" id={supersedes} />
          )
        }
      />
      <KeyValue label="Signer" value={<IdLink kind="actor" id={stmt.actor} />} />
      <KeyValue label="Created at" value={stmt.created_at} />
    </Stack>
  );
}

function ActorTrustSummary({ stmt }: { stmt: ActorTrustStatementJson }) {
  const decision = stmt.body.decision;
  return (
    <Stack spacing={1.5}>
      <KeyValue label="By actor" value={<IdLink kind="actor" id={stmt.actor} />} />
      <KeyValue label="About actor" value={<IdLink kind="actor" id={stmt.body.trusted_actor} />} />
      <KeyValue
        label="Decision"
        value={
          decision === undefined || decision === null ? (
            <StatusBadge tone="neutral">Withdrawn</StatusBadge>
          ) : decision === 'trusted' ? (
            <StatusBadge tone="success">Trusted</StatusBadge>
          ) : decision === 'untrusted' ? (
            <StatusBadge tone="error">Untrusted</StatusBadge>
          ) : (
            <StatusBadge tone="neutral">{decision}</StatusBadge>
          )
        }
      />
      <KeyValue label="Created at" value={stmt.created_at} />
    </Stack>
  );
}

function FallbackSummary({ value }: { value: StatementValue }) {
  if (value === null || typeof value !== 'object') {
    return <em>No body to summarize.</em>;
  }
  const obj = value as Record<string, unknown>;
  const actor = typeof obj['actor'] === 'string' ? obj['actor'] : undefined;
  const createdAt = typeof obj['created_at'] === 'string' ? obj['created_at'] : undefined;
  return (
    <Stack spacing={1.5}>
      <Typography variant="body2" sx={{ color: 'text.secondary' }}>
        No dedicated summary for this statement kind yet — the envelope below carries the full body. Add
        a typed guard in <code>@kairo/object-model</code> to surface it here.
      </Typography>
      {actor !== undefined && (
        <KeyValue label="Signer" value={<IdLink kind="actor" id={actor} />} />
      )}
      {createdAt !== undefined && <KeyValue label="Created at" value={createdAt} />}
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
