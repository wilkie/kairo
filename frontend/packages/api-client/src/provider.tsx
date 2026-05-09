// `KairoProvider` wires the api-client and TanStack Query into
// the React tree. Apps mount it once near the root.
//
// `useKairoClient()` is the hook callers reach for inside a
// component; it asserts the provider is mounted, so missing
// setup fails loud at render time rather than at the next
// network call.

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createContext, useContext, useMemo, type ReactNode } from 'react';
import type { KairoApiClient } from './client';

const KairoClientContext = createContext<KairoApiClient | null>(null);

export interface KairoProviderProps {
  /** The api-client instance every hook in the tree will use. */
  client: KairoApiClient;
  /**
   * Optional TanStack Query client. Defaults to one tuned for
   * inspector-style read traffic: 30s stale time, no automatic
   * refetch on window focus (the daemon's read endpoints are
   * cheap, but the inspector is read-heavy and over-fetching is
   * worse than mild staleness).
   */
  queryClient?: QueryClient;
  children: ReactNode;
}

const DEFAULT_STALE_TIME_MS = 30_000;

function defaultQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: DEFAULT_STALE_TIME_MS,
        refetchOnWindowFocus: false,
        retry: 1,
      },
    },
  });
}

export function KairoProvider({ client, queryClient, children }: KairoProviderProps) {
  const resolvedQueryClient = useMemo(() => queryClient ?? defaultQueryClient(), [queryClient]);
  return (
    <QueryClientProvider client={resolvedQueryClient}>
      <KairoClientContext.Provider value={client}>{children}</KairoClientContext.Provider>
    </QueryClientProvider>
  );
}

export function useKairoClient(): KairoApiClient {
  const client = useContext(KairoClientContext);
  if (client === null) {
    throw new Error(
      'useKairoClient called outside <KairoProvider>. Wrap your app in <KairoProvider client={createKairoClient(...)}>.',
    );
  }
  return client;
}
