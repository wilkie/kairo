// Tiny boundary that turns a TanStack Query result into the
// inspector's standard loading / error / success rendering.
// Centralized here so route pages don't re-implement the same
// branch per panel.
//
// Errors map through `KairoApiClientError.detail`'s discriminated
// kind so the message + detail strings reflect *what* went wrong
// (network, daemon code, malformed response).

import type { ReactNode } from 'react';
import type { KairoApiClientError } from '@kairo/api-client';
import { ErrorDisplay } from '@kairo/ui';
import Skeleton from '@mui/material/Skeleton';
import Stack from '@mui/material/Stack';

interface QueryLike<T> {
  isPending: boolean;
  isError: boolean;
  isSuccess: boolean;
  data?: T | undefined;
  error?: KairoApiClientError | null;
}

export interface QueryStatusBoundaryProps<T> {
  query: QueryLike<T>;
  /** What to render once `query.data` is available. */
  children: (data: T) => ReactNode;
  /** Override the loading skeleton. Defaults to three text rows. */
  loadingFallback?: ReactNode;
  /** Override the error title. */
  errorTitle?: ReactNode;
  /** Override the error message. */
  errorMessage?: ReactNode;
}

export function QueryStatusBoundary<T>({
  query,
  children,
  loadingFallback,
  errorTitle,
  errorMessage,
}: QueryStatusBoundaryProps<T>) {
  if (query.isPending) {
    return loadingFallback ?? <DefaultLoadingFallback />;
  }
  if (query.isError) {
    return (
      <ErrorDisplay
        title={errorTitle ?? 'Could not load this section'}
        message={errorMessage ?? 'The daemon returned an error.'}
        detail={query.error !== undefined && query.error !== null ? renderDetail(query.error) : undefined}
      />
    );
  }
  if (query.isSuccess && query.data !== undefined) {
    return <>{children(query.data)}</>;
  }
  return null;
}

function DefaultLoadingFallback() {
  return (
    <Stack spacing={1}>
      <Skeleton variant="text" width="60%" />
      <Skeleton variant="text" width="80%" />
      <Skeleton variant="text" width="50%" />
    </Stack>
  );
}

function renderDetail(error: KairoApiClientError): string {
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
