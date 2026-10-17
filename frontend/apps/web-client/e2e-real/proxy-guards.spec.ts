// The `kairo-web` request guards (`DECISIONS.md` §15) seen from
// outside the browser: what a cross-origin page or a rebound
// hostname would get. The Rust smoke tests pin the same rules;
// this keeps them honest against the built, served stack.

import { expect, test, type APIResponse } from '@playwright/test';
import { seed } from './helpers';

const ids = seed();

/** `error.code` from a `kairo.api.error.v1` envelope. */
async function errorCode(res: APIResponse): Promise<string | undefined> {
  const body = (await res.json()) as { error?: { code?: string } };
  return body.error?.code;
}

test('cross-origin sign-in is refused without minting a session', async ({ request }) => {
  const res = await request.post('/api/v1/sessions', {
    headers: { origin: 'http://localhost:3000' },
    data: { actor_id: ids.alice },
  });
  expect(res.status()).toBe(403);
  expect(await errorCode(res)).toBe('web_cross_origin');
  expect(res.headers()['set-cookie']).toBeUndefined();
});

test('rebound Host is refused even for reads', async ({ request }) => {
  const res = await request.get('/api/v1/version', { headers: { host: 'rebind.example' } });
  expect(res.status()).toBe(403);
  expect(await errorCode(res)).toBe('web_forbidden_host');
});

test('off-allowlist methods are 405 with Allow', async ({ request, baseURL }) => {
  const res = await request.put('/api/v1/statements', { headers: { origin: baseURL ?? '' } });
  expect(res.status()).toBe(405);
  expect(res.headers()['allow']).toBe('GET, HEAD, POST');
});
