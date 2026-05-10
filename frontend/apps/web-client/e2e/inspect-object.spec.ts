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
    // `exact: true` so e.g. the panel header "Genesis" doesn't
    // collide with the trust-panel empty state's "Waiting for
    // genesis" placeholder while the chained query resolves.
    await expect(
      page.getByRole('heading', { level: 3, name: title, exact: true }),
    ).toBeVisible();
  }

  // Branch tip + tag rows from the Alpha fixture. We query
  // by table-cell role rather than free text — exercises the
  // semantic table structure (per `WEB_CLIENT.md` §20
  // accessibility) and avoids ambiguity when the same string
  // appears elsewhere on the page.
  await expect(page.getByRole('cell', { name: 'head', exact: true })).toBeVisible();
  await expect(page.getByRole('cell', { name: 'experimental', exact: true })).toBeVisible();
  await expect(page.getByRole('cell', { name: 'v1.0.0', exact: true })).toBeVisible();
  await expect(page.getByRole('cell', { name: 'v1.1.0', exact: true })).toBeVisible();

  // Validation badge — Alpha's verifyObject fixture is
  // `indeterminate` (matches the daemon's behavior on a
  // revision-only object with no closure data).
  await expect(page.getByText('Indeterminate', { exact: true })).toBeVisible();
});
