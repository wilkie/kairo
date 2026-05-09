import { useVersion } from '@kairo/api-client';

export function App() {
  const version = useVersion();

  return (
    <main style={{ fontFamily: 'system-ui, sans-serif', padding: '2rem', maxWidth: '40rem' }}>
      <h1>Kairo</h1>
      <p>Phase 2 §5 web client — read-only inspector (slice 5: shell only).</p>
      <section>
        <h2>Daemon</h2>
        {version.isPending && <p>Loading version&hellip;</p>}
        {version.isError && (
          <p style={{ color: 'crimson' }}>
            Could not reach the daemon API at <code>/api/v1/version</code>:{' '}
            <code>{version.error.message}</code>
          </p>
        )}
        {version.isSuccess && (
          <dl>
            <dt>daemon_version</dt>
            <dd>
              <code>{version.data.daemon_version}</code>
            </dd>
            <dt>api_version</dt>
            <dd>
              <code>{version.data.api_version}</code>
            </dd>
            <dt>core_version</dt>
            <dd>
              <code>{version.data.core_version}</code>
            </dd>
            <dt>store_version</dt>
            <dd>
              <code>{version.data.store_version}</code>
            </dd>
          </dl>
        )}
      </section>
    </main>
  );
}
