// Real-stack object creation: the daemon signs an ObjectGenesis
// for the session actor (filling created_by, created_at, and the
// nonce), the SPA lands on the new object, and the creator holds
// root authority on it.

import { expect, test } from '@playwright/test';
import { seed, signInAs } from './helpers';

const ids = seed();

test('creator lands on a new object they own, listed on their page', async ({ page }) => {
  await page.goto(`/actors/${ids.bob}`);
  await signInAs(page, ids.bob);

  await page.getByTestId('create-object').click();
  await page.getByTestId('create-object-kind-input').fill('real-stack-dataset');
  await page.getByTestId('create-object-submit').click();

  await expect(page).toHaveURL(/\/objects\/z[1-9A-HJ-NP-Za-km-z]+$/);
  const objectId = new URL(page.url()).pathname.split('/').pop() ?? '';
  await expect(page.getByText('real-stack-dataset')).toBeVisible();

  // Root authority: the owner-only grant action is accepted.
  await page.getByTestId('grant-capability').click();
  await page.getByTestId('capability-grantee-input').fill(ids.carol);
  await page.getByTestId('capability-kind-ObjectBranch').check();
  await page.getByTestId('capability-grant-submit').click();
  await expect(page.getByTestId('capability-grant-form')).toHaveCount(0);

  await page.goto(`/actors/${ids.bob}`);
  await expect(page.locator(`a[href="/objects/${objectId}"]`)).toBeVisible();
});
