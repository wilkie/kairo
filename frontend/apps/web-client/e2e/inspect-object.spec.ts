// "Inspect an object end-to-end" — the cornerstone slice-10
// workflow per `WEB_CLIENT.md` §22. Loads the rich Alpha
// fixture and asserts that every object-detail panel renders
// with the data the registry seeds.

import { expect, test } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

test('renders every panel of the object-detail page for the rich Alpha fixture', async ({
  page,
}) => {
  await page.goto(bareRoute('object', mockIds.alpha));

  await expect(page.getByRole('heading', { level: 2, name: 'Object' })).toBeVisible();

  // Every panel header is an h3 (rendered by Panel via
  // CardHeader with `titleTypographyProps={{ variant: 'h3' }}`).
  for (const title of [
    'Validation',
    'Genesis',
    'Branches',
    'Tags',
    'Revisions',
    'Capability heads',
    'Trust opinions',
  ]) {
    await expect(page.getByRole('heading', { level: 3, name: title })).toBeVisible();
  }

  // Branch tip + tag rows from the Alpha fixture.
  await expect(page.getByText('head', { exact: true })).toBeVisible();
  await expect(page.getByText('experimental', { exact: true })).toBeVisible();
  await expect(page.getByText('v1.0.0', { exact: true })).toBeVisible();
  await expect(page.getByText('v1.1.0', { exact: true })).toBeVisible();

  // Validation badge — Alpha's verifyObject fixture is
  // `indeterminate` (matches the daemon's behavior on a
  // revision-only object with no closure data).
  await expect(page.getByText('Indeterminate', { exact: true })).toBeVisible();
});
