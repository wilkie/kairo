// E2E coverage for the first write UI: expressing a trust
// opinion via cookie-mode write.
//
// Loads the actor-detail page for Bob, signs in as Alice via
// the actor picker, opens the trust-opinion dialog, submits,
// and asserts the new opinion appears in the Trust opinions
// panel. MSW seeds two keystore actors and a session-write
// handler so the whole round trip runs in-page without a
// daemon.

import { expect, test } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

const ALICE_PREFIX = mockIds.alice.slice(0, 8);

test.describe('express trust opinion', () => {
  test('signed-in user can add a trust opinion that appears in the panel', async ({ page }) => {
    // Visit Bob's actor page. The trust panel starts empty
    // (mock registry seeds no opinions about Bob).
    await page.goto(bareRoute('actor', mockIds.bob));

    // Anonymous landing: the trust-opinion button is hidden.
    await expect(page.getByTestId('express-trust-opinion')).toHaveCount(0);

    // Sign in as Alice via the actor picker.
    await page.getByTestId('actor-picker-sign-in').click();
    await page.getByTestId('actor-picker-actor').first().click();
    await page.getByTestId('actor-picker-confirm-button').click();
    await expect(page.getByTestId('actor-picker-signed-in')).toContainText(ALICE_PREFIX);

    // The button appears once signed in.
    const trigger = page.getByTestId('express-trust-opinion');
    await expect(trigger).toBeVisible();

    // Open the dialog, submit the default (Trusted) decision.
    await trigger.click();
    await expect(page.getByTestId('trust-opinion-form')).toBeVisible();
    await page.getByTestId('trust-reason-input').fill('e2e-smoke');
    await page.getByTestId('trust-opinion-submit').click();

    // Dialog closes; the trust panel re-fetches and shows the
    // new opinion attributed to Alice.
    await expect(page.getByTestId('trust-opinion-form')).toHaveCount(0);
    await expect(page.getByRole('cell', { name: 'Trusted', exact: true })).toBeVisible();
  });

  test('untrusted decision lands as an Untrusted row', async ({ page }) => {
    await page.goto(bareRoute('actor', mockIds.bob));

    await page.getByTestId('actor-picker-sign-in').click();
    await page.getByTestId('actor-picker-actor').first().click();
    await page.getByTestId('actor-picker-confirm-button').click();

    await page.getByTestId('express-trust-opinion').click();
    await page.getByRole('radio', { name: 'Untrusted' }).click();
    await page.getByTestId('trust-opinion-submit').click();

    await expect(page.getByTestId('trust-opinion-form')).toHaveCount(0);
    await expect(page.getByRole('cell', { name: 'Untrusted', exact: true })).toBeVisible();
  });

  test('cancel from the dialog leaves the trust panel untouched', async ({ page }) => {
    await page.goto(bareRoute('actor', mockIds.bob));

    await page.getByTestId('actor-picker-sign-in').click();
    await page.getByTestId('actor-picker-actor').first().click();
    await page.getByTestId('actor-picker-confirm-button').click();

    await page.getByTestId('express-trust-opinion').click();
    await page.getByRole('button', { name: /cancel/i }).click();

    await expect(page.getByTestId('trust-opinion-form')).toHaveCount(0);
    // No "Trusted" cell — the panel is still empty.
    await expect(page.getByRole('cell', { name: 'Trusted', exact: true })).toHaveCount(0);
  });
});
