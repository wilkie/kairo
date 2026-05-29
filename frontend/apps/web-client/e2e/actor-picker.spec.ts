// E2E coverage for `<ActorPicker />`.
//
// The SPA boots with MSW seeded with two keystore actors
// (Alice + Bob). This spec drives the full anonymous →
// sign-in → confirm → signed-in → sign-out cycle through the
// real DOM, hitting the MSW handlers that match the daemon's
// session endpoints.

import { expect, test } from '@playwright/test';
import { mockIds } from './helpers';

const ALICE_PREFIX = mockIds.alice.slice(0, 8);
const ALICE_SUFFIX = mockIds.alice.slice(-4);
// Note: the mock IDs for Alice and Bob share the same 8-char
// prefix and 4-char suffix, so the truncated display
// (`<prefix>…<suffix>`) renders identically. Tests that need
// to pick a *specific* actor row select by row index (e.g.
// `nth(1)` for the second seeded actor) or by the full id in
// the row's `title` attribute, not by truncated text.

test.describe('actor picker', () => {
  test('signs in and signs out via the dashboard header', async ({ page }) => {
    await page.goto('/');

    // Anonymous landing: the picker shows a "Sign in" button.
    await expect(page.getByTestId('actor-picker-sign-in')).toBeVisible();

    // Open the popover; both seeded actors are listed.
    await page.getByTestId('actor-picker-sign-in').click();
    const items = page.getByTestId('actor-picker-actor');
    await expect(items).toHaveCount(2);
    await expect(items.first()).toContainText(ALICE_PREFIX);

    // Select Alice; confirmation view appears.
    await items.first().click();
    await expect(page.getByTestId('actor-picker-confirm')).toBeVisible();

    // Confirm; the picker flips to its signed-in state.
    await page.getByTestId('actor-picker-confirm-button').click();
    const signedIn = page.getByTestId('actor-picker-signed-in');
    await expect(signedIn).toBeVisible();
    await expect(signedIn).toContainText(ALICE_PREFIX);
    await expect(signedIn).toContainText(ALICE_SUFFIX);

    // Sign out; the picker returns to its anonymous state.
    await page.getByRole('button', { name: /sign out/i }).click();
    await expect(page.getByTestId('actor-picker-sign-in')).toBeVisible();
  });

  test('replacing a session via another sign-in works end-to-end', async ({ page }) => {
    await page.goto('/');

    // Sign in as Alice first.
    await page.getByTestId('actor-picker-sign-in').click();
    await page.getByTestId('actor-picker-actor').first().click();
    await page.getByTestId('actor-picker-confirm-button').click();
    await expect(page.getByTestId('actor-picker-signed-in')).toContainText(ALICE_PREFIX);

    // Sign out, then sign in as Bob — picker reflects the new actor.
    await page.getByRole('button', { name: /sign out/i }).click();
    await page.getByTestId('actor-picker-sign-in').click();
    // Pick the second seeded actor (Bob) — the mock IDs render
    // an identical truncated label, so we index by row.
    await page.getByTestId('actor-picker-actor').nth(1).click();
    await page.getByTestId('actor-picker-confirm-button').click();
    await expect(page.getByTestId('actor-picker-signed-in')).toBeVisible();
  });

  test('cancel from the confirmation step leaves the user anonymous', async ({ page }) => {
    await page.goto('/');

    await page.getByTestId('actor-picker-sign-in').click();
    await page.getByTestId('actor-picker-actor').first().click();
    await page.getByRole('button', { name: /cancel/i }).click();

    // Back to the list, both actors visible. No session created.
    await expect(page.getByTestId('actor-picker-actor')).toHaveCount(2);
    await expect(page.getByTestId('actor-picker-signed-in')).toHaveCount(0);
  });
});
