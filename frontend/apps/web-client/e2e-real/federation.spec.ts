// Real-stack provenance (`PHASE_2_FEDERATION.md` slice 6): an
// object pulled from another node by `scripts/federation-smoke.sh`
// shows where it came from. Local records show no badge.

import { expect, test } from '@playwright/test';
import { federation, seed } from './helpers';

const pulled = federation();
const ids = seed();
const peer = pulled.peerUrl.replace(/^https?:\/\//, '');

test('a pulled object names the node it came from', async ({ page }) => {
  await page.goto(`/objects/${pulled.object}`);
  await expect(page.getByTestId('provenance-badge')).toHaveText(`Fetched from ${peer}`);
  await expect(page.getByTestId('provenance-badge')).toHaveAccessibleName(/authenticated/);
});

test('the pulled revision carries provenance too', async ({ page }) => {
  await page.goto(`/statements/${pulled.revision}`);
  await expect(page.getByTestId('provenance-badge')).toHaveText(`Fetched from ${peer}`);
});

test('records created locally have no provenance badge', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await expect(page.getByText('Genesis').first()).toBeVisible();
  await expect(page.getByTestId('provenance-badge')).toHaveCount(0);
});
