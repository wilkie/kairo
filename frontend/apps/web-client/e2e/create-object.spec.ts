// E2E coverage for creating an object (MSW): from the signed-in
// actor's own page and from the Objects page, landing on the new
// object's page; the action is absent on other actors' pages.

import { expect, test, type Page } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

const ALICE_PREFIX = mockIds.alice.slice(0, 8);

async function signInAsAlice(page: Page) {
  await page.getByTestId('actor-picker-sign-in').click();
  await page.getByTestId('actor-picker-actor').first().click();
  await page.getByTestId('actor-picker-confirm-button').click();
  await expect(page.getByTestId('actor-picker-signed-in')).toContainText(ALICE_PREFIX);
}

test.describe('create object', () => {
  test('from the actor page, landing on the new object', async ({ page }) => {
    await page.goto(bareRoute('actor', mockIds.alice));
    await expect(page.getByTestId('create-object')).toHaveCount(0);
    await signInAsAlice(page);

    await page.getByTestId('create-object').click();
    await page.getByTestId('create-object-kind-input').fill('e2e-emulator');
    await page.getByTestId('create-object-submit').click();

    await expect(page).toHaveURL(/\/objects\/zMockSessionObject/);
    await expect(page.getByText('e2e-emulator')).toBeVisible();
  });

  test('from the objects page', async ({ page }) => {
    await page.goto('/objects');
    await signInAsAlice(page);
    await page.getByTestId('create-object').click();
    await page.getByTestId('create-object-submit').click();
    await expect(page).toHaveURL(/\/objects\/zMockSessionObject/);
  });

  test("is not offered on another actor's page", async ({ page }) => {
    await page.goto(bareRoute('actor', mockIds.bob));
    await signInAsAlice(page);
    await expect(page.getByTestId('express-trust-opinion')).toBeVisible();
    await expect(page.getByTestId('create-object')).toHaveCount(0);
  });
});
