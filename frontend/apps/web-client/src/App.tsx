import { useEffect, useState } from 'react';
import { createKairoClient, type VersionInfo } from '@kairo/api-client';

type LoadState =
  | { kind: 'loading' }
  | { kind: 'ok'; version: VersionInfo }
  | { kind: 'error'; message: string };

// Same-origin: in production the SPA is served by kairo-web from
// the same host:port that proxies /api/v1/*; in dev, Vite proxies
// the same path to a running kairo-web (see vite.config.ts).
const API_BASE = '';

export function App() {
  const [state, setState] = useState<LoadState>({ kind: 'loading' });

  useEffect(() => {
    let cancelled = false;
    const client = createKairoClient(API_BASE);

    void (async () => {
      try {
        const version = await client.getVersion();
        if (!cancelled) {
          setState({ kind: 'ok', version });
        }
      } catch (error) {
        if (!cancelled) {
          setState({
            kind: 'error',
            message: error instanceof Error ? error.message : String(error),
          });
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <main style={{ fontFamily: 'system-ui, sans-serif', padding: '2rem', maxWidth: '40rem' }}>
      <h1>Kairo</h1>
      <p>Phase 2 §5 web client — read-only inspector (slice 5: shell only).</p>
      <section>
        <h2>Daemon</h2>
        {state.kind === 'loading' && <p>Loading version&hellip;</p>}
        {state.kind === 'error' && (
          <p style={{ color: 'crimson' }}>
            Could not reach the daemon API at <code>/api/v1/version</code>:{' '}
            <code>{state.message}</code>
          </p>
        )}
        {state.kind === 'ok' && (
          <dl>
            <dt>daemon_version</dt>
            <dd>
              <code>{state.version.daemon_version}</code>
            </dd>
            <dt>api_version</dt>
            <dd>
              <code>{state.version.api_version}</code>
            </dd>
            <dt>core_version</dt>
            <dd>
              <code>{state.version.core_version}</code>
            </dd>
            <dt>store_version</dt>
            <dd>
              <code>{state.version.store_version}</code>
            </dd>
          </dl>
        )}
      </section>
    </main>
  );
}
