# ActorAttestationKeyAdd v1 Canonical Encoding

## Type

```text
ActorAttestationKeyAdd
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

`ActorAttestationKeyAdd` is an ordinary signed statement. Its
`StatementId` is derived from the unsigned statement envelope and body.
The signature proves authorship of those canonical bytes but is not part
of the statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ActorAttestationKeyAdd` appends a new public key to the actor's
**attestation key set** — the cold-storage authority surface that signs
emergency key events (`ActorEmergencyKeyRotation`,
`ActorEmergencyKeyRevocation`, and further `ActorAttestationKeyAdd`
statements).

The genesis-declared attestation set is fixed in `ActorGenesis`
(part of the canonical bytes that derive the `ActorId`). After genesis,
the operator may grow the set by publishing `ActorAttestationKeyAdd`
statements, each adding exactly one new attestation key. This exists so
operators can:

- rotate cold storage media (e.g. retiring an old YubiKey by adding a
  new one before destroying the old);
- expand recovery resilience (add a second cold-storage key kept in a
  different location);
- replace a lost or destroyed attestation key (provided at least one
  attestation key still works).

Attestation keys form a **set**, not a chain. There is no `supersedes`
field and no removal mechanism in v1 — the attestation set is
**append-only**. Compromise of an attestation key has no in-protocol
remediation in v1; see `ACTORS.md` §5.5.2 for the rationale.

> **Cross-actor add is invalid for `ActorAttestationKeyAdd`.** The
> envelope `actor` must equal the `subject`; no other actor can grow
> someone else's attestation set.

## Authority to Add

The envelope carries `signatures: Vec<Signature>`. The verifier accepts
the statement iff `signatures` contains
≥ `attestation_threshold_at(actor, created_at)` valid signatures from
*distinct* `key_id`s in the **attestation key set at `created_at`**:

> The attestation key set for `actor` at causal position `T` is the
> union of every `PublicKey` declared in `ActorGenesis.attestation_keys`
> and every `new_key` from an `ActorAttestationKeyAdd` statement signed
> by `actor` with `created_at ≤ T`, minus every `revoked_key` from an
> `ActorAttestationKeyRevocation` statement signed by `actor` with
> `created_at ≤ T`.

This means a freshly-added attestation key is itself eligible to
contribute to the threshold for subsequent `ActorAttestationKeyAdd`
statements — the attestation authority is a closed system that grows
on its own terms. The operational signing key surface (active key per
the rotation chain) **cannot** add attestation keys, even if it is
the only key the operator currently holds. This separation is
intentional: it keeps a compromised active key from quietly
registering attacker-controlled recovery keys. With
`attestation_threshold > 1`, a single compromised attestation key
also cannot register attacker-controlled keys (the attacker would
need ≥ threshold distinct compromised attestation keys). See
`ACTORS.md` §5.5.3.

## Resolution Rule

> The attestation key set for `actor` at causal position `T` is the
> union of `ActorGenesis.attestation_keys` and the `new_key` of every
> `ActorAttestationKeyAdd` statement signed by `actor` with
> `created_at ≤ T`. Order does not matter; duplicates collapse.

A duplicate add (same `(actor, new_key)`) is tolerated for federation
robustness — the verifier does not reject it. The set is idempotent;
the second add is redundant but not invalid.

## Validation

Every `ActorAttestationKeyAdd` must satisfy:

- `new_key` is not already in the attestation set at the statement's
  `created_at`. A duplicate is `Indeterminate` if predecessor
  attestation events have not been observed; once they are, the
  duplicate is treated as a redundant no-op (not an error).
- `new_key` is **disjoint** from any signing key the actor has held
  (genesis-initial, any `ActorKeyRotation.next_key`, any
  `ActorEmergencyKeyRotation.next_key`). Promoting a signing key into
  the attestation set would collapse the surface separation; the body
  validator rejects it at construction time. The introducing key event
  may not have been observed locally — in that case validation is
  `Indeterminate`.
- `signatures` contains ≥ `attestation_threshold_at(actor,
  created_at)` valid signatures, each from a distinct `key_id` in
  the attestation set at `created_at`. Sub-threshold counts and
  duplicate `key_id`s make the entire statement invalid.

## Example — Adding a second attestation key

```json
{
  "type": "ActorAttestationKeyAdd",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-12-01T10:00:00Z",
  "body": {
    "new_key": {
      "algorithm": "ed25519",
      "bytes": "base64-of-fresh-32-byte-attestation-public-key"
    }
  },
  "signatures": [
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<existing-attestation-key-id>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-existing-attestation-key"
    }
  ]
}
```

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ActorAttestationKeyAdd"` | `string` |
| version `1` | `u8` |
| `actor` | `ActorId` payload as `string` |
| `subject` (`actor:<self-actor-id>`) | internal Kairo reference as `string` |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `body` | body fields below |

## Canonical Body Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `new_key` | `PublicKey` (`string(algorithm) \|\| bytes(public_key)`) |

`PublicKey` follows the same canonical encoding as
`ActorGenesis.initial_key` (see `actor-genesis-v1.md`).

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signatures (any of them; the entire `signatures` array)
- the derived `KeyId` of `new_key`
- received-at timestamps
- source peer or federation route
- whether the signing attestation keys were declared at genesis or
  appended earlier via another `ActorAttestationKeyAdd`
- whether `new_key` is already in the attestation set (idempotence is
  resolution-time concern, not identity-defining)
- the resolved attestation threshold at `created_at`

## Rust-Equivalent Pseudocode

```text
canonical_public_key =
  string(algorithm) ||
  bytes(public_key)

canonical_body =
  canonical_public_key(new_key)

canonical_unsigned_statement =
  string("ActorAttestationKeyAdd") ||
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

- `actor` (envelope) and `subject` both name the actor whose
  attestation set is being grown. They must agree
  (`subject = "actor:" || actor`).
- `new_key` is operator-presented public key material; the matching
  private half is held externally in cold storage. Kairo never
  stores attestation private keys. The CLI MAY offer a
  generate-and-print convenience that emits the seed once and forgets
  it, mirroring the genesis flow.
- An `ActorAttestationKeyAdd` does **not** participate in the
  rotation chain. It is consulted only for resolving the attestation
  set; it is not a key event in the routine-rotation sense and never
  affects the active signing key.
- A valid signature proves an existing attestation key authorized the
  addition. It does not prove that the operator holds the private
  half of `new_key` — that is a key-management concern outside the
  protocol. (An attacker who compromised an attestation key could
  register their own attestation key, but doing so leaves an
  audit-trail event signed by the compromised attestation `key_id`,
  which operators should monitor for.)
- v1 has no removal mechanism. If an attestation key is compromised,
  the operator can publish a fresh `ActorGenesis` (different
  `ActorId`, continuity re-established socially), or wait for a
  future schema revision that introduces
  `ActorAttestationKeyRevocation`.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
