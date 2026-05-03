# ActorCapabilityRevocation v1 Canonical Encoding

## Type

```text
ActorCapabilityRevocation
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

`ActorCapabilityRevocation` is an ordinary signed statement. Its
`StatementId` is derived from the unsigned statement envelope and body.
The signature proves authorship of those canonical bytes but is not
part of the statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ActorCapabilityRevocation` retracts a previously issued
`ActorCapabilityGrant` (`schemas/canonical/actor-capability-grant-v1.md`).
The revocation is signed by the same actor that issued the grant
(only the original grantor may revoke their own grant in v1; multi-actor
revocation paths are deferred — see `specs/CAPABILITIES.md` §5.2).

Revocation has two modes:

- **Default (non-retroactive).** The grant becomes invalid for
  authorization checks at causal positions strictly **after** the
  revocation's `created_at`. Statements the grantee issued before the
  revocation remain valid.
- **Retroactive (`retroactive = true`).** The grant becomes invalid
  from inception. Every statement issued under it is re-evaluated and
  may flip to invalid. This propagates to every cross-actor
  `supersedes` edge that relied on the grant. Reserved for fraud or
  grantee key compromise.

Local policy may refuse to honor retroactive revocations issued long
after the fact (`specs/CAPABILITIES.md` §6.3).

## Resolution Rule

> A grant `G` is "revoked at causal position `P`" iff there exists an
> `ActorCapabilityRevocation` `R` such that:
> - `R.grantor = G.grantor` (signed by the same actor),
> - `R.revoked_grant = G.statement_id`,
> - either `R.retroactive = true`, **or** `R.created_at <= P`.

`evaluate_capability` (defined in `specs/CAPABILITIES.md` §6.1) folds
the revocation outcome into a `CapabilityEvaluation::Revoked(R.id)`
when applicable.

There is no chain of revocations on a single grant — one
`ActorCapabilityRevocation` is enough to retire the grant. A second
revocation naming the same `revoked_grant` is redundant; the validator
does not reject it (replays are tolerated for federation
robustness), but only the first observed revocation is load-bearing.
The most-restrictive interpretation wins: if any revocation for the
grant carries `retroactive = true`, the grant is treated as
retroactively revoked.

## Revocation Validation

Every `ActorCapabilityRevocation` must satisfy:

- `revoked_grant` references an `ActorCapabilityGrant` whose `actor`
  (the grantor in the envelope) equals this revocation's `actor`.
  Cross-grantor revocation is **invalid** — only the issuer may revoke
  their own grant in v1.
- A revocation whose `revoked_grant` does not resolve in the local
  store is `Indeterminate`, not invalid — same handling as missing
  grant predecessors. The revocation is recorded; once the referenced
  grant arrives, the resolver applies the revocation.
- `reason` is optional human-readable text; its presence does not
  affect resolution but it IS included in canonical bytes (changing
  the reason changes the `StatementId`).

## Example — Default revocation

Grantor `A` revokes a previously issued grant; statements `B` issued
under it before this point remain valid:

```json
{
  "type": "ActorCapabilityRevocation",
  "version": 1,
  "actor": "zQm<grantor-A>",
  "subject": "statement:zQm<revoked-grant-statement-id>",
  "created_at": "2026-06-01T12:00:00Z",
  "body": {
    "revoked_grant": "zQm<revoked-grant-statement-id>",
    "retroactive": false,
    "reason": "delegate stepped down from maintainer role"
  },
  "signature": { "...": "..." }
}
```

## Example — Retroactive revocation

Grantor `A` retroactively revokes a grant after discovering grantee
`B`'s key was compromised; every statement `B` issued under the grant
is re-evaluated and flips to invalid:

```json
{
  "type": "ActorCapabilityRevocation",
  "version": 1,
  "actor": "zQm<grantor-A>",
  "subject": "statement:zQm<revoked-grant-statement-id>",
  "created_at": "2026-06-15T08:30:00Z",
  "body": {
    "revoked_grant": "zQm<revoked-grant-statement-id>",
    "retroactive": true,
    "reason": "grantee key compromised; assume all uses fraudulent"
  },
  "signature": { "...": "..." }
}
```

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ActorCapabilityRevocation"` | `string` |
| version `1` | `u8` |
| `actor` (the grantor, signer) | `ActorId` payload as `string` |
| `subject` (`statement:<revoked-grant-statement-id>`) | internal Kairo reference as `string` |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `body` | body fields below |

## Canonical Body Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `revoked_grant` | `StatementId` payload as `string` |
| `retroactive` | `u8` — `0x00` for false, `0x01` for true |
| `reason` | `option<string>` — `0x00` if absent, `0x01 \|\| string(reason)` if present |

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signature
- received-at timestamps
- source peer or federation route
- whether the referenced `revoked_grant` is locally available
- whether any other `ActorCapabilityRevocation` already retired this
  grant

## Rust-Equivalent Pseudocode

```text
canonical_body =
  string(revoked_grant) ||
  u8(if retroactive then 0x01 else 0x00) ||
  option(reason, string)

canonical_unsigned_statement =
  string("ActorCapabilityRevocation") ||
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

- `actor` (the envelope field) is the original grantor — the same
  actor that signed the `ActorCapabilityGrant` being revoked. The
  validator rejects any revocation where this does not match the
  referenced grant's grantor.
- `subject` is the internal reference string
  `statement:<revoked-grant-statement-id>`. It must agree with
  `body.revoked_grant`. They are encoded separately to keep the
  envelope shape uniform across statement types.
- `retroactive = true` is destructive and may invalidate cross-actor
  `supersedes` edges that depended on the revoked grant — every
  statement the grantee issued under the grant is re-evaluated. Use
  for grantee key compromise, fraud, or administrative correction.
- `reason` is preserved in canonical bytes for audit; it does not
  affect resolution but is part of the statement's identity. A
  revocation with the same `revoked_grant` and `retroactive` but a
  different `reason` has a different `StatementId`.
- A revocation does not chain — there is no `supersedes` field. One
  revocation per grant is sufficient. Duplicate revocations naming the
  same grant are tolerated (replay-friendly for federation) but only
  the first observed revocation is load-bearing, with the
  most-restrictive `retroactive` interpretation winning.
- Per `specs/CAPABILITIES.md` §7.1, grantor key compromise requires the
  operator to enumerate grants signed by the compromised key and issue
  retroactive revocations for any whose continued validity is
  unacceptable. The default `ActorId`-binding model does not auto-kill
  grants on key revocation — that is the explicit cleanup runbook.
- For grants with `CapabilityConstraint::KeyPinned(KeyId)`, key
  revocation auto-invalidates the grant — no
  `ActorCapabilityRevocation` is needed. See
  `actor-capability-grant-v1.md` and `specs/CAPABILITIES.md` §7.2.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
