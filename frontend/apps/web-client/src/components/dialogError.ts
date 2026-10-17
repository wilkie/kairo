// Maps a mutation's `KairoApiClientError` onto the `{ code,
// message }` shape every write dialog in `@kairo/ui` renders
// inline. Shared by the dialog containers.

import type { KairoApiClientError } from '@kairo/api-client';

export interface DialogError {
  code: string;
  message: string;
}

export function toDialogError(error: KairoApiClientError): DialogError {
  const detail = error.detail;
  if (detail.kind === 'daemon') {
    return { code: detail.code, message: detail.message };
  }
  if (detail.kind === 'network') {
    return { code: 'network', message: detail.message };
  }
  return { code: 'decode', message: detail.message };
}
