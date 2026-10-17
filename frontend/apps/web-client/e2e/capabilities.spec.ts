// E2E coverage for the capability write surface (MSW):
//
// 1. Object owner (Alice) updates her grant to Bob — the dialog
//    pre-fills it and the row's kinds change.
// 2. Alice grants a new grantee — a new row appears.
// 3. Alice revokes her grant to Bob — the row shows Revoked and
//    loses its Revoke action.
// 4. Bob sees Revoke only on the grant he issued, and granting a
//    kind he doesn't hold with `delegable` is refused inline.

import { expect, test, type Page } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

const ALICE_PREFIX = mockIds.alice.slice(0, 8);
const NEW_GRANTEE = 'zMockActorDave00000000000000000000000000000';

async function signIn(page: Page, index: number, prefix: string) {
  await page.getByTestId('actor-picker-sign-in').click();
  await page.getByTestId('actor-picker-actor').nth(index).click();
  await page.getByTestId('actor-picker-confirm-button').click();
  await expect(page.getByTestId('actor-picker-signed-in')).toContainText(prefix);
}

/** The Capability heads row for `grantor → grantee`, matched on
 * the actor links (mock ids share long prefixes). */
function capRow(page: Page, grantor: string, grantee: string) {
  return page
    .getByRole('row')
    .filter({ has: page.locator(`a[href="${bareRoute('actor', grantor)}"]`) })
    .filter({ has: page.locator(`a[href="${bareRoute('actor', grantee)}"]`) });
}

test.describe('capabilities', () => {
  test('owner updates an existing grant with its kinds pre-filled', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.alpha));
    await expect(page.getByTestId('grant-capability')).toHaveCount(0);
    await signIn(page, 0, ALICE_PREFIX);

    await page.getByTestId('grant-capability').click();
    await page.getByTestId('capability-grantee-input').fill(mockIds.bob);
    await expect(page.getByTestId('capability-kind-ObjectVersionTag')).toBeChecked();
    await expect(page.getByTestId('capability-delegable')).toBeChecked();
    await expect(page.getByTestId('capability-grant-submit')).toContainText(/update/i);
    await page.getByTestId('capability-kind-ObjectBranch').check();
    await page.getByTestId('capability-grant-submit').click();

    await expect(page.getByTestId('capability-grant-form')).toHaveCount(0);
    await expect(capRow(page, mockIds.alice, mockIds.bob)).toContainText(
      'ObjectBranch, ObjectVersionTag',
    );
  });

  test('owner grants a new grantee', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.alpha));
    await signIn(page, 0, ALICE_PREFIX);

    await page.getByTestId('grant-capability').click();
    await page.getByTestId('capability-grantee-input').fill(NEW_GRANTEE);
    await page.getByTestId('capability-kind-ObjectRevision').check();
    await page.getByTestId('capability-expires-input').fill('2030-01-01');
    await expect(page.getByTestId('capability-grant-submit')).toContainText(/^grant$/i);
    await page.getByTestId('capability-grant-submit').click();

    await expect(page.getByTestId('capability-grant-form')).toHaveCount(0);
    const row = capRow(page, mockIds.alice, NEW_GRANTEE);
    await expect(row).toContainText('ObjectRevision');
    await expect(row).toContainText('2030-01-01T00:00:00Z');
  });

  test('grantor revokes a grant', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.alpha));
    await signIn(page, 0, ALICE_PREFIX);

    const row = capRow(page, mockIds.alice, mockIds.bob);
    await row.getByTestId('revoke-capability').click();
    await expect(page.getByTestId('capability-revoke-retroactive')).not.toBeChecked();
    await page.getByTestId('capability-revoke-reason').fill('e2e rotation');
    await page.getByTestId('capability-revoke-submit').click();

    await expect(page.getByTestId('capability-revoke-form')).toHaveCount(0);
    await expect(row).toContainText('Revoked');
    await expect(row.getByTestId('revoke-capability')).toHaveCount(0);
  });

  test('non-owner revokes only their own grants and cannot regrant without delegable', async ({
    page,
  }) => {
    await page.goto(bareRoute('object', mockIds.alpha));
    await signIn(page, 1, mockIds.bob.slice(0, 8));

    await expect(
      capRow(page, mockIds.alice, mockIds.bob).getByTestId('revoke-capability'),
    ).toHaveCount(0);
    await expect(
      capRow(page, mockIds.bob, mockIds.carol).getByTestId('revoke-capability'),
    ).toBeVisible();

    await page.getByTestId('grant-capability').click();
    await page.getByTestId('capability-grantee-input').fill(NEW_GRANTEE);
    await page.getByTestId('capability-kind-ObjectBranch').check();
    await page.getByTestId('capability-grant-submit').click();
    await expect(page.getByTestId('capability-grant-error')).toContainText(/may not grant/i);
    await expect(page.getByTestId('capability-grant-form')).toBeVisible();
  });
});
