// Legacy single-fixture exports. Slice 8 introduced a typed
// mock registry (`./registry.ts`) with multiple objects /
// actors / statements; the named exports here remain so that
// pre-registry callers (the contract test, ad-hoc consumers)
// keep working — each one just re-exposes the corresponding
// canonical entry from the registry.
//
// New code should reach for `mockRegistry` and `mockIds`
// directly. The fixture-as-aggregated-object pattern (`fixtures`
// below) only carries the single-row entries the slice 6 mock
// surface had.

import { legacyMockIdAliases, mockIds as registryIds, mockRegistry } from './registry';
import type { components } from '../generated/schema';

type Schemas = components['schemas'];

const aliceActor = mockRegistry.actors[registryIds.alice];
const alphaObject = mockRegistry.objects[registryIds.alpha];
const aliceSelfTrust = mockRegistry.trustPair[`${registryIds.alice}:${registryIds.alice}`];
const alphaHeadBranch = mockRegistry.branchLatest[
  `${registryIds.alpha}:head:${registryIds.alice}`
];
const alphaV1Tag = mockRegistry.versionTagLatest[
  `${registryIds.alpha}:v1.0.0:${registryIds.alice}`
];
const aliceCapabilityHeads = mockRegistry.capabilitiesFromGrantor[registryIds.alice] ?? [];
const alphaRevisionStmt = mockRegistry.statements[registryIds.alphaRev1Stmt];
const alphaBlobBytes = mockRegistry.blobs[registryIds.alphaManifestBlob] ?? new Uint8Array();
const alphaValidation = mockRegistry.verifyObject[registryIds.alpha];

if (
  aliceActor === undefined ||
  alphaObject === undefined ||
  aliceSelfTrust === undefined ||
  alphaHeadBranch === undefined ||
  alphaV1Tag === undefined ||
  alphaRevisionStmt === undefined ||
  alphaValidation === undefined
) {
  // Registry seeding bug — fail loud at import time.
  throw new Error('mock registry is missing canonical entries');
}

export const versionFixture: Schemas['VersionInfo'] = mockRegistry.version;
export const statusFixture: Schemas['StatusInfo'] = mockRegistry.status;
export const actorFixture: Schemas['ActorGenesisJson'] = aliceActor;
export const objectFixture: Schemas['ObjectGenesisStatementJson'] = alphaObject;
export const branchTipsFixture: Schemas['BranchTipDto'][] =
  mockRegistry.branchTips[registryIds.alpha] ?? [];
export const branchLatestFixture: Schemas['ObjectBranchStatementJson'] = alphaHeadBranch;
export const versionTagLatestFixture: Schemas['ObjectVersionTagStatementJson'] = alphaV1Tag;
export const trustFixture: Schemas['ActorTrustStatementJson'] = aliceSelfTrust;
export const capabilityHeadsFixture: Schemas['CapabilityHeadDto'][] = aliceCapabilityHeads;
export const versionTagHeadsFixture: Schemas['VersionTagHeadDto'][] =
  mockRegistry.versionTagHeads[registryIds.alpha] ?? [];
export const revisionHeadsFixture: Schemas['RevisionHeadDto'][] =
  mockRegistry.revisionHeads[registryIds.alpha] ?? [];
export const trustHeadsFixture: Schemas['TrustHeadDto'][] =
  mockRegistry.trustAbout[registryIds.alice] ?? [];
export const verifyObjectFixture: Schemas['ValidationResult'] = alphaValidation;

/** Polymorphic statement-by-id fixture — the canonical revision
 * statement on Alpha. */
export const statementFixture: unknown = alphaRevisionStmt;

/**
 * Aggregated single-row default fixture set. Kept for callers
 * that haven't migrated to `mockRegistry`. New code should
 * reach for the registry directly.
 */
export const fixtures = {
  version: versionFixture,
  status: statusFixture,
  actor: actorFixture,
  object: objectFixture,
  statement: statementFixture,
  branchTips: branchTipsFixture,
  branchLatest: branchLatestFixture,
  versionTagHeads: versionTagHeadsFixture,
  versionTagLatest: versionTagLatestFixture,
  revisionHeads: revisionHeadsFixture,
  trust: trustFixture,
  trustHeads: trustHeadsFixture,
  capabilityHeads: capabilityHeadsFixture,
  capabilityHeadsForObject: capabilityHeadsFixture,
  blob: alphaBlobBytes,
  verifyObject: verifyObjectFixture,
} as const;

/** Stable identifier strings the mock handlers and tests reach
 * for. The legacy short-name keys (`actor`, `object`, etc.)
 * point at Alice / Alpha for back-compat with the slice 6
 * contract tests; the full set of registry ids is also
 * exposed by spreading {@link registryIds}. */
export const mockIds = {
  ...registryIds,
  ...legacyMockIdAliases,
} as const;
