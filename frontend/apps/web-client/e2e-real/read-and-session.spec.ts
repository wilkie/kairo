// Real-stack reads and the session lifecycle: the built SPA
// renders seeded daemon data, and sign-in / sign-out round-trip
// through `kairo-web` to the daemon's session store (the path
// the old POST-block silently broke while MSW tests passed).

import { expect, test } from '@playwright/test';
import { seed, signInAs, tagRow } from './helpers';

const ids = seed();

test('object page renders seeded branches and tags from the daemon', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await expect(tagRow(page, '1.0.0', ids.alice)).toBeVisible();
  await expect(tagRow(page, '1.1.0', ids.alice)).toBeVisible();
  await expect(page.getByRole('row').filter({ hasText: 'experimental' })).toBeVisible();
});

test('session survives a reload and ends on sign-out', async ({ page }) => {
  await page.goto('/');
  await signInAs(page, ids.alice);

  // The HttpOnly cookie came back through the proxy, so a fresh
  // page load is still signed in.
  await page.reload();
  await expect(page.getByTestId('actor-picker-signed-in')).toContainText(ids.alice.slice(0, 8));

  await page.getByRole('button', { name: /sign out/i }).click();
  await expect(page.getByTestId('actor-picker-sign-in')).toBeVisible();
  await page.reload();
  await expect(page.getByTestId('actor-picker-sign-in')).toBeVisible();
});
