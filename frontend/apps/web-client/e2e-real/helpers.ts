// Shared helpers for the real-stack suite. IDs are whatever
// `scripts/smoke-seed.sh` generated for this run (bare payloads),
// passed from global setup through `KAIRO_E2E_SEED`.

import { expect, type Page } from '@playwright/test';

export interface SeedIds {
  alice: string;
  bob: string;
  carol: string;
  alpha: string;
  beta: string;
  alphaR1: string;
  alphaR2: string;
  alphaR3: string;
}

/** What `scripts/federation-smoke.sh` pulled into the store. */
export interface FederationIds {
  object: string;
  revision: string;
  peerUrl: string;
  nodeActor: string;
  objectFormat: string;
}

export function federation(): FederationIds {
  const raw = process.env['KAIRO_E2E_FEDERATION'];
  if (raw === undefined) {
    throw new Error('KAIRO_E2E_FEDERATION unset; run via playwright.real.config.ts');
  }
  return JSON.parse(raw) as FederationIds;
}

export function seed(): SeedIds {
  const raw = process.env['KAIRO_E2E_SEED'];
  if (raw === undefined) throw new Error('KAIRO_E2E_SEED unset; run via playwright.real.config.ts');
  return JSON.parse(raw) as SeedIds;
}

/** Sign in through the actor picker as `actorId`. The real
 * keystore lists actors in directory order, so pick by the
 * full id the picker carries in each row's `title`. */
export async function signInAs(page: Page, actorId: string): Promise<void> {
  await page.getByTestId('actor-picker-sign-in').click();
  await page.locator(`[data-testid="actor-picker-actor"][title="${actorId}"]`).click();
  await page.getByTestId('actor-picker-confirm-button').click();
  await expect(page.getByTestId('actor-picker-signed-in')).toContainText(actorId.slice(0, 8));
}

/** A Tags-panel row for `version` signed by `actorId`. */
export function tagRow(page: Page, version: string, actorId: string) {
  return page
    .getByRole('row')
    .filter({ has: page.getByRole('cell', { name: version, exact: true }) })
    .filter({ hasText: actorId.slice(0, 8) });
}

/** A Capability heads row for `grantor → grantee`, matched on the
 * actor links. */
export function capRow(page: Page, grantor: string, grantee: string) {
  return page
    .getByRole('row')
    .filter({ has: page.locator(`a[href="/actors/${grantor}"]`) })
    .filter({ has: page.locator(`a[href="/actors/${grantee}"]`) });
}
