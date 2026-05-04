# ACTORS.md

## Status

Draft specification.

This document defines Kairo actors: identities that create, sign, delegate,
revoke, and otherwise participate in object statement graphs.

Actors are the cryptographic identity layer used by statements and authority
evaluation. Local trust decisions are defined separately by `POLICY.md`.

---

## 1. Purpose

Actors provide identity and authorship for Kairo statements.

Actors are responsible for:

1. Signing statements.
2. Establishing object root authority.
3. Granting capabilities to other actors.
4. Delegating authority where permitted.
5. Revoking capabilities.
6. Rotating signing keys.
7. Supporting auditability of object history.

Actors are not responsible for:

1. Local trust policy.
2. Runtime permissions.
3. Search ranking.
4. Package validity.
5. User-interface identity presentation beyond metadata.

---

## 2. Relationship to Other Specs

Actors interact with:

- `STATEMENTS.md` for signed statements.
- `CORE_LIBRARY.md` for authority evaluation.
- `POLICY.md` for local trust decisions.
- `IDENTIFIERS.md` for `ActorId` format.
- `CAPABILITIES.md` for actor capabilities (delegated authority to issue statements on behalf of another actor).
- `SANDBOX.md` for runtime sandbox capabilities — what an executing artifact may access — which are separate from actor capabilities.
- `DAEMON.md` for local trust-root configuration.
- `API.md`, `CLI.md`, and `WEB_CLIENT.md` for presenting actor information.
- `PACKAGE.md` for preserving signed actor-related statements.

Important distinction:

```text
Actor authority is semantic.
Local trust is policy.
```

A statement may be cryptographically valid and semantically authorized, while local
policy still refuses to trust or execute it.

---

## 3. Core Concepts

### 3.1 Actor

An actor is an identity capable of signing statements.

Examples:

- Individual maintainer
- Organization
- Project team
- Automated build service
- Archive institution
- Research lab
- Local user identity

### 3.2 ActorId

An `ActorId` uniquely identifies an actor.

Recommended bare ID payload form:

```text
<encoded>
```

Standalone references use `actor:<id>` internally and `kairo:actor:<id>` for
external interchange.

The exact encoding is defined by `IDENTIFIERS.md`.

### 3.3 Signing Key

A signing key is a cryptographic key authorized to sign statements for an actor.

An actor may have multiple signing keys over time.

### 3.4 Actor Statement

An actor statement is a signed statement that establishes or modifies actor identity,
keys, metadata, delegation, or revocation state.

### 3.5 Capability Grant

A capability grant allows an actor to issue certain statement kinds for a scope.

Actor capabilities are distinct from runtime capabilities.

---

## 4. Actor Identity Model

### 4.1 Actor as stable identity

An actor is a stable identity that may survive signing-key rotation.

```text
ActorId != KeyId
```

A key proves it is authorized to speak for an actor. The actor itself is the stable
semantic identity.

In v1, `ActorId` is derived from a canonical `ActorGenesis` body:

```text
ActorGenesis {
  actor_kind,
  initial_public_key,
  nonce
}
```

The initial public key roots the actor, but the actor ID is not simply
`hash(public_key)`. The nonce allows distinct actors to intentionally start with
the same key without sharing identity, and the canonical genesis body provides
version/domain separation.

The canonical ActorGenesis v1 form is documented in:

```text
schemas/canonical/actor-genesis-v1.md
```

### 4.2 Actor metadata

Actor metadata may include:

- Display name
- Description
- Contact links
- Homepage
- Organization
- Public keys
- Recovery keys
- Expiration metadata

Metadata is advisory unless signed by the actor or an authorized actor.

### 4.3 Actor profiles

Actor profiles may be represented as objects or actor-specific records.

The chosen representation must preserve:

1. Actor ID.
2. Current authorized keys.
3. Revoked keys.
4. Key rotation history.
5. Delegation/capability statements.
6. Metadata statements.

---

## 5. Cryptographic Keys

### 5.1 Key types

Kairo should support a modern signature algorithm.

Required MVP algorithm:

```text
ed25519
```

Future algorithms may be supported through explicit algorithm identifiers.

Each actor holds two **disjoint** key surfaces:

- **Signing keys.** The operational surface that signs all routine
  statements (`ActorKeyRotation`, `ActorKeyRevocation`, `ObjectRevision`,
  `ObjectBranch`, `ActorTrust`, `ActorCapabilityGrant`, etc.). Actively
  managed via the rotation chain (§5.5).
- **Attestation keys.** The cold-storage authority surface that signs
  emergency key events (`ActorEmergencyKeyRotation`,
  `ActorEmergencyKeyRevocation`, `ActorAttestationKeyAdd`). Declared at
  `ActorGenesis` and append-only afterwards (§5.5.2). Never used to
  sign operational statements.

The two surfaces never overlap. A key registered as a signing key
cannot become an attestation key, and vice versa; the body validator
rejects any statement that would conflate them.

### 5.2 Key identifiers

A key should have a stable key ID.

Recommended form:

```text
<encoded>
```

A key ID should be derived from the public key material and algorithm. It uses
the same Kairo-native SHA-256 multihash/base58btc payload format as other IDs,
but it is not an `ActorId`.

### 5.3 Key record

Recommended structure:

```rust
pub struct ActorKey {
    pub key_id: KeyId,
    pub actor_id: ActorId,
    pub algorithm: SignatureAlgorithm,
    pub public_key: PublicKeyBytes,
    pub status: KeyStatus,
    pub created_by: Option<StatementId>,
    pub revoked_by: Option<StatementId>,
}
```

### 5.4 Key status

```rust
pub enum KeyStatus {
    Active,
    Revoked,
    Expired,
    Superseded,
}
```

`Active` and `Revoked` are the v1 surface. `Expired` is reserved for
future per-key expiry constraints; `Superseded` is the descriptive
state of any prior rotation that is no longer the chain leaf.

### 5.5 Key chain

Each actor has a per-actor key chain composed of:

- the implicit genesis-initial key from `ActorGenesis.initial_key`
  (no separate statement introduces it; it is active from the
  genesis's `created_at` onward);
- zero or more `ActorKeyRotation` statements (`STATEMENTS.md` §4.2f),
  each naming the next active key and `supersedes`-chaining at the
  prior rotation;
- zero or more `ActorKeyRevocation` statements (`STATEMENTS.md`
  §4.2g), each naming a `KeyId` whose signing authority is being
  retracted, optionally retroactively;
- zero or more `ActorEmergencyKeyRotation` statements (`STATEMENTS.md`
  §4.2h) and `ActorEmergencyKeyRevocation` statements (§4.2i) — same
  chain semantics as the routine variants, but signed by the
  attestation surface instead of the active signing key (see §5.5.2).

Two independent queries fall out of this chain:

1. **Active key at causal position `T`.** Walk the rotation chain;
   the active public key for `(actor, T)` is the `next_key` of the
   chain leaf with `created_at ≤ T`, or `ActorGenesis.initial_key` if
   no rotation precedes `T`. Chain precedence is authoritative; a
   successor that explicitly names its predecessor wins regardless of
   `created_at`.
2. **Revocation status of `(actor, key_id)` at `T`.** A key is
   revoked at `T` iff there exists an `ActorKeyRevocation` for
   `(actor, key_id)` such that either `retroactive = true` or
   `created_at ≤ T`. Revocation is standalone (no chain); the
   most-restrictive interpretation wins.

These compose into a single rule consumed by `§6.1`:

> A signed statement from `(actor, key_id)` at `created_at = T` is
> cryptographically valid iff (a) `key_id` matches the active key for
> `actor` at `T`, (b) `key_id` is not revoked for `actor` at `T`, and
> (c) the signature bytes verify against that key's public material.

Cross-actor key events (rotation or revocation) are **invalid** —
only the actor whose key it is may modify their own key chain. No
covering capability can authorize key authority on someone else's
behalf.

Retroactive revocation cascades through anything that depended on
statements signed by the revoked key. Capability grants signed by a
retroactively-revoked key flip to invalid, which in turn invalidates
statements that depended on them. The `KeyPinned` capability
constraint (`CAPABILITIES.md` §7.2) is the opt-in coupling between a
specific grant and a specific signing key — pinned grants are
auto-invalidated on revocation regardless of `retroactive`.

### 5.5.1 Bricking risk and the rotate-first rule

The protocol does **not** prevent an actor from revoking the only
key they hold. The verification rules are internally consistent:
the revocation itself is valid (signed by the active key at its
`created_at`), but immediately afterwards the actor has no key whose
signature would verify for any new statement — including a fresh
`ActorKeyRotation`. The actor is permanently dead; recovery means
publishing a new `ActorGenesis`, which produces a different
`ActorId`, with continuity re-established socially.

Operator hygiene: **always rotate first, then revoke.**

1. `ActorKeyRotation { next_key: K1, supersedes: null }` signed by K0.
2. `ActorKeyRevocation { revoked_key: K0_id, ... }` signed by K1.

The CLI (`kairo actor revoke-key`) refuses to revoke the only active
key without explicit confirmation. The protocol layer makes no such
check; direct callers of the bodies must enforce this themselves.

### 5.5.2 Cold-storage attestation keys

Both the bricking risk and the lost-active-key compromise scenario
point at the same primitive: a **separate authority surface**, declared
at `ActorGenesis` and append-only afterwards, that can sign emergency
key events even when the operator has no working active key.

Shape (v1):

- One or more attestation public keys are declared in
  `ActorGenesis.attestation_keys` alongside `initial_key`. They are
  part of the canonical genesis bytes — and therefore part of the
  `ActorId`. An attacker cannot swap them out without producing a
  different actor.
- Attestation keys sign **only** the three emergency body kinds:
  `ActorEmergencyKeyRotation` (`STATEMENTS.md` §4.2h),
  `ActorEmergencyKeyRevocation` (§4.2i), and `ActorAttestationKeyAdd`
  (§4.2j). They have no authority over operational statements
  (revisions, branches, tags, capability grants, trust) and the
  verifier rejects any operational statement signed by an attestation
  `key_id`.
- The attestation set may be grown after genesis via
  `ActorAttestationKeyAdd`, signed by an existing attestation key. The
  operational signing surface cannot grow the attestation set —
  separation is enforced at the verifier.
- Attestation keys are **not revocable** in v1. Compromise of an
  attestation key has no in-protocol remediation; the operator must
  publish a fresh `ActorGenesis` (different `ActorId`, continuity
  re-established socially). A future schema revision may introduce
  `ActorAttestationKeyRevocation`.

Resolution rule:

> The attestation key set for `(actor, T)` is
> `ActorGenesis.attestation_keys ∪ { add.new_key | add ∈
> ActorAttestationKeyAdd statements signed by actor with
> created_at ≤ T }`.

Cold-storage discipline:

- Kairo never stores attestation private key material. Operators
  hold the private halves externally — YubiKey, hardware wallet,
  air-gapped device, encrypted seed in a safe.
- The CLI MAY offer a generate-and-print convenience (e.g.
  `kairo actor create --generate-attestation-key`) that produces a
  fresh keypair, prints the seed once to stdout with an explicit
  "this will not be saved" warning, embeds only the public key in
  the genesis, and drops the seed from process memory before
  exiting. The operator is responsible for capturing the seed into
  external cold storage.
- Operator-presented public keys (`--attestation-key <hex-pubkey>`)
  are the recommended path because they force the use of a real
  cold-storage tool to produce the keypair; the private half never
  enters Kairo's process at all.

Operational implications:

- The v1 bricking risk in §5.5.1 is now recoverable as long as at
  least one attestation key remains: the operator publishes
  `ActorEmergencyKeyRotation` from cold storage to introduce a fresh
  active key, then resumes routine operation.
- A leaked attestation key alone cannot silently sign forged
  operational statements. The attacker would have to first
  emergency-rotate to a key they control, then sign with that —
  leaving an emergency-rotation event in the audit trail signed by
  the compromised attestation `key_id`. Operators should monitor for
  unexpected emergency rotations.

---

## 6. Signatures

Statements must be signed by an active key authorized for the statement’s actor at
the statement’s causal position.

A signature must cover:

1. Statement body.
2. Actor ID.
3. Object ID.
4. Statement kind.
5. Actor sequence.
6. Previous actor statement reference.
7. Causal parents.
8. Any other fields declared signed by `STATEMENTS.md`.

The exact canonical signing payload is defined by `STATEMENTS.md` and `SCHEMA.md`.

### 6.1 Signature verification

Each statement kind binds to one of two signing surfaces:

- **Operational kinds** (everything except the three emergency kinds
  below) — signed by the actor's **active signing key** per the
  rotation chain (§5.5).
- **Emergency kinds** — `ActorEmergencyKeyRotation`,
  `ActorEmergencyKeyRevocation`, and `ActorAttestationKeyAdd` — signed
  by an **attestation key** in the actor's attestation set at
  `created_at` (§5.5.2).

The two surfaces never overlap: an operational statement signed by an
attestation `key_id` is invalid even if the bytes verify; an emergency
statement signed by an active-signing `key_id` is invalid even if the
bytes verify. Verifier dispatch is by statement kind.

Core verification must check, in order:

1. **Surface dispatch.** Determine the expected signing surface from the
   statement kind (operational vs emergency).
2. **Key admissibility.**
   - Operational: `signature.key_id` matches the actor's active key
     at `created_at` per the rotation chain in §5.5, **and** that
     `key_id` is not revoked for the actor at `created_at` per the
     revocation set in §5.5.
   - Emergency: `signature.key_id` is in the actor's attestation set
     at `created_at` per §5.5.2.
3. The signature bytes verify against the resolved key's public
   material under the declared algorithm.
4. Statement payload matches signed canonical bytes.

Invalid signatures make statements invalid.

Missing key data (rotation, revocation, or attestation-add statements
not yet observed locally) may make validation indeterminate rather than
invalid — same handling as missing predecessors elsewhere in the
protocol.

### 6.1.1 Genesis statements are not symmetric

`ActorGenesis` and `ObjectGenesis` are treated **differently** with respect to
signatures, on purpose:

- **`ActorGenesis` carries no signature.** The body contains the actor's
  initial public key, and the body's canonical bytes derive the `ActorId`.
  Possession of the matching private key is enforced on every later signed
  statement, not on the genesis itself. A self-signed genesis would be
  circular and adds no security: an attacker without the private key who
  publishes a body with a stolen public key produces a different `ActorId`
  (different `actor_kind` / `nonce` / `created_at`), and that `ActorId` is
  inert because they still cannot sign as it. There is no impersonation
  vector to close.

- **`ObjectGenesis` carries a signature.** The body says "actor X created
  object lineage Y at time T," invoking external authority. The signature
  is required to authenticate the binding from the actor to the object
  claim. The signature is **not** part of `ObjectId` material (so the same
  body can be re-signed without changing identity), but verifying the
  signature against the actor's resolved key is required to trust the
  claim.

When introducing a new statement type, ask: does the body's content-
addressed identity already authenticate the claim, or does the claim invoke
external authority? Self-attesting bodies can be unsigned; bodies that bind
an actor to a claim about something else MUST be signed.

### 6.2 Verification result model

Statement verification produces a structured `VerificationReport` with three
**independent** dimensions. None of them imply or override the others:

- **Signature status** — whether the cryptographic signature verified against
  the resolved key. Possible values include `Valid`, `Invalid`,
  `UnsupportedAlgorithm`, `Malformed`, `AlgorithmMismatch`, and `NotEvaluated`
  (when the actor could not be resolved).
- **Actor resolution** — whether the actor declared on the statement was
  resolvable. Possible values include `Resolved`, `NotFound`,
  `ResolverUnavailable` (transient/operational), and `SignatureActorMismatch`
  (the signature's actor field disagrees with the envelope's actor field).
- **Trust evaluation** — first-person local opinion. Possible values are
  `Trusted`, `Untrusted`, `Unknown` (no opinion published, or the chain leaf
  is a withdrawal), and `Unevaluated` (the caller did not supply a `by_actor`
  to evaluate from). Backed by `ActorTrust` statements; see `STATEMENTS.md`
  §4.2c.

Rules:

1. A `Valid` signature does not imply trust. A statement with a valid
   signature from an unknown or untrusted actor is still cryptographically
   valid, not authoritative.
2. Trust never overrides cryptographic validity. A trusted actor with an
   `Invalid` signature is still invalid.
3. `ResolverUnavailable` is operational, not semantic. Callers should retry
   or report it differently from `NotFound`.
4. Trust is **always parameterized by a truster**. `Trusted` from actor X's
   perspective says nothing about what actor Y thinks. There is no
   node-wide "is trusted" — each truster has their own opinion, resolved
   independently.
5. The report shape is stable: `verify_envelope_statement` fills signature
   and actor resolution; `evaluate_trust(by_actor, of_actor, trust_resolver)`
   in `kairo-statement::verify` fills the trust field when a truster is
   provided.

---

## 7. Actor Chains

Actor activity for a given object is represented by per-actor signed statement
chains as defined in `CORE_LIBRARY.md`.

For each `(object_id, actor_id)` pair:

1. Actor statements must have monotonic sequence numbers.
2. Non-initial statements must reference previous actor statements.
3. Chain continuity is required for validation when the chain affects the snapshot.
4. Broken chains are invalid if contradictory data is present.
5. Missing required chain predecessors make validation indeterminate.

Actor chains prove continuity of a signer’s participation in an object.

---

## 8. Actor Resolution

Actor verification depends on resolving actor identity data:

```text
ActorId -> ActorGenesis
ActorId -> active keys
```

The identity layer defines this as a resolver interface rather than as a
specific storage layout. Implementations may resolve actors from:

- in-memory package contents
- local stores
- daemon indexes
- federation caches
- archival bundles
- future databases

The resolver returns the actor's full key surface as defined by §5.5:
the implicit genesis-initial key from `ActorGenesis`, the rotation
chain (`ActorKeyRotation` statements), and the revocation set
(`ActorKeyRevocation` statements). Verifiers consume this through two
queries — "active key at `T`" and "is `(actor, key_id)` revoked at
`T`?" — and compose them per §6.1. Delegation and authority checks
that go beyond identity (capabilities) live in `CAPABILITIES.md` and
extend the same resolver boundary.

Missing actor data makes validation indeterminate rather than trusted.

---

## 9. Root Authority

Every object must have a root authority.

Root authority is established by an object creation statement or equivalent root
statement.

A root authority statement must identify one or more initial actors or authority
keys.

Example conceptual structure:

```rust
pub struct ObjectRootAuthority {
    pub object_id: ObjectId,
    pub root_actors: Vec<ActorId>,
    pub root_capabilities: Vec<ActorCapability>,
}
```

No non-root statement can be authoritative unless authority can be traced to root
authority through valid grants and delegations.

---

## 10. Actor Capabilities

Actor capabilities define delegated authority — one actor empowering another
to issue specific statement kinds on a scoped target. The authoritative
specification is **`CAPABILITIES.md`**. This section is a pointer; the
shape sketched in earlier drafts of §10–12 has been superseded by the
locked design in that document.

These are different from runtime sandbox capabilities in `SANDBOX.md` (which
govern what an executing artifact may access — filesystem, network, GPU,
etc.).

Authoritative entry points:

- `Capability { scope, statement_kinds, delegable, constraints }` —
  `CAPABILITIES.md` §4 and `schemas/canonical/actor-capability-grant-v1.md`.
- `CapabilityScope` (object / actor) — `CAPABILITIES.md` §4.1.
- `CapabilityConstraint` (`ExpiresAt`, `MaxDelegationDepth`, `KeyPinned`) —
  `CAPABILITIES.md` §4.3.
- Resolution rules (`evaluate_capability`) — `CAPABILITIES.md` §6.1; the
  resolver is implemented in `kairo-statement::verify`.
- Cross-actor `supersedes` honored when a covering capability exists —
  `CAPABILITIES.md` §6.2; implemented for `ObjectVersionTag` by
  `kairo-store::FilesystemStore::latest_version_tag`.

---

## 11. Grants

A grant is signed via the `ActorCapabilityGrant` statement type. Validity
rules and chain semantics live in `CAPABILITIES.md` §5.1 and §6.1.

Per Decision A in `CAPABILITIES.md` §9, grants are first-person (sharded
by grantor); per Decision G, the chain leaf for a `(grantor, grantee,
scope)` triple is the source of truth — `statement_kinds` does not union
across siblings. Issue a successor with `supersedes` set to extend or
narrow.

---

## 12. Revocation

Revocation is signed via the `ActorCapabilityRevocation` statement type
(`CAPABILITIES.md` §5.2). Default revocation invalidates statements with
`created_at` strictly after the revocation; `retroactive = true`
invalidates the grant from inception (`CAPABILITIES.md` §6.3). In v1 only
the original grantor may revoke (cross-grantor revocation is invalid).

### 12.3 Key revocation

Revoking a signing key means future statements signed by that key are invalid
after the revocation point.

Earlier statements remain valid unless retroactive key compromise semantics are
explicitly declared.

For capability grants specifically, key revocation does **not**
auto-invalidate the grant by default — capabilities anchor on `ActorId`,
not `KeyId` (`CAPABILITIES.md` §7). The `KeyPinned` constraint
(`CAPABILITIES.md` §7.2) opts a grant in to auto-invalidation when the
named signing key is revoked, for high-stakes delegations where rotation
should not survive a compromise window. The grantor key-compromise
cleanup runbook is in `CAPABILITIES.md` §7.1.

---

## 13. Key Rotation

Actors must support signing-key rotation.

Key rotation should be represented by signed statements:

1. Add new key.
2. Mark old key superseded or revoked.
3. Optionally require overlap signatures from both keys.
4. Preserve audit trail.

### 13.1 Rotation validity

A key rotation is valid if:

1. Existing active actor key signs the rotation, or
2. A designated recovery key signs the rotation, or
3. A valid actor governance rule authorizes the rotation.

### 13.2 Recovery keys

Actors may define recovery keys.

Recovery keys should be more restricted than active signing keys.

Recovery keys may:

- Add a new active key.
- Revoke compromised keys.
- Restore actor control.

Recovery keys should not automatically have broad object-authoring capabilities
unless explicitly granted.

---

## 14. Multi-Signature and Governance

Some actors, especially organizations or archives, may require multi-signature
governance.

Supported patterns may include:

- M-of-N key approval
- Role-based approval
- Time-delayed changes
- Emergency recovery path
- Separate signing and administrative keys

A multi-signature governance rule must be explicit and deterministic.

Core may validate governance requirements when they are part of semantic actor
authority.

Local policy may require stricter governance than the actor itself declares.

---

## 15. Actor Metadata and Presentation

Actor metadata is useful for display but must not be confused with authority.

Display metadata may include:

```json
{
  "display_name": "Example Archive",
  "description": "An archival institution.",
  "homepage": "https://example.invalid",
  "contacts": []
}
```

Clients must indicate when actor metadata is:

- Verified
- Self-signed
- Third-party attested
- Unverified
- Stale
- Revoked/superseded

Search may index actor metadata, but search results remain unverified until validated.

---

## 16. Actor Attestations

Actors may issue attestations about other actors.

Examples:

- Organization vouches for maintainer.
- Archive institution verifies project identity.
- Research lab verifies author identity.

Attestations are statements.

Attestations may inform local policy but do not automatically grant semantic
authority unless the object authority graph gives them that role.

---

## 17. Actor vs Local User

An actor is a Kairo identity.

A local user is the person or account operating a daemon/CLI/web client.

They may be related but are not identical.

A local user may control one or more actors.

The daemon should manage local actor keys securely.

---

## 18. Key Storage

Private key storage is outside the semantic core but must be addressed by daemon
or tooling.

Recommended requirements:

1. Private keys must not be stored unencrypted by default.
2. Private keys should use OS keychain or encrypted key store where possible.
3. CLI signing operations should require explicit key access.
4. Web client should not directly handle private keys unless a browser-based key
   model is explicitly designed.
5. Backup and recovery must be documented.

---

## 19. Actor Import and Export

Packages may include actor-related statements and metadata.

Importing actor data does not mean the local node trusts the actor.

Local policy decides whether an actor is trusted.

Actor statements must preserve signatures exactly.

---

## 20. API Representation

Actor DTOs should include:

```json
{
  "actor_id": "z6MkActor...",
  "display_name": "Example Actor",
  "keys": [
    {
      "key_id": "key_...",
      "algorithm": "ed25519",
      "status": "active"
    }
  ],
  "trust": {
    "local_policy": "trusted"
  },
  "validation": {
    "status": "valid"
  }
}
```

API responses must distinguish:

- Cryptographic validity
- Semantic authority
- Local trust

---

## 21. CLI Mapping

Recommended commands may include:

```text
kairo actor show <actor-id>
kairo actor keys <actor-id>
kairo actor create
kairo actor key add
kairo actor key revoke
kairo actor trust
kairo actor untrust
```

Trust commands modify local policy, not actor semantics.

---

## 22. Web Client Mapping

The web client should display:

- Actor identity
- Key status
- Authority path
- Local trust status
- Statements signed by actor
- Capability grants and revocations
- Warnings for untrusted or indeterminate actor state

The web client must not present local trust as cryptographic validity.

---

## 23. Validation Outcomes

Actor-related validation may produce:

- Valid actor key
- Invalid signature
- Missing key data
- Revoked key
- Expired key
- Missing authority path
- Invalid delegation
- Revoked capability
- Conflicting actor metadata
- Indeterminate actor state

These must be represented as structured validation issues.

---

## 24. Security Requirements

Actor systems must:

1. Use modern signature algorithms.
2. Preserve signed bytes exactly.
3. Support key revocation.
4. Support key rotation.
5. Avoid treating display names as identity.
6. Avoid treating local trust as semantic authority.
7. Avoid treating semantic authority as local trust.
8. Protect private keys.
9. Make delegation explicit.
10. Make revocation explicit.
11. Treat missing authority data as indeterminate.
12. Treat invalid signatures as invalid.
13. Avoid retroactive invalidation unless explicit.
14. Provide audit trails for authority changes.

---

## 25. Implementation Checklist

A conforming initial implementation should provide:

1. Actor ID type.
2. Key ID type.
3. Ed25519 signing support.
4. Signature verification.
5. Actor key records.
6. Key add/revoke statements.
7. Actor metadata statements.
8. Root authority representation.
9. Capability grant statements.
10. Capability revoke statements.
11. Delegation validation.
12. Key rotation support.
13. Revocation semantics.
14. Actor authority path construction.
15. Structured actor validation issues.
16. API DTOs for actors.
17. CLI actor inspection commands.
18. Web actor display components.
19. Local trust integration with `POLICY.md`.
20. Package import/export preservation of actor statements.

---

End of `ACTORS.md`.
