# ActorKeyRevocation v1 Canonical Encoding

## Type

```text
ActorKeyRevocation
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

`ActorKeyRevocation` is an ordinary signed statement. Its `StatementId`
is derived from the unsigned statement envelope and body. The signature
proves authorship of those canonical bytes but is not part of the
statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ActorKeyRevocation` retracts the signing authority of a specific key
that previously belonged to the envelope actor. It exists to handle key
compromise: the operator declares "any signature attributable to this
`KeyId` past this point — and optionally retroactively from inception —
is not authorized."

Revocation has two modes, mirroring `ActorCapabilityRevocation`
(`schemas/canonical/actor-capability-revocation-v1.md`):

- **Default (non-retroactive).** Statements signed by `revoked_key`
  with `created_at` strictly **after** this revocation's `created_at`
  are invalid. Statements signed before the revocation remain valid.
- **Retroactive (`retroactive = true`).** All statements signed by
  `revoked_key` are invalid, regardless of when they were signed. This
  is the key-compromise mode and propagates through anything that
  depended on those statements (capability grants, branches, tags).

Local policy may refuse to honor retroactive revocations issued long
after the fact, mirroring `CAPABILITIES.md` §6.3.

## Authority to Revoke

In v1, `ActorKeyRevocation` must be signed by the envelope actor's
**currently active key** at the revocation's `created_at` (resolved
via the rotation chain — see `actor-key-rotation-v1.md`). That active
key MAY be `revoked_key` itself (the actor revoking their own current
key, immediately after which the actor has no active signing surface
until they publish a successor `ActorKeyRotation`).

Cold-storage attestation keys for emergency revocation when the
current active key is lost are deferred to Phase 2 §10 follow-on work.
The practical operator pattern when discovering key compromise is:

1. Publish `ActorKeyRotation` introducing a fresh `next_key`, signed
   by the still-uncompromised currently active key (must beat the
   attacker to this).
2. Publish `ActorKeyRevocation` with `revoked_key = <compromised KeyId>`
   and `retroactive = true`, signed by the freshly rotated-in active
   key.

Cross-actor revocation is **invalid** — only the actor whose key it
is may revoke their own keys. A capability cannot delegate the right
to revoke another actor's key.

## Resolution Rule

> A key `K` belonging to `actor` is "revoked at causal position `P`"
> iff there exists an `ActorKeyRevocation` `R` such that:
> - `R.actor = actor` (same envelope actor),
> - `R.body.revoked_key = K`,
> - either `R.body.retroactive = true`, **or** `R.created_at <= P`.

Revocation does **not** chain via `supersedes`. A second
`ActorKeyRevocation` naming the same `(actor, revoked_key)` is
redundant; the validator does not reject it (replays are tolerated for
federation robustness), but the **most restrictive** interpretation
wins: if any revocation for the key carries `retroactive = true`, the
key is treated as retroactively revoked.

The rotation chain (`actor-key-rotation-v1.md`) and the revocation
set are queried independently:

- "What is the active key for `actor` at `T`?" — rotation chain leaf
  with `created_at ≤ T`, falling back to `ActorGenesis.initial_key`.
- "Is key `K` revoked for `actor` at `T`?" — any revocation matches
  per the rule above.

A signed statement `S` from `(actor, key_id)` at `created_at = T` is
cryptographically valid iff:

1. `key_id` matches the active key for `actor` at `T`, AND
2. `key_id` is not revoked for `actor` at `T`, AND
3. The signature bytes verify against that key's public material.

## Revocation Validation

Every `ActorKeyRevocation` must satisfy:

- `revoked_key` references a key the envelope actor has held at some
  causal position — i.e. either `ActorGenesis.initial_key`'s derived
  `KeyId` or some prior `ActorKeyRotation.next_key`'s derived `KeyId`.
  A revocation referencing a `KeyId` the actor never held is
  `Indeterminate` until the introducing statement is observed; it is
  not invalid (the introducing statement may arrive later).
- The revocation itself must verify against the actor's currently
  active key at `created_at` (per the rotation chain).
- `reason` is optional human-readable text; its presence does not
  affect resolution but it IS included in canonical bytes (changing
  the reason changes the `StatementId`).

## Example — Default revocation

The actor declares an old (already-rotated-away) key formally retired:

```json
{
  "type": "ActorKeyRevocation",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-09-15T08:35:00Z",
  "body": {
    "revoked_key": "zQm<old-key-id>",
    "retroactive": false,
    "reason": "old laptop decommissioned"
  },
  "signature": {
    "actor": "zQm<actor>",
    "key_id": "zQm<current-active-key-id>",
    "algorithm": "ed25519",
    "bytes": "base64-signature"
  }
}
```

## Example — Retroactive revocation (compromise)

```json
{
  "type": "ActorKeyRevocation",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-10-01T18:42:00Z",
  "body": {
    "revoked_key": "zQm<compromised-key-id>",
    "retroactive": true,
    "reason": "key recovered from a stolen device; treat all signatures as forged"
  },
  "signature": {
    "actor": "zQm<actor>",
    "key_id": "zQm<freshly-rotated-key-id>",
    "algorithm": "ed25519",
    "bytes": "base64-signature"
  }
}
```

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ActorKeyRevocation"` | `string` |
| version `1` | `u8` |
| `actor` | `ActorId` payload as `string` |
| `subject` (`actor:<self-actor-id>`) | internal Kairo reference as `string` |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `body` | body fields below |

## Canonical Body Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `revoked_key` | `KeyId` payload as `string` |
| `retroactive` | `u8` — `0x00` for false, `0x01` for true |
| `reason` | `option<string>` — `0x00` if absent, `0x01 \|\| string(reason)` if present |

`KeyId` is the derived identifier of the key being revoked, in the
same `z<base58btc(multihash_sha2_256(...))>` payload format used
elsewhere (see `actor-genesis-v1.md` §"Key IDs").

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signature
- received-at timestamps
- source peer or federation route
- whether this `ActorKeyRevocation` statement currently wins
  resolution (revocation is standalone, not chained)

## Rust-Equivalent Pseudocode

```text
canonical_body =
  string(revoked_key) ||
  u8(if retroactive { 0x01 } else { 0x00 }) ||
  option(reason, string)

canonical_unsigned_statement =
  string("ActorKeyRevocation") ||
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

- `actor` (envelope) and `subject` both name the actor whose key is
  being revoked. They must agree (`subject = "actor:" || actor`).
- `revoked_key` is the `KeyId` payload — a string in the same shape as
  `signature.key_id`. The verifier uses string equality on this field;
  the introducing rotation (or the genesis) is consulted only to
  resolve which `PublicKey` material that `KeyId` corresponds to.
- `retroactive` is an explicit boolean. The default in JSON is
  `false`; canonical bytes carry exactly one byte (`0x00` or `0x01`).
- `reason` is optional human-readable text. It does not affect
  resolution but is preserved for audit. It IS included in canonical
  bytes — changing the reason changes the `StatementId`.
- `created_at` is the actor's self-claim of when the revocation took
  effect. Canonical bytes are `i64` Unix epoch seconds (big-endian);
  JSON interchange uses strict RFC 3339 UTC seconds with the literal
  `Z` suffix and no fractional seconds.
- A valid signature proves that the actor's currently active key at
  the revocation's `created_at` authorized the revocation. It does
  not require — or forbid — the active key to be the same as
  `revoked_key`.
- Retroactive revocation cascades: any `ActorCapabilityGrant`
  signed by `revoked_key` becomes invalid (its signature no longer
  verifies under the active-key-at-causal-position rule), which in
  turn invalidates statements that depended on grants from that
  grantor. `KeyPinned` capability constraints
  (`CAPABILITIES.md` §7.2) are auto-invalidated when the pinned key
  is revoked, regardless of `retroactive`.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
