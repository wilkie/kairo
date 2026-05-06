# ActorEmergencyKeyRotation v1 Canonical Encoding

## Type

```text
ActorEmergencyKeyRotation
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

`ActorEmergencyKeyRotation` is an ordinary signed statement. Its
`StatementId` is derived from the unsigned statement envelope and body.
The signature proves authorship of those canonical bytes but is not part
of the statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ActorEmergencyKeyRotation` is the cold-storage recovery counterpart to
`ActorKeyRotation`. The body shape is identical — `next_key` and
`supersedes` — but the statement is signed by a **cold-storage
attestation key** instead of the actor's currently active signing key.

This is the escape hatch for two scenarios:

1. **Lost active key.** The operator no longer holds the active signing
   key (lost device, destroyed disk, forgotten passphrase). A normal
   `ActorKeyRotation` is impossible because there is no working active
   key to sign it.
2. **Compromised active key.** The active key is in an attacker's hands.
   The operator must rotate to a fresh key from a surface the attacker
   does not control.

Cold-storage attestation keys are declared at `ActorGenesis` (and
appended via `ActorAttestationKeyAdd`); see `ACTORS.md` §5.5.2 and
`schemas/canonical/actor-genesis-v1.md`.

`ActorEmergencyKeyRotation` and `ActorKeyRotation` share the same
rotation chain — both contribute leaves to the per-actor `(rotation,
revocation, emergency-rotation, emergency-revocation)` chain — and are
resolved by the same active-key-at-causal-position rule. The distinction
is purely the **signing surface**: the verifier accepts a different key
set for emergency events than for routine ones.

> **Cross-actor supersession is invalid for `ActorEmergencyKeyRotation`.**
> A `supersedes` reference must resolve to a prior key event (any
> rotation or revocation, emergency or routine) signed by the **same
> envelope actor**. Key authority is first-person; no other actor can
> rotate someone else's keys, even with a covering capability or an
> attestation key of their own.

## Resolution Rule

Same as `ActorKeyRotation` (`schemas/canonical/actor-key-rotation-v1.md`
"Resolution Rule"). The chain spans every `ActorKeyRotation`,
`ActorKeyRevocation`, `ActorEmergencyKeyRotation`, and
`ActorEmergencyKeyRevocation` statement signed by `actor`. The chain leaf
is the statement no other key-event statement supersedes.

## Active-Key-At-Causal-Position

Same as `ActorKeyRotation`. `ActorEmergencyKeyRotation` extends the chain
exactly like a routine rotation; the resolved `next_key` becomes the
actor's active signing key for any subsequent statement at `T' > T`.

## Signing-Surface Rule

The envelope carries `signatures: Vec<Signature>` instead of a single
`signature` (`ACTORS.md` §5.5.3). The verifier accepts the statement
iff:

1. Every entry in `signatures` has a `key_id` in the actor's
   **attestation key set at `T`** (resolved per
   `actor-attestation-key-revocation-v1.md` "Resolution Rule" — the
   set composes `ActorGenesis.attestation_keys`, every
   `ActorAttestationKeyAdd.new_key` ≤ `T`, minus every
   `ActorAttestationKeyRevocation.revoked_key` ≤ `T`).
2. All `signatures[i].key_id` values are distinct (duplicate
   signatures by the same key do not double-count).
3. `signatures.len() >= attestation_threshold_at(actor, T)`
   (`ACTORS.md` §5.5.3 — the threshold composes
   `ActorGenesis.attestation_threshold` overlaid by
   `ActorAttestationThresholdChange` statements ≤ `T`).
4. Every signature byte sequence verifies against its respective
   public key under the declared algorithm.

Sub-threshold counts, duplicate `key_id`s, signatures from keys
outside the attestation set, or any single byte-verification failure
make the entire statement invalid (not "verify what we have and
ignore the rest").

This is a strict departure from `ActorKeyRotation`, whose single
signature must match the active key per the rotation chain.
Attestation keys never qualify as "active" for routine statements;
signing keys never qualify as "attestation" for emergency
statements. The two surfaces never overlap.

## Chain Validation

Every `ActorEmergencyKeyRotation` is one of two shapes:

- **First key event after genesis (`supersedes = null`):**
  - Signed by an attestation key declared in `ActorGenesis.attestation_keys`.
  - Rotates away from the genesis-initial key to `next_key`.
  - Permitted even if no compromise or loss has occurred — operators
    may use emergency rotation for any reason. Routine hygiene should
    still prefer `ActorKeyRotation` so the operational signing key
    spends its time signing operational statements.
- **Successor (`supersedes != null`):**
  - Signed by an attestation key in the actor's attestation set at
    `created_at`.
  - `supersedes` references an existing key-event statement (any kind)
    for the **same envelope actor**.

A successor whose `supersedes` does not resolve in the local store is
`Indeterminate`, not invalid — same handling as missing predecessors
elsewhere. The successor remains a valid leaf; the chain just doesn't
extend backwards in history.

A successor whose `supersedes` resolves to a key-event statement signed
by a **different actor** is **invalid** — no cross-actor recovery.

Forks are not blocked. They are preserved as audit signal and almost
always indicate compromise or a split-brain operator.

## Example — First emergency rotation, threshold = 1

```json
{
  "type": "ActorEmergencyKeyRotation",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-11-01T09:00:00Z",
  "body": {
    "next_key": {
      "algorithm": "ed25519",
      "bytes": "base64-of-fresh-32-byte-public-key"
    },
    "supersedes": null
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

## Example — Emergency rotation, threshold = 2

The actor's `attestation_threshold` is 2; two distinct attestation
keys cosigned the same canonical bytes. `signatures` is sorted
ascending by `key_id`.

```json
{
  "type": "ActorEmergencyKeyRotation",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2026-11-15T22:30:00Z",
  "body": {
    "next_key": {
      "algorithm": "ed25519",
      "bytes": "base64-of-newer-32-byte-public-key"
    },
    "supersedes": "zQm<prior-key-event-statement-id>"
  },
  "signatures": [
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<attestation-key-id-A>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-attestation-key-A"
    },
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<attestation-key-id-B>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-attestation-key-B"
    }
  ]
}
```

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ActorEmergencyKeyRotation"` | `string` |
| version `1` | `u8` |
| `actor` | `ActorId` payload as `string` |
| `subject` (`actor:<self-actor-id>`) | internal Kairo reference as `string` |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `body` | body fields below |

## Canonical Body Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `next_key` | `PublicKey` (`string(algorithm) \|\| bytes(public_key)`) |
| `supersedes` | `option<string>` — `0x00` for the first key event, `0x01 \|\| string(StatementId payload)` otherwise |

`PublicKey` follows the same canonical encoding as
`ActorGenesis.initial_key` (see `actor-genesis-v1.md`).

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signatures (any of them; the entire `signatures` array)
- the derived `KeyId` of `next_key`
- received-at timestamps
- source peer or federation route
- whether this statement currently wins resolution
- whether the referenced `supersedes` statement is locally available
- whether the signing attestation keys were declared at genesis or
  appended later via `ActorAttestationKeyAdd`
- the resolved attestation threshold at `created_at` (the threshold
  is a verification-time check, not identity-defining)

## Rust-Equivalent Pseudocode

```text
canonical_public_key =
  string(algorithm) ||
  bytes(public_key)

canonical_body =
  canonical_public_key(next_key) ||
  option(supersedes, string)

canonical_unsigned_statement =
  string("ActorEmergencyKeyRotation") ||
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

- `actor` (envelope) and `subject` both name the actor whose keys are
  being rotated. They must agree (`subject = "actor:" || actor`).
- The body shape is byte-identical to `ActorKeyRotation`; only the
  `type` marker and the verifier's signing-surface rule differ. This
  asymmetry is intentional — distinct types make recovery events
  grep-able in audit, and let the verifier apply one rule per kind
  rather than dispatching on signature surface within a single kind.
- `next_key` is the new active signing key. The attestation key
  signing this statement does **not** become the new active key; it
  remains an attestation-only authority.
- The signing-surface rule means a leaked attestation key alone is not
  enough to silently sign forged operational statements. The attacker
  would have to first emergency-rotate to a key they control, then
  sign with that — which leaves an emergency-rotation event in the
  audit trail signed by an attestation `key_id`. Operators should
  monitor for unexpected emergency rotations.
- Recovery from emergency rotation does **not** auto-revoke the prior
  active key. If the operator believes the prior key was compromised,
  they should follow up with an `ActorEmergencyKeyRevocation` (or a
  routine `ActorKeyRevocation` signed by the freshly emergency-rotated
  active key, since the active key surface is now usable again).

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
