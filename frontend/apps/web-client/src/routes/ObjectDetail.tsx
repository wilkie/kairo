// `/objects/$id` — composite object inspector. Composes:
//
//   - Genesis envelope panel (object id, kind, signer, created_at)
//   - Branches table (one row per (actor, name) chain leaf)
//   - Tags table (one row per (actor, version) chain leaf)
//   - Revision history table (chronological, ascending)
//   - Capability heads table (object-scoped grants)
//   - Trust opinions panel (about the genesis signer for v1;
//     full multi-actor aggregation is a slice 8.5 follow-up
//     once we have richer fixture data to drive the layout).
//
// Per `WEB_CLIENT.md` §15, every panel carries a locality
// badge that is *deliberately distinct* from validation status.
// V1 is single-store, so every panel renders `local`; the
// federation surface fills in the other states later.

import {
  useBranches,
  useCapabilitiesForObject,
  useObject,
  useRevisions,
  useTrustAbout,
  useVerifyObject,
  useVersionTags,
  type BranchTipDto,
  type CapabilityHeadDto,
  type ObjectGenesisStatementJson,
  type RevisionHeadDto,
  type TrustHeadDto,
  type ValidationResult,
  type VersionTagHeadDto,
} from '@kairo/api-client';
import { idPrefix, truncateId, validationStatusDescription } from '@kairo/object-model';
import {
  EmptyState,
  LocalityBadge,
  Panel,
  StatusBadge,
  Table,
  type TableColumn,
} from '@kairo/ui';
import { ValidationBadge, ValidationIssueList } from '@kairo/validation-viewer';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import type { ReactNode } from 'react';
import { IdLink, IdText } from '../components/IdLink';
import { QueryStatusBoundary } from '../components/QueryStatusBoundary';

export interface ObjectDetailProps {
  id: string;
}

export function ObjectDetail({ id }: ObjectDetailProps) {
  // Wire form is the bare payload — the daemon returns bare
  // ids in JSON bodies and accepts bare ids on URL paths
  // (`ObjectId::Display` writes `as_str()`, no prefix). The
  // `kairo:<kind>:` prefix is presentational only; we compose
  // it for the page header below.
  const objectQ = useObject(id);
  const verifyQ = useVerifyObject(id);
  const branchesQ = useBranches(id);
  const tagsQ = useVersionTags(id);
  const revisionsQ = useRevisions(id);
  const capsQ = useCapabilitiesForObject(id);

  // Trust panel is keyed off the genesis signer; we can only
  // ask the daemon once that id resolves. Pass an explicit
  // empty string when missing — the `enabled` flag suppresses
  // the actual fetch.
  const createdBy = objectQ.data?.body.created_by;
  const trustQ = useTrustAbout(createdBy ?? '', { enabled: createdBy !== undefined });

  return (
    <>
      <Typography variant="h2" component="h2">
        Object
      </Typography>
      <Box sx={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', wordBreak: 'break-all' }}>
        {idPrefix('object')}
        {id}
      </Box>

      <Panel
        title="Validation"
        description="Live read of /api/v1/verify-object."
        actions={
          <Stack direction="row" spacing={1}>
            {verifyQ.data !== undefined ? (
              <ValidationBadge status={verifyQ.data.status} />
            ) : (
              <ValidationBadge status="unverified" />
            )}
            <LocalityBadge state="local" />
          </Stack>
        }
      >
        <QueryStatusBoundary query={verifyQ}>
          {(data) => <ValidationContent data={data} />}
        </QueryStatusBoundary>
      </Panel>

      <Panel
        title="Genesis"
        description="The signed statement that creates this object."
        actions={<LocalityBadge state="local" />}
      >
        <QueryStatusBoundary query={objectQ}>
          {(data) => <GenesisFields data={data} />}
        </QueryStatusBoundary>
      </Panel>

      <Panel
        title="Branches"
        description="Mutable named pointers; one row per (actor, name) chain leaf."
        actions={<LocalityBadge state="local" />}
      >
        <QueryStatusBoundary query={branchesQ}>
          {(rows) => <BranchesTable rows={rows} />}
        </QueryStatusBoundary>
      </Panel>

      <Panel
        title="Tags"
        description="Semver release pointers; one row per (actor, version) chain leaf."
        actions={<LocalityBadge state="local" />}
      >
        <QueryStatusBoundary query={tagsQ}>
          {(rows) => <TagsTable rows={rows} />}
        </QueryStatusBoundary>
      </Panel>

      <Panel
        title="Revisions"
        description="Chronological revision history (ascending)."
        actions={<LocalityBadge state="local" />}
      >
        <QueryStatusBoundary query={revisionsQ}>
          {(rows) => <RevisionsTable rows={rows} />}
        </QueryStatusBoundary>
      </Panel>

      <Panel
        title="Capability heads"
        description="Cross-actor grants scoped to this object."
        actions={<LocalityBadge state="local" />}
      >
        <QueryStatusBoundary query={capsQ}>
          {(rows) => <CapabilitiesTable rows={rows} />}
        </QueryStatusBoundary>
      </Panel>

      <Panel
        title="Trust opinions"
        description={
          createdBy === undefined
            ? 'Loaded after the genesis resolves.'
            : `What other actors say about the genesis signer (${truncateId(createdBy)}). Full multi-actor aggregation is a follow-up.`
        }
        actions={<LocalityBadge state="local" />}
      >
        {createdBy === undefined ? (
          <EmptyState title="Waiting for genesis" description="Trust opinions render once the object's signer is known." />
        ) : (
          <QueryStatusBoundary query={trustQ}>
            {(rows) => <TrustTable rows={rows} subjectActor={createdBy} />}
          </QueryStatusBoundary>
        )}
      </Panel>
    </>
  );
}

// ---------------------------------------------------------------------------
// Per-section views

function ValidationContent({ data }: { data: ValidationResult }) {
  const headRefs: ReactNode[] = [];
  if (data.revision_statement_id !== undefined && data.revision_statement_id !== null) {
    headRefs.push(
      <KeyValue
        key="revision"
        label="Resolved revision"
        value={<IdLink kind="statement" id={data.revision_statement_id} />}
      />,
    );
  }
  if (data.branch_name !== undefined && data.branch_name !== null) {
    headRefs.push(<KeyValue key="branch" label="Resolved branch" value={data.branch_name} />);
  }
  return (
    <Stack spacing={2}>
      <Typography variant="body2" sx={{ color: 'text.secondary' }}>
        {validationStatusDescription(data.status)}
      </Typography>
      {headRefs}
      <Box>
        <Typography variant="body2" sx={{ color: 'text.secondary', mb: 1 }}>
          {data.issues.length} issue{data.issues.length === 1 ? '' : 's'}
        </Typography>
        <ValidationIssueList
          issues={data.issues}
          renderRef={(kind, id) => <IdLink kind={kind} id={id} />}
        />
      </Box>
    </Stack>
  );
}

function GenesisFields({ data }: { data: ObjectGenesisStatementJson }) {
  const body = data.body;
  return (
    <Stack spacing={1.5}>
      <KeyValue label="Object kind" value={<code>{body.object_kind}</code>} />
      <KeyValue
        label="Created by"
        value={<IdLink kind="actor" id={body.created_by} />}
      />
      <KeyValue label="Created at" value={body.created_at} />
      <KeyValue label="Nonce" value={<IdText id={body.nonce} />} />
    </Stack>
  );
}

function BranchesTable({ rows }: { rows: ReadonlyArray<BranchTipDto> }) {
  const columns: ReadonlyArray<TableColumn<BranchTipDto>> = [
    { key: 'name', header: 'Name', cell: (r) => <strong>{r.name}</strong> },
    { key: 'actor', header: 'Actor', cell: (r) => <IdLink kind="actor" id={r.actor} /> },
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
      rowKey={(r) => `${r.actor}:${r.name}`}
      emptyState={<EmptyState title="No branches" description="Nothing has been published as a branch tip for this object." />}
    />
  );
}

function TagsTable({ rows }: { rows: ReadonlyArray<VersionTagHeadDto> }) {
  const columns: ReadonlyArray<TableColumn<VersionTagHeadDto>> = [
    { key: 'version', header: 'Version', cell: (r) => <strong>{r.version}</strong> },
    { key: 'actor', header: 'Actor', cell: (r) => <IdLink kind="actor" id={r.actor} /> },
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
      rowKey={(r) => `${r.actor}:${r.version}`}
      emptyState={<EmptyState title="No tags" description="Nothing has been tagged on this object." />}
    />
  );
}

function RevisionsTable({ rows }: { rows: ReadonlyArray<RevisionHeadDto> }) {
  const columns: ReadonlyArray<TableColumn<RevisionHeadDto>> = [
    {
      key: 'revision',
      header: 'Revision',
      cell: (r) => <IdText id={r.revision_id} />,
    },
    {
      key: 'parents',
      header: 'Parents',
      cell: (r) =>
        r.parents.length === 0 ? (
          <em style={{ color: 'var(--mui-palette-text-secondary, #666)' }}>(initial)</em>
        ) : (
          <Stack spacing={0.5}>
            {r.parents.map((p) => (
              <IdText key={p} id={p} />
            ))}
          </Stack>
        ),
    },
    {
      key: 'manifest',
      header: 'Manifest',
      cell: (r) => <IdLink kind="blob" id={r.manifest_hash} />,
    },
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
      emptyState={<EmptyState title="No revisions" description="No ObjectRevision statements observe this object yet." />}
    />
  );
}

function CapabilitiesTable({ rows }: { rows: ReadonlyArray<CapabilityHeadDto> }) {
  const columns: ReadonlyArray<TableColumn<CapabilityHeadDto>> = [
    { key: 'grantor', header: 'Grantor', cell: (r) => <IdLink kind="actor" id={r.grantor} /> },
    { key: 'grantee', header: 'Grantee', cell: (r) => <IdLink kind="actor" id={r.grantee} /> },
    {
      key: 'scope',
      header: 'Scope',
      cell: (r) => <ScopeCell scope={r.scope} />,
    },
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
      emptyState={<EmptyState title="No capability heads" description="No actor has granted capabilities scoped to this object." />}
    />
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
        description={`No actor has expressed trust about ${truncateId(subjectActor)}.`}
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

// ---------------------------------------------------------------------------
// Small bits

function KeyValue({ label, value }: { label: string; value: ReactNode }) {
  return (
    <Box sx={{ display: 'grid', gridTemplateColumns: '10rem 1fr', columnGap: 2, alignItems: 'baseline' }}>
      <Typography variant="body2" sx={{ color: 'text.secondary' }}>
        {label}
      </Typography>
      <Box>{value}</Box>
    </Box>
  );
}

function ScopeCell({ scope }: { scope: CapabilityHeadDto['scope'] }) {
  if (scope === null || typeof scope !== 'object') {
    return <span>{String(scope)}</span>;
  }
  // The CapabilityScopeJson is a tagged union — the daemon
  // emits exactly one key per variant (snake_case). We render
  // the variant name + its payload as a compact summary.
  const entries = Object.entries(scope);
  if (entries.length === 0) {
    return <em>(none)</em>;
  }
  const [variant, payload] = entries[0] ?? ['', null];
  if (typeof payload === 'string') {
    if (variant === 'object') {
      return (
        <span>
          object: <IdLink kind="object" id={payload} />
        </span>
      );
    }
    if (variant === 'actor') {
      return (
        <span>
          actor: <IdLink kind="actor" id={payload} />
        </span>
      );
    }
    return (
      <span>
        {variant}: <IdText id={payload} />
      </span>
    );
  }
  // Fallback — render as JSON-y key/value for unknown shapes.
  return <code>{`${variant}: ${JSON.stringify(payload)}`}</code>;
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

