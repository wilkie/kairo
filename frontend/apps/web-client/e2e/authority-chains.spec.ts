// Slice-9 follow-up: the Authority chains panel on the object
// detail page. Asserts that the AuthorityChainView mounts with
// the correct accessible name, that the two seeded chains
// (Alice → Bob single-hop head + Alice → Bob → Carol two-hop
// re-delegation) both render, and that each actor in the
// longer chain is reachable as a link.

import { expect, test } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

test('authority chains panel renders both seeded chains with actor links', async ({ page }) => {
  await page.goto(bareRoute('object', mockIds.alpha));

  await expect(
    page.getByRole('heading', { level: 3, name: 'Authority chains', exact: true }),
  ).toBeVisible();

  // The chain view wraps the list of chains in `role="list"`
  // with a one-line summary `aria-label`. Alpha's fixture has
  // two heads — Alice → Bob and Bob → Carol — so the view
  // computes two chains (Alice → Bob single hop, plus
  // Alice → Bob → Carol two-hop walking back through Bob's
  // upstream grant).
  const list = page.getByRole('list', { name: 'Authority chains: 2 chains' });
  await expect(list).toBeVisible();

  // Each chain is a `role="listitem"`.
  const items = list.getByRole('listitem');
  await expect(items).toHaveCount(2);

  // All three actors appear somewhere in the panel: Alice is the
  // root authority on both chains; Bob is the grantee in the
  // first chain and a re-grantor in the second; Carol is the
  // grantee in the longer chain. IdLink exposes the full id as
  // its accessible name.
  for (const actorId of [mockIds.alice, mockIds.bob, mockIds.carol]) {
    await expect(list.getByRole('link', { name: actorId }).first()).toBeVisible();
  }

  // Both grant statement ids are linkable from the chain edges.
  // The Alice→Bob grant appears in two chains — once as chain
  // 1's only step, once as chain 2's upstream walk-back — so
  // expect at least one matching link rather than exactly one.
  for (const stmtId of [mockIds.aliceGrantsBobStmt, mockIds.bobGrantsCarolStmt]) {
    await expect(list.getByRole('link', { name: stmtId }).first()).toBeVisible();
  }
});
