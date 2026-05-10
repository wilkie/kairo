// "Render an `unverified` raw statement listing" — the
// statement-detail page never runs the verifier itself, so
// per `WEB_CLIENT.md` §10 it must not let the user infer
// validity. The page shoulders this with an `Unverified`
// validation badge in the panel header alongside an explicit
// "this view does not run verification" copy line.

import { expect, test } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

test('statement detail page renders the Unverified badge', async ({ page }) => {
  await page.goto(bareRoute('statement', mockIds.alphaRev1Stmt));

  await expect(page.getByRole('heading', { level: 2, name: 'Statement' })).toBeVisible();
  await expect(page.getByText('Unverified', { exact: true })).toBeVisible();
});
