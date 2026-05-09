// Public package surface. The `mock` subpath
// (`@kairo/api-client/mock`) is exported separately so MSW and
// its handlers never leak into the production bundle.

export { createKairoClient, type KairoApiClient, type CreateKairoClientOptions } from './client';
export type { VersionInfo, StatusInfo, ValidationResult } from './client';

export { createTransport, type TransportOptions } from './transport';

export { unwrapEnvelope, EnvelopeError } from './envelope';
export type { ApiEnvelope, ApiSuccess, ApiFailure } from './envelope';

export { KairoApiClientError, type ApiClientError } from './error';

export { kairoKeys } from './keys';
export { KairoProvider, useKairoClient, type KairoProviderProps } from './provider';

export { useVersion } from './hooks';

// Generated path/component types — exposed so callers can
// reference exact wire shapes by name (e.g.,
// `components['schemas']['BranchTipDto']`).
export type { paths, components } from './generated/schema';
