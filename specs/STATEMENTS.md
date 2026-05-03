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
specific `ObjectRevision` statement. Resolution rule: for
`(actor, object, name)`, the current branch is whichever `ObjectBranch`
statement signed by that actor for that pair has the greatest
`(envelope.created_at, statement_id)`. Older `ObjectBranch` statements stay
valid evidence of past claims; only the latest is load-bearing.

`name = "head"` is the conventional default the CLI assumes when no name is
given. It is not reserved at the protocol level.

The canonical ObjectBranch v1 form is documented in:

```text
schemas/canonical/object-branch-v1.md
```

#### Future: `ObjectBranch v2` with `supersedes`

`ObjectBranch v1` resolves purely on `(created_at, statement_id)`. This
shares a known wart with the original `ObjectVersionTag` design: when
two updates collide on `created_at` (signed in the same second), the
statement-id lex tiebreak can pick a winner the actor did not intend.
`ObjectVersionTag v1` solved this by adding a `supersedes` chain edge
and resolving by chain leaf (see §4.2b and
`schemas/canonical/object-version-tag-v1.md`).

Branches have the same wart, but adding `supersedes` to `ObjectBranch`
would change its canonical bytes — every existing v1 `StatementId`
would re-derive differently. The fix is therefore a v2 schema bump,
not an in-place addition.

This work is **deferred** until either (a) a real same-second branch
collision causes user-visible damage, or (b) the §10 capability /
authority model lands. (b) is the more likely trigger: cross-actor
supersession (one actor taking over another's branch via a delegation
or maintainership grant) needs a chain edge in branches the same way
it needs one in tags. Designing `ObjectBranch v2` together with the
capability model — instead of designing it now in isolation and
redesigning when capabilities arrive — is the right scope.

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
same `(object, version)`. The protocol records this claim, but the MVP
per-actor resolver does not honor cross-actor edges — they're recorded
for audit and await the §10 capability/authority model (delegation,
co-maintainer grants, ownership transfer) to become load-bearing for
resolution.

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
   `ObjectVersionTag`, which only declines to honor cross-actor edges
   in the MVP resolver but records them at the protocol layer). Trust
   is first-person: only the truster who signed `S` may publish a
   successor that supersedes `S`.
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
- **`content`** — always `Indeterminate` in the MVP. A future content-layer
  check (TODO §11) verifies that `revision`, `parents`, and the working
  tree's manifest match the local Git repository.

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
to fetch the genesis and manifest, which is why missing inputs are reported
as `*NotProvided` (indeterminate) rather than as failure. The store-backed
`kairo verify object` command (TODO §8) will become the primary caller.

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
