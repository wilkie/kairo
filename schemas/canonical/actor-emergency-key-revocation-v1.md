# ActorEmergencyKeyRevocation v1 Canonical Encoding

## Type

```text
ActorEmergencyKeyRevocation
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

`ActorEmergencyKeyRevocation` is an ordinary signed statement. Its
`StatementId` is derived from the unsigned statement envelope and body.
The signature proves authorship of those canonical bytes but is not part
of the statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ActorEmergencyKeyRevocation` is the cold-storage counterpart to
`ActorKeyRevocation`. The body shape — `revoked_key`, `retroactive`,
`reason` — is identical, but the statement is signed by a **cold-storage
attestation key** instead of the actor's currently active signing key.

This exists so an operator can retract a compromised signing key even
when they no longer hold (or no longer trust) the active signing key
itself. The routine `ActorKeyRevocation` requires the active key to
sign the revocation; if the active key is the compromised one and the
attacker is using it, the operator has to first
`ActorEmergencyKeyRotation` to a fresh key they control, then
revoke from that — or, more directly, sign the revocation from cold
storage with `ActorEmergencyKeyRevocation`.

The revocation modes (default vs retroactive) and resolution semantics
are identical to `ActorKeyRevocation`
(`schemas/canonical/actor-key-revocation-v1.md`). Local policy may
refuse to honor retroactive emergency revocations issued long after the
fact, mirroring `CAPABILITIES.md` §6.3.

## Authority to Revoke

The envelope carries `signatures: Vec<Signature>`. The verifier accepts
the statement iff `signatures` contains
≥ `attestation_threshold_at(actor, created_at)` valid signatures from
*distinct* `key_id`s in the attestation set at `created_at` (the same
set and the same multi-signature rule described in
`schemas/canonical/actor-emergency-key-rotation-v1.md` "Signing-Surface
Rule"). See `ACTORS.md` §5.5.3.

Cross-actor emergency revocation is **invalid** — only the actor whose
key it is may revoke their own keys. A capability cannot delegate this
authority, and an attestation key controlled by a different actor
cannot revoke another actor's keys.

The revocation MAY target any key the actor has held — the genesis
`initial_key`, any `ActorKeyRotation.next_key`, or any
`ActorEmergencyKeyRotation.next_key`. Attestation keys themselves are
**not** revocable in v1; they are append-only after genesis.

## Resolution Rule

Same as `ActorKeyRevocation`. The revocation set for `(actor, key_id)`
spans both `ActorKeyRevocation` and `ActorEmergencyKeyRevocation`
statements; the **most-restrictive** interpretation wins (any
`retroactive = true` revocation makes the key retroactively revoked).

A key is revoked at causal position `T` iff there exists any revocation
(routine or emergency) for `(actor, key_id)` such that either
`retroactive = true` or `created_at ≤ T`.

## Revocation Validation

Every `ActorEmergencyKeyRevocation` must satisfy:

- `revoked_key` references a key the envelope actor has held at some
  causal position. A revocation referencing a `KeyId` the actor never
  held is `Indeterminate` until the introducing statement is observed;
  it is not invalid.
- `signatures` contains ≥ `attestation_threshold_at(actor,
  created_at)` valid signatures, each from a distinct `key_id` in
  the attestation set at `created_at`. Sub-threshold counts and
  duplicate `key_id`s make the entire statement invalid.
- `reason` is optional human-readable text; its presence does not
  affect resolution but it IS included in canonical bytes (changing
  the reason changes the `StatementId`).

## Example — Emergency revocation of compromised active key

The operator believes the active signing key has been exfiltrated and
wants to invalidate it from cold storage immediately, before staging an
`ActorEmergencyKeyRotation`:

```json
{
  "type": "ActorEmergencyKeyRevocation",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-11-01T08:55:00Z",
  "body": {
    "revoked_key": "zQm<compromised-active-key-id>",
    "retroactive": true,
    "reason": "key suspected exfiltrated; emergency revoke from cold storage"
  },
  "signatures": [
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<attestation-key-id>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-attestation-key"
    }
  ]
}
```

The operator typically follows up immediately with an
`ActorEmergencyKeyRotation` introducing a fresh active key.

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ActorEmergencyKeyRevocation"` | `string` |
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

`KeyId` is the derived identifier of the key being revoked, in the same
`z<base58btc(multihash_sha2_256(...))>` payload format used elsewhere
(see `actor-genesis-v1.md` §"Key IDs").

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signatures (any of them; the entire `signatures` array)
- received-at timestamps
- source peer or federation route
- whether this statement currently wins resolution (revocation is
  standalone, not chained)
- whether the signing attestation keys were declared at genesis or
  appended later via `ActorAttestationKeyAdd`
- the resolved attestation threshold at `created_at`

## Rust-Equivalent Pseudocode

```text
canonical_body =
  string(revoked_key) ||
  u8(if retroactive { 0x01 } else { 0x00 }) ||
  option(reason, string)

canonical_unsigned_statement =
  string("ActorEmergencyKeyRevocation") ||
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
- The body shape is byte-identical to `ActorKeyRevocation`; only the
  `type` marker and the verifier's signing-surface rule differ.
- An attestation key MAY revoke the operational key it would
  emergency-rotate to — there is no rule preventing self-targeted
  emergency revocation of the freshly-rotated active key. The
  operator should publish a follow-up `ActorEmergencyKeyRotation` to
  restore an active signing surface; otherwise the actor has no key
  to sign routine statements with (the bricking risk in
  `ACTORS.md` §5.5.1, but recoverable as long as another attestation
  key remains).
- Attestation keys themselves are **not** targets of any revocation
  in v1. Compromise of an attestation key has no in-protocol
  remediation in v1 beyond publishing a fresh `ActorGenesis` (which
  produces a new `ActorId` — continuity is re-established socially).
  A future revision may introduce an `ActorAttestationKeyRevocation`
  signed by another attestation key; the v1 design intentionally
  defers it until the operator-experience need is concrete.
- Retroactive emergency revocation cascades through anything that
  depended on statements signed by the revoked key, identical to
  routine revocation. `KeyPinned` capability constraints
  (`CAPABILITIES.md` §7.2) are auto-invalidated when the pinned key
  is revoked, regardless of `retroactive`.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
