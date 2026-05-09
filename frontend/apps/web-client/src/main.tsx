// Async bootstrap so we can register the MSW worker (when
// `VITE_USE_MOCK_API=true`) before the app issues its first
// fetch. Without the await the first render races the worker
// startup and lands real network requests at a daemon that may
// not be running.

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { KairoProvider, createKairoClient } from '@kairo/api-client';
import { RouterProvider } from './router';

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

  const kairoClient = createKairoClient({ baseUrl: '' });

  createRoot(root).render(
    <StrictMode>
      <KairoProvider client={kairoClient}>
        <RouterProvider />
      </KairoProvider>
    </StrictMode>,
  );
}

void bootstrap();
