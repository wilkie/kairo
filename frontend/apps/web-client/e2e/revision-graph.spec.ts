// Slice-9 follow-up: the Revision graph panel on the object
// detail page. Asserts that the StatementGraphView mounts with
// the correct accessible name and that each seeded revision is
// represented as a clickable link in the graph's label column.
//
// We query by role + accessible name (the SVG itself is
// `aria-hidden`; the figure wrapper carries the summary
// `aria-label`) so the test exercises the same surface a screen
// reader would.

import { expect, test } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

test('revision graph renders one figure with the seeded revisions as links', async ({ page }) => {
  await page.goto(bareRoute('object', mockIds.alpha));

  await expect(
    page.getByRole('heading', { level: 3, name: 'Revision graph', exact: true }),
  ).toBeVisible();

  // The graph view wraps its SVG + label column in a `role="figure"`
  // with a one-line summary `aria-label`. Alpha's fixture seeds
  // three linear revisions.
  const figure = page.getByRole('figure', { name: 'Revision graph: 3 revisions' });
  await expect(figure).toBeVisible();

  // Each revision label is an IdLink — `<a>` with the full
  // statement id as the accessible name. Assert all three
  // revisions are reachable from inside the figure.
  for (const stmtId of [mockIds.alphaRev1Stmt, mockIds.alphaRev2Stmt, mockIds.alphaRev3Stmt]) {
    await expect(figure.getByRole('link', { name: stmtId })).toBeVisible();
  }
});
