// Shared types for the artifact-viewer registry
// (`WEB_CLIENT.md` §12.3).
//
// `ArtifactRecord` is what each viewer's `canView` predicate
// inspects; `ArtifactViewerProps` is what gets passed to the
// viewer React component when it's chosen. The registry only
// concerns itself with content-addressed bytes — there is no
// MIME type, no file extension, no out-of-band hint per
// §12 (the daemon's blob endpoint is `application/octet-stream`
// for everything; the inspector decides what to do with the
// bytes itself).

import type { ComponentType } from 'react';

/** What the registry sees about a blob: the wire id, the
 * fetched bytes, and the cheap sniff result so viewers don't
 * each re-decode the bytes themselves. */
export interface ArtifactRecord {
  /** `kairo:blob:zXyz…` */
  blobId: string;
  /** The raw bytes as fetched from `/api/v1/blobs/{id}`. */
  bytes: Uint8Array;
  /** Output of {@link sniffArtifact}. Cached on the record so
   * each `canView` predicate inspects the same shape. */
  sniff: ArtifactSniff;
}

/** Result of a content sniff. v1 only distinguishes text vs
 * binary; the `text` field is present when the sniff
 * succeeded as UTF-8. */
export interface ArtifactSniff {
  kind: 'text' | 'binary';
  byteLength: number;
  /** Decoded UTF-8 text when `kind === 'text'`. */
  text?: string;
}

/** Props every viewer component receives. */
export interface ArtifactViewerProps {
  record: ArtifactRecord;
}

/** Safety class per `WEB_CLIENT.md` §12.3. v1 viewers are all
 * `inline-safe`; sandboxed and runtime-required viewers are
 * post-v1 (HTML applications, JavaScript, emulators, VM
 * displays etc.). */
export type ArtifactViewerSafety =
  | 'inline-safe'
  | 'sandbox-required'
  | 'daemon-runtime-required';

/** One entry in the registry. */
export interface ArtifactViewer {
  /** Stable identifier used for diagnostics + the registry's
   * "which viewer chose this record" affordance. */
  id: string;
  /** Human label for badges / "viewing as text" indicators. */
  label: string;
  /** True if the viewer can render this record. The registry
   * picks the first viewer whose `canView` returns true. */
  canView: (record: ArtifactRecord) => boolean;
  /** The React component that renders the record. */
  Viewer: ComponentType<ArtifactViewerProps>;
  /** Safety class — drives any "this needs a sandbox" UI. */
  safety: ArtifactViewerSafety;
}
