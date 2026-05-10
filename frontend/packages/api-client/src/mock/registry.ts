// Typed mock registry that drives the MSW handlers in dev.
//
// Two seeded objects exercise the slice 8 inspector at the
// extremes the spec calls out:
//
// - **Alpha** — created by Alice, multi-revision (3),
//   multi-branch (head + experimental), multi-tag (v1.0.0,
//   v1.1.0), one cross-actor capability head
//   (Alice → Bob, ObjectVersionTag), and trust opinions about
//   Alice from Bob (trusted) and Carol (untrusted). Hits every
//   populated panel.
// - **Beta** — created by Alice, genesis only. Hits every
//   empty-state panel.
//
// Three actors:
//
// - **Alice** — rich genesis, 2-of-3 attestation threshold.
// - **Bob** — single-key genesis. Subject of one capability
//   grant from Alice and an opinion-about-Alice.
// - **Carol** — single-key genesis. Subject of one
//   opinion-about-Alice (untrusted).
//
// Handlers look up against this registry; missing entries
// return a daemon-style 404 envelope. To extend the dev
// surface, edit this file in one place — every handler picks
// up new entries automatically.

import type { components } from '../generated/schema';

type Schemas = components['schemas'];

// ---------------------------------------------------------------------------
// Canonical IDs

export const mockIds = {
  // Actors
  alice: 'kairo:actor:zMockActor00000000000000000000000000000000',
  bob: 'kairo:actor:zMockActorBob000000000000000000000000000000',
  carol: 'kairo:actor:zMockActorCarol00000000000000000000000000',

  // Objects
  alpha: 'kairo:object:zMockObject00000000000000000000000000000000',
  beta: 'kairo:object:zMockObjectBeta000000000000000000000000000',

  // Revision statements (Alpha)
  alphaRev1Stmt: 'kairo:stmt:zMockRevisionStmt00000000000000000000000000',
  alphaRev2Stmt: 'kairo:stmt:zMockRevisionStmtAlpha2000000000000000000',
  alphaRev3Stmt: 'kairo:stmt:zMockRevisionStmtAlpha3000000000000000000',

  // Branch statements (Alpha)
  alphaHeadBranchStmt: 'kairo:stmt:zMockBranchStmt000000000000000000000000000000',
  alphaExperimentalBranchStmt: 'kairo:stmt:zMockBranchStmtAlphaExp00000000000000000000',

  // Tag statements (Alpha)
  alphaV1Stmt: 'kairo:stmt:zMockTagStmt0000000000000000000000000000000000',
  alphaV1_1Stmt: 'kairo:stmt:zMockTagStmtAlphaV1_1000000000000000000000',

  // Trust statements (about Alice)
  bobTrustsAliceStmt: 'kairo:stmt:zMockTrustStmt000000000000000000000000000000',
  carolTrustsAliceStmt: 'kairo:stmt:zMockTrustStmtCarolAlice00000000000000000',

  // Capability grant (Alice → Bob, scope=Alpha)
  aliceGrantsBobStmt: 'kairo:stmt:zMockGrantStmt000000000000000000000000000000',

  // Blobs
  // - alphaManifestBlob: binary fixture; what revisions point at.
  // - textBlob:          plain text (TOML-ish manifest sample).
  // - jsonBlob:          JSON sample so the JSON viewer is exercisable.
  alphaManifestBlob: 'kairo:blob:zMockBlob000000000000000000000000000000000000',
  textBlob: 'kairo:blob:zMockBlobText000000000000000000000000000000',
  jsonBlob: 'kairo:blob:zMockBlobJson000000000000000000000000000000',

  // Revision IDs (git:sha256:* — not kairo-prefixed)
  alphaRev1: 'git:sha256:0000000000000000000000000000000000000001',
  alphaRev2: 'git:sha256:0000000000000000000000000000000000000002',
  alphaRev3: 'git:sha256:0000000000000000000000000000000000000003',
} as const;

// Back-compat aliases so existing callers using `mockIds.actor`
// / `mockIds.object` / etc. keep resolving. The canonical
// "primary" mock entries are Alice + Alpha.
export const legacyMockIdAliases = {
  actor: mockIds.alice,
  object: mockIds.alpha,
  blob: mockIds.alphaManifestBlob,
  branchStmt: mockIds.alphaHeadBranchStmt,
  revisionStmt: mockIds.alphaRev1Stmt,
  tagStmt: mockIds.alphaV1Stmt,
  trustStmt: mockIds.bobTrustsAliceStmt,
  grantStmt: mockIds.aliceGrantsBobStmt,
  revision: mockIds.alphaRev1,
} as const;

// ---------------------------------------------------------------------------
// Inline blob payloads — small enough to keep alongside the
// id table so the dev surface is reproducible without an
// external fixtures directory.

const SAMPLE_TEXT_BLOB = `[kairo]
schema = 1
kind = "kairo/object"
name = "alpha"

[content]
kind = "tree"
`;

const SAMPLE_JSON_BLOB = JSON.stringify(
  {
    object: 'alpha',
    revision: 'git:sha256:0000000000000000000000000000000000000003',
    parents: ['git:sha256:0000000000000000000000000000000000000002'],
    notes: 'sample structured manifest payload — for the JSON viewer demo',
    metrics: { revisions: 3, branches: 2, tags: 2 },
  },
  null,
  2,
);

// ---------------------------------------------------------------------------
// Static building blocks

const NONCE = '0'.repeat(64);
const PUBKEY_B64 = 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=';
const PUBKEY2_B64 = 'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA=';
const PUBKEY3_B64 = 'CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCA=';
const SIG_B64 = 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA' + 'A'.repeat(44);
const T = '2026-01-01T00:00:00Z';

function publicKey(bytes: string): Schemas['PublicKeyJson'] {
  return { algorithm: 'ed25519', bytes };
}

function signatureFor(actor: string): Schemas['SignatureJson'] {
  return {
    actor,
    key_id: 'mock-key-1',
    algorithm: 'ed25519',
    bytes: SIG_B64,
  };
}

// ---------------------------------------------------------------------------
// Actors

const aliceGenesis: Schemas['ActorGenesisJson'] = {
  type: 'ActorGenesis',
  version: 1,
  actor_kind: 'kairo/actor',
  initial_key: publicKey(PUBKEY_B64),
  attestation_keys: [publicKey(PUBKEY_B64), publicKey(PUBKEY2_B64), publicKey(PUBKEY3_B64)],
  attestation_threshold: 2,
  created_at: T,
  nonce: NONCE,
};

const bobGenesis: Schemas['ActorGenesisJson'] = {
  type: 'ActorGenesis',
  version: 1,
  actor_kind: 'kairo/actor',
  initial_key: publicKey(PUBKEY2_B64),
  attestation_keys: [publicKey(PUBKEY2_B64)],
  attestation_threshold: 1,
  created_at: T,
  nonce: NONCE,
};

const carolGenesis: Schemas['ActorGenesisJson'] = {
  type: 'ActorGenesis',
  version: 1,
  actor_kind: 'kairo/actor',
  initial_key: publicKey(PUBKEY3_B64),
  attestation_keys: [publicKey(PUBKEY3_B64)],
  attestation_threshold: 1,
  created_at: T,
  nonce: NONCE,
};

// ---------------------------------------------------------------------------
// Objects (genesis)

const alphaGenesis: Schemas['ObjectGenesisStatementJson'] = {
  type: 'ObjectGenesis',
  version: 1,
  body: {
    object_kind: 'kairo/object',
    created_by: mockIds.alice,
    created_at: T,
    nonce: NONCE,
  },
  signature: signatureFor(mockIds.alice),
};

const betaGenesis: Schemas['ObjectGenesisStatementJson'] = {
  type: 'ObjectGenesis',
  version: 1,
  body: {
    object_kind: 'kairo/object',
    created_by: mockIds.alice,
    created_at: T,
    nonce: NONCE,
  },
  signature: signatureFor(mockIds.alice),
};

// ---------------------------------------------------------------------------
// Revisions (Alpha): rev1 → rev2 → rev3 (linear)

const alphaRev1Stmt: unknown = {
  type: 'ObjectRevision',
  version: 1,
  actor: mockIds.alice,
  subject: `object:${mockIds.alpha}`,
  created_at: T,
  body: {
    object: mockIds.alpha,
    revision: mockIds.alphaRev1,
    parents: [],
    manifest_hash: mockIds.alphaManifestBlob,
    attests_reachable_history: false,
  },
  signature: signatureFor(mockIds.alice),
};

const alphaRev2Stmt: unknown = {
  type: 'ObjectRevision',
  version: 1,
  actor: mockIds.alice,
  subject: `object:${mockIds.alpha}`,
  created_at: '2026-01-02T00:00:00Z',
  body: {
    object: mockIds.alpha,
    revision: mockIds.alphaRev2,
    parents: [mockIds.alphaRev1],
    manifest_hash: mockIds.alphaManifestBlob,
    attests_reachable_history: false,
  },
  signature: signatureFor(mockIds.alice),
};

const alphaRev3Stmt: unknown = {
  type: 'ObjectRevision',
  version: 1,
  actor: mockIds.alice,
  subject: `object:${mockIds.alpha}`,
  created_at: '2026-01-03T00:00:00Z',
  body: {
    object: mockIds.alpha,
    revision: mockIds.alphaRev3,
    parents: [mockIds.alphaRev2],
    manifest_hash: mockIds.alphaManifestBlob,
    attests_reachable_history: false,
  },
  signature: signatureFor(mockIds.alice),
};

const alphaRevisionHeads: Schemas['RevisionHeadDto'][] = [
  {
    actor: mockIds.alice,
    object: mockIds.alpha,
    revision_id: mockIds.alphaRev1,
    statement_id: mockIds.alphaRev1Stmt,
    parents: [],
    manifest_hash: mockIds.alphaManifestBlob,
    created_at: T,
  },
  {
    actor: mockIds.alice,
    object: mockIds.alpha,
    revision_id: mockIds.alphaRev2,
    statement_id: mockIds.alphaRev2Stmt,
    parents: [mockIds.alphaRev1],
    manifest_hash: mockIds.alphaManifestBlob,
    created_at: '2026-01-02T00:00:00Z',
  },
  {
    actor: mockIds.alice,
    object: mockIds.alpha,
    revision_id: mockIds.alphaRev3,
    statement_id: mockIds.alphaRev3Stmt,
    parents: [mockIds.alphaRev2],
    manifest_hash: mockIds.alphaManifestBlob,
    created_at: '2026-01-03T00:00:00Z',
  },
];

// ---------------------------------------------------------------------------
// Branches (Alpha)

const alphaHeadBranchStmt: Schemas['ObjectBranchStatementJson'] = {
  type: 'ObjectBranch',
  version: 1,
  actor: mockIds.alice,
  subject: `object:${mockIds.alpha}`,
  created_at: '2026-01-03T00:00:00Z',
  body: {
    object: mockIds.alpha,
    name: 'head',
    revision: mockIds.alphaRev3Stmt,
  },
  signature: signatureFor(mockIds.alice),
};

const alphaExperimentalBranchStmt: Schemas['ObjectBranchStatementJson'] = {
  type: 'ObjectBranch',
  version: 1,
  actor: mockIds.bob,
  subject: `object:${mockIds.alpha}`,
  created_at: '2026-01-02T00:00:00Z',
  body: {
    object: mockIds.alpha,
    name: 'experimental',
    revision: mockIds.alphaRev2Stmt,
  },
  signature: signatureFor(mockIds.bob),
};

const alphaBranchTips: Schemas['BranchTipDto'][] = [
  {
    actor: mockIds.alice,
    object: mockIds.alpha,
    name: 'head',
    statement_id: mockIds.alphaHeadBranchStmt,
    created_at: '2026-01-03T00:00:00Z',
  },
  {
    actor: mockIds.bob,
    object: mockIds.alpha,
    name: 'experimental',
    statement_id: mockIds.alphaExperimentalBranchStmt,
    created_at: '2026-01-02T00:00:00Z',
  },
];

// ---------------------------------------------------------------------------
// Tags (Alpha): v1.0.0, v1.1.0

const alphaV1Stmt: Schemas['ObjectVersionTagStatementJson'] = {
  type: 'ObjectVersionTag',
  version: 1,
  actor: mockIds.alice,
  subject: `object:${mockIds.alpha}`,
  created_at: '2026-01-02T00:00:00Z',
  body: {
    object: mockIds.alpha,
    version: 'v1.0.0',
    target: mockIds.alphaRev2Stmt,
  },
  signature: signatureFor(mockIds.alice),
};

const alphaV1_1Stmt: Schemas['ObjectVersionTagStatementJson'] = {
  type: 'ObjectVersionTag',
  version: 1,
  actor: mockIds.alice,
  subject: `object:${mockIds.alpha}`,
  created_at: '2026-01-03T00:00:00Z',
  body: {
    object: mockIds.alpha,
    version: 'v1.1.0',
    target: mockIds.alphaRev3Stmt,
  },
  signature: signatureFor(mockIds.alice),
};

const alphaVersionTagHeads: Schemas['VersionTagHeadDto'][] = [
  {
    actor: mockIds.alice,
    object: mockIds.alpha,
    version: 'v1.0.0',
    statement_id: mockIds.alphaV1Stmt,
    created_at: '2026-01-02T00:00:00Z',
  },
  {
    actor: mockIds.alice,
    object: mockIds.alpha,
    version: 'v1.1.0',
    statement_id: mockIds.alphaV1_1Stmt,
    created_at: '2026-01-03T00:00:00Z',
  },
];

// ---------------------------------------------------------------------------
// Capability heads (Alice → Bob, scope=Alpha, ObjectVersionTag)

const aliceGrantsBobStmt: unknown = {
  type: 'ActorCapabilityGrant',
  version: 1,
  actor: mockIds.alice,
  subject: `actor:${mockIds.bob}`,
  created_at: T,
  body: {
    grantee: mockIds.bob,
    capability: {
      scope: { object: mockIds.alpha },
      kinds: ['ObjectVersionTag'],
      delegable: false,
      constraints: [],
    },
  },
  signature: signatureFor(mockIds.alice),
};

const aliceCapabilityHead: Schemas['CapabilityHeadDto'] = {
  grantor: mockIds.alice,
  grantee: mockIds.bob,
  scope: { object: mockIds.alpha },
  statement_id: mockIds.aliceGrantsBobStmt,
  created_at: T,
};

// ---------------------------------------------------------------------------
// Trust statements

const bobTrustsAliceStmt: Schemas['ActorTrustStatementJson'] = {
  type: 'ActorTrust',
  version: 1,
  actor: mockIds.bob,
  subject: `actor:${mockIds.alice}`,
  created_at: T,
  body: {
    trusted_actor: mockIds.alice,
    decision: 'trusted',
  },
  signature: signatureFor(mockIds.bob),
};

const carolTrustsAliceStmt: Schemas['ActorTrustStatementJson'] = {
  type: 'ActorTrust',
  version: 1,
  actor: mockIds.carol,
  subject: `actor:${mockIds.alice}`,
  created_at: T,
  body: {
    trusted_actor: mockIds.alice,
    decision: 'untrusted',
  },
  signature: signatureFor(mockIds.carol),
};

// Self-trust on the canonical mock actor — preserves the
// behavior the slice 6 contract test asserts
// (`getTrust(mockIds.actor, mockIds.actor)` resolves trusted).
const aliceTrustsSelfStmt: Schemas['ActorTrustStatementJson'] = {
  type: 'ActorTrust',
  version: 1,
  actor: mockIds.alice,
  subject: `actor:${mockIds.alice}`,
  created_at: T,
  body: {
    trusted_actor: mockIds.alice,
    decision: 'trusted',
  },
  signature: signatureFor(mockIds.alice),
};

const trustHeadsAboutAlice: Schemas['TrustHeadDto'][] = [
  {
    by_actor: mockIds.bob,
    trusted_actor: mockIds.alice,
    statement_id: mockIds.bobTrustsAliceStmt,
    created_at: T,
    decision: 'trusted',
  },
  {
    by_actor: mockIds.carol,
    trusted_actor: mockIds.alice,
    statement_id: mockIds.carolTrustsAliceStmt,
    created_at: T,
    decision: 'untrusted',
  },
];

// ---------------------------------------------------------------------------
// Validation results

const alphaValidation: Schemas['ValidationResult'] = {
  object_id: mockIds.alpha,
  status: 'indeterminate',
  issues: [
    {
      kind: 'manifest_not_provided',
      severity: 'info',
      message: 'no manifest available; the daemon does not resolve manifests from a working tree',
      statement_id: mockIds.alphaRev3Stmt,
      details: {},
    },
    {
      kind: 'content_layer_indeterminate',
      severity: 'info',
      message: 'no Git repository was supplied; the content layer cannot be verified server-side',
      statement_id: mockIds.alphaRev3Stmt,
      details: {},
    },
  ],
  revision_statement_id: mockIds.alphaRev3Stmt,
  branch_name: 'head',
};

const betaValidation: Schemas['ValidationResult'] = {
  object_id: mockIds.beta,
  status: 'valid',
  issues: [],
};

// ---------------------------------------------------------------------------
// Registry

export interface MockRegistry {
  /** Daemon metadata. */
  version: Schemas['VersionInfo'];
  status: Schemas['StatusInfo'];

  /** `GET /api/v1/actors/:id`. */
  actors: Record<string, Schemas['ActorGenesisJson']>;
  /** `GET /api/v1/objects/:id`. */
  objects: Record<string, Schemas['ObjectGenesisStatementJson']>;
  /** `GET /api/v1/statements/:id` — polymorphic. */
  statements: Record<string, unknown>;

  /** `GET /api/v1/branches/:object`. */
  branchTips: Record<string, Schemas['BranchTipDto'][]>;
  /** `GET /api/v1/branches/:object/:name/latest?actor=`. Key is
   * `${object}:${name}:${actor}`; `?actor=` defaults to the
   * object's `created_by`. */
  branchLatest: Record<string, Schemas['ObjectBranchStatementJson']>;

  /** `GET /api/v1/version-tags/:object`. */
  versionTagHeads: Record<string, Schemas['VersionTagHeadDto'][]>;
  /** `GET /api/v1/version-tags/:object/:version?actor=`. Key is
   * `${object}:${version}:${actor}`. */
  versionTagLatest: Record<string, Schemas['ObjectVersionTagStatementJson']>;

  /** `GET /api/v1/revisions/:object`. */
  revisionHeads: Record<string, Schemas['RevisionHeadDto'][]>;

  /** `GET /api/v1/trust/:by/:of`. Key is `${by}:${of}`. */
  trustPair: Record<string, Schemas['ActorTrustStatementJson']>;
  /** `GET /api/v1/trust/about/:of`. */
  trustAbout: Record<string, Schemas['TrustHeadDto'][]>;

  /** `GET /api/v1/capabilities/:grantor`. */
  capabilitiesFromGrantor: Record<string, Schemas['CapabilityHeadDto'][]>;
  /** `GET /api/v1/capabilities/for-object/:object`. */
  capabilitiesForObject: Record<string, Schemas['CapabilityHeadDto'][]>;

  /** `GET /api/v1/blobs/:id`. */
  blobs: Record<string, Uint8Array>;

  /** `GET /api/v1/verify-object/:id`. */
  verifyObject: Record<string, Schemas['ValidationResult']>;
}

const versionFixture: Schemas['VersionInfo'] = {
  daemon_version: 'mock-0.1.0',
  api_version: 'v1',
  core_version: 'mock-0.1.0',
  store_version: 'mock-0.1.0',
};

const statusFixture: Schemas['StatusInfo'] = {
  daemon_running: true,
  store_path: '/mock/store',
  store_schema_version: '1',
  pid: 1,
  daemon_version: 'mock-0.1.0',
};

export const mockRegistry: MockRegistry = {
  version: versionFixture,
  status: statusFixture,

  actors: {
    [mockIds.alice]: aliceGenesis,
    [mockIds.bob]: bobGenesis,
    [mockIds.carol]: carolGenesis,
  },

  objects: {
    [mockIds.alpha]: alphaGenesis,
    [mockIds.beta]: betaGenesis,
  },

  statements: {
    [mockIds.alphaRev1Stmt]: alphaRev1Stmt,
    [mockIds.alphaRev2Stmt]: alphaRev2Stmt,
    [mockIds.alphaRev3Stmt]: alphaRev3Stmt,
    [mockIds.alphaHeadBranchStmt]: alphaHeadBranchStmt,
    [mockIds.alphaExperimentalBranchStmt]: alphaExperimentalBranchStmt,
    [mockIds.alphaV1Stmt]: alphaV1Stmt,
    [mockIds.alphaV1_1Stmt]: alphaV1_1Stmt,
    [mockIds.bobTrustsAliceStmt]: bobTrustsAliceStmt,
    [mockIds.carolTrustsAliceStmt]: carolTrustsAliceStmt,
    [mockIds.aliceGrantsBobStmt]: aliceGrantsBobStmt,
  },

  branchTips: {
    [mockIds.alpha]: alphaBranchTips,
    [mockIds.beta]: [],
  },

  branchLatest: {
    [`${mockIds.alpha}:head:${mockIds.alice}`]: alphaHeadBranchStmt,
    [`${mockIds.alpha}:experimental:${mockIds.bob}`]: alphaExperimentalBranchStmt,
  },

  versionTagHeads: {
    [mockIds.alpha]: alphaVersionTagHeads,
    [mockIds.beta]: [],
  },

  versionTagLatest: {
    [`${mockIds.alpha}:v1.0.0:${mockIds.alice}`]: alphaV1Stmt,
    [`${mockIds.alpha}:v1.1.0:${mockIds.alice}`]: alphaV1_1Stmt,
  },

  revisionHeads: {
    [mockIds.alpha]: alphaRevisionHeads,
    [mockIds.beta]: [],
  },

  trustPair: {
    [`${mockIds.alice}:${mockIds.alice}`]: aliceTrustsSelfStmt,
    [`${mockIds.bob}:${mockIds.alice}`]: bobTrustsAliceStmt,
    [`${mockIds.carol}:${mockIds.alice}`]: carolTrustsAliceStmt,
  },

  trustAbout: {
    [mockIds.alice]: trustHeadsAboutAlice,
    [mockIds.bob]: [],
    [mockIds.carol]: [],
  },

  capabilitiesFromGrantor: {
    [mockIds.alice]: [aliceCapabilityHead],
    [mockIds.bob]: [],
    [mockIds.carol]: [],
  },

  capabilitiesForObject: {
    [mockIds.alpha]: [aliceCapabilityHead],
    [mockIds.beta]: [],
  },

  blobs: {
    [mockIds.alphaManifestBlob]: new Uint8Array([0xde, 0xad, 0xbe, 0xef]),
    [mockIds.textBlob]: new TextEncoder().encode(SAMPLE_TEXT_BLOB),
    [mockIds.jsonBlob]: new TextEncoder().encode(SAMPLE_JSON_BLOB),
  },

  verifyObject: {
    [mockIds.alpha]: alphaValidation,
    [mockIds.beta]: betaValidation,
  },
};
