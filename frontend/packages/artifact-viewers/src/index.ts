// `@kairo/artifact-viewers` — content-sniff-driven viewer
// registry per `WEB_CLIENT.md` §12.3 and slice 10 of
// `PHASE_2_WEB_CLIENT.md`.
//
// v1 ships three `inline-safe` viewers: text, JSON (with
// hand-rolled syntax coloring), and a binary placeholder +
// download. Image / audio / video / markdown / CSV viewers
// and any sandboxed / runtime-required viewers are deferred
// to follow-up slices once real workloads exist.

export { BlobPreview, useChosenArtifactViewer, type BlobPreviewProps } from './BlobPreview';
export { defaultArtifactViewers, pickViewer } from './registry';
export { sniffArtifact } from './sniffer';
export type {
  ArtifactRecord,
  ArtifactSniff,
  ArtifactViewer,
  ArtifactViewerProps,
  ArtifactViewerSafety,
} from './types';
export { textViewer } from './viewers/Text';
export { jsonViewer } from './viewers/Json';
export { binaryViewer } from './viewers/Binary';
