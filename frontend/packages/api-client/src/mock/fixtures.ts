// Canned response bodies the MSW handlers serve. Field shapes
// match the OpenAPI schema exactly — typed against
// `components['schemas']['*']` so a daemon-side schema change
// surfaces as a TS error here, not a runtime mismatch in the
// inspector.
//
// Every fixture is a complete response body (the inner
// `result`); the handlers wrap it in the success envelope so
// the wire format matches the real daemon byte-for-byte.

import type { components } from '../generated/schema';

const MOCK_ACTOR = 'kairo:actor:zMockActor00000000000000000000000000000000';
const MOCK_OBJECT = 'kairo:object:zMockObject00000000000000000000000000000000';
const MOCK_REVISION_STMT = 'kairo:stmt:zMockRevisionStmt00000000000000000000000000';
const MOCK_BRANCH_STMT = 'kairo:stmt:zMockBranchStmt000000000000000000000000000000';
const MOCK_TAG_STMT = 'kairo:stmt:zMockTagStmt0000000000000000000000000000000000';
const MOCK_TRUST_STMT = 'kairo:stmt:zMockTrustStmt000000000000000000000000000000';
const MOCK_GRANT_STMT = 'kairo:stmt:zMockGrantStmt000000000000000000000000000000';
const MOCK_BLOB = 'kairo:blob:zMockBlob000000000000000000000000000000000000';
const MOCK_REVISION = 'git:sha256:0000000000000000000000000000000000000001';
const MOCK_NONCE = '0'.repeat(64);
const MOCK_PUBKEY_B64 = 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=';
const MOCK_SIG_B64 = 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA' + 'A'.repeat(44);
const MOCK_TIMESTAMP = '2026-01-01T00:00:00Z';

const mockSignature: components['schemas']['SignatureJson'] = {
  actor: MOCK_ACTOR,
  key_id: 'mock-key-1',
  algorithm: 'ed25519',
  bytes: MOCK_SIG_B64,
};

const mockPublicKey: components['schemas']['PublicKeyJson'] = {
  algorithm: 'ed25519',
  bytes: MOCK_PUBKEY_B64,
};

export const versionFixture: components['schemas']['VersionInfo'] = {
  daemon_version: 'mock-0.1.0',
  api_version: 'v1',
  core_version: 'mock-0.1.0',
  store_version: 'mock-0.1.0',
};

export const statusFixture: components['schemas']['StatusInfo'] = {
  daemon_running: true,
  store_path: '/mock/store',
  store_schema_version: '1',
  pid: 1,
  daemon_version: 'mock-0.1.0',
};

export const actorFixture: components['schemas']['ActorGenesisJson'] = {
  type: 'ActorGenesis',
  version: 1,
  actor_kind: 'kairo/actor',
  initial_key: mockPublicKey,
  attestation_keys: [mockPublicKey],
  attestation_threshold: 1,
  created_at: MOCK_TIMESTAMP,
  nonce: MOCK_NONCE,
};

export const objectFixture: components['schemas']['ObjectGenesisStatementJson'] = {
  type: 'ObjectGenesis',
  version: 1,
  body: {
    object_kind: 'kairo/object',
    created_by: MOCK_ACTOR,
    created_at: MOCK_TIMESTAMP,
    nonce: MOCK_NONCE,
  },
  signature: mockSignature,
};

export const branchTipsFixture: components['schemas']['BranchTipDto'][] = [
  {
    actor: MOCK_ACTOR,
    object: MOCK_OBJECT,
    name: 'head',
    statement_id: MOCK_BRANCH_STMT,
    created_at: MOCK_TIMESTAMP,
  },
];

export const branchLatestFixture: components['schemas']['ObjectBranchStatementJson'] = {
  type: 'ObjectBranch',
  version: 1,
  actor: MOCK_ACTOR,
  subject: `object:${MOCK_OBJECT}`,
  created_at: MOCK_TIMESTAMP,
  body: {
    object: MOCK_OBJECT,
    name: 'head',
    revision: MOCK_REVISION_STMT,
  },
  signature: mockSignature,
};

export const versionTagLatestFixture: components['schemas']['ObjectVersionTagStatementJson'] = {
  type: 'ObjectVersionTag',
  version: 1,
  actor: MOCK_ACTOR,
  subject: `object:${MOCK_OBJECT}`,
  created_at: MOCK_TIMESTAMP,
  body: {
    object: MOCK_OBJECT,
    version: 'v1.0.0',
    target: MOCK_REVISION,
  },
  signature: mockSignature,
};

export const trustFixture: components['schemas']['ActorTrustStatementJson'] = {
  type: 'ActorTrust',
  version: 1,
  actor: MOCK_ACTOR,
  subject: `actor:${MOCK_ACTOR}`,
  created_at: MOCK_TIMESTAMP,
  body: {
    trusted_actor: MOCK_ACTOR,
    decision: 'trusted',
  },
  signature: mockSignature,
};

export const capabilityHeadsFixture: components['schemas']['CapabilityHeadDto'][] = [
  {
    grantor: MOCK_ACTOR,
    grantee: MOCK_ACTOR,
    scope: { object: MOCK_OBJECT },
    statement_id: MOCK_GRANT_STMT,
    created_at: MOCK_TIMESTAMP,
  },
];

export const verifyObjectFixture: components['schemas']['ValidationResult'] = {
  object_id: MOCK_OBJECT,
  status: 'indeterminate',
  issues: [
    {
      kind: 'manifest_not_provided',
      severity: 'info',
      message: 'no manifest available; the daemon does not resolve manifests from a working tree',
      statement_id: MOCK_REVISION_STMT,
      details: {},
    },
    {
      kind: 'content_layer_indeterminate',
      severity: 'info',
      message: 'no Git repository was supplied; the content layer cannot be verified server-side',
      statement_id: MOCK_REVISION_STMT,
      details: {},
    },
  ],
  revision_statement_id: MOCK_REVISION_STMT,
  branch_name: 'head',
};

/**
 * Polymorphic statement-by-id fixture. Defaults to the same
 * revision the branch tip points at, but tests can swap in a
 * different statement kind.
 */
export const statementFixture: unknown = {
  type: 'ObjectRevision',
  version: 1,
  actor: MOCK_ACTOR,
  subject: `object:${MOCK_OBJECT}`,
  created_at: MOCK_TIMESTAMP,
  body: {
    object: MOCK_OBJECT,
    revision: MOCK_REVISION,
    parents: [],
    manifest_hash: MOCK_BLOB,
    attests_reachable_history: false,
  },
  signature: mockSignature,
};

/**
 * Default fixture set. Tests can override individual fields by
 * passing a partial spread to `createHandlers({...defaults,
 * version: customVersion})`.
 */
export const fixtures = {
  version: versionFixture,
  status: statusFixture,
  actor: actorFixture,
  object: objectFixture,
  statement: statementFixture,
  branchTips: branchTipsFixture,
  branchLatest: branchLatestFixture,
  versionTagLatest: versionTagLatestFixture,
  trust: trustFixture,
  capabilityHeads: capabilityHeadsFixture,
  blob: new Uint8Array([0xde, 0xad, 0xbe, 0xef]),
  verifyObject: verifyObjectFixture,
} as const;

/**
 * Stable identifier strings the mock handlers and tests reach
 * for. Re-exported so tests can build URLs without typing the
 * 50-char base58 forms.
 */
export const mockIds = {
  actor: MOCK_ACTOR,
  object: MOCK_OBJECT,
  blob: MOCK_BLOB,
  branchStmt: MOCK_BRANCH_STMT,
  revisionStmt: MOCK_REVISION_STMT,
  tagStmt: MOCK_TAG_STMT,
  trustStmt: MOCK_TRUST_STMT,
  grantStmt: MOCK_GRANT_STMT,
  revision: MOCK_REVISION,
} as const;
