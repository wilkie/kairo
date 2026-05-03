# ObjectVersionTag v1 Canonical Encoding

## Type

```text
ObjectVersionTag
```

## Version

```text
1
```

## Domain Separator

```text
kairo.statement.v1
```

## Derived ID

`ObjectVersionTag` is an ordinary signed statement. Its `StatementId` is derived
from the unsigned statement envelope and body. The signature proves authorship
of those canonical bytes but is not part of the statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ObjectVersionTag` is a named, actor-scoped, mutable pointer that binds a
**semver** version string to a specific `ObjectRevision` statement, or
withdraws a previously published binding. It is the Kairo analogue of an
npm/cargo/registry release tag: a way for an actor to publicly claim
"for object X, my release named *V* is currently revision-statement *R*"
— or to revoke that claim.

Resolution is latest-wins on `(actor, object, version)`, identical to
`ObjectBranch`. The difference from `ObjectBranch` is that `version` must
parse as a strict semver 2.0.0 string, and every non-genesis tag carries
an explicit `supersedes` pointer at the prior tag in its chain so the
rebind / revoke history is reconstructable without inferring from
timestamp order.

A future dependency resolver consumes these tags, so consumers that need
build reproducibility must record the resolved `StatementId` (or
`SnapshotId`) in a lockfile equivalent — the version string alone is not
a stable handle.

## Resolution Rule

> For `(actor, object, version)`, the current tag is the **leaf of the
> supersedes chain**: an `ObjectVersionTag` statement signed by `actor`
> for `(object, version)` whose `statement_id` is not referenced by any
> other such statement's `supersedes` field.

Chain precedence is authoritative: a successor statement that explicitly
names its predecessor via `supersedes` is unambiguously later than that
predecessor, regardless of `created_at`. `(envelope.created_at,
statement_id)` is **only** a fork tiebreak — applied when the chain has
multiple leaves (an actor signed two tags both pointing at the same
predecessor, or two genesis tags), the leaf with the greatest
`(created_at, statement_id)` wins.

If the head's `target` is a `StatementId`, the version is **bound** to
that `ObjectRevision`. If `target` is absent (the revocation shape),
the version is **withdrawn** for that actor; the resolver returns the
withdrawal along with the `supersedes` chain so callers can audit what
was withdrawn.

Cryptographic validity, actor resolution, and trust evaluation are
reported independently and do not affect resolution at the statement
layer.

### Cross-actor supersession

`supersedes` may reference an `ObjectVersionTag` from a **different
actor** for the same `(object, version)`. The protocol records the
claim, but the MVP per-actor resolver intentionally does not honor
cross-actor edges — `latest_version_tag(actor, object, version)`
considers only that actor's own statements. Honoring a B-supersedes-A
claim requires an authority story (delegation, co-maintainer grants,
ownership transfer) which lands with the §10 trust/capability model.
Until then, cross-actor supersession is *expressible* and visible in
audit history, but not load-bearing for resolution.

## Tag Chain Validation

Every `ObjectVersionTag` is one of two shapes:

- **Genesis (no `supersedes`):**
  - `target` is present (a bind — you cannot revoke a version that was
    never bound).
  - `supersedes` is absent.
- **Successor:**
  - `target` is present (rebind) or absent (revoke).
  - `supersedes` is present and must reference an existing
    `ObjectVersionTag` for the **same `(object, version)`**.

Cross-actor `supersedes` references are permitted (see "Cross-actor
supersession" under Resolution Rule). What is **not** permitted is a
`supersedes` that resolves to a tag for a different `(object, version)`
— that's not a chain edge, that's a category error.

A successor whose `supersedes` does not resolve in the local store is
`Indeterminate`, not invalid — same handling as missing parent
revisions. The chain just doesn't extend backwards in history; the
successor is still a valid leaf.

Forks (two successor tags both naming the same `supersedes`) are not
blocked — in a federated system an actor may double-publish from two
devices. The resolver picks a single head among the chain leaves via
the fork-tiebreak rule above; the fork is preserved as an explicit
audit signal rather than silently smoothed over.

## Example — Bind

```json
{
  "type": "ObjectVersionTag",
  "version": 1,
  "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
  "subject": "object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
  "created_at": "2026-05-01T14:32:07Z",
  "body": {
    "object": "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
    "version": "1.2.3",
    "target": "zQmTbHEDi1jqyu1WKzmUaT9eJ48nWjMv55GrW88JArfCZUu",
    "supersedes": null
  },
  "signature": {
    "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
    "key_id": "primary",
    "algorithm": "ed25519",
    "bytes": "base64-signature"
  }
}
```

## Example — Revoke

```json
{
  "type": "ObjectVersionTag",
  "version": 1,
  "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
  "subject": "object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
  "created_at": "2026-05-02T09:15:00Z",
  "body": {
    "object": "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
    "version": "1.2.3",
    "target": null,
    "supersedes": "zQmPriorTagStatementId..."
  },
  "signature": { "...": "..." }
}
```

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ObjectVersionTag"` | `string` |
| version `1` | `u8` |
| `actor` | `ActorId` payload as `string` |
| `subject` | internal Kairo reference as `string` |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `body` | body fields below |

## Canonical Body Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `object` | `ObjectId` payload as `string` |
| `version` | semver string (validated as semver 2.0.0) as `string` |
| `target` | `option<string>` — `0x00` for revoke, `0x01 \|\| string(StatementId payload)` for bind |
| `supersedes` | `option<string>` — `0x00` for genesis, `0x01 \|\| string(StatementId payload)` for successor |

`version` must satisfy semver 2.0.0 (`MAJOR.MINOR.PATCH` plus optional
pre-release and build metadata, e.g. `1.2.3`, `1.2.3-rc.1`,
`1.2.3+build.5`). Invalid version strings are rejected at body
construction time and never reach canonical encoding. Build metadata
participates in canonical bytes but is ignored by the future dependency
resolver's ordering, per semver convention.

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signature
- received-at timestamps
- source peer or federation route
- local trust decisions
- whether this `ObjectVersionTag` statement currently wins resolution
- whether the referenced `target` (or `supersedes`) is locally available

## Rust-Equivalent Pseudocode

```text
canonical_body =
  string(object) ||
  string(version) ||
  option(target, string) ||
  option(supersedes, string)

canonical_unsigned_statement =
  string("ObjectVersionTag") ||
  u8(1) ||
  string(actor) ||
  string(subject) ||
  i64_be(created_at_epoch_seconds) ||
  canonical_body

statement_id =
  sha2_256_multihash_base58btc(
    "kairo.statement.v1" || canonical_unsigned_statement
  )
```

## Notes

- `object` is the bare `ObjectId` payload. The `subject` is the internal
  reference string, such as `object:<id>`. Both must agree on the same
  object, but are encoded separately to keep the envelope shape uniform
  across statement types.
- `target`, when present, is a `StatementId` pointing at a specific signed
  `ObjectRevision`. It is encoded as the bare `StatementId` payload.
  Pointing at a `StatementId` (not a raw `RevisionId`) keeps verification
  crisp and lets actors adopt revisions signed by other actors by
  reference, without re-signing them.
- `supersedes`, when present, is a `StatementId` pointing at the prior
  `ObjectVersionTag` this one replaces. Validation requires it to resolve
  to a tag for the **same `(actor, object, version)`**; rejecting
  cross-key references is what makes the chain meaningful.
- A revocation (`target` absent) **must** carry `supersedes`. A genesis
  tag **must not** carry `supersedes` and **must not** be a revocation.
  Other shapes are invalid.
- `created_at` is the actor's self-claim of when the tag was published.
  Canonical bytes are `i64` Unix epoch seconds (big-endian); JSON
  interchange uses strict RFC 3339 UTC seconds with the literal `Z`
  suffix and no fractional seconds. It is the supersession key, so
  `created_at` monotonicity matters for an actor publishing multiple
  tag updates; ties resolve on `statement_id`.
- A valid signature proves only that the actor made the claim. It does
  not prove that the pointed-at revision exists, that the actor has
  authority over the object, or that any other actor agrees.
- Distinct from `ObjectBranch`: branches accept arbitrary names; tags
  require strict semver. Both share the same actor-scoped, latest-wins
  resolution shape.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
