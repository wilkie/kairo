// Slice-10 critical workflows: "view an invalid validation
// result" and "view a conflicted validation result". Drives
// the dedicated Gamma (invalid) and Delta (conflicted)
// fixtures the registry seeds for these states; confirms each
// status renders a distinct, accessible badge label per
// `WEB_CLIENT.md` §10/§20.

import { expect, test } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

test('invalid object renders the Invalid badge with an error-severity issue', async ({ page }) => {
  await page.goto(bareRoute('object', mockIds.gamma));

  await expect(page.getByRole('heading', { level: 3, name: 'Validation' })).toBeVisible();
  await expect(page.getByText('Invalid', { exact: true })).toBeVisible();

  // The fixture issue's wire-stable kind code surfaces as a
  // monospace chip; its severity badge reads "Error".
  await expect(page.getByText('signature_invalid', { exact: true })).toBeVisible();
  await expect(page.getByText('Error', { exact: true })).toBeVisible();
});

test('conflicted object renders the Conflicted badge with a warning-severity issue', async ({
  page,
}) => {
  await page.goto(bareRoute('object', mockIds.delta));

  await expect(page.getByRole('heading', { level: 3, name: 'Validation' })).toBeVisible();
  await expect(page.getByText('Conflicted', { exact: true })).toBeVisible();
  await expect(page.getByText('cross_actor_conflict', { exact: true })).toBeVisible();
  await expect(page.getByText('Warning', { exact: true })).toBeVisible();
});
