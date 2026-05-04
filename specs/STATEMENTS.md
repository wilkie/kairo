# STATEMENTS.md

## 1. Overview

Statements are signed, content-addressed claims about objects, artifacts, and their relationships.

Statements:

- are immutable
- are independently verifiable
- are not part of object versioned content
- form the basis of trust and federation

A statement represents:

"Actor X claims Y about Z."

---

## 2. Statement Identity

Each statement has:

- statement hash (content-addressed identity)
- signature (binding actor to the statement)

### 2.1 Statement Hash

The statement hash is computed from a canonical representation of the statement
content. The canonical form is the domain-tagged length-prefixed binary
encoding documented under `schemas/canonical/`. JSON is interchange only and is
never the hash input. See `SCHEMA.md` section 6.

Rules:

- canonical serialization (deterministic)
- excludes the signature itself
- identical content → identical hash

For ordinary signed statements, the `StatementId` is derived from the unsigned
canonical statement. The signature proves authorship of those canonical bytes
but does not change statement identity.

`ObjectGenesis` is special because its unsigned body derives the `ObjectId`.
The signed ObjectGenesis statement proves the origin claim, but the signature
envelope is excluded from Object ID material.

Conceptually, new statement types should follow this generic shape:

```text
StatementBody -> canonical body fields
UnsignedStatement {
  type,
  version,
  actor,
  subject,
  created_at,
  body
} -> StatementId
SignedStatement {
  unsigned_statement,
  signature
}
```

Every signed statement carries `created_at` on the envelope. Genesis bodies
that derive identity (`ActorGenesisBody`, `ObjectGenesisBody`) carry their own
`created_at` in the body itself, since their canonical bytes are hashed
standalone to derive `ActorId` / `ObjectId`. `created_at` is RFC 3339 / UTC /
`Z` / second-granularity in JSON and `i64` Unix epoch seconds, big-endian, in
canonical bytes. The timestamp is the actor's self-claim of when the statement
was made; it is not a trusted observation.

Adding a statement type should require defining only its body fields and
canonical body encoding. The unsigned/signed statement envelope process is
shared.

---

## 3. Signature Model

A statement includes:

{
  "id": "z6MkStatement...",
  "signature": {
    "actor": "z6MkActor...",
    "sig": "..."
  }
}

Signature semantics:

- signature = Sign(statement_hash)
- proves authorship
- does not imply correctness
- JSON signature bytes use standard base64 encoding.

---

## 4. Statement Types

### 4.1 ObjectGenesis Statement

Creates stable Object lineage identity. `ObjectGenesis` is signed, but its
signature is excluded from the Object ID hash so the same genesis body can be
re-signed without changing the Object ID.

The canonical ObjectGenesis v1 form is documented in:

```text
schemas/canonical/object-genesis-v1.md
```

### 4.2a ObjectBranch Statement

An `ObjectBranch` statement is a named, actor-scoped, mutable pointer at a
specific `ObjectRevision` statement. Like `ObjectVersionTag`, it carries
an explicit `supersedes` pointer at the prior `ObjectBranch` it
replaces, so the advance history is reconstructable without inferring
from `created_at` order. The genesis advance for an
`(actor, object, name)` triple has `supersedes = null`; successors set
`supersedes` to the prior chain leaf's `StatementId`. The CLI
(`kairo branch set`) auto-computes this — callers don't need to track
it manually.

Resolution honors **chain precedence**: the head for
`(actor, object, name)` is the leaf of the supersedes chain — a
statement no other statement supersedes. A successor that explicitly
names its predecessor is unambiguously later regardless of
`created_at`. `(created_at, statement_id)` is only a fork tiebreak,
applied when the chain has multiple leaves. Older `ObjectBranch`
statements stay valid evidence of past claims; only the chain leaf is
load-bearing.

`supersedes` may reference a branch from a **different actor** for the
same `(object, name)`. The resolver in
`kairo-store::FilesystemStore::latest_branch` honors that edge when
the successor's signer holds an `ObjectBranch` capability on the
object at the successor's `created_at` (per `CAPABILITIES.md` §6.2).
Without a covering grant, the cross-actor edge is recorded but not
honored — each actor keeps their own per-actor head. This is the
direct parallel of the cross-actor flip already implemented for
`ObjectVersionTag`.

`name = "head"` is the conventional default the CLI assumes when no name is
given. It is not reserved at the protocol level.

The canonical ObjectBranch v1 form is documented in:

```text
schemas/canonical/object-branch-v1.md
```

### 4.2b ObjectVersionTag Statement

An `ObjectVersionTag` statement binds a strict semver 2.0.0 string to a
specific `ObjectRevision` statement (a *bind*) or withdraws a previously
published binding (a *revoke*). Like `ObjectBranch` it is actor-scoped
and mutable. Differences from `ObjectBranch`:

1. The version name must parse as semver 2.0.0; the future dependency
   resolver consumes these strings.
2. Every non-genesis tag carries an explicit `supersedes` pointer at
   the prior `ObjectVersionTag` it replaces, so the rebind / revoke
   history is reconstructable without inferring from `created_at`
   order. The genesis tag has `supersedes = null` and must be a bind;
   a revoke with no chain reference is a shape violation.
3. Resolution honors **chain precedence**: the head for
   `(actor, object, version)` is the leaf of the supersedes chain — a
   statement no other statement supersedes. A successor that explicitly
   names its predecessor is unambiguously later regardless of
   `created_at`. `(created_at, statement_id)` is only a fork tiebreak,
   applied when the chain has multiple leaves.

`supersedes` may reference a tag from a **different actor** for the
same `(object, version)`. The resolver in
`kairo-store::FilesystemStore::latest_version_tag` honors that edge
when the successor's signer holds an `ObjectVersionTag` capability on
the object at the successor's `created_at` (per `CAPABILITIES.md`
§6.2). Without a covering grant, the cross-actor edge is recorded but
not honored — each actor keeps their own per-actor head.

Because tags are mutable, **consumers that need build reproducibility
must record the resolved `StatementId` (or `SnapshotId`)** in their
lockfile equivalent. The version string alone is not a stable handle —
the actor may rebind or revoke it, and different resolvers may end up
at different revisions for the same `(actor, object, version)`
depending on what statements they have seen.

The canonical ObjectVersionTag v1 form is documented in:

```text
schemas/canonical/object-version-tag-v1.md
```

### 4.2c ActorTrust Statement

An `ActorTrust` statement records a **first-person** opinion that one
actor (`by_actor`) holds about another (`trusted_actor`) — `trusted`,
`untrusted`, or a withdrawal of any prior opinion. Like
`ObjectVersionTag` it is actor-scoped and mutable, with chain
precedence over timestamp.

Differences from `ObjectVersionTag`:

1. The lookup key is `(by_actor, trusted_actor)` — there is no object.
   Trust is about who is trustworthy, not about what they signed.
2. `decision` is one of `"trusted"`, `"untrusted"`, or `null`. A `null`
   decision is a withdrawal: it retracts any prior opinion. The shape
   `decision = null && supersedes = null` is invalid (you can't
   withdraw nothing).
3. Cross-actor `supersedes` is **invalid** for trust (tighter than
   `ObjectVersionTag`, which honors cross-actor edges when a covering
   capability exists). Trust is first-person: only the truster who
   signed `S` may publish a successor that supersedes `S` —
   capabilities cannot delegate the right to publish trust on
   another's behalf (Decision B in `CAPABILITIES.md` §9).
4. An optional `reason` string, included in canonical bytes, lets the
   truster annotate why.

`evaluate_trust(by_actor, of_actor)` (in `kairo-statement::verify`)
folds the chain leaf into a `TrustEvaluation`:

- `Some("trusted")` → `Trusted`
- `Some("untrusted")` → `Untrusted`
- `None` (withdrawal) → `Unknown` (equivalent to never having an
  opinion; the audit history is still on disk)
- No statement → `Unknown`
- Caller did not supply `by_actor` → `Unevaluated`

Trust is **informational**: it never makes a cryptographically valid
statement invalid, and it never validates an invalid one. Callers
compose the two independently, per §6.1.

The canonical ActorTrust v1 form is documented in:

```text
schemas/canonical/actor-trust-v1.md
```

### 4.2d ActorCapabilityGrant Statement

An `ActorCapabilityGrant` statement records a **delegation**: the
grantor (the signer) authorizes a `grantee` actor to issue a specified
set of statement kinds against a scoped target, optionally bounded by
constraints. This is the distributed-systems sense of "capability" — a
transferable, unforgeable token of authority that makes cross-actor
authority claims load-bearing in the statement graph.

Differences from `ActorTrust`:

1. The lookup key is `(grantor, grantee, scope)` — three coordinates.
   `scope` is either `Object(O)` or `Actor(A)`; kind narrowing lives
   entirely in the body's `statement_kinds` field, not in scope (per
   `specs/CAPABILITIES.md` Decision E).
2. Each `(grantor, grantee, scope)` triple has at most one active
   chain. Successor grants must declare `supersedes`. A second
   genesis-shape grant on an existing triple is a shape violation.
3. Cross-grantor `supersedes` is **invalid**, matching trust. Only the
   original grantor may supersede their own grant.
4. The body carries a nested `Capability { scope, statement_kinds,
   delegable, constraints }`. `statement_kinds` is sorted and
   deduplicated in canonical bytes; `constraints` is sorted by tag
   byte with at most one of each variant.
5. Constraints today: `ExpiresAt(timestamp)`,
   `MaxDelegationDepth(u8)`, `KeyPinned(KeyId)`. `KeyPinned` is the
   opt-in escape hatch for high-stakes grants where auto-revocation
   on grantor key compromise matters more than survivability across
   routine key rotations.

Resolution: `evaluate_capability(grantee, target, at)` (defined in
`specs/CAPABILITIES.md` §6.1, implemented in `kairo-statement::verify`)
folds the chain leaf, the revocation status, the recursive
grantor-authority check, and the constraint satisfaction check into a
`CapabilityEvaluation` value. The load-bearing payoff is the
`ObjectVersionTag` resolver flip: cross-actor `supersedes` is honored
when capability evaluation succeeds. That flip is implemented in
`kairo-store::FilesystemStore::latest_version_tag` /
`list_version_tags`.

Per-`(scope, kind)` shape validity is conservative in v1: `Object`
scope accepts `ObjectRevision`, `ObjectBranch`, `ObjectVersionTag`;
`Actor` scope accepts no kinds today (reserved for future actor-
surface statement types). The validator rejects invalid combinations
at body construction time.

The canonical ActorCapabilityGrant v1 form is documented in:

```text
schemas/canonical/actor-capability-grant-v1.md
```

See `specs/CAPABILITIES.md` for the complete model, decisions, and
deferred work.

### 4.2e ActorCapabilityRevocation Statement

An `ActorCapabilityRevocation` statement retracts a previously issued
`ActorCapabilityGrant`. Differences from the grant:

1. The signer must equal the original grantor — cross-grantor
   revocation is invalid in v1 (multi-actor revocation paths are
   deferred).
2. Revocations do **not** chain. There is no `supersedes` field.
   Duplicate revocations naming the same grant are tolerated for
   federation replay; the most-restrictive interpretation wins
   (any `retroactive = true` revocation makes the grant retroactively
   invalid).
3. Default revocation invalidates the grant for statements created
   strictly *after* the revocation's `created_at`. `retroactive =
   true` invalidates the grant from inception, propagating to every
   statement issued under it (including cross-actor `supersedes`
   edges that depended on the grant).

Per `specs/CAPABILITIES.md` §7.1, grantor key compromise requires the
operator to enumerate grants signed by the compromised key and issue
retroactive revocations as part of the cleanup runbook — grants
default to `ActorId` binding and are not auto-killed by key
revocation. The `KeyPinned` constraint is the opt-in escape hatch.

The canonical ActorCapabilityRevocation v1 form is documented in:

```text
schemas/canonical/actor-capability-revocation-v1.md
```

### 4.2f ActorKeyRotation Statement

An `ActorKeyRotation` statement is a first-person declaration by an
actor that, from this point forward, the actor's active signing key is
the body's `next_key`. It is the routine-hygiene mechanism that lets
actors swap keys without losing their stable `ActorId` (`ACTORS.md`
§5.5).

Differences from earlier statement types:

1. The lookup key is `actor` only — there is no per-object scope. Each
   actor has exactly one rotation chain.
2. The genesis-initial key from `ActorGenesis` is **implicit** in the
   chain; no separate key event introduces it. The first
   `ActorKeyRotation` for an actor has `supersedes = null` and is
   signed by the genesis-initial key.
3. Successor rotations carry an explicit `supersedes` chain edge
   pointing at the prior `ActorKeyRotation`. Resolution honors **chain
   precedence**: the active key is the chain leaf's `next_key`. The
   genesis-initial key is the implicit predecessor of the first
   rotation.
4. Cross-actor `supersedes` is **invalid** — only the actor whose key
   it is may rotate their own keys, even with a covering capability.
5. The signature on the rotation statement itself is produced by the
   **prior** active key (the genesis-initial key on the first rotation,
   the chain-leaf's `next_key` thereafter).

Resolution: the active key for `(actor, T)` is the `next_key` of the
rotation chain leaf with `created_at ≤ T`, falling back to
`ActorGenesis.initial_key` if no rotation precedes `T`. The verifier
applies this rule when checking any signed statement from `actor` —
the `signature.key_id` field selects the candidate, and that candidate
must equal the resolved active key for `(actor, T)`.

The canonical ActorKeyRotation v1 form is documented in:

```text
schemas/canonical/actor-key-rotation-v1.md
```

### 4.2g ActorKeyRevocation Statement

An `ActorKeyRevocation` statement retracts the signing authority of a
specific `KeyId` previously held by the envelope actor. It exists to
handle key compromise: the operator declares "any signature attributed
to this `KeyId` past this point — and optionally retroactively — is
not authorized."

Differences from `ActorKeyRotation`:

1. Revocation is **standalone** — there is no `supersedes` chain.
   Duplicate revocations naming the same `(actor, revoked_key)` are
   tolerated for federation replay; the **most-restrictive**
   interpretation wins (any `retroactive = true` revocation makes the
   key retroactively revoked).
2. Default revocation invalidates statements signed by `revoked_key`
   with `created_at` strictly **after** the revocation; `retroactive =
   true` invalidates them from inception. This is symmetric with
   `ActorCapabilityRevocation.retroactive`.
3. Cross-actor revocation is **invalid** — only the actor whose key it
   is may revoke their own keys.
4. Authority to revoke in v1: the revocation must be signed by the
   actor's **currently active key** at the revocation's `created_at`
   (resolved via the rotation chain). That active key MAY be
   `revoked_key` itself (the actor revoking their own current key).
   Cold-storage attestation keys for emergency revocation when the
   current active key is lost are deferred to Phase 2 §10 follow-on
   work.

Verifier integration: when checking any signed statement from
`(actor, key_id, T)`, the statement is invalid iff `key_id` is revoked
for `actor` at `T` (per the resolution rule in
`actor-key-revocation-v1.md`). This rule composes with the
active-key-at-causal-position check from rotation:

> A signed statement is cryptographically valid iff (a) `key_id`
> matches the active key at `T` per the rotation chain, (b) `key_id`
> is not revoked for `actor` at `T`, and (c) the signature bytes
> verify against that key's public material.

Retroactive revocation cascades through anything that depended on
statements signed by the revoked key — including `ActorCapabilityGrant`
statements (whose subsequent grants then propagate to every
statement issued under them). The `KeyPinned` capability constraint
(`CAPABILITIES.md` §7.2) is auto-invalidated when the pinned key is
revoked, regardless of `retroactive` — pinning is the opt-in coupling
between a grant and a specific signing key.

The canonical ActorKeyRevocation v1 form is documented in:

```text
schemas/canonical/actor-key-revocation-v1.md
```

### 4.2h ActorEmergencyKeyRotation Statement

An `ActorEmergencyKeyRotation` is the cold-storage counterpart to
`ActorKeyRotation` (§4.2f). The body shape is identical — `next_key`
plus `supersedes` — but the statement is signed by a **cold-storage
attestation key** declared in `ActorGenesis.attestation_keys` (or
appended via `ActorAttestationKeyAdd`, §4.2j) instead of by the actor's
currently active signing key.

This is the recovery mechanism for the two scenarios in `ACTORS.md`
§5.5.2 — lost active key and compromised active key. Without it,
either condition would brick the actor (`ACTORS.md` §5.5.1).

Differences from `ActorKeyRotation`:

1. The verifier accepts the signature iff `signature.key_id` is in the
   actor's **attestation key set** at `created_at` — never the
   rotation-chain active key. The two signing surfaces never overlap.
2. The chain semantics are otherwise identical — emergency rotations
   contribute leaves to the same per-actor key-event chain, and the
   active-key-at-causal-position rule walks them transparently.
3. Cross-actor `supersedes` is **invalid** — including across
   attestation surfaces. An attestation key controlled by a different
   actor cannot rotate this actor's keys.

Resolution: same as `ActorKeyRotation` — the active key for `(actor, T)`
is the chain leaf's `next_key`, regardless of whether the leaf was a
routine or emergency rotation.

The canonical ActorEmergencyKeyRotation v1 form is documented in:

```text
schemas/canonical/actor-emergency-key-rotation-v1.md
```

### 4.2i ActorEmergencyKeyRevocation Statement

An `ActorEmergencyKeyRevocation` is the cold-storage counterpart to
`ActorKeyRevocation` (§4.2g). The body shape — `revoked_key`,
`retroactive`, `reason` — is identical, but the signature must be
produced by an attestation key in the actor's attestation set at
`created_at`.

This exists so an operator can retract a compromised signing key from
cold storage without first having to emergency-rotate to a fresh active
key. The revocation modes (default vs retroactive) and resolution
semantics are identical to the routine variant; the revocation set for
`(actor, key_id)` spans both kinds, and the most-restrictive
interpretation wins.

Attestation keys themselves are **not** revocable in v1 — they are
append-only via `ActorAttestationKeyAdd`. Compromise of an attestation
key has no in-protocol remediation in v1; see `ACTORS.md` §5.5.2.

The canonical ActorEmergencyKeyRevocation v1 form is documented in:

```text
schemas/canonical/actor-emergency-key-revocation-v1.md
```

### 4.2j ActorAttestationKeyAdd Statement

An `ActorAttestationKeyAdd` statement appends a new public key to the
actor's attestation key set. The genesis-declared set in
`ActorGenesis.attestation_keys` is fixed (part of the canonical bytes
that derive `ActorId`); after genesis, the operator may grow the set by
publishing one of these statements per added key.

Differences from the rotation/revocation kinds:

1. Attestation keys form a **set**, not a chain. There is no
   `supersedes` field; ordering is irrelevant. The set is **append-only**
   in v1 — there is no `ActorAttestationKeyRevocation` and no removal
   mechanism.
2. The signature must be produced by an existing attestation key in the
   set at `created_at`. The operational signing key surface (active key
   per the rotation chain) **cannot** grow the attestation set, even
   if it is the only key the operator currently holds. This separation
   keeps a compromised active key from quietly registering
   attacker-controlled recovery keys.
3. `new_key` must be disjoint from any signing key the actor has held.
   Promoting a signing key into the attestation surface would collapse
   the cold-storage separation; the body validator rejects it.

Resolution: the attestation set for `(actor, T)` is
`ActorGenesis.attestation_keys ∪ { add.new_key | add ∈
ActorAttestationKeyAdd statements signed by actor with created_at ≤ T }`.

The canonical ActorAttestationKeyAdd v1 form is documented in:

```text
schemas/canonical/actor-attestation-key-add-v1.md
```

### 4.2 ObjectRevision Statement

Records that an actor claims a storage revision belongs to a specific Kairo
Object lineage. For Git-backed objects, this binds an immutable commit and its
parent relationship to an Object without relying on mutable branch or tag names.

Example:

```json
{
  "type": "ObjectRevision",
  "version": 1,
  "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
  "subject": "object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
  "body": {
    "object": "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
    "revision": "git:sha256:revision",
    "parents": ["git:sha256:parent"],
    "manifest_hash": "zQmfBE2w2UqKJhGZAxK4ZWb4JuCrvxnN9P4YKuvzfbPSvD5",
    "attests_reachable_history": true
  }
}
```

The canonical ObjectRevision v1 form is documented in:

```text
schemas/canonical/object-revision-v1.md
```

### 4.3 Build Statement

Records a successful build:

{
  "type": "kairo.statement.build.v1",
  "subject": {
    "object": "z6MkObject...",
    "snapshot": "z6MkSnapshot..."
  },
  "target": "release",
  "resolvedManifest": {
    "hash": "z6MkResolvedManifest..."
  },
  "artifact": {
    "snapshot": "z6MkArtifactSnapshot..."
  },
  "result": "success"
}

---

### 4.4 Provides Statement

Declares a capability:

{
  "type": "kairo.statement.provides.v1",
  "subject": {
    "object": "z6MkObject...",
    "target": "release",
    "output": "static-lib"
  },
  "provides": "lib:zlib:static",
  "version": "1.3.1"
}

---

### 4.5 Observation Statement

Records observed behavior:

{
  "type": "kairo.statement.observation.v1",
  "observedStatement": "z6MkStatement...",
  "notes": [
    {
      "kind": "resolution-evidence",
      "requested": "tool:make",
      "resolved": "z6MkMakeObject...",
      "result": "success"
    }
  ]
}

---

### 4.6 Inference Statement

Records inferred properties:

{
  "type": "kairo.statement.inference.v1",
  "subject": {
    "object": "z6MkObject...",
    "snapshot": "z6MkSnapshot..."
  },
  "path": "file.exe",
  "inferred": ["data:exe:mz"],
  "method": "magic-bytes"
}

---

## 5. Canonicalization

Statements MUST be canonicalized before hashing.

Requirements:

- deterministic field order
- stable binary primitive encodings
- length-prefixed strings and bytes
- explicit option/list encodings
- no dependence on JSON object key ordering

Canonical forms are documented under:

```text
schemas/canonical/
```

The canonical schema for `ObjectGenesis` v1 is:

```text
schemas/canonical/object-genesis-v1.md
```

The canonical schema for `ObjectRevision` v1 is:

```text
schemas/canonical/object-revision-v1.md
```

JSON interchange schemas belong under:

```text
schemas/json/
```

JSON interchange schemas describe external representation. They are not the
canonical hash input unless a canonical schema explicitly says so.

The JSON interchange schemas for the first statement types are:

```text
schemas/json/statement-envelope-v1.schema.json
schemas/json/object-genesis-v1.schema.json
schemas/json/object-revision-v1.schema.json
```

External clients that verify IDs or signatures must implement the relevant
canonical schema exactly.

---

## 6. Trust Model

Statements are not inherently trusted.

Trust is derived from:

- actor identity
- signature validity
- observed outcomes
- reproducibility
- social consensus

### 6.1 Verification result model

Statement verification returns a structured `VerificationReport` with three
independent dimensions: **signature status**, **actor resolution**, and
**trust evaluation**. The full model is defined in `ACTORS.md` §6.2; the
critical rules are:

- Cryptographic validity (signature + actor resolution) and trust evaluation
  are independent. A valid signature does not imply trust; trust does not
  override invalid signatures.
- Trust is **first-person**: it is parameterized by *who* is asking. When the
  caller supplies `by_actor`, `evaluate_trust` (see §4.2c) folds that
  truster's `ActorTrust` chain leaf into one of `Trusted | Untrusted |
  Unknown`. When no `by_actor` is supplied, trust stays `Unevaluated`.
- `ResolverUnavailable` is operational and must be reported distinctly from
  `NotFound`.

The Rust implementation lives in `kairo-statement::verify` and is consumed by
the CLI's `revision verify-actor-genesis` command.

### 6.2 ObjectRevision validation report

Signature verification only proves "actor X attested to these bytes." It does
not prove the bytes are coherent with the rest of the system. A separate
**`ObjectRevisionValidationReport`** answers four independent statement-layer
questions about a signed revision; each is reported in its own field rather
than collapsed into a single boolean:

- **`object_consistency`** — does `revision.object` match the `ObjectId`
  derived from the resolved `ObjectGenesis`? Variants:
  - `Consistent` — the resolved genesis derives the same id.
  - `Mismatch { expected, actual }` — different ids; the wrong genesis was
    supplied or one record is corrupt.
  - `GenesisNotProvided` — no genesis was supplied; **indeterminate**.
- **`manifest_binding`** — does `revision.manifest_hash` match the canonical
  hash of the parsed `kairo.toml`, and does any declared `[kairo].object`
  agree with `revision.object`?
  - `Bound` — both hold.
  - `HashMismatch { expected, actual }` — declared hash differs from the
    actual canonical hash.
  - `DeclaredObjectMismatch { expected, actual }` — manifest names a
    different object than the revision binds to.
  - `ManifestNotProvided` — no manifest was supplied; **indeterminate**.
- **`parents`** — `NoParents` (initial revision) or `Declared { count }`.
  The statement layer cannot prove that the named parents exist; that
  belongs to the content (Git) layer.
- **`content`** — does the storage commit named by `revision` exist
  locally, and do its parents agree with the revision's declared
  parents? Variants:
  - `Verified` — commit found and parents agree (set-equality;
    ordering is not enforced).
  - `ParentMismatch { expected, actual }` — declared parents disagree
    with the Git commit's actual parents.
  - `CommitNotFound` — the commit is not in the configured Git repo.
  - `Indeterminate` — no Git lookup was performed (no repo provided
    or the revision uses a non-`git:sha256:` scheme).

What each revision field actually proves once validated:

- **`revision`** — the actor stands behind a specific storage revision id.
  The statement layer takes it as opaque; coherence with Git is content-layer
  work.
- **`parents`** — claimed predecessors of `revision`. Statement-layer
  validation only counts them; content-layer validation checks them against
  Git commit parents.
- **`manifest_hash`** — the actor pinned a specific canonical manifest at
  the time of signing. Validation against a parsed `kairo.toml` proves
  the working manifest is the one the revision was signed for.

`ObjectRevisionValidationReport` and `validate_object_revision` live in
`kairo-object`. The validator is **pure** (no I/O); the caller decides how
to fetch the genesis, manifest, and Git commit lookup, which is why
missing inputs are reported as `*NotProvided` / `Indeterminate` rather
than as failure. The store-backed `kairo verify object` command is its
primary caller, plumbing the local `FilesystemStore` for genesis and
revision lookups, the `kairo-git` repository for commit + parent
checks, and (optionally, when `--as <truster>` is given or auto-picked
from the keystore) the `TrustResolver` for the trust dimension.

---

## 7. Statement Graph

Statements form a graph:

objects → statements → observations → trust

This enables:

- capability inference
- dependency resolution heuristics
- reproducibility verification

---

## 8. Federation

Statements are:

- shareable across nodes
- independently verifiable
- append-only

Nodes may:

- accept
- reject
- ignore
- prioritize

based on local trust policy.

---

## 9. Design Principles

- statements are claims, not truth
- signatures bind actors to claims
- trust is derived, not assigned
- statements are content-addressed
- federation is decentralized
