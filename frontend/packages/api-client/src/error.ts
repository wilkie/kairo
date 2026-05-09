// Public client-side error type. Mirrors `WEB_CLIENT.md` §6
// `ApiClientError`; later slices add the validation /
// unauthorized variants when the relevant endpoints are wired.

export type ApiClientError =
  | { kind: 'network'; message: string }
  | { kind: 'daemon'; code: string; message: string; status: number | undefined }
  | { kind: 'decode'; message: string };

export class KairoApiClientError extends Error {
  readonly detail: ApiClientError;

  constructor(detail: ApiClientError) {
    super(formatMessage(detail));
    this.name = 'KairoApiClientError';
    this.detail = detail;
  }
}

function formatMessage(detail: ApiClientError): string {
  switch (detail.kind) {
    case 'network':
      return `network error: ${detail.message}`;
    case 'daemon': {
      const status = detail.status === undefined ? '' : ` (HTTP ${detail.status})`;
      return `daemon error ${detail.code}${status}: ${detail.message}`;
    }
    case 'decode':
      return `decode error: ${detail.message}`;
  }
}
