# ActorTrust v1 Canonical Encoding

## Type

```text
ActorTrust
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

`ActorTrust` is an ordinary signed statement. Its `StatementId` is derived
from the unsigned statement envelope and body. The signature proves
authorship of those canonical bytes but is not part of the statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ActorTrust` records a local actor's first-person opinion about another
actor's identity claims: trusted, untrusted, or withdrawn (no opinion).
Resolution is per-truster: `(by_actor, trusted_actor)` is the lookup
key, and each truster's chain is independent.

Trust is **not** authority over a specific object. "I trust this
actor's signatures" is a statement-acceptance opinion, not a grant of
write-access to objects. The capability / ownership-log model that
gives an actor authority over a specific object is a separate layer
(see `OBJECT.md` and the cross-actor supersession discussion in
`object-version-tag-v1.md`).

`ActorTrust` shares the `ObjectVersionTag` body shape — bind / revoke
distinction expressed via an optional `decision` field, every
non-genesis statement carrying an explicit `supersedes` chain edge —
with one tightening:

> **Cross-actor supersession is invalid for `ActorTrust`.** A
> `supersedes` reference must resolve to a prior `ActorTrust` from the
> **same `by_actor`** for the **same `trusted_actor`**. Trust is
> first-person; "actor B overrides actor A's trust opinion" has no
> coherent semantic.

This is tighter than `ObjectVersionTag`, where cross-actor `supersedes`
is recorded for audit and awaits the §10 capability model to become
load-bearing.

Trust statements are local in MVP — they are signed (so they can be
exported, federated, or published as web-of-trust signals later) but
the local resolver consumes only same-node statements and never
publishes them automatically.

## Resolution Rule

> For `(by_actor, trusted_actor)`, the current trust decision is the
> **leaf of the supersedes chain**: an `ActorTrust` statement signed
> by `by_actor` for `trusted_actor` whose `statement_id` is not
> referenced by any other such statement's `supersedes` field.

Chain precedence is authoritative — a successor statement that
explicitly names its predecessor is unambiguously later than that
predecessor, regardless of `created_at`. `(envelope.created_at,
statement_id)` is **only** a fork tiebreak, applied when the chain
has multiple leaves.

The leaf's `decision` maps to the runtime `TrustEvaluation` enum:

| `decision` | `TrustEvaluation` |
|---|---|
| `"trusted"` | `Trusted` |
| `"untrusted"` | `Untrusted` |
| `null` (withdrawn) | `Unknown` |
| no statements at all | `Unknown` |

## Tag Chain Validation

Every `ActorTrust` is one of two shapes:

- **Genesis (no `supersedes`):**
  - `decision` must be present (`"trusted"` or `"untrusted"`).
    Withdrawing nothing is meaningless.
  - `supersedes` is absent.
- **Successor:**
  - `decision` may be `"trusted"`, `"untrusted"`, or `null`
    (withdrawal).
  - `supersedes` is present and must reference an existing
    `ActorTrust` for the **same `(by_actor, trusted_actor)`**.

A successor whose `supersedes` does not resolve in the local store is
`Indeterminate`, not invalid — same handling as missing parent
revisions or unresolved tag predecessors. The successor remains a
valid leaf; the chain just doesn't extend backwards in history.

A successor whose `supersedes` resolves to an `ActorTrust` from a
**different** `by_actor` is **invalid** — cross-actor trust
supersession is not part of the model.

Forks are not blocked (an actor signing two trust statements both
naming the same `supersedes` from two devices). The resolver picks
the head among chain leaves via fork tiebreak; the fork is preserved
as audit signal.

## Example — Grant trust

```json
{
  "type": "ActorTrust",
  "version": 1,
  "actor": "zQm<by-actor>",
  "subject": "actor:zQm<trusted-actor>",
  "created_at": "2026-05-03T14:32:07Z",
  "body": {
    "trusted_actor": "zQm<trusted-actor>",
    "decision": "trusted",
    "reason": "manually verified key fingerprint",
    "supersedes": null
  },
  "signature": { "...": "..." }
}
```

## Example — Block trust (rebind to untrusted)

```json
{
  "type": "ActorTrust",
  "version": 1,
  "actor": "zQm<by-actor>",
  "subject": "actor:zQm<trusted-actor>",
  "created_at": "2026-05-04T09:15:00Z",
  "body": {
    "trusted_actor": "zQm<trusted-actor>",
    "decision": "untrusted",
    "reason": "key was found on a compromised laptop",
    "supersedes": "zQm<prior-statement-id>"
  },
  "signature": { "...": "..." }
}
```

## Example — Withdraw

```json
{
  "type": "ActorTrust",
  "version": 1,
  "actor": "zQm<by-actor>",
  "subject": "actor:zQm<trusted-actor>",
  "created_at": "2026-05-05T12:00:00Z",
  "body": {
    "trusted_actor": "zQm<trusted-actor>",
    "decision": null,
    "reason": "no longer relevant",
    "supersedes": "zQm<prior-statement-id>"
  },
  "signature": { "...": "..." }
}
```

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ActorTrust"` | `string` |
| version `1` | `u8` |
| `actor` (the truster, `by_actor`) | `ActorId` payload as `string` |
| `subject` (`actor:<trusted-actor-id>`) | internal Kairo reference as `string` |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `body` | body fields below |

## Canonical Body Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `trusted_actor` | `ActorId` payload as `string` |
| `decision` | `option<string>` — `0x00` for withdrawal, `0x01 \|\| string("trusted" \| "untrusted")` otherwise |
| `reason` | `option<string>` — `0x00` if absent, `0x01 \|\| string(reason)` if present |
| `supersedes` | `option<string>` — `0x00` for genesis, `0x01 \|\| string(StatementId payload)` for successor |

`decision` must be either `"trusted"` or `"untrusted"` when present;
the parser rejects any other value at body construction time and the
canonical encoder never emits other strings.

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signature
- received-at timestamps
- source peer or federation route
- whether this `ActorTrust` statement currently wins resolution
- whether the referenced `supersedes` statement is locally available

## Rust-Equivalent Pseudocode

```text
canonical_body =
  string(trusted_actor) ||
  option(decision, string) ||
  option(reason, string) ||
  option(supersedes, string)

canonical_unsigned_statement =
  string("ActorTrust") ||
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

- `actor` (the envelope field) is the truster — the local actor whose
  signing key authorizes this opinion. `trusted_actor` (in the body)
  is the actor being judged.
- `subject` is the internal reference string `actor:<trusted-actor-id>`.
  It must agree with `body.trusted_actor`. They are encoded
  separately to keep the envelope shape uniform across statement
  types.
- `decision` is encoded via `option<string>` (rather than a fixed
  `u8` enum tag) for shape consistency with `ObjectVersionTag.target`,
  where `null` similarly means "withdrawn / no successor."
- `reason` is optional human-readable text. It does not affect
  resolution and is preserved for audit. It IS included in canonical
  bytes — changing the reason changes the `StatementId`.
- `created_at` is the actor's self-claim of when the decision was
  made. Canonical bytes are `i64` Unix epoch seconds (big-endian);
  JSON interchange uses strict RFC 3339 UTC seconds with the literal
  `Z` suffix and no fractional seconds. It is the fork tiebreak when
  multiple chain leaves exist; ties on it resolve on `statement_id`.
- A valid signature proves only that the actor made the claim. It
  does not bind any other party to honor that opinion.
- Distinct from `ObjectVersionTag`: trust is per-`(by_actor,
  trusted_actor)` and cross-actor supersession is invalid; tags are
  per-`(actor, object, version)` and cross-actor supersession is
  permitted (recorded for audit, awaiting capability model to honor).

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
