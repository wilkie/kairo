// E2E coverage for the version-tag write surface:
//
// 1. Object owner (Alice) retargets the existing `1.0.0` tag
//    and sees its statement id change in the Tags panel.
// 2. Object owner creates a brand-new tag.
// 3. A capability grantee (Bob, via the seeded Alice → Bob
//    `ObjectVersionTag` grant on Alpha) can tag too — the
//    non-owner path the branch dialog's mock doesn't cover.
//
// The capability-denied path is covered at the mock-contract
// layer (`packages/api-client/tests/contract.test.ts`): every
// keystore actor holds the capability on Alpha, and no other
// seeded object has revisions to tag.

import { expect, test } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

const ALICE_PREFIX = mockIds.alice.slice(0, 8);
const BOB_PREFIX = mockIds.bob.slice(0, 8);

async function signIn(page: import('@playwright/test').Page, index: number, prefix: string) {
  await page.getByTestId('actor-picker-sign-in').click();
  await page.getByTestId('actor-picker-actor').nth(index).click();
  await page.getByTestId('actor-picker-confirm-button').click();
  await expect(page.getByTestId('actor-picker-signed-in')).toContainText(prefix);
}

function tagRow(page: import('@playwright/test').Page, version: string, prefix: string) {
  return page
    .getByRole('row')
    .filter({ has: page.getByRole('cell', { name: version, exact: true }) })
    .filter({ hasText: prefix });
}

test.describe('tag version', () => {
  test('object owner can retarget an existing tag', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.alpha));
    await expect(page.getByTestId('tag-version')).toHaveCount(0);

    await signIn(page, 0, ALICE_PREFIX);
    await expect(page.getByTestId('tag-version')).toBeVisible();

    const row = tagRow(page, '1.0.0', ALICE_PREFIX);
    const prior = (await row.locator('td').nth(2).textContent())?.trim() ?? '';
    expect(prior.length).toBeGreaterThan(0);

    await page.getByTestId('tag-version').click();
    await page.getByTestId('version-tag-input').fill('1.0.0');
    await expect(page.getByTestId('version-tag-mode-hint')).toContainText(/replaces your prior/i);
    await expect(page.getByTestId('version-tag-submit')).toContainText(/retarget/i);
    await page.getByTestId('version-tag-submit').click();

    await expect(page.getByTestId('version-tag-form')).toHaveCount(0);
    await expect(row.locator('td').nth(2)).not.toHaveText(prior);
  });

  test('object owner can create a fresh tag', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.alpha));
    await signIn(page, 0, ALICE_PREFIX);

    await page.getByTestId('tag-version').click();
    await page.getByTestId('version-tag-input').fill('v2.0.0');
    await expect(page.getByTestId('version-tag-submit')).toBeDisabled();
    await page.getByTestId('version-tag-input').fill('2.0.0');
    await expect(page.getByTestId('version-tag-mode-hint')).toContainText(/creates a new/i);
    await page.getByTestId('version-tag-submit').click();

    await expect(page.getByTestId('version-tag-form')).toHaveCount(0);
    await expect(tagRow(page, '2.0.0', ALICE_PREFIX)).toBeVisible();
  });

  test('capability grantee can tag an object it does not own', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.alpha));
    await signIn(page, 1, BOB_PREFIX);

    await page.getByTestId('tag-version').click();
    await page.getByTestId('version-tag-input').fill('1.2.0-rc.1');
    await page.getByTestId('version-tag-submit').click();

    await expect(page.getByTestId('version-tag-form')).toHaveCount(0);
    await expect(tagRow(page, '1.2.0-rc.1', BOB_PREFIX)).toBeVisible();
  });
});
