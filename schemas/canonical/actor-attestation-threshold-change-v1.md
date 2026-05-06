# ActorAttestationThresholdChange v1 Canonical Encoding

## Type

```text
ActorAttestationThresholdChange
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

`ActorAttestationThresholdChange` is an ordinary signed statement. Its
`StatementId` is derived from the unsigned statement envelope and body.
The signatures prove authorship of those canonical bytes but are not
part of the statement ID.

```text
StatementId = z<base58btc(multihash_sha2_256(domain || canonical_unsigned_statement_bytes))>
```

## Purpose

`ActorAttestationThresholdChange` mutates the M of the M-of-N quorum
required to sign attestation-surface emergency statements
(`ACTORS.md` §5.5.3). The threshold is initially fixed at
`ActorGenesis.attestation_threshold`; this statement is the only way
to change it after genesis.

Threshold changes exist so an operator can:

- **Raise** the threshold over time as they distribute more
  attestation keys to additional cold-storage devices, parties, or
  geographies (e.g. solo 1-of-1 → 2-of-3 after onboarding a partner).
- **Lower** the threshold to recover from a permanently lost
  attestation key when `M < N` (the surviving keys still meet the
  current threshold and can sign a lower one).

The asymmetric authority rule below prevents an attacker who has
just-barely reached the threshold from quietly consolidating control
by lowering it.

> **Cross-actor threshold change is invalid.** The envelope `actor`
> must equal the `subject`; no other actor can mutate someone else's
> attestation threshold. A capability cannot delegate this authority.

## Asymmetric Authority Rule

The verifier accepts `signatures` based on the *direction* of the
threshold change at `created_at`. Let `current = attestation_threshold_at(actor, created_at)` (excluding this
statement) and `new = body.new_threshold`.

| Direction | Required signature count |
|---|---|
| **Raise** (`new > current`) | ≥ `max(current, new)` distinct attestation signatures |
| **Lower** (`new < current`) | ≥ `current` distinct attestation signatures |
| **No-op** (`new == current`) | ≥ `current` distinct attestation signatures |

Every signature must come from a distinct `key_id` in the actor's
attestation set at `created_at`. Sub-required counts and duplicate
`key_id`s make the entire statement invalid.

The asymmetry on raises prevents the following attack: an attacker who
compromises exactly `current` distinct attestation keys could otherwise
*lower* the threshold to 1, sign anything they want unilaterally, then
*raise* it back. By demanding `max(current, new)` for any raise, a
threshold raise commits the actor to a *new floor*: the attacker
needs that many keys before they can ever lower again.

Lowers and no-ops require only `current` because by the time an
attacker has reached `current`, they already have full control and
no further authority can be wrung out of the lower / no-op rules.

## Resolution Rule

> The attestation threshold for `(actor, T)` is
> `ActorGenesis.attestation_threshold` overlaid by every valid
> `ActorAttestationThresholdChange` statement signed by `actor` with
> `created_at ≤ T` ordered by `(created_at, statement_id)` ascending.
> The most recent valid change at or before `T` wins.

A threshold change with the same `(actor, new_threshold)` as the
current resolved threshold is a no-op for resolution but a valid
statement (the operator may have used it for an audit-trail "I
explicitly reaffirm threshold = N" gesture). Duplicates collapse via
StatementId equality.

## Validation

Every `ActorAttestationThresholdChange` must satisfy:

- `1 ≤ new_threshold ≤ |attestation_set at created_at|`. A change
  that would raise the threshold above the available key count is
  invalid (the actor would be immediately bricked: no future
  emergency event could meet the new threshold). The store rejects
  such statements at put time with `StoreError::Rejected`.
- `signatures` meets the asymmetric authority rule above. The store
  validates this against the attestation set and threshold *at
  `created_at`* before persisting.
- All `signatures[i].key_id` values are in the attestation set at
  `created_at` and are distinct.

## Example — Raising threshold from 1 to 3

The actor previously had `attestation_threshold = 1` and three
attestation keys. The operator wants to require quorum for all
emergency events going forward. The raise rule requires
`max(1, 3) = 3` distinct attestation signatures.

```json
{
  "type": "ActorAttestationThresholdChange",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2027-01-15T10:00:00Z",
  "body": {
    "new_threshold": 3
  },
  "signatures": [
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<attestation-key-A>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-A"
    },
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<attestation-key-B>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-B"
    },
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<attestation-key-C>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-C"
    }
  ]
}
```

`signatures` is sorted ascending by `key_id` in canonical bytes; the
JSON form matches that ordering.

## Example — Lowering threshold from 3 to 2 after a lost key

One of three attestation keys was permanently lost. The operator
needs to lower threshold from 3 to 2 to keep recovery possible. The
lower rule requires `current = 3` signatures — fortunately one of
the three keys still being usable means this statement can be
signed by the two surviving keys plus the third's last
known-good operator (e.g., a hardware wallet recovery)... or the
statement is impossible and the actor is bricked.

In the case where two keys remain (the third is lost):

```json
{
  "type": "ActorAttestationThresholdChange",
  "version": 1,
  "actor": "zQm<actor>",
  "subject": "actor:zQm<actor>",
  "created_at": "2027-02-01T18:00:00Z",
  "body": {
    "new_threshold": 2
  },
  "signatures": [
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<surviving-attestation-key-A>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-A"
    },
    {
      "actor": "zQm<actor>",
      "key_id": "zQm<surviving-attestation-key-B>",
      "algorithm": "ed25519",
      "bytes": "base64-signature-by-B"
    }
  ]
}
```

This statement is **invalid** under the asymmetric rule: lowering
requires `current = 3` distinct signatures and only 2 are available.
The actor is bricked at the recovery surface — operator hygiene
demands `M-of-N` with `N > M` precisely to avoid this scenario
(`ACTORS.md` §5.5.3 *Resilience hygiene*). With 3-of-5, losing one
key leaves 4 surviving keys, more than enough to sign a lower to 3.

## Canonical Envelope Fields

The shared unsigned statement envelope is encoded before the body:

| Field | Encoding |
|---|---|
| type `"ActorAttestationThresholdChange"` | `string` |
| version `1` | `u8` |
| `actor` | `ActorId` payload as `string` |
| `subject` (`actor:<self-actor-id>`) | internal Kairo reference as `string` |
| `created_at` | `Timestamp` (`i64` epoch seconds) |
| `body` | body fields below |

## Canonical Body Fields

Fields are encoded in this exact order:

| Field | Encoding |
|---|---|
| `new_threshold` | `u8`, `1 ≤ new_threshold ≤ 255` (further bounded at validation time by `|attestation_set at created_at|`) |

## Excluded Fields

The following must not be included in Statement ID canonical bytes:

- signatures (any of them; the entire `signatures` array)
- received-at timestamps
- source peer or federation route
- whether this statement currently wins resolution
- whether the signing attestation keys were declared at genesis or
  appended later via `ActorAttestationKeyAdd`
- the *current* attestation threshold at `created_at` (it is a
  resolution-time computation; the same body bytes derive the same
  StatementId regardless of context)
- whether the change is a raise, lower, or no-op (this is computed
  at validation time, not identity-defining)

## Rust-Equivalent Pseudocode

```text
canonical_body =
  u8(new_threshold)

canonical_unsigned_statement =
  string("ActorAttestationThresholdChange") ||
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
  threshold is being changed. They must agree
  (`subject = "actor:" || actor`).
- The signing surface is **attestation only**. Operational signing
  keys cannot change the attestation threshold, even if they are the
  only key the operator currently holds. Verifier dispatch enforces
  this at the surface-rule layer (`ACTORS.md` §6.1); targeting an
  operational key in `signatures` is invalid.
- A threshold change does not by itself add or remove attestation
  keys; it only changes the quorum size. Pair with
  `ActorAttestationKeyAdd` (to grow N) or
  `ActorAttestationKeyRevocation` (to shrink N) — each statement
  is independent and ordered by `created_at`. Operators wanting to
  go from 1-of-1 to 3-of-3 stage two `ActorAttestationKeyAdd`s
  first (each signed at threshold = 1), then issue this statement
  with `new_threshold = 3` (signed by all three keys per the
  raise rule).
- A change with `new_threshold > |attestation_set at created_at|`
  is invalid even if the operator could collect enough signatures
  in the abstract — the resulting threshold would exceed the
  available key count and brick recovery. The store rejects this
  with `StoreError::Rejected`.
- Operator hygiene: configure `M-of-N` with `N > M` for resilience
  to lost keys. `M-of-M` plus a single lost key cannot lower
  threshold (the lower rule needs `current` signatures, one fewer
  than required). See `ACTORS.md` §5.5.3 *Resilience hygiene*.
- A threshold change is an audit-trail event regardless of whether
  it changes the resolved value. An unexpected change in
  `kairo actor key-history` is a strong signal of attestation-surface
  compromise (the same hygiene point as
  `ActorAttestationKeyRevocation`). See `THREAT_MODEL.md` §8.

## Test Vectors

Test vectors will be added once fixture generation is stabilized.
