// Dashboard at `/`. Pulls live `useVersion` + `useDaemonStatus`
// and renders a panel per data source.

import { useDaemonStatus, useVersion } from '@kairo/api-client';
import type { KairoApiClientError } from '@kairo/api-client';
import { ErrorDisplay, Panel, StatusBadge } from '@kairo/ui';
import Box from '@mui/material/Box';
import Skeleton from '@mui/material/Skeleton';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';

export function Dashboard() {
  const version = useVersion();
  const status = useDaemonStatus();

  const reachable = !version.isError && !status.isError;

  return (
    <>
      <Typography variant="h2" component="h2">
        Dashboard
      </Typography>

      <Panel
        title="Daemon"
        description="Live read of /api/v1/version + /api/v1/status."
        actions={
          reachable ? (
            <StatusBadge tone="success">Reachable</StatusBadge>
          ) : (
            <StatusBadge tone="error">Unreachable</StatusBadge>
          )
        }
      >
        {(version.isPending || status.isPending) && (
          <Stack spacing={1}>
            <Skeleton variant="text" width="60%" />
            <Skeleton variant="text" width="40%" />
            <Skeleton variant="text" width="50%" />
          </Stack>
        )}

        {version.isError && (
          <ErrorDisplay
            title="Could not reach the daemon"
            message="The version endpoint is unreachable."
            detail={renderErrorDetail(version.error)}
          />
        )}

        {version.isSuccess && status.isSuccess && (
          <MetaList
            entries={[
              ['daemon_version', version.data.daemon_version],
              ['api_version', version.data.api_version],
              ['core_version', version.data.core_version],
              ['store_version', version.data.store_version],
              ['store_path', status.data.store_path],
              ['store_schema_version', status.data.store_schema_version],
              ['pid', String(status.data.pid)],
            ]}
          />
        )}
      </Panel>
    </>
  );
}

interface MetaListProps {
  entries: ReadonlyArray<readonly [label: string, value: string]>;
}

function MetaList({ entries }: MetaListProps) {
  return (
    <Box
      component="dl"
      sx={{
        display: 'grid',
        gridTemplateColumns: 'max-content 1fr',
        rowGap: 1,
        columnGap: 3,
        m: 0,
      }}
    >
      {entries.map(([label, value]) => (
        <Box key={label} sx={{ display: 'contents' }}>
          <Typography component="dt" variant="body2" sx={{ color: 'text.secondary' }}>
            {label}
          </Typography>
          <Typography
            component="dd"
            variant="body2"
            sx={{
              m: 0,
              fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
              wordBreak: 'break-all',
            }}
          >
            {value}
          </Typography>
        </Box>
      ))}
    </Box>
  );
}

function renderErrorDetail(error: KairoApiClientError): string {
  const d = error.detail;
  switch (d.kind) {
    case 'network':
      return `network: ${d.message}`;
    case 'daemon':
      return `daemon: ${d.code}${d.status === undefined ? '' : ` (HTTP ${d.status})`}: ${d.message}`;
    case 'decode':
      return `decode: ${d.message}`;
  }
}
