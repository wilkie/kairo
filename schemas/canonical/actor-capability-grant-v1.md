# ActorCapabilityGrant v1 Canonical Encoding

## Type

```text
ActorCapabilityGrant
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

`ActorCapabilityGrant` is an ordinary signed statement. Its `StatementId`
is derived from the unsigned statement envelope and body. The signature
proves authorship of those canonical bytes but is not part of the
statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ActorCapabilityGrant` is a signed delegation: the **grantor** authorizes
the **grantee** to issue a specified set of statement kinds against a
specified scope, optionally bounded by constraints (expiration,
delegation depth, opt-in pinning to a specific signing key).

It is the distributed-systems sense of "capability" — a transferable,
unforgeable token of authority — and is the mechanism that makes
cross-actor authority claims (e.g. `ObjectVersionTag` cross-actor
`supersedes`) load-bearing in the statement graph.

Resolution is per-`(grantor, grantee, scope)` triple, with chain
precedence: each triple has at most one active chain leaf, and that leaf
is the source of truth for the grantee's current authority from this
grantor on this scope. See `specs/CAPABILITIES.md` §5.1.1 and §6.1.

`ActorCapabilityGrant` is paired with `ActorCapabilityRevocation`
(`schemas/canonical/actor-capability-revocation-v1.md`), which retracts
a grant.

## Resolution Rule

> For `(grantor, grantee, scope)`, the current grant is the **leaf of
> the supersedes chain**: an `ActorCapabilityGrant` statement signed by
> `grantor` for `(grantee, scope)` whose `statement_id` is not
> referenced by any other such statement's `supersedes` field.

Chain precedence is authoritative — a successor that explicitly names
its predecessor via `supersedes` is unambiguously later than that
predecessor, regardless of `created_at`. `(envelope.created_at,
statement_id)` is **only** a fork tiebreak, applied when the chain has
multiple leaves.

The leaf's `Capability` is the grantee's complete authorized set on
this scope from this grantor, subject to revocation (see
`actor-capability-revocation-v1.md`) and constraint satisfaction
(`ExpiresAt`, `MaxDelegationDepth`, `KeyPinned`).

`evaluate_capability` (defined in `specs/CAPABILITIES.md` §6.1) folds
the chain leaf, the revocation status, and the recursive grantor-
authority check into a `CapabilityEvaluation` value.

## Grant Chain Validation

Every `ActorCapabilityGrant` is one of two shapes:

- **Genesis (no `supersedes`):**
  - `supersedes` is absent.
  - `capability` is present and non-empty (statement_kinds non-empty,
    scope present).
- **Successor:**
  - `supersedes` is present and must reference an existing
    `ActorCapabilityGrant` for the **same `(grantor, grantee, scope)`**.
  - `capability` carries the complete desired authorized set as of this
    successor (omitted kinds are implicitly revoked within this triple
    at the successor's causal position).

Cross-grantor `supersedes` is **invalid** — only the same `grantor` may
supersede their own prior grant. (Tighter than `ObjectVersionTag`,
matching `ActorTrust`.)

A second genesis grant on an existing triple — `supersedes = null` when
the triple already has an active chain — is a shape violation and
rejected at put-time. See `specs/CAPABILITIES.md` Decision G.

A successor whose `supersedes` does not resolve in the local store is
`Indeterminate`, not invalid — same handling as missing parent
revisions or unresolved trust predecessors. The successor remains a
valid leaf; the chain just doesn't extend backwards in history.

A successor whose `supersedes` resolves to an `ActorCapabilityGrant`
for a **different** `(grantor, grantee, scope)` is invalid — that is
not a chain edge, that is a category error.

Forks (two successors both naming the same `supersedes`) are not
blocked. The resolver picks a single head among chain leaves via the
fork-tiebreak rule above; the fork is preserved as audit signal.

## Self-Grants

`grantor == grantee` is structurally valid but degenerate (the grantor
authorizes themselves, which adds nothing to existing root-or-delegated
authority). The validator does not reject; tooling should warn.

## Example — Object-scoped grant (genesis)

Grantor `A` authorizes grantee `B` to issue `ObjectVersionTag` and
`ObjectBranch` statements on object `O`, delegable to one further hop,
with no expiration:

```json
{
  "type": "ActorCapabilityGrant",
  "version": 1,
  "actor": "zQm<grantor-A>",
  "subject": "actor:zQm<grantee-B>",
  "created_at": "2026-05-03T14:32:07Z",
  "body": {
    "grantee": "zQm<grantee-B>",
    "capability": {
      "scope": { "object": "zQm<object-O>" },
      "statement_kinds": ["ObjectBranch", "ObjectVersionTag"],
      "delegable": true,
      "constraints": [
        { "max_delegation_depth": 1 }
      ]
    },
    "supersedes": null
  },
  "signature": { "...": "..." }
}
```

## Example — Successor narrowing the kind set

The same grantor `A` later supersedes the prior grant, removing
`ObjectBranch` (B is now release-only):

```json
{
  "type": "ActorCapabilityGrant",
  "version": 1,
  "actor": "zQm<grantor-A>",
  "subject": "actor:zQm<grantee-B>",
  "created_at": "2026-05-10T09:00:00Z",
  "body": {
    "grantee": "zQm<grantee-B>",
    "capability": {
      "scope": { "object": "zQm<object-O>" },
      "statement_kinds": ["ObjectVersionTag"],
      "delegable": true,
      "constraints": [
        { "max_delegation_depth": 1 }
      ]
    },
    "supersedes": "zQm<prior-grant-statement-id>"
  },
  "signature": { "...": "..." }
}
```

## Example — Actor-scoped grant with `KeyPinned`

Grantor `A` authorizes `B` to issue `ActorMetadata` statements on `A`'s
own actor surface, pinned to `A`'s current signing key:

```json
{
  "type": "ActorCapabilityGrant",
  "version": 1,
  "actor": "zQm<grantor-A>",
  "subject": "actor:zQm<grantee-B>",
  "created_at": "2026-05-03T14:32:07Z",
  "body": {
    "grantee": "zQm<grantee-B>",
    "capability": {
      "scope": { "actor": "zQm<grantor-A>" },
      "statement_kinds": ["ActorMetadata"],
      "delegable": false,
      "constraints": [
        { "key_pinned": "<key-id-of-current-signing-key>" }
      ]
    },
    "supersedes": null
  },
  "signature": { "...": "..." }
}
```

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ActorCapabilityGrant"` | `string` |
| version `1` | `u8` |
| `actor` (the grantor, signer) | `ActorId` payload as `string` |
| `subject` (`actor:<grantee-id>`) | internal Kairo reference as `string` |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `body` | body fields below |

## Canonical Body Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `grantee` | `ActorId` payload as `string` |
| `capability` | nested `Capability` (see below) |
| `supersedes` | `option<string>` — `0x00` for genesis, `0x01 \|\| string(StatementId payload)` for successor |

### Capability sub-encoding

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `scope` | tagged `CapabilityScope` (see below) |
| `statement_kinds` | `list<string>` — sorted lexically, deduplicated, non-empty |
| `delegable` | `u8` — `0x00` for false, `0x01` for true |
| `constraints` | `list<CapabilityConstraint>` — sorted by tag byte ascending, at most one per variant |

`statement_kinds` strings are the canonical kind names defined in
`kairo-statement` (e.g. `"ObjectVersionTag"`, `"ObjectBranch"`,
`"ObjectRevision"`, `"ActorMetadata"`). The canonical encoder rejects
unrecognized kinds, empty lists, and duplicate entries at body
construction time.

`constraints` carries at most one of each variant. Multiple constraints
of the same variant are a shape violation. The canonical encoder sorts
by tag byte for determinism.

### CapabilityScope tagged encoding

| Variant | Tag | Payload |
|---|---|---|
| `Object(ObjectId)` | `0x00` | `ObjectId` payload as `string` |
| `Actor(ActorId)` | `0x01` | `ActorId` payload as `string` |

### CapabilityConstraint tagged encoding

| Variant | Tag | Payload |
|---|---|---|
| `ExpiresAt(i64)` | `0x00` | `i64` epoch seconds (BE, two's complement) |
| `MaxDelegationDepth(u8)` | `0x01` | `u8` depth |
| `KeyPinned(KeyId)` | `0x02` | `KeyId` payload as `string` |

## Per-`(scope, kind)` Shape Validity

Some `(scope, kind)` pairs are nonsensical and rejected at body
construction time:

- `Actor(A)` may only carry kinds that operate on the grantor's actor
  surface (e.g. actor metadata, key management). Object-targeted kinds
  like `ObjectRevision`, `ObjectBranch`, `ObjectVersionTag` are
  invalid in this scope.
- `Object(O)` may only carry kinds that operate on objects.

The exact per-kind validity rules are defined alongside each statement
kind's canonical specification. The grant validator consults that table
at body construction time.

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signature
- received-at timestamps
- source peer or federation route
- whether this `ActorCapabilityGrant` currently wins resolution
- whether the referenced `supersedes` statement is locally available
- whether the grantor's authority to issue this grant has been
  evaluated to `Held` locally
- any local index entries (e.g. the materialized cross-cutting
  `(object, grantee)` index)

## Rust-Equivalent Pseudocode

```text
canonical_capability_scope =
  case scope of
    Object(o) -> u8(0x00) || string(o)
    Actor(a)  -> u8(0x01) || string(a)

canonical_capability_constraint =
  case constraint of
    ExpiresAt(t)            -> u8(0x00) || i64_be(t)
    MaxDelegationDepth(d)   -> u8(0x01) || u8(d)
    KeyPinned(k)            -> u8(0x02) || string(k)

canonical_capability =
  canonical_capability_scope ||
  list(sorted_dedup(statement_kinds), string) ||
  u8(if delegable then 0x01 else 0x00) ||
  list(sorted_by_tag(constraints), canonical_capability_constraint)

canonical_body =
  string(grantee) ||
  canonical_capability ||
  option(supersedes, string)

canonical_unsigned_statement =
  string("ActorCapabilityGrant") ||
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

- `actor` (the envelope field) is the **grantor** — the signer of the
  grant. `grantee` (in the body) is the actor receiving authority.
- `subject` is the internal reference string `actor:<grantee-id>`. It
  must agree with `body.grantee`. The two are encoded separately to
  keep the envelope shape uniform across statement types.
- The `(grantor, grantee, scope)` triple is the chain key. Different
  scopes (different objects, or object vs. actor) are independent
  triples and have independent chains.
- All kinds within one grant share the grant's constraint set —
  per-kind constraint variance is not expressible in v1. See
  `specs/CAPABILITIES.md` §11.G for the documented trade-off.
- `created_at` is the actor's self-claim of when the grant was issued.
  Canonical bytes are `i64` Unix epoch seconds (big-endian); JSON
  interchange uses strict RFC 3339 UTC seconds with the literal `Z`
  suffix and no fractional seconds. It is the fork tiebreak when
  multiple chain leaves exist; ties resolve on `statement_id`.
- A valid signature proves only that the grantor made the claim. It
  does not prove that the grantor *had* the authority to delegate —
  that is recomputed by `evaluate_capability` against the local node's
  view of root authority and any upstream capability chain.
- Per `specs/CAPABILITIES.md` Decision E, kind narrowing lives entirely
  in `statement_kinds`. There is no kind-aware scope variant; adding
  future scope variants (`Snapshot`, `Artifact`, `ObjectFamily`)
  reuses the same `statement_kinds` field without parallel kind-aware
  variants.
- Adding a new statement kind to Kairo later does **not** retroactively
  expand existing grants — the grant's `statement_kinds` is a closed
  enumeration as of grant time. Operators must issue a superseding
  grant to pick up new kinds.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
