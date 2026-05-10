// Renders the issue array from a `ValidationResult` as a
// stacked list. Each row carries a severity badge + the
// wire-stable `kind` + a human message; statement / actor
// references are rendered as plain truncated ids since the
// validation-viewer package can't depend on the consumer's
// router shape — call sites pass an optional `linkRenderer`
// to upgrade them into navigable links (the inspector does).

import type { ReactNode } from 'react';
import type { ValidationIssue, ValidationIssueSeverity } from '@kairo/api-client';
import { severityLabel, truncateId } from '@kairo/object-model';
import { EmptyState, StatusBadge, type StatusBadgeTone } from '@kairo/ui';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';

const SEVERITY_TO_TONE: Record<ValidationIssueSeverity, StatusBadgeTone> = {
  info: 'info',
  warning: 'warn',
  error: 'error',
};

const MONOSPACE = 'ui-monospace, SFMono-Regular, Menlo, monospace';

export interface ValidationIssueListProps {
  issues: ReadonlyArray<ValidationIssue>;
  /** Optional renderer for an `ObjectId` / `ActorId` /
   * `StatementId` reference; the inspector wires this to a
   * `<Link>` component so issues navigate. Default renders
   * the truncated id as plain monospace text. */
  renderRef?: (kind: 'actor' | 'statement', id: string) => ReactNode;
  /** Override the empty-state rendering (e.g. to hide the
   * panel entirely or render a "valid" affirmation instead). */
  emptyState?: ReactNode;
}

export function ValidationIssueList({ issues, renderRef, emptyState }: ValidationIssueListProps) {
  if (issues.length === 0) {
    return (
      emptyState ?? (
        <EmptyState
          title="No issues"
          description="The daemon's verifier raised no findings."
        />
      )
    );
  }
  return (
    <Stack spacing={2} component="ul" sx={{ listStyle: 'none', p: 0, m: 0 }}>
      {issues.map((issue, idx) => (
        <Box component="li" key={`${issue.kind}:${idx}`}>
          <IssueItem issue={issue} renderRef={renderRef} />
        </Box>
      ))}
    </Stack>
  );
}

function IssueItem({
  issue,
  renderRef,
}: {
  issue: ValidationIssue;
  renderRef?: ValidationIssueListProps['renderRef'];
}) {
  const refRenderer = renderRef ?? defaultRenderRef;
  const hasDetails =
    issue.details !== null && typeof issue.details === 'object' && Object.keys(issue.details).length > 0;
  return (
    <Stack spacing={1}>
      <Stack direction="row" spacing={1} alignItems="center" sx={{ flexWrap: 'wrap', rowGap: 1 }}>
        <StatusBadge tone={SEVERITY_TO_TONE[issue.severity]}>
          {severityLabel(issue.severity)}
        </StatusBadge>
        <Box component="code" sx={{ fontFamily: MONOSPACE, fontSize: '0.8125rem' }}>
          {issue.kind}
        </Box>
      </Stack>
      <Typography variant="body2">{issue.message}</Typography>
      {(isPresent(issue.statement_id) || isPresent(issue.actor_id)) && (
        <Stack direction="row" spacing={2} sx={{ flexWrap: 'wrap', rowGap: 0.5 }}>
          {isPresent(issue.statement_id) && (
            <RefRow label="Statement" inner={refRenderer('statement', issue.statement_id)} />
          )}
          {isPresent(issue.actor_id) && (
            <RefRow label="Actor" inner={refRenderer('actor', issue.actor_id)} />
          )}
        </Stack>
      )}
      {hasDetails && (
        <Box
          component="pre"
          sx={{
            fontFamily: MONOSPACE,
            fontSize: '0.8125rem',
            color: 'text.secondary',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            m: 0,
          }}
        >
          {JSON.stringify(issue.details, null, 2)}
        </Box>
      )}
    </Stack>
  );
}

function RefRow({ label, inner }: { label: string; inner: ReactNode }) {
  return (
    <Stack direction="row" spacing={1} alignItems="baseline">
      <Typography variant="body2" sx={{ color: 'text.secondary' }}>
        {label}:
      </Typography>
      {inner}
    </Stack>
  );
}

function isPresent(value: string | null | undefined): value is string {
  return typeof value === 'string' && value.length > 0;
}

function defaultRenderRef(_kind: 'actor' | 'statement', id: string): ReactNode {
  return (
    <Box component="span" sx={{ fontFamily: MONOSPACE, fontSize: '0.8125rem' }}>
      {truncateId(id)}
    </Box>
  );
}
