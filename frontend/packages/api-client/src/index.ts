export {
  createKairoClient,
  KairoApiClientError,
  type KairoApiClient,
  type ApiClientError,
  type VersionInfo,
} from './client';

// Re-export the generated path/component types so consumers can
// reference the exact wire shapes without importing the
// generated module directly. Treat this as the package surface;
// internal callers should always reach for these names.
export type { paths, components } from './generated/schema';
