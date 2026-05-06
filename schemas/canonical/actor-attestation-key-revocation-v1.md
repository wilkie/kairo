# ActorAttestationKeyRevocation v1 Canonical Encoding

## Type

```text
ActorAttestationKeyRevocation
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

`ActorAttestationKeyRevocation` is an ordinary signed statement. Its
`StatementId` is derived from the unsigned statement envelope and body.
The signature proves authorship of those canonical bytes but is not part
of the statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ActorAttestationKeyRevocation` retracts the recovery authority of a
specific attestation key the actor has previously held — either declared
in `ActorGenesis.attestation_keys` or appended via
`ActorAttestationKeyAdd` (`schemas/canonical/actor-attestation-key-add-v1.md`).
Once revoked, the named key can no longer sign emergency key events
(`ActorEmergencyKeyRotation`, `ActorEmergencyKeyRevocation`,
`ActorAttestationKeyAdd`, or further `ActorAttestationKeyRevocation`
statements).

This exists so an operator can retire a compromised cold-storage key
without permanently retiring the actor identity itself. Without this
primitive, a one-time attestation-key compromise is forever
(`THREAT_MODEL.md` §5.11, §5.12, §6.1).

> **Cross-actor revocation is invalid for `ActorAttestationKeyRevocation`.**
> The envelope `actor` must equal the `subject`; no other actor can shrink
> someone else's attestation set. A capability cannot delegate this
> authority.

## Asymmetry with `ActorKeyRevocation` — no `retroactive` flag

Unlike `ActorKeyRevocation` (`schemas/canonical/actor-key-revocation-v1.md`)
and `ActorEmergencyKeyRevocation`
(`schemas/canonical/actor-emergency-key-revocation-v1.md`), this body
**has no `retroactive` field**. The asymmetry is intentional:

- Operational keys sign user-visible consequential statements directly
  (revisions, branches, tags, capability grants, trust). When such a key
  is compromised, retroactively invalidating its signatures is the
  cleanup mechanism — every assertion the adversary signed becomes
  invalid.
- Attestation keys never sign consequential statements directly. They
  only sign emergency events that introduce or modify operational keys
  (`ActorEmergencyKeyRotation`, `ActorEmergencyKeyRevocation`,
  `ActorAttestationKeyAdd`, `ActorAttestationKeyRevocation`).
  Consequential damage from a compromised attestation key always flows
  through the operational keys those events introduce.

The right cleanup path for "an adversary used a compromised attestation
key to emergency-rotate in a malicious operational key" is therefore
two statements: `ActorAttestationKeyRevocation` (signed by another
attestation key) stops the bleeding by revoking the cold-storage key,
and `ActorKeyRevocation { retroactive: true }` (signed by the operator's
new active key) unwinds the historical damage at the operational layer
where it accrued.

A revoked attestation key remains historically witnessed — events it
signed before its revocation's `created_at` remain valid. This is
deliberate. The operator who legitimately rotated cold storage media
six months ago should not have their old `ActorEmergencyKeyRotation`
retroactively invalidated by a routine cold-storage retirement today.

## Authority to Revoke

The envelope carries `signatures: Vec<Signature>`. The verifier accepts
the statement iff `signatures` contains
≥ `attestation_threshold_at(actor, created_at)` valid signatures from
*distinct* `key_id`s in the **attestation key set at `created_at`** —
the same set described in
`schemas/canonical/actor-emergency-key-rotation-v1.md` "Signing-Surface
Rule":

> The attestation key set for `actor` at causal position `T` is the
> union of:
>
> - every `PublicKey` declared in `ActorGenesis.attestation_keys`,
> - every `new_key` from an `ActorAttestationKeyAdd` statement signed
>   by `actor` with `created_at ≤ T`,
>
> minus every `revoked_key` of an `ActorAttestationKeyRevocation`
> statement signed by `actor` with `created_at ≤ T`.

`revoked_key` MAY be among the signing `key_id`s (self-revocation
contributes one signature toward threshold). Under threshold = 1 this
is the legitimate "burn this key" gesture from a single operator;
under threshold > 1 it is one of multiple cosigners. Self-revocation
alone is sufficient only when threshold = 1.

The operational signing surface (active key per the rotation chain)
**cannot** revoke attestation keys, even if it is the only key the
operator currently holds. This separation mirrors
`ActorAttestationKeyAdd`'s authority rule and keeps a compromised
operational key from quietly shrinking the recovery surface.

## Resolution Rule

> The attestation key set for `(actor, T)` is
> `(ActorGenesis.attestation_keys ∪ { add.new_key | add ∈ adds(actor),
>   add.created_at ≤ T })
>  ∖ { rev.revoked_key | rev ∈ revs(actor), rev.created_at ≤ T }`,
> where `adds(actor)` is the set of valid `ActorAttestationKeyAdd`
> statements signed by `actor` and `revs(actor)` is the set of valid
> `ActorAttestationKeyRevocation` statements signed by `actor`.

Order does not matter; revocations and adds compose by set semantics.
A duplicate revocation (same `(actor, revoked_key)`) is a redundant
no-op for federation robustness — the validator does not reject it.

## Validation

Every `ActorAttestationKeyRevocation` must satisfy:

- `revoked_key` is in the attestation key set at the statement's
  `created_at`. A revocation referencing a `KeyId` the actor never
  held in their attestation set is `Indeterminate` until the
  introducing statement (`ActorGenesis` or `ActorAttestationKeyAdd`)
  is observed; once observed, a revocation of an unknown attestation
  key is treated as a redundant no-op (the key was never authoritative
  to begin with).
- The **resulting attestation set size is ≥ the resulting attestation
  threshold** (`ACTORS.md` §5.5.3, generalized form of the §5.5.1
  bricking guard). A revocation that would drop the set below the
  threshold (e.g., revoking the operator's last attestation key when
  threshold = 1, or revoking the third of three keys when threshold =
  3) is **invalid**. The operator must either stage an
  `ActorAttestationKeyAdd` first, or stage an
  `ActorAttestationThresholdChange` lowering the threshold first
  (subject to the asymmetric authority rule in §5.5.3). The CLI
  refuses to construct a violating revocation; the store rejects it
  at put time so direct callers cannot bypass the guard.
- `signatures` contains ≥ `attestation_threshold_at(actor,
  created_at)` valid signatures, each from a distinct `key_id` in
  the attestation set at `created_at`. The signing keys MAY include
  `revoked_key` itself (self-revocation contributes one signature
  toward threshold). Sub-threshold counts and duplicate `key_id`s
  make the entire statement invalid.
- `reason` is optional human-readable text; its presence does not
  affect resolution but it IS included in canonical bytes (changing
  the reason changes the `StatementId`).

## Example — Retiring a compromised attestation key

The operator believes one of their cold-storage keys has been
exfiltrated and revokes it from another attestation key they still
hold:

```json
{
  "type": "ActorAttestationKeyRevocation",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-12-15T09:30:00Z",
  "body": {
    "revoked_key": "zQm<compromised-attestation-key-id>",
    "reason": "yubikey reported lost"
  },
  "signatures": [
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<other-attestation-key-id>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-other-attestation-key"
    }
  ]
}
```

## Example — Self-revocation

The operator wants to burn a single attestation key they no longer
trust, but only after staging a replacement:

```json
{
  "type": "ActorAttestationKeyAdd",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-12-15T09:00:00Z",
  "body": {
    "new_key": {
      "algorithm": "ed25519",
      "bytes": "base64-of-fresh-replacement-attestation-key"
    }
  },
  "signatures": [
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<key-to-be-retired>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-key-to-be-retired"
    }
  ]
}
```

…immediately followed by:

```json
{
  "type": "ActorAttestationKeyRevocation",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-12-15T09:01:00Z",
  "body": {
    "revoked_key": "zQm<key-to-be-retired>",
    "reason": "self-revoke after staging replacement"
  },
  "signatures": [
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<key-to-be-retired>",
      "algorithm": "ed25519",
      "bytes": "base64-self-signature"
    }
  ]
}
```

After both statements, only the freshly-added key remains in the
attestation set. The set-size guard passes because the add landed
first; under threshold = 1 the surviving single key is sufficient.

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ActorAttestationKeyRevocation"` | `string` |
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
| `reason` | `option<string>` — `0x00` if absent, `0x01 \|\| string(reason)` if present |

`KeyId` is the derived identifier of the attestation key being revoked,
in the same `z<base58btc(multihash_sha2_256(...))>` payload format used
elsewhere (see `actor-genesis-v1.md` §"Key IDs").

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signatures (any of them; the entire `signatures` array)
- received-at timestamps
- source peer or federation route
- whether this statement currently wins resolution (revocation is
  standalone, not chained)
- whether the signing attestation keys were declared at genesis or
  appended later via `ActorAttestationKeyAdd`
- whether the resulting attestation set size meets the threshold (the
  size guard is a resolution-time validation, not identity-defining)
- the resolved attestation threshold at `created_at`

## Rust-Equivalent Pseudocode

```text
canonical_body =
  string(revoked_key) ||
  option(reason, string)

canonical_unsigned_statement =
  string("ActorAttestationKeyRevocation") ||
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
  attestation set is being shrunk. They must agree
  (`subject = "actor:" || actor`).
- There is **no `retroactive` flag**. Emergency events signed by a
  revoked attestation key before the revocation's `created_at` remain
  valid. Cleanup of consequential damage is a routine
  `ActorKeyRevocation { retroactive: true }` against the operational
  keys those emergency events introduced. See "Asymmetry" above.
- Operational signing keys (per the rotation chain) are not affected
  by this statement and are not eligible to sign it. Targeting an
  operational key with `revoked_key` is `Indeterminate` (the key was
  never in the attestation set).
- A valid signature proves an attestation key in the set at
  `created_at` authorized the revocation. It does not prove the
  operator still holds the private half — by design, since the
  legitimate "this key is compromised, burn it" gesture should
  succeed even when only the adversary holds the key.
- Operator hygiene: pair revocation with detection. An unexpected
  `ActorAttestationKeyRevocation` in `kairo actor key-history` is a
  strong signal of attestation-surface compromise. See
  `THREAT_MODEL.md` §8.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
