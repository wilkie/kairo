// E2E coverage for provenance badges (MSW, `PHASE_2_FEDERATION.md`
// slice 5): records a federation pull brought in say where they came
// from; local records show no badge.

import { expect, test } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

test.describe('provenance', () => {
  test('a fetched object names its peer', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.beta));
    await expect(page.getByTestId('provenance-badge')).toHaveText(
      'Fetched from archive.example.org',
    );
  });

  test('a statement seen from two peers counts the second', async ({ page }) => {
    await page.goto(bareRoute('statement', mockIds.alphaRev2Stmt));
    await expect(page.getByTestId('provenance-badge')).toHaveText(
      'Fetched from archive.example.org +1',
    );
  });

  test('local records show no badge', async ({ page }) => {
    await page.goto(bareRoute('object', mockIds.alpha));
    await expect(page.getByText('Genesis').first()).toBeVisible();
    await expect(page.getByTestId('provenance-badge')).toHaveCount(0);
    await page.goto(bareRoute('actor', mockIds.alice));
    await expect(page.getByTestId('provenance-badge')).toHaveCount(0);
  });
});
