// Real-stack writes: each dialog's statement is signed by the
// daemon for the session actor, passes the daemon's real
// capability evaluation, and persists in the store across a
// reload. Tests share the seeded store and run serially; each
// writes names no other test reads. The capability tests come
// last because they change what Bob and Carol may do.

import { expect, test } from '@playwright/test';
import { capRow, seed, signInAs, tagRow } from './helpers';

const ids = seed();

test.describe.configure({ mode: 'serial' });

test('owner creates a version tag that persists', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await signInAs(page, ids.alice);

  await page.getByTestId('tag-version').click();
  await page.getByTestId('version-tag-input').fill('2.0.0');
  await page.getByTestId('version-tag-submit').click();
  await expect(page.getByTestId('version-tag-form')).toHaveCount(0);
  await expect(tagRow(page, '2.0.0', ids.alice)).toBeVisible();

  await page.reload();
  await expect(tagRow(page, '2.0.0', ids.alice)).toBeVisible();
});

test('owner retargets an existing tag via a derived supersedes', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await signInAs(page, ids.alice);

  const row = tagRow(page, '1.0.0', ids.alice);
  const prior = (await row.locator('td').nth(2).textContent())?.trim() ?? '';
  expect(prior.length).toBeGreaterThan(0);

  await page.getByTestId('tag-version').click();
  await page.getByTestId('version-tag-input').fill('1.0.0');
  await expect(page.getByTestId('version-tag-submit')).toContainText(/retarget/i);
  await page.getByTestId('version-tag-submit').click();
  await expect(page.getByTestId('version-tag-form')).toHaveCount(0);

  // A wrong `supersedes` would have been rejected by the store;
  // a new chain leaf means the daemon accepted the derived one.
  await page.reload();
  await expect(row.locator('td').nth(2)).not.toHaveText(prior);
});

test('two-hop delegate (Alice → Bob → Carol) can tag', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await signInAs(page, ids.carol);

  await page.getByTestId('tag-version').click();
  await page.getByTestId('version-tag-input').fill('1.2.0-rc.1');
  await page.getByTestId('version-tag-submit').click();
  await expect(page.getByTestId('version-tag-form')).toHaveCount(0);
  await expect(tagRow(page, '1.2.0-rc.1', ids.carol)).toBeVisible();
});

test('actor without an ObjectBranch capability is refused by the daemon', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await signInAs(page, ids.bob);

  await page.getByTestId('update-branch').click();
  await page.getByTestId('branch-name-input').fill('bob-was-here');
  await page.getByTestId('branch-update-submit').click();

  await expect(page.getByTestId('branch-update-error')).toBeVisible();
  await expect(page.getByTestId('branch-update-form')).toBeVisible();
  await page.reload();
  await expect(page.getByRole('row').filter({ hasText: 'bob-was-here' })).toHaveCount(0);
});

test('owner advances a branch', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await signInAs(page, ids.alice);

  await page.getByTestId('update-branch').click();
  await page.getByTestId('branch-name-input').fill('release-e2e');
  await page.getByTestId('branch-update-submit').click();
  await expect(page.getByTestId('branch-update-form')).toHaveCount(0);

  await page.reload();
  await expect(page.getByRole('row').filter({ hasText: 'release-e2e' })).toBeVisible();
});

test('signed-in actor records a trust opinion', async ({ page }) => {
  await page.goto(`/actors/${ids.carol}`);
  await signInAs(page, ids.alice);

  await page.getByTestId('express-trust-opinion').click();
  await page.getByTestId('trust-reason-input').fill('real-stack e2e');
  await page.getByTestId('trust-opinion-submit').click();
  await expect(page.getByTestId('trust-opinion-form')).toHaveCount(0);

  await page.reload();
  await expect(page.getByRole('cell', { name: 'Trusted', exact: true })).toBeVisible();
});

test('owner extends a grant with a derived supersedes', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await signInAs(page, ids.alice);

  await page.getByTestId('grant-capability').click();
  await page.getByTestId('capability-grantee-input').fill(ids.bob);
  // Pre-filled from the seeded Alice → Bob grant.
  await expect(page.getByTestId('capability-kind-ObjectVersionTag')).toBeChecked();
  await expect(page.getByTestId('capability-delegable')).toBeChecked();
  await page.getByTestId('capability-kind-ObjectBranch').check();
  await page.getByTestId('capability-grant-submit').click();
  await expect(page.getByTestId('capability-grant-form')).toHaveCount(0);

  await page.reload();
  await expect(capRow(page, ids.alice, ids.bob)).toContainText('ObjectBranch');
});

test('grantee can now write the newly granted kind', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await signInAs(page, ids.bob);

  await page.getByTestId('update-branch').click();
  await page.getByTestId('branch-name-input').fill('bob-granted');
  await page.getByTestId('branch-update-submit').click();
  await expect(page.getByTestId('branch-update-form')).toHaveCount(0);

  await page.reload();
  await expect(page.getByRole('row').filter({ hasText: 'bob-granted' })).toBeVisible();
});

test('delegate without delegable cannot grant onward', async ({ page }) => {
  // Carol holds ObjectVersionTag from Bob, delegable = false.
  await page.goto(`/objects/${ids.alpha}`);
  await signInAs(page, ids.carol);

  await page.getByTestId('grant-capability').click();
  await page.getByTestId('capability-grantee-input').fill(ids.alice);
  await page.getByTestId('capability-kind-ObjectVersionTag').check();
  await page.getByTestId('capability-grant-submit').click();

  await expect(page.getByTestId('capability-grant-error')).toBeVisible();
  await expect(page.getByTestId('capability-grant-form')).toBeVisible();
});

test('grantor revokes, and the revoked delegate is refused', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await signInAs(page, ids.bob);

  const row = capRow(page, ids.bob, ids.carol);
  await row.getByTestId('revoke-capability').click();
  await page.getByTestId('capability-revoke-reason').fill('real-stack e2e');
  await page.getByTestId('capability-revoke-submit').click();
  await expect(page.getByTestId('capability-revoke-form')).toHaveCount(0);

  await page.reload();
  await expect(row).toContainText('Revoked');
  await expect(row.getByTestId('revoke-capability')).toHaveCount(0);
});

test('revoked delegate can no longer tag', async ({ page }) => {
  await page.goto(`/objects/${ids.alpha}`);
  await signInAs(page, ids.carol);

  await page.getByTestId('tag-version').click();
  await page.getByTestId('version-tag-input').fill('1.3.0');
  await page.getByTestId('version-tag-submit').click();
  await expect(page.getByTestId('version-tag-error')).toBeVisible();

  await page.reload();
  await expect(tagRow(page, '1.3.0', ids.carol)).toHaveCount(0);
});
