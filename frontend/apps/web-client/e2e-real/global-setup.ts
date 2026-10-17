// Global setup for the real-stack e2e suite: seed a throwaway
// store, start `kairo-daemon` on it, start `kairo-web` serving
// the built SPA in front of it, and hand the seeded IDs to the
// tests via `KAIRO_E2E_SEED` (env set here is visible to
// workers). Returns the teardown.
//
// Binaries come from `$KAIRO_BIN_DIR` (default
// `<repo>/target/debug`); nothing here builds them, so a stale
// binary is the caller's responsibility — CI builds fresh.

import { spawn, spawnSync, type ChildProcess } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { REAL_STACK_PORT } from '../playwright.real.config';

const here = dirname(fileURLToPath(import.meta.url));
const webClientDir = resolve(here, '..');
const repoRoot = resolve(webClientDir, '../../..');
const binDir = process.env['KAIRO_BIN_DIR'] ?? join(repoRoot, 'target/debug');
const spaDir = join(webClientDir, 'dist');

function requireFile(path: string, hint: string): void {
  if (!existsSync(path)) {
    throw new Error(`real-stack e2e: missing ${path}\n  ${hint}`);
  }
}

async function waitFor(what: string, probe: () => Promise<boolean>, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await probe()) return;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`real-stack e2e: ${what} not ready within ${timeoutMs}ms`);
}

function start(name: string, bin: string, args: string[], log: string[]): ChildProcess {
  const child = spawn(bin, args, { stdio: ['ignore', 'pipe', 'pipe'] });
  const capture = (chunk: Buffer) => log.push(`[${name}] ${chunk.toString()}`);
  child.stdout?.on('data', capture);
  child.stderr?.on('data', capture);
  return child;
}

export default async function globalSetup(): Promise<() => Promise<void>> {
  const kairo = join(binDir, 'kairo');
  const daemonBin = join(binDir, 'kairo-daemon');
  const webBin = join(binDir, 'kairo-web');
  for (const bin of [kairo, daemonBin, webBin]) {
    requireFile(bin, 'run `cargo build --bins` from the repo root, or set KAIRO_BIN_DIR');
  }
  requireFile(
    join(spaDir, 'index.html'),
    'run `pnpm turbo run build --filter @kairo/web-client...` from frontend/',
  );

  const root = mkdtempSync(join(tmpdir(), 'kairo-e2e-real-'));
  const store = join(root, 'store');
  const seed = spawnSync(join(repoRoot, 'scripts/smoke-seed.sh'), [store], {
    env: { ...process.env, KAIRO: kairo },
    encoding: 'utf8',
  });
  if (seed.status !== 0) {
    throw new Error(`real-stack e2e: smoke-seed.sh failed\n${seed.stdout}\n${seed.stderr}`);
  }
  process.env['KAIRO_E2E_SEED'] = readFileSync(join(store, 'seed-ids.json'), 'utf8');

  const log: string[] = [];
  const socket = join(store, 'daemon.sock');
  const daemon = start('daemon', daemonBin, ['--store', store], log);
  const web = start(
    'web',
    webBin,
    ['--daemon-socket', socket, '--spa-dir', spaDir, '--bind', `127.0.0.1:${REAL_STACK_PORT}`],
    log,
  );

  const teardown = async () => {
    for (const child of [web, daemon]) {
      if (child.exitCode === null) {
        child.kill('SIGTERM');
        await new Promise((r) => child.once('exit', r));
      }
    }
    rmSync(root, { recursive: true, force: true });
  };

  try {
    await waitFor('kairo-web', async () => {
      try {
        const res = await fetch(`http://127.0.0.1:${REAL_STACK_PORT}/api/v1/version`);
        return res.ok;
      } catch {
        return false;
      }
    });
  } catch (error) {
    await teardown();
    throw new Error(`${String(error)}\n${log.join('')}`);
  }

  return teardown;
}
