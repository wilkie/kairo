// Async bootstrap so we can register the MSW worker (when
// `VITE_USE_MOCK_API=true`) before the app issues its first
// fetch. Without the await the first render races the worker
// startup and lands real network requests at a daemon that may
// not be running.

import { lazy, StrictMode, Suspense } from 'react';
import { createRoot } from 'react-dom/client';
import { KairoProvider, createKairoClient } from '@kairo/api-client';
import { KairoUiProvider } from '@kairo/ui';
import { ErrorBoundary } from './ErrorBoundary';
import { RouterProvider, router } from './router';

// Unified TanStack devtools shell — hosts both the Router and
// React Query panels behind a single floating launcher. Dev-only
// and lazy-loaded, so nothing ships in production.
const Devtools = import.meta.env.DEV
  ? lazy(async () => {
      const [{ TanStackDevtools }, { ReactQueryDevtoolsPanel }, { TanStackRouterDevtoolsPanel }] =
        await Promise.all([
          import('@tanstack/react-devtools'),
          import('@tanstack/react-query-devtools'),
          import('@tanstack/router-devtools'),
        ]);
      return {
        default: () => (
          <TanStackDevtools
            plugins={[
              { name: 'TanStack Query', render: <ReactQueryDevtoolsPanel /> },
              { name: 'TanStack Router', render: <TanStackRouterDevtoolsPanel router={router} /> },
            ]}
          />
        ),
      };
    })
  : () => null;

async function bootstrap() {
  if (import.meta.env.VITE_USE_MOCK_API === 'true') {
    const { startMockWorker } = await import('@kairo/api-client/mock');
    await startMockWorker({ onUnhandledRequest: 'bypass' });
    console.info('[kairo-web] mock API enabled (VITE_USE_MOCK_API=true)');
  }

  const root = document.getElementById('root');
  if (!root) {
    throw new Error('missing #root element in index.html');
  }

  // Use the page origin as the api-client base URL so requests
  // resolve to absolute `/api/v1/...` paths regardless of which
  // route the SPA is currently rendering. (Without an absolute
  // base, ky's relative-path resolution would interpret a call
  // from `/objects/zXyz` as `/objects/api/v1/...`, which the
  // proxy doesn't match — the SPA fallback would then return
  // index.html and the api-client would surface "<!doctype …
  // is not valid JSON".)
  const kairoClient = createKairoClient({ baseUrl: window.location.origin });

  createRoot(root).render(
    <StrictMode>
      <KairoUiProvider>
        <ErrorBoundary>
          <KairoProvider client={kairoClient}>
            <RouterProvider />
            <Suspense fallback={null}>
              <Devtools />
            </Suspense>
          </KairoProvider>
        </ErrorBoundary>
      </KairoUiProvider>
    </StrictMode>,
  );
}

void bootstrap();
