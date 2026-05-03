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

Core verification must check:

1. Signature bytes are valid for the declared algorithm.
2. Public key belongs to the claimed actor.
3. Key was active at the statement’s causal position.
4. Key had not been revoked before the statement.
5. Statement payload matches signed canonical bytes.

Invalid signatures make statements invalid.

Missing key data may make validation indeterminate.

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

The MVP resolver only needs to resolve `ActorGenesis` by derived `ActorId` and
return the genesis initial key. This is sufficient to verify statements signed
by the actor's root key. Later key rotation, revocation, delegation, and
authority checks extend the same resolver boundary.

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

Actor capabilities define which statement kinds an actor may issue.

These are different from runtime sandbox capabilities in `SANDBOX.md` (which
govern what an executing artifact may access — filesystem, network, GPU,
etc.).

### 10.1 Capability fields

Recommended structure:

```rust
pub struct ActorCapability {
    pub scope: AuthorityScope,
    pub statement_kinds: Vec<StatementKindDiscriminant>,
    pub grantor: ActorId,
    pub grantee: ActorId,
    pub delegable: bool,
    pub constraints: Vec<AuthorityConstraint>,
}
```

### 10.2 Authority scope

Capability scope may include:

```rust
pub enum AuthorityScope {
    Object(ObjectId),
    ObjectSnapshot(ObjectId),
    Artifact(ObjectId, ArtifactId),
    Build(ObjectId),
    Runtime(ObjectId),
    Metadata(ObjectId),
    Statement(StatementId),
}
```

### 10.3 Standard actor capabilities

Recommended standard capabilities:

```text
object.admin
object.metadata.write
object.statement.grant
object.statement.revoke
object.artifact.add
object.artifact.supersede
object.build.add
object.build.supersede
object.runtime.add
object.runtime.supersede
object.release.mark
object.dependency.add
object.dependency.remove
actor.key.add
actor.key.revoke
actor.metadata.write
```

### 10.4 Delegation

A capability may be delegable.

If `delegable = false`, the grantee may use the capability but may not grant it to
another actor.

Delegation chains must be explicit.

### 10.5 Constraints

Capability constraints may include:

- Expiration
- Statement-kind restriction
- Snapshot/frontier restriction
- Artifact type restriction
- Environment restriction
- Required co-signers
- Maximum delegation depth
- Non-retroactivity
- Human approval requirement

Constraints must be deterministic and validated by core where semantic.

Local policy may impose additional constraints.

---

## 11. Grants

A grant is a signed statement that gives a capability to another actor.

Grant validity requires:

1. Grantor has authority to grant the capability.
2. Grantor’s capability is delegable if delegation is involved.
3. Grant statement signature is valid.
4. Grant is causally available before the grantee uses the capability.
5. Grant has not been revoked before use.
6. Grant constraints are satisfied.

If a required grant is missing, validation is indeterminate.

If a grant is present but invalid, dependent statements are invalid unless another
valid authority path exists.

---

## 12. Revocation

Revocation removes or limits actor authority.

Revocation may target:

- Actor capability grant
- Signing key
- Actor metadata statement
- Delegation path
- Specific statement authority
- Actor participation in an object

### 12.1 Default revocation behavior

By default:

1. Revocation applies only to causally future statements.
2. Revocation does not retroactively invalidate earlier valid statements.
3. Historical snapshots before revocation remain valid.
4. Revocation must itself be authorized.
5. Revocation must identify its target precisely.

### 12.2 Retroactive revocation

Retroactive revocation is allowed only if defined by a specific statement kind and
requires stronger authority than ordinary revocation.

Retroactive revocation should be rare.

Use cases may include:

- Compromised key
- Fraudulent grant
- Administrative correction

Retroactive revocation must be explicit and visible in validation results.

### 12.3 Key revocation

Revoking a signing key means future statements signed by that key are invalid
after the revocation point.

Earlier statements remain valid unless retroactive key compromise semantics are
explicitly declared.

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
