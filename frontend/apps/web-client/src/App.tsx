import { useDaemonStatus, useVersion } from '@kairo/api-client';
import { truncateId } from '@kairo/object-model';

export function App() {
  const version = useVersion();
  const status = useDaemonStatus();

  return (
    <main style={{ fontFamily: 'system-ui, sans-serif', padding: '2rem', maxWidth: '40rem' }}>
      <h1>Kairo</h1>
      <p>Phase 2 §5 web client — read-only inspector (slice 6: typed api-client + hooks).</p>

      <section>
        <h2>Daemon version</h2>
        {version.isPending && <p>Loading version&hellip;</p>}
        {version.isError && (
          <p style={{ color: 'crimson' }}>
            Could not reach the daemon API: <code>{version.error.message}</code>
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

      <section>
        <h2>Daemon status</h2>
        {status.isPending && <p>Loading status&hellip;</p>}
        {status.isError && (
          <p style={{ color: 'crimson' }}>
            Could not reach status: <code>{status.error.message}</code>
          </p>
        )}
        {status.isSuccess && (
          <dl>
            <dt>store_path</dt>
            <dd>
              <code>{truncateId(status.data.store_path, { prefixChars: 12, suffixChars: 12 })}</code>
            </dd>
            <dt>schema_version</dt>
            <dd>
              <code>{status.data.store_schema_version}</code>
            </dd>
            <dt>pid</dt>
            <dd>
              <code>{status.data.pid}</code>
            </dd>
          </dl>
        )}
      </section>
    </main>
  );
}
