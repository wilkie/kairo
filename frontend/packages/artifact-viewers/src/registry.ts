// Registry composition — wires the v1 viewer set
// (`PHASE_2_WEB_CLIENT.md` slice 10) into a typed list and
// exposes the `pickViewer` helper that walks the list in
// order and returns the first match.
//
// Order matters:
//
//   1. JSON  — narrows on parse-validity, so genuine JSON beats
//              generic text.
//   2. Text  — handles every UTF-8 payload that didn't parse
//              as JSON.
//   3. Binary — catch-all for anything else.
//
// The binary viewer is the safety net; `canView` returns true
// unconditionally and it only ever wins when the payload
// failed both prior checks. v1 ships nothing else.
//
// Apps that want extra viewers can build their own list with
// `[customViewer, ...defaultArtifactViewers]`.

import type { ArtifactRecord, ArtifactViewer } from './types';
import { textViewer } from './viewers/Text';
import { jsonViewer } from './viewers/Json';
import { binaryViewer } from './viewers/Binary';

/** v1 default viewer set, in priority order. */
export const defaultArtifactViewers: ReadonlyArray<ArtifactViewer> = [
  jsonViewer,
  textViewer,
  binaryViewer,
];

/** Walk a registry and return the first viewer whose
 * `canView` accepts the record. The default registry's binary
 * viewer is a catch-all, so this never returns `null` against
 * `defaultArtifactViewers`; the return type stays nullable
 * for callers passing custom registries that may not include
 * a fallback. */
export function pickViewer(
  record: ArtifactRecord,
  registry: ReadonlyArray<ArtifactViewer> = defaultArtifactViewers,
): ArtifactViewer | null {
  for (const viewer of registry) {
    if (viewer.canView(record)) {
      return viewer;
    }
  }
  return null;
}
