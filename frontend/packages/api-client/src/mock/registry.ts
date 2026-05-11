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
//
// Bare-payload form (no `kairo:<kind>:` prefix) — matches the
// daemon's wire contract. The daemon takes bare ids on URL
// paths (e.g. `/api/v1/objects/zXyz`) and returns bare ids
// inside JSON response bodies (e.g. `created_by: "zXyz"`).
// `ObjectId::Display` writes `as_str()` with no prefix; the
// `kairo:<kind>:` form is presentational only and the
// inspector composes it at render time.

export const mockIds = {
  // Actors
  alice: 'zMockActor00000000000000000000000000000000',
  bob: 'zMockActorBob000000000000000000000000000000',
  carol: 'zMockActorCarol00000000000000000000000000',

  // Objects
  // - alpha: rich (multi-rev/branch/tag/cap/trust); verify → indeterminate
  // - beta:  genesis only;                          verify → valid
  // - gamma: genesis only;                          verify → invalid
  // - delta: genesis only;                          verify → conflicted
  // The latter two exist so the slice 10 e2e suite can
  // exercise every distinct ValidationBadge tone.
  alpha: 'zMockObject00000000000000000000000000000000',
  beta: 'zMockObjectBeta000000000000000000000000000',
  gamma: 'zMockObjectGamma00000000000000000000000000',
  delta: 'zMockObjectDelta00000000000000000000000000',

  // Revision statements (Alpha)
  alphaRev1Stmt: 'zMockRevisionStmt00000000000000000000000000',
  alphaRev2Stmt: 'zMockRevisionStmtAlpha2000000000000000000',
  alphaRev3Stmt: 'zMockRevisionStmtAlpha3000000000000000000',

  // Branch statements (Alpha)
  alphaHeadBranchStmt: 'zMockBranchStmt000000000000000000000000000000',
  alphaExperimentalBranchStmt: 'zMockBranchStmtAlphaExp00000000000000000000',

  // Tag statements (Alpha)
  alphaV1Stmt: 'zMockTagStmt0000000000000000000000000000000000',
  alphaV1_1Stmt: 'zMockTagStmtAlphaV1_1000000000000000000000',

  // Trust statements (about Alice)
  bobTrustsAliceStmt: 'zMockTrustStmt000000000000000000000000000000',
  carolTrustsAliceStmt: 'zMockTrustStmtCarolAlice00000000000000000',

  // Capability grant (Alice → Bob, scope=Alpha)
  aliceGrantsBobStmt: 'zMockGrantStmt000000000000000000000000000000',

  // Blobs
  // - alphaManifestBlob: binary fixture; what revisions point at.
  // - textBlob:          plain text (TOML-ish manifest sample).
  // - jsonBlob:          JSON sample so the JSON viewer is exercisable.
  alphaManifestBlob: 'zMockBlob000000000000000000000000000000000000',
  textBlob: 'zMockBlobText000000000000000000000000000000',
  jsonBlob: 'zMockBlobJson000000000000000000000000000000',

  // Revision IDs (git:sha256:* — kept fully-qualified; the
  // `git:` scheme is the wire form for storage refs and is
  // not a kairo-namespace id).
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

const gammaGenesis: Schemas['ObjectGenesisStatementJson'] = {
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

const deltaGenesis: Schemas['ObjectGenesisStatementJson'] = {
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
// Per-actor signed-statement audit lists
//
// Mirrors the daemon's `statements_by_actor` index. ObjectGenesis is
// intentionally absent (it carries `created_by`, not the envelope
// `actor` field every other statement type uses), matching the
// server-side rule.

const aliceStatementsByActor: Schemas['StatementByActorDto'][] = [
  {
    actor: mockIds.alice,
    statement_id: mockIds.alphaRev1Stmt,
    kind: 'ObjectRevision',
    created_at: T,
  },
  {
    actor: mockIds.alice,
    statement_id: mockIds.alphaV1Stmt,
    kind: 'ObjectVersionTag',
    created_at: '2026-01-02T00:00:00Z',
  },
  {
    actor: mockIds.alice,
    statement_id: mockIds.alphaRev2Stmt,
    kind: 'ObjectRevision',
    created_at: '2026-01-02T00:00:00Z',
  },
  {
    actor: mockIds.alice,
    statement_id: mockIds.alphaRev3Stmt,
    kind: 'ObjectRevision',
    created_at: '2026-01-03T00:00:00Z',
  },
  {
    actor: mockIds.alice,
    statement_id: mockIds.alphaV1_1Stmt,
    kind: 'ObjectVersionTag',
    created_at: '2026-01-03T00:00:00Z',
  },
  {
    actor: mockIds.alice,
    statement_id: mockIds.alphaHeadBranchStmt,
    kind: 'ObjectBranch',
    created_at: '2026-01-03T00:00:00Z',
  },
  {
    actor: mockIds.alice,
    statement_id: mockIds.aliceGrantsBobStmt,
    kind: 'ActorCapabilityGrant',
    created_at: T,
  },
];

const bobStatementsByActor: Schemas['StatementByActorDto'][] = [
  {
    actor: mockIds.bob,
    statement_id: mockIds.alphaExperimentalBranchStmt,
    kind: 'ObjectBranch',
    created_at: '2026-01-02T00:00:00Z',
  },
  {
    actor: mockIds.bob,
    statement_id: mockIds.bobTrustsAliceStmt,
    kind: 'ActorTrust',
    created_at: T,
  },
];

const carolStatementsByActor: Schemas['StatementByActorDto'][] = [
  {
    actor: mockIds.carol,
    statement_id: mockIds.carolTrustsAliceStmt,
    kind: 'ActorTrust',
    created_at: T,
  },
];

// ---------------------------------------------------------------------------
// Per-actor owned-objects audit lists
//
// Mirrors the daemon's `objects_by_actor` index. One entry per
// `ObjectGenesis` whose `created_by` is the actor; the inspector
// renders this as the "Created objects" table on the actor page.

const aliceObjectsByActor: Schemas['ObjectByActorDto'][] = [
  {
    actor: mockIds.alice,
    object_id: mockIds.alpha,
    object_kind: 'kairo/object',
    created_at: T,
  },
  {
    actor: mockIds.alice,
    object_id: mockIds.beta,
    object_kind: 'kairo/object',
    created_at: T,
  },
  {
    actor: mockIds.alice,
    object_id: mockIds.gamma,
    object_kind: 'kairo/object',
    created_at: T,
  },
  {
    actor: mockIds.alice,
    object_id: mockIds.delta,
    object_kind: 'kairo/object',
    created_at: T,
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

const gammaValidation: Schemas['ValidationResult'] = {
  object_id: mockIds.gamma,
  status: 'invalid',
  issues: [
    {
      kind: 'signature_invalid',
      severity: 'error',
      message: 'mock fixture: forced invalid for the validation badge e2e test',
      details: {},
    },
  ],
};

const deltaValidation: Schemas['ValidationResult'] = {
  object_id: mockIds.delta,
  status: 'conflicted',
  issues: [
    {
      kind: 'cross_actor_conflict',
      severity: 'warning',
      message: 'mock fixture: forced conflicted for the validation badge e2e test',
      details: {},
    },
  ],
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

  /** `GET /api/v1/actors/:id/statements`. */
  statementsByActor: Record<string, Schemas['StatementByActorDto'][]>;

  /** `GET /api/v1/actors/:id/objects`. */
  objectsByActor: Record<string, Schemas['ObjectByActorDto'][]>;

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
    [mockIds.gamma]: gammaGenesis,
    [mockIds.delta]: deltaGenesis,
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
    [mockIds.gamma]: [],
    [mockIds.delta]: [],
  },

  branchLatest: {
    [`${mockIds.alpha}:head:${mockIds.alice}`]: alphaHeadBranchStmt,
    [`${mockIds.alpha}:experimental:${mockIds.bob}`]: alphaExperimentalBranchStmt,
  },

  versionTagHeads: {
    [mockIds.alpha]: alphaVersionTagHeads,
    [mockIds.beta]: [],
    [mockIds.gamma]: [],
    [mockIds.delta]: [],
  },

  versionTagLatest: {
    [`${mockIds.alpha}:v1.0.0:${mockIds.alice}`]: alphaV1Stmt,
    [`${mockIds.alpha}:v1.1.0:${mockIds.alice}`]: alphaV1_1Stmt,
  },

  revisionHeads: {
    [mockIds.alpha]: alphaRevisionHeads,
    [mockIds.beta]: [],
    [mockIds.gamma]: [],
    [mockIds.delta]: [],
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

  statementsByActor: {
    [mockIds.alice]: aliceStatementsByActor,
    [mockIds.bob]: bobStatementsByActor,
    [mockIds.carol]: carolStatementsByActor,
  },

  objectsByActor: {
    [mockIds.alice]: aliceObjectsByActor,
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
    [mockIds.gamma]: [],
    [mockIds.delta]: [],
  },

  blobs: {
    [mockIds.alphaManifestBlob]: new Uint8Array([0xde, 0xad, 0xbe, 0xef]),
    [mockIds.textBlob]: new TextEncoder().encode(SAMPLE_TEXT_BLOB),
    [mockIds.jsonBlob]: new TextEncoder().encode(SAMPLE_JSON_BLOB),
  },

  verifyObject: {
    [mockIds.alpha]: alphaValidation,
    [mockIds.beta]: betaValidation,
    [mockIds.gamma]: gammaValidation,
    [mockIds.delta]: deltaValidation,
  },
};
