// "Preview a text blob; preview a binary blob." — exercises
// the artifact-viewers registry through the `/blobs/$id` route.
// The text and binary fixtures live in the mock registry
// (`textBlob` and `alphaManifestBlob` respectively); each
// should land on the matching viewer per the
// `defaultArtifactViewers` priority order (JSON → text →
// binary).

import { expect, test } from '@playwright/test';
import { bareRoute, mockIds } from './helpers';

test('text blob renders via the Text viewer', async ({ page }) => {
  await page.goto(bareRoute('blob', mockIds.textBlob));

  await expect(page.getByRole('heading', { level: 2, name: 'Blob' })).toBeVisible();
  // Panel title doubles as the chosen-viewer affordance.
  await expect(page.getByRole('heading', { level: 3, name: 'Viewing as Text' })).toBeVisible();

  // The seeded TOML manifest content surfaces inside the
  // monospace `<pre>`.
  await expect(page.getByText('[kairo]')).toBeVisible();
});

test('binary blob renders via the Binary viewer with a download affordance', async ({ page }) => {
  await page.goto(bareRoute('blob', mockIds.alphaManifestBlob));

  await expect(page.getByRole('heading', { level: 2, name: 'Blob' })).toBeVisible();
  await expect(page.getByRole('heading', { level: 3, name: 'Viewing as Binary' })).toBeVisible();

  // The 4-byte fixture surfaces its byte count alongside the
  // download button.
  await expect(page.getByText('4 bytes', { exact: true })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Download' })).toBeVisible();
});
