// TanStack Query hooks, one per daemon endpoint.
//
// Slice 5 ships `useVersion` only. Slice 6 fills in the rest in
// the same shape: take the api-client out of `useKairoClient`,
// wire a stable key from `kairoKeys`, and let consumers branch
// on the `data | error | isPending` triple TanStack Query
// returns.

import { useQuery, type UseQueryResult } from '@tanstack/react-query';
import type { KairoApiClientError } from './error';
import type { VersionInfo } from './client';
import { kairoKeys } from './keys';
import { useKairoClient } from './provider';

export function useVersion(): UseQueryResult<VersionInfo, KairoApiClientError> {
  const client = useKairoClient();
  return useQuery<VersionInfo, KairoApiClientError>({
    queryKey: kairoKeys.daemonVersion(),
    queryFn: () => client.getVersion(),
  });
}
