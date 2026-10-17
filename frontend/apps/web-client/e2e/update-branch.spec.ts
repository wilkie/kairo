// E2E coverage for the branch-update write surface:
//
// 1. Object owner (Alice) updates the existing `head` tip and
//    sees the tip's statement id change in the Branches panel.
// 2. Object owner creates a brand-new branch — appears as a new
//    row in the panel.
// 3. Non-owner (Bob) signs in and hits the 403 `not_authorized`
//    surface inline in the dialog without the panel mutating.

import { expect, test } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

const ALICE_PREFIX = mockIds.alice.slice(0, 8);
const BOB_PREFIX = mockIds.bob.slice(0, 8);

async function signInAsFirstActor(page: import('@playwright/test').Page) {
  await page.getByTestId('actor-picker-sign-in').click();
  await page.getByTestId('actor-picker-actor').first().click();
  await page.getByTestId('actor-picker-confirm-button').click();
  await expect(page.getByTestId('actor-picker-signed-in')).toContainText(ALICE_PREFIX);
}

async function signInAsSecondActor(page: import('@playwright/test').Page) {
  await page.getByTestId('actor-picker-sign-in').click();
  await page.getByTestId('actor-picker-actor').nth(1).click();
  await page.getByTestId('actor-picker-confirm-button').click();
  await expect(page.getByTestId('actor-picker-signed-in')).toContainText(BOB_PREFIX);
}

test.describe('update branch', () => {
  test('object owner can update the head tip in place', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.alpha));

    // Anonymous: trigger is hidden.
    await expect(page.getByTestId('update-branch')).toHaveCount(0);

    await signInAsFirstActor(page);
    await expect(page.getByTestId('update-branch')).toBeVisible();

    // Capture the existing `head` tip's statement id before
    // the update so we can verify it changes.
    const headRow = page
      .getByRole('row')
      .filter({ hasText: 'head' })
      .filter({ hasText: ALICE_PREFIX });
    const priorStatement = (await headRow.locator('td').nth(2).textContent())?.trim() ?? '';
    expect(priorStatement.length).toBeGreaterThan(0);

    await page.getByTestId('update-branch').click();
    await expect(page.getByTestId('branch-update-form')).toBeVisible();

    // Type the existing branch name → dialog flips to Update.
    await page.getByTestId('branch-name-input').fill('head');
    await expect(page.getByTestId('branch-mode-hint')).toContainText(/replaces your prior tip/i);
    await expect(page.getByTestId('branch-update-submit')).toContainText(/update/i);
    await page.getByTestId('branch-update-submit').click();

    // Dialog closes; the panel re-renders with a fresh
    // statement id for the head tip.
    await expect(page.getByTestId('branch-update-form')).toHaveCount(0);
    await expect(headRow.locator('td').nth(2)).not.toHaveText(priorStatement);
  });

  test('object owner can create a fresh branch', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.alpha));
    await signInAsFirstActor(page);

    await page.getByTestId('update-branch').click();
    await page.getByTestId('branch-name-input').fill('release-2.x');
    await expect(page.getByTestId('branch-mode-hint')).toContainText(/creates a new branch/i);
    await expect(page.getByTestId('branch-update-submit')).toContainText(/create/i);
    await page.getByTestId('branch-update-submit').click();

    await expect(page.getByTestId('branch-update-form')).toHaveCount(0);
    // The Branches table grows: a new row with the fresh name.
    await expect(page.getByRole('row').filter({ hasText: 'release-2.x' })).toBeVisible();
  });

  test('non-owner sees a 403 surface inline and the panel is untouched', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.alpha));
    await signInAsSecondActor(page);

    await page.getByTestId('update-branch').click();
    await page.getByTestId('branch-name-input').fill('head');
    await page.getByTestId('branch-update-submit').click();

    // Dialog stays open; error visible inline.
    const error = page.getByTestId('branch-update-error');
    await expect(error).toBeVisible();
    await expect(error).toContainText(/lacks an ObjectBranch capability/i);
    await expect(page.getByTestId('branch-update-form')).toBeVisible();

    // No new "head" branch entry from Bob in the panel.
    const bobHeadRow = page
      .getByRole('row')
      .filter({ hasText: 'head' })
      .filter({ hasText: BOB_PREFIX });
    await expect(bobHeadRow).toHaveCount(0);
  });
});
