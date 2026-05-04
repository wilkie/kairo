# ObjectBranch v1 Canonical Encoding

## Type

```text
ObjectBranch
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

`ObjectBranch` is an ordinary signed statement. Its `StatementId` is derived from
the unsigned statement envelope and body. The signature proves authorship of
those canonical bytes but is not part of the statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ObjectBranch` is a named, actor-scoped, mutable pointer at a specific
`ObjectRevision` statement. It is the Kairo analogue of a Git ref: a way for
an actor to publicly claim "for object X, my revision named *N* is currently
revision-statement *R*."

Mutability is achieved without mutable storage: a successor `ObjectBranch`
statement supersedes its predecessor. Older `ObjectBranch` statements stay
valid evidence of past claims; only the chain leaf is load-bearing for
resolving "what is the current branch?"

The string `name = "head"` is the conventional default the CLI assumes when
no name is given. It is not reserved at the protocol level; actors may use
any name (`"release"`, `"audit"`, `"alice-staging"`, etc.).

## Resolution Rule

> For `(actor, object, name)`, the current branch is the **chain leaf** —
> the `ObjectBranch` statement no other statement supersedes within the
> chain rooted at this `(actor, object, name)` triple. If the chain has
> multiple leaves (a fork), pick the one with the greatest
> `(envelope.created_at, statement_id)`.

A successor that explicitly names its predecessor via `supersedes` is
unambiguously later regardless of `created_at`. `(created_at, statement_id)`
ordering is only a fork tiebreak.

`supersedes` may name a `ObjectBranch` statement signed by a **different
actor** for the same `(object, name)`. The cross-actor edge is honored by
the resolver iff the successor's signer holds an `ObjectBranch` capability
on `object` at the successor's `created_at` — see `specs/CAPABILITIES.md`
§6.2 (the same rule applied to `ObjectVersionTag`). Without a covering
capability the cross-actor edge is recorded but not honored.

Cryptographic validity, actor resolution, and trust evaluation are reported
independently and do not affect resolution at the statement layer.

## Examples

### Genesis advance

```json
{
  "type": "ObjectBranch",
  "version": 1,
  "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
  "subject": "object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
  "created_at": "2026-05-01T14:32:07Z",
  "body": {
    "object": "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
    "name": "head",
    "revision": "zQmTbHEDi1jqyu1WKzmUaT9eJ48nWjMv55GrW88JArfCZUu",
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

### Successor advance superseding the genesis

```json
{
  "type": "ObjectBranch",
  "version": 1,
  "actor": "zQmTn1mdQDA1ryQZsiqYfRbqj5DGcG8TNvYcRmBrBLAuk5t",
  "subject": "object:zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
  "created_at": "2026-05-02T09:11:30Z",
  "body": {
    "object": "zQmR83z7U8QpdpnLXSwbQaa29Tz9DWTH6YspqDQEtTfGFrk",
    "name": "head",
    "revision": "zQmNewRevisionStatementId...",
    "supersedes": "zPriorBranchStatementId..."
  },
  "signature": { "...": "..." }
}
```

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ObjectBranch"` | `string` |
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
| `name` | branch name as `string` |
| `revision` | `StatementId` payload as `string` (the pointed-at `ObjectRevision`) |
| `supersedes` | optional `StatementId` payload as `string` (`0x00` for none, `0x01 || string(id)` for some) |

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signature
- received-at timestamps
- source peer or federation route
- local trust decisions
- whether this `ObjectBranch` statement currently wins resolution

## Rust-Equivalent Pseudocode

```text
canonical_body =
  string(object) ||
  string(name) ||
  string(revision) ||
  option(supersedes, |id| string(id))

canonical_unsigned_statement =
  string("ObjectBranch") ||
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
- `revision` is a `StatementId`, not a raw `RevisionId`. It points at a
  specific signed `ObjectRevision` statement. This keeps verification crisp
  and lets actors adopt revisions signed by other actors by reference,
  without re-signing them.
- `supersedes` is the chain edge. The genesis advance for an
  `(actor, object, name)` triple has `supersedes = null`. Successors set
  `supersedes` to the prior chain leaf's `StatementId`. The CLI
  (`kairo branch set`) auto-computes this — callers don't need to track
  it manually.
- Cross-actor `supersedes` is the load-bearing case: capability resolution
  (`evaluate_capability` in `kairo-statement::verify`) gates whether
  another actor's branch advance is allowed to take over an `(object,
  name)` chain. Without a covering capability the cross-actor edge is
  recorded but not honored, mirroring `ObjectVersionTag`'s behavior.
- `created_at` is the actor's self-claim of when the branch was published.
  Canonical bytes are `i64` Unix epoch seconds (big-endian); JSON
  interchange uses strict RFC 3339 UTC seconds with the literal `Z`
  suffix and no fractional seconds. With chain precedence in v1, ordering
  matters only for forks (multiple chain leaves at the same triple).
- A valid signature proves only that the actor made the claim. It does not
  prove that the pointed-at revision exists, that it is reachable, that
  the actor has authority over the object, or that any other actor agrees.
- Symmetric with `ObjectVersionTag`: both carry an explicit `supersedes`
  chain and the same cross-actor authority-aware resolution. Branches
  accept arbitrary names; tags require strict semver and additionally
  encode bind-vs-revoke via the `target` option.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
